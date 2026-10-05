//! Levelezős (aszinkron) játékmód: játéklogika, szerver, mentés, határidők, visszaállítás, lista (a Python
//! `test_async_games.py` megfelelője).

mod common;
use common::push::PushSink;
use common::*;
use rand::SeedableRng;
use rand::rngs::StdRng;
use scrabble::async_games;
use scrabble::db::RowExt;
use scrabble::game::{Game, MAX_TIMEOUTS};
use scrabble::server::core;
use scrabble::util;
use serde_json::{Value, json};

const ALMA_HAND: [&str; 7] = ["A", "L", "M", "A", "K", "Ö", "R"];

fn alma() -> Value {
    word_tiles(7, 6, &["A", "L", "M", "A"])
}

// ===================================================================================================
// Játéklogika
// ===================================================================================================

fn game_of(names: &[&str], hours: i64) -> Game {
    let mut g = Game::with_defaults("room1");
    g.async_mode = true;
    g.turn_hours = hours;
    for (i, name) in names.iter().enumerate() {
        g.add_player(&format!("p{i}"), name).unwrap();
    }
    g.start().unwrap();
    g.reset_deadline();
    g
}

fn game() -> Game {
    game_of(&["Anna", "Béla"], 24)
}

fn current_name_of(g: &Game) -> String {
    g.current_player().unwrap().name.clone()
}

#[test]
fn build_game_sets_up_the_players_and_the_state() {
    let friends = vec![(20, "Béla".to_string()), (30, "Cili".to_string())];
    let mut rng = StdRng::seed_from_u64(1);
    let (game, known) = async_games::build_game("r1", "sid-anna", "Anna", 10, &friends, 48, &mut rng);
    assert_eq!(game.players.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Anna", "Béla", "Cili"]);
    assert_eq!((known["Anna"], known["Béla"], known["Cili"]), (10, 20, 30));
    assert_eq!(game.players.iter().map(|p| p.disconnected).collect::<Vec<_>>(), vec![false, true, true]);
    assert_eq!(game.players[1].id, async_games::placeholder_id("r1", 20));
    assert!(game.started && game.async_mode);
    assert_eq!(game.turn_hours, 48);
    let deadline = game.turn_deadline.expect("határidő");
    assert!((deadline - (util::now() + 48.0 * 3600.0)).abs() < 5.0);
    assert!(game.players.iter().all(|p| p.hand.len() == 7));
    assert!(!game.challenge_mode);
    assert_eq!(game.hint_limit, 0);
}

#[test]
fn the_starting_player_is_random() {
    let mut starters = std::collections::HashSet::new();
    for seed in 0..30 {
        let mut rng = StdRng::seed_from_u64(seed);
        let (game, _) = async_games::build_game("r", "s", "A", 1, &[(2, "B".to_string())], 24, &mut rng);
        starters.insert(current_name_of(&game));
    }
    assert_eq!(starters, ["A".to_string(), "B".to_string()].into_iter().collect());
}

#[test]
fn duplicate_names_are_made_unique() {
    let mut rng = StdRng::seed_from_u64(1);
    let friends = vec![(2, "anna".to_string()), (3, "Anna".to_string())];
    let (game, known) = async_games::build_game("r", "s", "Anna", 1, &friends, 24, &mut rng);
    let lower: std::collections::HashSet<String> = game.players.iter().map(|p| p.name.to_lowercase()).collect();
    assert_eq!(lower.len(), 3);
    let mut ids: Vec<i64> = known.values().copied().collect();
    ids.sort();
    assert_eq!(ids, vec![1, 2, 3]);
}

#[test]
fn the_unique_name_helper() {
    let taken = |names: &[&str]| -> Vec<String> { names.iter().map(|s| s.to_string()).collect() };
    assert_eq!(async_games::unique_name("Béla", &taken(&["Anna"])), "Béla");
    assert_eq!(async_games::unique_name("Anna", &taken(&["anna"])), "Anna (2)");
    assert_eq!(async_games::unique_name("Anna", &taken(&["Anna", "Anna (2)"])), "Anna (3)");
}

#[test]
fn offline_players_are_not_skipped() {
    let mut g = game_of(&["Anna", "Béla", "Cili"], 24);
    g.players[1].disconnected = true;
    g.pass_turn("p0", false).unwrap();
    assert_eq!(current_name_of(&g), "Béla"); // élő játékban átugornánk
}

#[test]
fn live_games_still_skip_disconnected_players() {
    let mut g = Game::with_defaults("live");
    for (i, name) in ["Anna", "Béla", "Cili"].iter().enumerate() {
        g.add_player(&format!("p{i}"), name).unwrap();
    }
    g.start().unwrap();
    g.players[1].disconnected = true;
    g.pass_turn("p0", false).unwrap();
    assert_eq!(current_name_of(&g), "Cili");
}

#[test]
fn skipping_the_disconnected_current_player_is_a_noop_in_async_games() {
    let mut g = game();
    g.players[0].disconnected = true;
    assert!(!g.skip_disconnected_current());
    assert_eq!(current_name_of(&g), "Anna");
}

#[test]
fn the_deadline_restarts_every_turn() {
    let mut g = game_of(&["Anna", "Béla"], 24);
    let first = g.turn_deadline.unwrap();
    g.turn_deadline = Some(first - 3600.0);
    g.pass_turn("p0", false).unwrap();
    let now_deadline = g.turn_deadline.unwrap();
    assert!(now_deadline > first - 3600.0);
    assert!((now_deadline - (util::now() + 24.0 * 3600.0)).abs() < 5.0);
}

#[test]
fn live_games_have_no_deadline() {
    let mut g = Game::with_defaults("live");
    g.add_player("p", "A").unwrap();
    g.start().unwrap();
    assert!(g.turn_deadline.is_none());
    assert_eq!(g.get_state(Some("p"))["async_mode"], false);
}

#[test]
fn the_state_exposes_the_deadline() {
    let g = game_of(&["Anna", "Béla"], 72);
    let state = g.get_state(Some("p0"));
    assert_eq!(state["async_mode"], true);
    assert_eq!(state["turn_hours"], 72);
    assert_eq!(state["turn_deadline"], json!(g.turn_deadline));
}

#[test]
fn a_timeout_passes_and_is_logged() {
    let mut g = game();
    assert!(g.expire_turn());
    assert_eq!(current_name_of(&g), "Béla");
    assert_eq!(g.players[0].timeouts, 1);
    assert_eq!(g.last_action_info, Some(json!({"type": "timeout", "player": "Anna"})));
    assert_eq!(g.move_log.last().unwrap().action_type, "pass");
}

#[test]
fn own_moves_reset_the_timeout_counter() {
    let mut g = game();
    g.expire_turn(); // Anna 1
    g.pass_turn("p1", false).unwrap();
    g.pass_turn("p0", false).unwrap(); // saját passz nullázza
    assert_eq!(g.players[0].timeouts, 0);
}

#[test]
fn three_timeouts_in_a_row_forfeit_the_game() {
    let mut g = game_of(&["Anna", "Béla", "Cili"], 24);
    for _ in 0..MAX_TIMEOUTS {
        assert!(!g.finished);
        g.current_player_idx = 0;
        g.expire_turn();
    }
    assert!(g.finished && g.players[0].resigned);
    assert!(!g.winners.iter().any(|p| p.name == "Anna"));
}

#[test]
fn expiring_is_a_noop_outside_async_games_and_after_the_end() {
    let mut live = Game::with_defaults("live");
    live.add_player("p", "A").unwrap();
    live.start().unwrap();
    assert!(!live.expire_turn());
    let mut g = game();
    g.resign("p0").unwrap();
    assert!(!g.expire_turn());
}

#[test]
fn timeouts_end_a_dead_game_through_the_scoreless_rule() {
    let mut g = game();
    for _ in 0..6 {
        if g.finished {
            break;
        }
        g.expire_turn();
    }
    assert!(g.finished);
}

#[test]
fn a_resigner_never_wins() {
    let mut g = game();
    g.players[0].score = 200;
    g.players[1].score = 10;
    g.players[0].hand.clear();
    g.players[1].hand.clear();
    g.resign("p0").unwrap();
    assert!(g.finished);
    assert_eq!(g.winners.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Béla"]);
    assert_eq!(g.last_action_info, Some(json!({"type": "resigned", "player": "Anna"})));
}

#[test]
fn the_best_remaining_player_wins_a_multiplayer_game() {
    let mut g = game_of(&["Anna", "Béla", "Cili"], 24);
    for (p, s) in g.players.iter_mut().zip([5, 50, 30]) {
        p.score = s;
        p.hand.clear();
    }
    g.resign("p1").unwrap();
    assert_eq!(g.winners.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Cili"]);
}

#[test]
fn resign_errors() {
    let mut g = game();
    assert!(g.resign("nincs").is_err());
    g.resign("p0").unwrap();
    assert_eq!(g.resign("p1").unwrap_err(), "A játék véget ért.");
    let mut fresh = Game::with_defaults("x");
    fresh.add_player("p", "A").unwrap();
    assert!(fresh.resign("p").is_err());
}

#[test]
fn the_state_marks_the_resigned_player() {
    let mut g = game();
    g.resign("p0").unwrap();
    assert_eq!(g.get_state(Some("p1"))["players"][0]["resigned"], true);
}

#[test]
fn async_fields_survive_a_save() {
    let mut g = game_of(&["Anna", "Béla"], 72);
    g.players[0].timeouts = 2;
    let saved: Value = serde_json::from_str(&g.to_save_dict().to_string()).unwrap();
    let restored = Game::from_save_dict(&saved).unwrap();
    assert!(restored.async_mode);
    assert_eq!(restored.turn_hours, 72);
    assert_eq!(restored.turn_deadline, g.turn_deadline);
    assert_eq!(restored.players[0].timeouts, 2);
}

#[test]
fn the_resigned_flag_survives_a_save() {
    let mut g = game();
    g.resign("p0").unwrap();
    let restored = Game::from_save_dict(&g.to_save_dict()).unwrap();
    assert!(restored.players[0].resigned);
    assert_eq!(restored.winners.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Béla"]);
}

#[test]
fn old_saves_are_not_async() {
    let mut data = game().to_save_dict();
    for key in ["async_mode", "turn_hours", "turn_deadline"] {
        data.as_object_mut().unwrap().remove(key);
    }
    let restored = Game::from_save_dict(&data).unwrap();
    assert!(!restored.async_mode);
    assert!(restored.turn_deadline.is_none());
}

// ===================================================================================================
// Szerver
// ===================================================================================================

fn befriend(server: &TestServer, a: i64, b: i64) {
    assert!(server.app.db.send_friend_request(a, b).0);
    assert!(server.app.db.accept_friend_request(b, a).0);
}

fn events_named(events: &[(String, Value)], name: &str) -> Vec<Value> {
    events.iter().filter(|(n, _)| n == name).map(|(_, d)| d.clone()).collect()
}

fn error_messages(events: &[(String, Value)]) -> Vec<String> {
    events_named(events, "error").iter().filter_map(|d| d["message"].as_str().map(|s| s.to_string())).collect()
}

fn has_error(events: &[(String, Value)], fragment: &str) -> bool {
    error_messages(events).iter().any(|m| m.contains(fragment))
}

fn names(events: &[(String, Value)]) -> Vec<&str> {
    events.iter().map(|(n, _)| n.as_str()).collect()
}

fn subscribe(server: &TestServer, user_id: i64, sink: &PushSink, path: &str) {
    let sub = sink.subscription(path);
    server.app.db.save_push_subscription(user_id, &sub.endpoint, &sub.p256dh, &sub.auth, "hu").unwrap();
}

struct Pair {
    a: Sio,
    a_id: i64,
    b: Sio,
    b_id: i64,
}

/// Két barát; az első létrehoz egy levelezős játékot a másodikkal (az első a kezdő).
async fn setup_with(server: &TestServer, hours: i64) -> Pair {
    *server.app.async_starter.lock() = Some(0);
    let a = server.player(1).await;
    let b = server.player(2).await;
    let (a_id, b_id) = (server.user_id("u1@example.com"), server.user_id("u2@example.com"));
    befriend(server, a_id, b_id);
    a.emit("create_async_game", json!({"name": "Esti játék", "friend_ids": [b_id], "turn_hours": hours}));
    a.settle_long().await;
    a.take();
    b.take();
    Pair { a, a_id, b, b_id }
}

async fn setup(server: &TestServer) -> Pair {
    setup_with(server, 24).await
}

fn room_ids(server: &TestServer) -> Vec<String> {
    server.app.state.lock().rooms.keys().cloned().collect()
}

fn db_game_id(server: &TestServer) -> i64 {
    with_room(server, |room| room.db_game_id).expect("a levelezős játék mentve van")
}

fn saved_state(server: &TestServer) -> Value {
    let row = server.app.db.get_game_by_id(db_game_id(server)).unwrap().unwrap();
    serde_json::from_str(&row.text("state_json")).unwrap()
}

fn move_count(server: &TestServer) -> usize {
    server.app.db.get_game_moves(db_game_id(server)).unwrap().len()
}

fn action_ok(events: &[(String, Value)]) -> bool {
    find(events, "action_result").is_some_and(|d| d["success"] == json!(true))
}

fn current_game_name(server: &TestServer) -> String {
    with_room(server, |room| current_name_of(&room.game))
}

// --- létrehozás ---

#[tokio::test(flavor = "multi_thread")]
async fn creating_persists_and_invites() {
    let server = TestServer::start().await;
    let sink = PushSink::start();
    *server.app.async_starter.lock() = Some(0);
    let a = server.player(1).await;
    let b = server.player(2).await;
    let (a_id, b_id) = (server.user_id("u1@example.com"), server.user_id("u2@example.com"));
    befriend(&server, a_id, b_id);
    subscribe(&server, b_id, &sink, "send/abcdef123456");
    a.emit("create_async_game", json!({"name": "Esti játék", "friend_ids": [b_id], "turn_hours": 72}));
    a.settle_long().await;
    let got = a.take();
    let joined = find(&got, "room_joined").unwrap();
    assert_eq!(joined["is_async"], true);
    let state = find(&got, "game_state").unwrap();
    assert_eq!(state["async_mode"], true);
    assert_eq!(state["turn_hours"], 72);
    let (is_async, is_private, db_id) = with_room(&server, |room| (room.is_async, room.is_private, room.db_game_id));
    assert!(is_async && is_private);
    let db_id = db_id.expect("mentve");
    // tartósan mentve, mindkét játékossal
    let row = server.app.db.get_game_by_id(db_id).unwrap().unwrap();
    assert_eq!(row.int("is_async"), 1);
    assert_eq!(row.text("status"), "active");
    let mut users: Vec<i64> = server.app.db.get_game_players(db_id).unwrap().iter().filter_map(|(_, u, _)| *u).collect();
    users.sort();
    assert_eq!(users, vec![a_id, b_id]);
    // a meghívott értesítést kap (a kezdő játékos a létrehozó, ezért meghívót, nem „te jössz”-öt)
    b.settle().await;
    let invited = events_named(&b.take(), "async_invited");
    assert_eq!(invited, vec![json!({"game_id": db_id, "room_name": "Esti játék", "from_name": "Jatekos1"})]);
    let received = sink.wait_for(1, 5).await;
    assert_eq!(received.len(), 1, "{received:?}");
    assert_eq!(sink.bodies(), vec!["Meghívtak egy levelezős játékba: Esti játék".to_string()]);
    assert!(server.app.state.lock().get_rooms_list().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_friend_who_starts_gets_the_turn_notification_only() {
    let server = TestServer::start().await;
    let sink = PushSink::start();
    *server.app.async_starter.lock() = Some(1);
    let a = server.player(1).await;
    let b = server.player(2).await;
    let (a_id, b_id) = (server.user_id("u1@example.com"), server.user_id("u2@example.com"));
    befriend(&server, a_id, b_id);
    subscribe(&server, b_id, &sink, "send/abcdef123456");
    a.emit("create_async_game", json!({"name": "Esti", "friend_ids": [b_id]}));
    a.settle_long().await;
    sink.wait_for(1, 5).await;
    assert_eq!(sink.bodies(), vec!["Te jössz! · Esti".to_string()]);
    b.settle().await;
    let turn = events_named(&b.take(), "async_your_turn");
    assert_eq!(turn.len(), 1);
    assert_eq!(turn[0]["room_name"], "Esti");
}

#[tokio::test(flavor = "multi_thread")]
async fn creation_validation() {
    let payloads = [
        json!({"friend_ids": []}),
        json!({"friend_ids": "x"}),
        json!({}),
        json!({"friend_ids": [1, 1]}),
        json!({"friend_ids": [true]}),
        json!({"friend_ids": [2, 3, 4, 5]}),
        json!({"friend_ids": [999]}),
    ];
    let server = TestServer::start().await;
    let a = server.player(1).await;
    server.player(2).await;
    relax_socket_limits(&server.app, &["create_async_game"]);
    for payload in payloads {
        let mut data = json!({"name": "X"});
        for (k, v) in payload.as_object().unwrap() {
            data[k] = v.clone();
        }
        let got = a.call("create_async_game", data.clone()).await;
        assert!(!error_messages(&got).is_empty(), "{data}: {got:?}");
        assert_eq!(room_count(&server), 0, "{data}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn only_friends_can_be_invited() {
    let server = TestServer::start().await;
    let a = server.player(1).await;
    server.player(2).await;
    let b_id = server.user_id("u2@example.com");
    let got = a.call("create_async_game", json!({"name": "X", "friend_ids": [b_id]})).await;
    assert!(has_error(&got, "barátaidat"), "{got:?}");
    assert_eq!(room_count(&server), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn guests_cannot_create() {
    let server = TestServer::start().await;
    let guest = server.guest("Vendég").await;
    let got = guest.call("create_async_game", json!({"name": "X", "friend_ids": [1]})).await;
    assert!(has_error(&got, "regisztrált"), "{got:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_turn_hours_fall_back_to_the_default() {
    let server = TestServer::start().await;
    let _pair = setup_with(&server, 5).await;
    assert_eq!(with_room(&server, |room| room.game.turn_hours), async_games::DEFAULT_TURN_HOURS);
}

#[tokio::test(flavor = "multi_thread")]
async fn cannot_create_while_in_a_room() {
    let server = TestServer::start().await;
    let p = setup(&server).await;
    let got = p.a.call("create_async_game", json!({"name": "X", "friend_ids": [p.b_id]})).await;
    assert!(has_error(&got, "szobában"), "{got:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_feature_switch_blocks_creation() {
    let server = TestServer::start().await;
    server.app.db.with(|tx| server.app.settings.store(tx, "feature_async", &json!(false), None).map(|_| ()).map_err(|_| rusqlite::Error::InvalidQuery)).unwrap();
    let a = server.player(1).await;
    let b = server.player(2).await;
    let (a_id, b_id) = (server.user_id("u1@example.com"), server.user_id("u2@example.com"));
    befriend(&server, a_id, b_id);
    let got = a.call("create_async_game", json!({"name": "X", "friend_ids": [b_id]})).await;
    assert!(!error_messages(&got).is_empty(), "{got:?}");
    assert_eq!(room_count(&server), 0);
    let _ = b;
}

// --- játék ---

#[tokio::test(flavor = "multi_thread")]
async fn the_game_survives_and_the_other_player_gets_the_turn() {
    let server = TestServer::start().await;
    let p = setup(&server).await;
    let sink = PushSink::start();
    subscribe(&server, p.b_id, &sink, "send/abcdef123456");
    set_hand(&server, "Jatekos1", &ALMA_HAND);
    let got = p.a.call("place_tiles", alma()).await;
    assert!(action_ok(&got), "{got:?}");
    // Béla offline, de sorra kerül (nem ugorjuk át), és értesítést kap
    assert_eq!(current_game_name(&server), "Jatekos2");
    sink.wait_for(1, 5).await;
    assert_eq!(sink.bodies().last().cloned(), Some("Te jössz! · Esti játék".to_string()));
    p.b.settle().await;
    assert!(events_named(&p.b.take(), "async_your_turn").iter().any(|e| e["room_name"] == "Esti játék"));
    // a lépés azonnal mentve van
    let state = saved_state(&server);
    assert_eq!(state["current_player_idx"], 1);
    assert_eq!(state["board"][7][6]["letter"], "A");
    assert_eq!(move_count(&server), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn disconnecting_keeps_the_room_and_the_game() {
    let server = TestServer::start().await;
    let p = setup(&server).await;
    p.a.close();
    p.b.settle().await;
    assert_eq!(room_count(&server), 1);
    let (disconnected, started, finished) = with_room(&server, |room| (room.game.players[0].disconnected, room.game.started, room.game.finished));
    assert!(disconnected && started && !finished);
    assert!(server.app.state.lock().player_rooms.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn leaving_the_view_keeps_the_game() {
    let server = TestServer::start().await;
    let p = setup(&server).await;
    let got = p.a.call("leave_room", Value::Null).await;
    assert!(names(&got).contains(&"room_left"), "{:?}", names(&got));
    assert_eq!(room_count(&server), 1);
    assert!(with_room(&server, |room| room.game.players[0].disconnected));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_player_who_left_the_view_gets_no_game_state_in_the_lobby() {
    // különben a kliense azonnal (és a másik játékos minden lépésénél) visszaugrana a játékba
    let server = TestServer::start().await;
    let p = setup(&server).await;
    set_hand(&server, "Jatekos1", &ALMA_HAND);
    p.a.call("place_tiles", alma()).await;
    let got = p.a.call("leave_room", Value::Null).await;
    assert_eq!(names(&got), vec!["room_left"]);
    let db_id = db_game_id(&server);
    p.b.call("open_async_game", json!({"game_id": db_id})).await;
    p.b.emit0("pass_turn");
    p.b.settle().await;
    let got: Vec<String> = p.a.take().into_iter().map(|(n, _)| n).collect();
    assert!(!got.contains(&"game_state".to_string()), "{got:?}");
    assert!(got.contains(&"async_your_turn".to_string()), "{got:?}");
    let got = p.a.call("open_async_game", json!({"game_id": db_id})).await;
    assert!(names(&got).contains(&"game_state"), "{:?}", names(&got));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_invited_friend_opens_the_game_from_the_list() {
    let server = TestServer::start().await;
    let p = setup(&server).await;
    let got = p.b.call("open_async_game", json!({"game_id": db_game_id(&server)})).await;
    let joined = find(&got, "room_joined").unwrap();
    assert_eq!(joined["is_async"], true);
    assert_eq!(joined["is_owner"], false);
    let state = find(&got, "game_state").unwrap();
    assert_eq!(state["async_mode"], true);
    assert!(names(&got).contains(&"game_started"));
    let me = state["players"].as_array().unwrap().iter().find(|pl| pl["name"] == "Jatekos2").unwrap();
    assert_eq!(me["hand"].as_array().unwrap().len(), 7);
    let (player_id, room_id) = with_room(&server, |room| (room.game.players[1].id.clone(), room.id.clone()));
    assert_eq!(server.app.state.lock().player_rooms.get(&player_id), Some(&room_id));
}

#[tokio::test(flavor = "multi_thread")]
async fn both_play_through_an_exchange_of_moves() {
    let server = TestServer::start().await;
    let p = setup(&server).await;
    set_hand(&server, "Jatekos1", &ALMA_HAND);
    p.a.call("place_tiles", alma()).await;
    p.b.call("open_async_game", json!({"game_id": db_game_id(&server)})).await;
    let got = p.b.call("pass_turn", Value::Null).await;
    assert!(action_ok(&got), "{got:?}");
    assert_eq!(current_game_name(&server), "Jatekos1");
    assert_eq!(move_count(&server), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn another_device_takes_over() {
    let server = TestServer::start().await;
    let p = setup(&server).await;
    let db_id = db_game_id(&server);
    p.b.call("open_async_game", json!({"game_id": db_id})).await;
    let second = Sio::connect(&server).await;
    second.emit("set_name", json!({"name": "Jatekos2", "is_guest": false, "auth_token": server.socket_token(p.b_id)}));
    second.settle().await;
    second.take();
    let got = second.call("open_async_game", json!({"game_id": db_id})).await;
    assert!(names(&got).contains(&"room_joined") && names(&got).contains(&"game_state"), "{:?}", names(&got));
    assert!(names(&p.b.take()).contains(&"room_left"));
}

#[tokio::test(flavor = "multi_thread")]
async fn open_errors() {
    let server = TestServer::start().await;
    let p = setup(&server).await;
    let c = server.player(3).await;
    relax_socket_limits(&server.app, &["open_async_game"]);
    let db_id = db_game_id(&server);
    assert!(has_error(&c.call("open_async_game", json!({"game_id": db_id})).await, "részese"));
    assert!(has_error(&c.call("open_async_game", json!({"game_id": 9999})).await, "nem található"));
    assert!(!error_messages(&c.call("open_async_game", json!({"game_id": "x"})).await).is_empty());
    // már szobában
    assert!(has_error(&p.a.call("open_async_game", json!({"game_id": db_id})).await, "szobában"));
}

#[tokio::test(flavor = "multi_thread")]
async fn hints_are_off() {
    let server = TestServer::start().await;
    let p = setup(&server).await;
    let got = p.a.call("request_hint", Value::Null).await;
    assert_eq!(find(&got, "hint_result").unwrap()["success"], false);
}

// --- újraindítás és betöltés ---

#[tokio::test(flavor = "multi_thread")]
async fn the_game_is_rebuilt_from_the_database() {
    let server = TestServer::start().await;
    let p = setup(&server).await;
    set_hand(&server, "Jatekos1", &ALMA_HAND);
    p.a.call("place_tiles", alma()).await;
    let deadline = with_room(&server, |room| room.game.turn_deadline);
    p.a.close();
    p.b.settle().await;
    let db_id = db_game_id(&server);
    {
        let mut st = server.app.state.lock();
        st.rooms.clear(); // a szerver újraindult
        st.join_codes.clear();
    }
    assert_eq!(core::restore_async_games(&server.app), 1);
    let (rid, is_async, restored_db, all_off, current, dl, letter, log, known, second_id) = with_room(&server, |room| {
        (
            room.id.clone(),
            room.is_async,
            room.db_game_id,
            room.game.players.iter().all(|pl| pl.disconnected),
            current_name_of(&room.game),
            room.game.turn_deadline,
            room.game.board.cells[7][6].map(|c| c.letter.as_str().to_string()),
            room.game.move_log.len(),
            room.known_user_ids.clone(),
            room.game.players[1].id.clone(),
        )
    });
    assert!(is_async);
    assert_eq!(restored_db, Some(db_id));
    assert!(all_off);
    assert_eq!(current, "Jatekos2");
    assert_eq!(dl, deadline);
    assert_eq!(letter.as_deref(), Some("A"));
    assert_eq!(log, 1);
    assert_eq!(known.get("Jatekos1"), Some(&Some(p.a_id)));
    assert_eq!(known.get("Jatekos2"), Some(&Some(p.b_id)));
    assert_eq!(second_id, async_games::placeholder_id(&rid, p.b_id));
}

#[tokio::test(flavor = "multi_thread")]
async fn restoring_twice_does_not_duplicate_rooms() {
    let server = TestServer::start().await;
    let _p = setup(&server).await;
    assert_eq!(core::restore_async_games(&server.app), 1);
    assert_eq!(room_count(&server), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn opening_loads_a_missing_room_on_demand() {
    let server = TestServer::start().await;
    let p = setup(&server).await;
    let db_id = db_game_id(&server);
    p.a.close();
    p.b.settle().await;
    {
        let mut st = server.app.state.lock();
        st.rooms.clear();
        st.join_codes.clear();
    }
    let got = p.b.call("open_async_game", json!({"game_id": db_id})).await;
    assert!(names(&got).contains(&"room_joined") && names(&got).contains(&"game_state"), "{:?} {:?}", names(&got), error_messages(&got));
    assert_eq!(room_count(&server), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn finished_games_are_not_restored() {
    let server = TestServer::start().await;
    let p = setup(&server).await;
    p.a.call("resign_game", Value::Null).await;
    server.app.state.lock().rooms.clear();
    assert_eq!(core::restore_async_games(&server.app), 0);
}

// --- határidők ---

#[tokio::test(flavor = "multi_thread")]
async fn an_expired_turn_passes_and_saves() {
    let server = TestServer::start().await;
    let p = setup(&server).await;
    p.a.close();
    p.b.settle().await;
    with_room(&server, |room| room.game.turn_deadline = Some(util::now() - 1.0));
    assert_eq!(core::expire_async_turns(&server.app, None), 1);
    assert_eq!(current_game_name(&server), "Jatekos2");
    assert_eq!(with_room(&server, |room| room.game.players[0].timeouts), 1);
    let state = saved_state(&server);
    assert_eq!(state["current_player_idx"], 1);
    assert!(state["turn_deadline"].as_f64().unwrap() > util::now());
}

#[tokio::test(flavor = "multi_thread")]
async fn turns_not_yet_expired_are_left_alone() {
    let server = TestServer::start().await;
    let _p = setup(&server).await;
    assert_eq!(core::expire_async_turns(&server.app, None), 0);
    let deadline = with_room(&server, |room| room.game.turn_deadline.unwrap());
    assert_eq!(core::expire_async_turns(&server.app, Some(deadline - 10.0)), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn repeated_timeouts_forfeit_finish_and_clean_up() {
    let server = TestServer::start().await;
    let p = setup(&server).await;
    p.a.close();
    p.b.settle().await;
    let (db_id, room_id) = (db_game_id(&server), room_ids(&server)[0].clone());
    for _ in 0..=MAX_TIMEOUTS {
        let done = server.app.state.lock().rooms.get(&room_id).is_none_or(|r| r.game.finished);
        if done {
            break;
        }
        with_room(&server, |room| {
            room.game.current_player_idx = 0;
            room.game.turn_deadline = Some(util::now() - 1.0);
        });
        core::expire_async_turns(&server.app, None);
    }
    let row = server.app.db.get_game_by_id(db_id).unwrap().unwrap();
    assert_eq!(row.text("status"), "finished");
    assert!(!server.app.state.lock().rooms.contains_key(&room_id), "senki sincs bent: a szoba megszűnik");
    let results = server.app.db.get_game_results(db_id).unwrap();
    let winners: Vec<&str> = results.iter().filter(|r| r["is_winner"] == json!(true)).filter_map(|r| r["player_name"].as_str()).collect();
    assert_eq!(winners, vec!["Jatekos2"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn connected_players_see_the_timeout() {
    let server = TestServer::start().await;
    let p = setup(&server).await;
    with_room(&server, |room| room.game.turn_deadline = Some(util::now() - 1.0));
    core::expire_async_turns(&server.app, None);
    p.a.settle().await;
    let state = p.a.state();
    assert_eq!(state["current_player_name"], "Jatekos2");
    assert_eq!(state["last_action_info"]["type"], "timeout");
}

// --- feladás ---

#[tokio::test(flavor = "multi_thread")]
async fn resigning_ends_and_records_the_result() {
    let server = TestServer::start().await;
    let p = setup(&server).await;
    let db_id = db_game_id(&server);
    let got = p.a.call("resign_game", Value::Null).await;
    assert!(action_ok(&got), "{got:?}");
    let (finished, resigned) = with_room(&server, |room| (room.game.finished, room.game.players[0].resigned));
    assert!(finished && resigned);
    assert_eq!(server.app.db.get_game_by_id(db_id).unwrap().unwrap().text("status"), "finished");
    let results = server.app.db.get_game_results(db_id).unwrap();
    let winners: Vec<&str> = results.iter().filter(|r| r["is_winner"] == json!(true)).filter_map(|r| r["player_name"].as_str()).collect();
    assert_eq!(winners, vec!["Jatekos2"]);
    assert_eq!(server.app.db.get_user_by_id(p.b_id).unwrap().unwrap().games_won, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn resigning_is_only_for_correspondence_games() {
    let server = TestServer::start().await;
    let a = server.player(1).await;
    a.call("create_room", json!({"name": "Élő", "max_players": 2})).await;
    a.call("start_game", Value::Null).await;
    let got = a.call("resign_game", Value::Null).await;
    assert!(!names(&got).contains(&"action_result"), "{:?}", names(&got));
    assert!(!with_room(&server, |room| room.game.finished));
}

// --- listák és útvonalak ---

#[tokio::test(flavor = "multi_thread")]
async fn the_list_marks_whose_turn_it_is() {
    let server = TestServer::start().await;
    let p = setup(&server).await;
    let http = server.http_as("u1@example.com").await;
    let data = http.get("/api/async/games").await.json();
    assert_eq!(data["my_turn_count"], 1);
    assert_eq!(data["games"].as_array().unwrap().len(), 1);
    let game = &data["games"][0];
    assert_eq!(game["my_turn"], true);
    assert_eq!(game["my_name"], "Jatekos1");
    assert_eq!(game["current_player"], "Jatekos1");
    assert_eq!(game["room_name"], "Esti játék");
    assert_eq!(game["turn_hours"], 24);
    let mut players: Vec<&str> = game["players"].as_array().unwrap().iter().filter_map(|pl| pl["name"].as_str()).collect();
    players.sort();
    assert_eq!(players, vec!["Jatekos1", "Jatekos2"]);
    assert_eq!(game["turn_deadline"], json!(with_room(&server, |room| room.game.turn_deadline)));
    assert_eq!(game["game_id"], db_game_id(&server));
    let other = server.http_as("u2@example.com").await;
    assert_eq!(other.get("/api/async/games").await.json()["games"][0]["my_turn"], false);
    let _ = p;
}

#[tokio::test(flavor = "multi_thread")]
async fn games_where_it_is_my_turn_come_first() {
    let server = TestServer::start().await;
    let p = setup(&server).await;
    p.a.call("leave_room", Value::Null).await;
    let c = server.player(3).await;
    let c_id = server.user_id("u3@example.com");
    befriend(&server, p.a_id, c_id);
    p.a.call("create_async_game", json!({"name": "Második", "friend_ids": [c_id]})).await;
    // a másodikban Cili jön
    {
        let mut st = server.app.state.lock();
        let room_id = st.rooms.values().find(|r| r.name == "Második").map(|r| r.id.clone()).expect("második játék");
        let room = st.rooms.get_mut(&room_id).unwrap();
        room.game.current_player_idx = 1;
        room.persisted_sig = None;
        core::emit_all_states(&server.app, &mut st, &room_id);
    }
    let http = server.http_as("u1@example.com").await;
    let games = http.get("/api/async/games").await.json()["games"].clone();
    let order: Vec<&str> = games.as_array().unwrap().iter().filter_map(|g| g["room_name"].as_str()).collect();
    assert_eq!(order, vec!["Esti játék", "Második"]);
    let _ = c;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_list_requires_login_and_hides_finished_and_foreign_games() {
    let server = TestServer::start().await;
    let p = setup(&server).await;
    assert_eq!(server.http().get("/api/async/games").await.status, 401);
    server.ensure_user(3);
    let c = server.http_as("u3@example.com").await;
    assert!(c.get("/api/async/games").await.json()["games"].as_array().unwrap().is_empty());
    p.a.call("resign_game", Value::Null).await;
    let a = server.http_as("u1@example.com").await;
    assert!(a.get("/api/async/games").await.json()["games"].as_array().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn saved_games_and_restore_ignore_correspondence_games() {
    let server = TestServer::start().await;
    let p = setup(&server).await;
    p.a.call("leave_room", Value::Null).await;
    let http = server.http_as("u1@example.com").await;
    assert!(http.get("/api/auth/saved-games").await.json()["games"].as_array().unwrap().is_empty());
    let got = p.a.call("restore_game", json!({"game_id": db_game_id(&server)})).await;
    assert!(has_error(&got, "Levelezős"), "{got:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_abandon_route_refuses_correspondence_games() {
    let server = TestServer::start().await;
    let _p = setup(&server).await;
    let http = server.http_as("u1@example.com").await;
    let resp = http.post(&format!("/api/game/{}/abandon", db_game_id(&server)), Value::Null).await;
    assert_eq!(resp.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn correspondence_games_have_replay_and_ratings_like_any_game() {
    let server = TestServer::start().await;
    let p = setup(&server).await;
    set_hand(&server, "Jatekos1", &ALMA_HAND);
    p.a.call("place_tiles", alma()).await;
    p.a.call("resign_game", Value::Null).await;
    let http = server.http_as("u1@example.com").await;
    let moves = http.get(&format!("/api/game/{}/moves", db_game_id(&server))).await.json();
    assert_eq!(moves["success"], true);
    assert_eq!(moves["finished"], true);
    assert_eq!(moves["moves"].as_array().unwrap().len(), 1);
    let rating = |id: i64| server.app.db.get_user_by_id(id).unwrap().unwrap().rating;
    assert!(rating(p.b_id) > 1200, "{}", rating(p.b_id));
    assert!(rating(p.a_id) < 1200, "{}", rating(p.a_id));
}
