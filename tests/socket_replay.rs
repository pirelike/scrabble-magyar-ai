//! Kör időzítő a játékállapotban, a lépésnapló és a visszajátszás tartós mentése, a játék minden lehetséges
//! vége (a Python `test_timer_and_replay.py` megfelelője).

mod common;
use common::*;
use scrabble::server::core;
use scrabble::util;
use serde_json::{Value, json};

/// Az utolsó `game_state` esemény adata.
fn last_state(events: &[(String, Value)]) -> Value {
    events.iter().rev().find(|(n, _)| n == "game_state").map(|(_, d)| d.clone()).expect("nincs game_state esemény")
}

fn action_ok(events: &[(String, Value)]) -> bool {
    find(events, "action_result").is_some_and(|d| d["success"] == json!(true))
}

fn finished(server: &TestServer) -> bool {
    with_room(server, |room| room.game.finished)
}

fn db_game_id(server: &TestServer) -> Option<i64> {
    with_room(server, |room| room.db_game_id)
}

fn room_id(server: &TestServer) -> String {
    server.app.state.lock().rooms.keys().next().cloned().expect("nincs szoba")
}

/// Hat egymást követő passz (a soron lévővel kezdve): a játék véget ér.
async fn six_passes(server: &TestServer, a: &Sio, b: &Sio) {
    for _ in 0..6 {
        let cur = current(&[a, b]);
        cur.emit0("pass_turn");
        cur.settle().await;
        a.take();
        b.take();
        if finished(server) {
            break;
        }
    }
}

// ===================================================================================================
// Időzítő a játékállapotban
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn start_game_with_timer_sets_expires_at_in_the_first_state() {
    let server = TestServer::start().await;
    let a = server.player(1).await;
    let b = server.player(2).await;
    a.emit("create_room", json!({"name": "R", "max_players": 2, "turn_time_limit": 60}));
    a.settle().await;
    b.emit("join_room", json!({"code": a.code()}));
    b.settle().await;
    a.take();
    a.emit0("start_game");
    a.settle_long().await;
    let state = last_state(&a.take());
    assert_eq!(state["turn_time_limit"], 60);
    let expires = state["turn_timer_expires_at"].as_f64().expect("a lejárat már az első állapotban ott van");
    assert!(expires > util::now());
    assert!(expires <= util::now() + 65.0);
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_time_limit_expires_at_is_null() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2})).await;
    let cur = current(&[&a, &b]);
    let events = cur.call("pass_turn", Value::Null).await;
    let state = last_state(&events);
    assert!(state["turn_timer_expires_at"].is_null());
    assert_eq!(state["turn_time_limit"], 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn pass_turn_with_timer_sends_the_new_turns_expiry() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2, "turn_time_limit": 90})).await;
    let initial = with_room(&server, |room| room.turn_timer_expires_at).expect("induláskor is fut az óra");
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    let cur = current(&[&a, &b]);
    let state = last_state(&cur.call("pass_turn", Value::Null).await);
    let expires = state["turn_timer_expires_at"].as_f64().expect("a passz utáni állapotban már az új lejárat van");
    assert!(expires > util::now());
    assert!(expires > initial, "az új kör friss órát kap");
    assert!(expires <= util::now() + 95.0);
}

#[tokio::test(flavor = "multi_thread")]
async fn exchange_with_timer_sends_the_new_turns_expiry() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2, "turn_time_limit": 60})).await;
    let cur = current(&[&a, &b]);
    let events = cur.call("exchange_tiles", json!({"indices": [0]})).await;
    assert!(action_ok(&events), "{events:?}");
    assert!(last_state(&events)["turn_timer_expires_at"].is_f64());
}

#[tokio::test(flavor = "multi_thread")]
async fn place_tiles_with_timer_sends_the_next_turns_expiry() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2, "turn_time_limit": 60})).await;
    let cur = current(&[&a, &b]);
    set_hand(&server, &format!("Jatekos{}", cur.name_n), &["A", "Z", "E", "T", "K", "L", "R"]);
    let events = cur.call("place_tiles", word_tiles(7, 7, &["A", "Z"])).await;
    assert!(action_ok(&events), "{events:?}");
    let state = last_state(&events);
    assert!(state["turn_timer_expires_at"].as_f64().is_some_and(|t| t > util::now()), "{state}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_expiry_is_in_the_future_and_about_a_turn_long() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2, "turn_time_limit": 60})).await;
    let before = util::now();
    let cur = current(&[&a, &b]);
    let state = last_state(&cur.call("pass_turn", Value::Null).await);
    let expires = state["turn_timer_expires_at"].as_f64().unwrap();
    assert!(expires > before);
    assert!(expires <= before + 65.0);
}

async fn withdraw_leaves(left: f64, expected: f64) {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2, "turn_time_limit": 60, "challenge_mode": true})).await;
    let placer = current(&[&a, &b]);
    let name = format!("Jatekos{}", placer.name_n);
    set_hand(&server, &name, &["A", "B", "C", "D", "E", "F", "G"]);
    // a kör nagy része már eltelt
    with_room(&server, |room| room.turn_timer_expires_at = Some(util::now() + left));
    let events = placer.call("place_tiles", word_tiles(7, 6, &["A", "B"])).await;
    assert!(action_ok(&events), "{events:?}");
    assert!(with_room(&server, |room| room.game.pending_challenge.is_some()));
    let events = placer.call("withdraw_words", Value::Null).await;
    assert!(with_room(&server, |room| room.game.pending_challenge.is_none()));
    let remaining = last_state(&events)["turn_timer_expires_at"].as_f64().expect("az óra folytatódik") - util::now();
    assert!(remaining > expected - 2.0 && remaining <= expected + 0.5, "hátralévő idő: {remaining}, várt: {expected}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_withdrawn_placement_continues_the_remaining_time() {
    withdraw_leaves(25.0, 25.0).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_withdrawn_placement_leaves_at_least_ten_seconds() {
    withdraw_leaves(2.0, 10.0).await;
}

// ===================================================================================================
// A lépésnapló és a visszajátszás tartós mentése
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn pass_moves_are_recorded_in_the_move_log() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2})).await;
    assert_eq!(with_room(&server, |room| room.game.move_log.len()), 0);
    let cur = current(&[&a, &b]);
    cur.call("pass_turn", Value::Null).await;
    let (count, kind, who) = with_room(&server, |room| {
        let m = &room.game.move_log[0];
        (room.game.move_log.len(), m.action_type.clone(), m.player_name.clone())
    });
    assert_eq!(count, 1);
    assert_eq!(kind, "pass");
    assert_eq!(who, format!("Jatekos{}", cur.name_n));
}

#[tokio::test(flavor = "multi_thread")]
async fn six_passes_end_the_game_and_the_moves_are_in_the_database() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2})).await;
    let first = current(&[&a, &b]).name_n;
    let second = if first == 1 { 2 } else { 1 };
    six_passes(&server, &a, &b).await;
    assert!(finished(&server), "6 passz után a játék véget ér");
    let id = db_game_id(&server).expect("a játék mentve van");
    let moves = server.app.db.get_game_moves(id).unwrap();
    assert_eq!(moves.len(), 6);
    assert!(moves.iter().all(|m| m.action_type == "pass"));
    let names: Vec<String> = moves.iter().map(|m| m.player_name.clone()).collect();
    let expected: Vec<String> = (0..6).map(|i| format!("Jatekos{}", if i % 2 == 0 { first } else { second })).collect();
    assert_eq!(names, expected);
    assert_eq!(moves.iter().map(|m| m.move_number).collect::<Vec<_>>(), vec![1, 2, 3, 4, 5, 6]);
}

#[tokio::test(flavor = "multi_thread")]
async fn board_snapshots_are_saved_with_the_moves() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2})).await;
    six_passes(&server, &a, &b).await;
    let moves = server.app.db.get_game_moves(db_game_id(&server).unwrap()).unwrap();
    assert_eq!(moves.len(), 6);
    for m in &moves {
        let snapshot: Value = serde_json::from_str(m.board_snapshot_json.as_deref().expect("van pillanatkép")).unwrap();
        assert!(snapshot.is_array(), "a pillanatkép lista");
        assert_eq!(snapshot.as_array().unwrap().len(), 15);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_placed_word_is_recorded_with_its_tiles_and_saved() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2})).await;
    let cur = current(&[&a, &b]);
    set_hand(&server, &format!("Jatekos{}", cur.name_n), &["A", "Z", "E", "T", "K", "L", "R"]);
    let events = cur.call("place_tiles", word_tiles(7, 7, &["A", "Z"])).await;
    assert!(action_ok(&events), "{events:?}");
    a.take();
    b.take();
    six_passes(&server, &a, &b).await;
    assert!(finished(&server));
    let moves = server.app.db.get_game_moves(db_game_id(&server).unwrap()).unwrap();
    let kinds: Vec<&str> = moves.iter().map(|m| m.action_type.as_str()).collect();
    assert!(kinds.contains(&"place"), "{kinds:?}");
    let place = moves.iter().find(|m| m.action_type == "place").unwrap();
    let details: Value = serde_json::from_str(place.details_json.as_deref().unwrap()).unwrap();
    assert!(details["tiles"].is_array(), "{details}");
    assert_eq!(details["tiles"].as_array().unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn challenge_mode_accepted_move_is_logged_and_saved_with_the_game() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2, "challenge_mode": true})).await;
    let placer = current(&[&a, &b]);
    let other = others(&[&a, &b], placer)[0];
    set_hand(&server, &format!("Jatekos{}", placer.name_n), &["A", "Z", "E", "T", "K", "L", "R"]);
    placer.call("place_tiles", word_tiles(7, 7, &["A", "Z"])).await;
    assert!(with_room(&server, |room| room.game.pending_challenge.is_some()));
    other.call("accept_words", Value::Null).await;
    // a lépésnaplóban azonnal ott van, az adatbázisba a mentéssel kerül (mint a Pythonban)
    let kinds = with_room(&server, |room| room.game.move_log.iter().map(|m| m.action_type.clone()).collect::<Vec<_>>());
    assert_eq!(kinds, vec!["challenge_accept"]);
    let id = db_game_id(&server).expect("a játék a kezdetekor mentve van");
    assert!(server.app.db.get_game_moves(id).unwrap().is_empty());
    // a tulajdonos kézi mentése a naplót is az adatbázisba írja
    a.call("save_game", Value::Null).await;
    let moves = server.app.db.get_game_moves(id).unwrap();
    assert_eq!(moves.iter().map(|m| m.action_type.as_str()).collect::<Vec<_>>(), vec!["challenge_accept"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_initial_save_has_no_moves() {
    let server = TestServer::start().await;
    let (_a, _b) = started_pair(&server, json!({"name": "R", "max_players": 2})).await;
    let id = db_game_id(&server).expect("az induláskor mentés készül");
    assert!(server.app.db.get_game_moves(id).unwrap().is_empty());
}

// ===================================================================================================
// Játék vége → előzmények és statisztika
// ===================================================================================================

/// Két regisztrált játékos szobája (Jatekos1 a tulajdonos), elindítva.
async fn registered_pair(server: &TestServer, options: Value) -> (Sio, Sio) {
    started_pair(server, options).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_registered_players_finished_game_appears_in_the_history() {
    let server = TestServer::start().await;
    let (a, b) = registered_pair(&server, json!({"name": "R", "max_players": 2})).await;
    six_passes(&server, &a, &b).await;
    assert!(finished(&server));
    let uid = server.user_id("u1@example.com");
    let history = server.app.db.get_user_game_history(uid, 20).unwrap();
    assert_eq!(history.len(), 1, "a befejezett játék az előzményekben van");
    let score = with_room(&server, |room| room.game.players.iter().find(|p| p.name == "Jatekos1").unwrap().score);
    assert_eq!(history[0]["final_score"], json!(score));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_profile_api_returns_the_history_after_a_game() {
    let server = TestServer::start().await;
    let (a, b) = registered_pair(&server, json!({"name": "R", "max_players": 2})).await;
    six_passes(&server, &a, &b).await;
    assert!(finished(&server));
    let http = server.http_as("u1@example.com").await;
    let resp = http.get("/api/auth/profile").await;
    assert_eq!(resp.status, 200);
    let data = resp.json();
    assert_eq!(data["success"], json!(true));
    assert_eq!(data["history"].as_array().unwrap().len(), 1);
    assert_eq!(data["history"][0]["game_id"], json!(db_game_id(&server).unwrap()));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_challenge_mode_game_ending_by_accept_is_saved() {
    let server = TestServer::start().await;
    let (a, b) = registered_pair(&server, json!({"name": "R", "max_players": 2, "challenge_mode": true})).await;
    let placer = current(&[&a, &b]);
    let other = others(&[&a, &b], placer)[0];
    // a zsák üres és a lerakónak két zsetonja van: a lerakás elfogadása után vége a játéknak
    with_room(&server, |room| room.game.bag.tiles.clear());
    set_hand(&server, &format!("Jatekos{}", placer.name_n), &["A", "B"]);
    placer.call("place_tiles", word_tiles(7, 6, &["A", "B"])).await;
    assert!(with_room(&server, |room| room.game.pending_challenge.is_some()), "a lerakás szavazásra vár");
    other.call("accept_words", Value::Null).await;
    a.settle().await;
    assert!(finished(&server), "az utolsó zsetonok elfogadása lezárja a játékot");
    let history = server.app.db.get_user_game_history(server.user_id(&format!("u{}@example.com", placer.name_n)), 20).unwrap();
    assert_eq!(history.len(), 1, "a szavazással végződő játék is mentődik");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_challenge_mode_game_ending_by_passes_is_saved() {
    let server = TestServer::start().await;
    let (a, b) = registered_pair(&server, json!({"name": "R", "max_players": 2, "challenge_mode": true})).await;
    six_passes(&server, &a, &b).await;
    assert!(finished(&server));
    assert_eq!(server.app.db.get_user_game_history(server.user_id("u1@example.com"), 20).unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guests_game_is_not_in_a_registered_users_history() {
    let server = TestServer::start().await;
    let bystander = server.create_user("hist4@example.com", "Hist4");
    let a = server.guest("Vendeg1").await;
    let b = server.guest("Vendeg2").await;
    a.emit("create_room", json!({"name": "R", "max_players": 2}));
    a.settle().await;
    b.emit("join_room", json!({"code": a.code()}));
    b.settle().await;
    a.emit0("start_game");
    a.settle_long().await;
    a.take();
    b.take();
    six_passes(&server, &a, &b).await;
    assert!(finished(&server));
    assert!(server.app.db.get_user_game_history(bystander, 20).unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn games_played_is_updated_after_the_game() {
    let server = TestServer::start().await;
    let (a, b) = registered_pair(&server, json!({"name": "R", "max_players": 2})).await;
    six_passes(&server, &a, &b).await;
    assert!(finished(&server));
    let user = server.app.db.get_user_by_id(server.user_id("u1@example.com")).unwrap().unwrap();
    assert_eq!(user.games_played, 1);
}

// ===================================================================================================
// A játék minden lehetséges vége elmenti a játékot
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_sixth_scoreless_turn_by_exchange_ends_and_saves_the_game() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2})).await;
    with_room(&server, |room| room.game.scoreless_turns = 5);
    let cur = current(&[&a, &b]);
    let events = cur.call("exchange_tiles", json!({"indices": [0]})).await;
    assert!(action_ok(&events), "{events:?}");
    assert!(finished(&server), "a hatodik pont nélküli kör (egy csere) lezárja a játékot");
    let id = db_game_id(&server).expect("mentve");
    let moves = server.app.db.get_game_moves(id).unwrap();
    assert_eq!(moves.iter().map(|m| m.action_type.as_str()).collect::<Vec<_>>(), vec!["exchange"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_non_challenge_placement_emptying_the_hand_ends_and_saves_the_game() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2})).await;
    let cur = current(&[&a, &b]);
    with_room(&server, |room| room.game.bag.tiles.clear());
    set_hand(&server, &format!("Jatekos{}", cur.name_n), &["A", "Z"]);
    let events = cur.call("place_tiles", word_tiles(7, 7, &["A", "Z"])).await;
    assert!(action_ok(&events), "{events:?}");
    assert!(with_room(&server, |room| room.game.pending_challenge.is_none()), "nincs megtámadás");
    assert!(finished(&server), "üres kéz és üres zsák: vége");
    let moves = server.app.db.get_game_moves(db_game_id(&server).expect("mentve")).unwrap();
    assert!(moves.iter().any(|m| m.action_type == "place"), "a záró lerakás a visszajátszásban van");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_challenge_timeout_auto_accept_can_end_and_save_the_game() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2, "challenge_mode": true})).await;
    let cur = current(&[&a, &b]);
    with_room(&server, |room| room.game.bag.tiles.clear());
    set_hand(&server, &format!("Jatekos{}", cur.name_n), &["A", "B"]);
    cur.call("place_tiles", word_tiles(7, 6, &["A", "B"])).await;
    assert!(with_room(&server, |room| room.game.pending_challenge.is_some()));
    let timer_id = with_room(&server, |room| room.challenge_timer_id());
    // a visszaszámlálás lejárata (a várakozás nélkül)
    core::challenge_timeout(&server.app, &room_id(&server), timer_id);
    assert!(finished(&server), "az automatikus elfogadás lezárja a játékot");
    let moves = server.app.db.get_game_moves(db_game_id(&server).expect("mentve")).unwrap();
    assert!(moves.iter().any(|m| m.action_type == "challenge_accept"), "{:?}", moves.iter().map(|m| &m.action_type).collect::<Vec<_>>());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stale_challenge_timer_does_nothing() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2, "challenge_mode": true})).await;
    let cur = current(&[&a, &b]);
    set_hand(&server, &format!("Jatekos{}", cur.name_n), &["A", "B", "C", "D", "E", "F", "G"]);
    cur.call("place_tiles", word_tiles(7, 6, &["A", "B"])).await;
    let timer_id = with_room(&server, |room| room.challenge_timer_id());
    core::challenge_timeout(&server.app, &room_id(&server), timer_id + 1);
    assert!(with_room(&server, |room| room.game.pending_challenge.is_some()), "a régi időzítő nem fogad el semmit");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_turn_timeout_auto_pass_can_end_and_save_the_game() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2, "turn_time_limit": 60})).await;
    // már öt pont nélküli kör volt: az automatikus passz a hatodik
    with_room(&server, |room| room.game.scoreless_turns = 5);
    let timer_id = with_room(&server, |room| room.turn_timer_id());
    core::turn_timeout(&server.app, &room_id(&server), timer_id);
    assert!(finished(&server), "az automatikus passz lezárja a játékot");
    let moves = server.app.db.get_game_moves(db_game_id(&server).expect("mentve")).unwrap();
    assert!(moves.iter().any(|m| m.action_type == "pass"), "az automatikus passz a visszajátszásban van");
    let events = a.take();
    let message = events.iter().find(|(n, d)| n == "action_result" && d["message"].as_str().is_some_and(|m| m.starts_with("Időtúllépés"))).map(|(_, d)| d["message"].clone());
    assert!(message.is_some(), "az időtúllépés üzenete kiment: {:?}", event_names(&events));
    let _ = b;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_turn_timeout_passes_for_the_current_player_and_starts_the_next_clock() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2, "turn_time_limit": 60})).await;
    let cur = current(&[&a, &b]);
    let timer_id = with_room(&server, |room| room.turn_timer_id());
    core::turn_timeout(&server.app, &room_id(&server), timer_id);
    cur.settle().await;
    assert!(!finished(&server));
    assert!(!cur.my_turn() || cur.state()["current_player"] != json!(cur.my_id()), "a kör továbbadódott");
    let state = cur.state();
    assert!(state["turn_timer_expires_at"].as_f64().is_some_and(|t| t > util::now()), "az új körnek új órája van");
    assert_eq!(with_room(&server, |room| room.game.scoreless_turns), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stale_turn_timer_does_nothing() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2, "turn_time_limit": 60})).await;
    let timer_id = with_room(&server, |room| room.turn_timer_id());
    // a kör közben lejár a régi időzítő: új kör, új azonosító
    current(&[&a, &b]).call("pass_turn", Value::Null).await;
    core::turn_timeout(&server.app, &room_id(&server), timer_id);
    assert_eq!(with_room(&server, |room| room.game.scoreless_turns), 1, "a régi időzítő nem passzol újra");
}
