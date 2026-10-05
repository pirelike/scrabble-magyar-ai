//! Robotok a szerveren: szoba létrehozása, lépések, tipp, előnézet, tipp-limit (a Python `test_bots.py` szerver része).
//!
//! A robotok gondolkodási idejét a tesztek a `bot_think_multiplier` beállítással nullára / nagyra állítják, a
//! lépést pedig közvetlenül is meghívják (`play_bot_turn`), mint a Python tesztek.

mod common;
use common::*;
use scrabble::ai::Difficulty;
use scrabble::server::core;
use serde_json::{Value, json};

fn errors(events: &[(String, Value)]) -> Vec<String> {
    events.iter().filter(|(n, _)| n == "error").filter_map(|(_, d)| d["message"].as_str().map(|s| s.to_string())).collect()
}

fn set_setting(server: &TestServer, key: &str, value: Value) {
    server.app.db.with(|tx| server.app.settings.store(tx, key, &value, None).map_err(|e| rusqlite::Error::InvalidParameterName(e.0))).unwrap();
    server.app.apply_runtime_settings();
}

/// A robotok csak hívásra lépnek (a beütemezett lépés a teszt végéig nem jön el).
fn manual_bots(server: &TestServer) {
    set_setting(server, "bot_think_multiplier", json!(5.0));
}

fn levels(server: &TestServer) -> Vec<Option<Difficulty>> {
    with_room(server, |r| r.game.players.iter().skip(1).map(|p| p.difficulty).collect())
}

async fn create(server: &TestServer, owner: &Sio, extra: Value) -> Vec<(String, Value)> {
    let mut opts = json!({"name": "R", "max_players": 4});
    for (k, v) in extra.as_object().unwrap() {
        opts[k] = v.clone();
    }
    let _ = server;
    owner.call("create_room", opts).await
}

async fn solo_started(server: &TestServer, extra: Value) -> Sio {
    let client = server.player(1).await;
    let mut opts = json!({"ai_players": ["hard"]});
    for (k, v) in extra.as_object().unwrap() {
        opts[k] = v.clone();
    }
    create(server, &client, opts).await;
    client.call("start_game", Value::Null).await;
    client
}

async fn play(server: &TestServer, turn_id: Option<u64>, turn_number: Option<i64>) -> bool {
    let room_id = server.app.state.lock().rooms.keys().next().unwrap().clone();
    core::play_bot_turn(server.app.clone(), room_id, turn_id, turn_number).await
}

// ---------------------------------------------------------------------------------------------------
// szoba létrehozása robotokkal
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn rooms_are_created_with_bots() {
    let server = TestServer::start().await;
    manual_bots(&server);
    let c = server.player(1).await;
    create(&server, &c, json!({"ai_players": [10, 2]})).await;
    assert_eq!(with_room(&server, |r| r.game.players.iter().map(|p| p.is_bot).collect::<Vec<_>>()), vec![false, true, true]);
    assert_eq!(levels(&server), vec![Some(Difficulty::Level(10)), Some(Difficulty::Level(2))]);
    let lobby = with_room(&server, |r| r.to_lobby_dict());
    assert_eq!((lobby["bots"].as_i64(), lobby["players"].as_i64()), (Some(2), Some(3)));
}

#[tokio::test(flavor = "multi_thread")]
async fn difficulty_inputs_are_normalised() {
    for (input, expected) in
        [(json!(["hard", "easy"]), vec![10u8, 3]), (json!(["4", "9"]), vec![4, 9]), (json!(["nonsense", "hard", 5, null, 0, 11, true, 2.5, {}]), vec![10, 5])]
    {
        let server = TestServer::start().await;
        manual_bots(&server);
        let c = server.player(1).await;
        create(&server, &c, json!({"ai_players": input})).await;
        let got: Vec<u8> = levels(&server).into_iter().filter_map(|d| d.and_then(|d| d.level())).collect();
        assert_eq!(got, expected, "{input}");
    }
    let server = TestServer::start().await;
    let c = server.player(1).await;
    create(&server, &c, json!({"ai_players": "hard"})).await;
    assert_eq!(with_room(&server, |r| r.game.players.len()), 1, "nem lista → nincs robot");
}

#[tokio::test(flavor = "multi_thread")]
async fn bot_count_is_limited() {
    let server = TestServer::start().await;
    manual_bots(&server);
    let c = server.player(1).await;
    create(&server, &c, json!({"max_players": 2, "ai_players": ["easy", "easy", "easy"]})).await;
    assert_eq!(with_room(&server, |r| r.game.players.len()), 2);
    c.call("leave_room", Value::Null).await;
    create(&server, &c, json!({"max_players": 4, "ai_players": vec!["easy"; 9]})).await;
    assert_eq!(with_room(&server, |r| r.game.players.len()), 4);
}

#[tokio::test(flavor = "multi_thread")]
async fn bot_names() {
    let server = TestServer::start().await;
    manual_bots(&server);
    let c = server.player(1).await;
    create(&server, &c, json!({"ai_players": ["easy", "easy", "easy"]})).await;
    let names: std::collections::HashSet<String> = with_room(&server, |r| r.game.players.iter().map(|p| p.name.clone()).collect());
    assert_eq!(names.len(), 4);
    // a fokozat sávja
    let tiers: Vec<&str> = (1..=10).map(|n| core::bot_tier(Some(Difficulty::Level(n)))).collect();
    assert_eq!(tiers, vec!["easy", "easy", "easy", "medium", "medium", "medium", "medium", "hard", "hard", "hard"]);
    let mut game = scrabble::game::Game::with_defaults("g");
    game.add_player("H", "Human").unwrap();
    for (level, tier) in [(1u8, "easy"), (5, "medium"), (9, "hard")] {
        let name = core::bot_name(&game, Some(Difficulty::Level(level)));
        assert!(core::bot_name_pool(tier).contains(&name.as_str()), "{name} / {tier}");
    }
    for level in 1..=10u8 {
        let (low, high) = core::think_delay(core::bot_tier(Some(Difficulty::Level(level))));
        assert!(0.0 < low && low <= high);
    }
    // ember nevével nem ütközik
    let server = TestServer::start().await;
    let rita = server.guest("Rita").await;
    create(&server, &rita, json!({"ai_players": ["medium"]})).await;
    assert_ne!(with_room(&server, |r| r.game.players[1].name.clone()), "Rita");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_room_full_with_bots_cannot_be_joined() {
    let server = TestServer::start().await;
    manual_bots(&server);
    let owner = server.player(1).await;
    create(&server, &owner, json!({"max_players": 2, "ai_players": ["easy"]})).await;
    let other = server.player(2).await;
    let events = other.call("join_room", json!({"code": owner.code()})).await;
    assert!(errors(&events).iter().any(|m| m.contains("megtelt")));
}

// ---------------------------------------------------------------------------------------------------
// robotlépések
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn the_bot_plays_and_the_turn_returns_to_the_human() {
    let server = TestServer::start().await;
    manual_bots(&server);
    let client = solo_started(&server, json!({})).await;
    assert!(client.my_turn(), "az első játékos az ember");
    client.call("pass_turn", Value::Null).await;
    client.take();
    assert!(play(&server, None, None).await);
    client.settle().await;
    let (human_turn, last_player, bot_name, n) = with_room(&server, |r| {
        let h = r.game.history();
        (!r.game.current_player().unwrap().is_bot, h.last().unwrap()["player"].clone(), r.game.players[1].name.clone(), h.len())
    });
    assert!(human_turn);
    assert_eq!(last_player, bot_name);
    let states = client.peek("game_state");
    assert_eq!(states.last().unwrap()["history"].as_array().unwrap().last().unwrap()["n"].as_u64(), Some(n as u64));
    // a robot pontját a lépés pontja adja
    let (score, last) = with_room(&server, |r| (r.game.players[1].score as i64, r.game.history().last().unwrap().clone()));
    if last["type"] == "place" {
        assert_eq!(score, last["score"].as_i64().unwrap());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn stale_and_misplaced_bot_turns_are_ignored() {
    let server = TestServer::start().await;
    manual_bots(&server);
    let client = solo_started(&server, json!({})).await;
    // az ember következik: a robot nem lép
    assert!(!play(&server, None, None).await);
    client.call("pass_turn", Value::Null).await;
    let stale = with_room(&server, |r| r.bot_turn_id());
    with_room(&server, |r| {
        r.invalidate_bot_turn();
    });
    assert!(!play(&server, Some(stale), None).await);
    let turn = with_room(&server, |r| r.game.turn_number);
    assert!(!play(&server, None, Some(turn - 1)).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn bots_wait_without_a_connected_human_and_during_votes() {
    let server = TestServer::start().await;
    manual_bots(&server);
    let client = solo_started(&server, json!({})).await;
    client.call("pass_turn", Value::Null).await;
    with_room(&server, |r| {
        r.game.players[0].disconnected = true;
        assert!(!core::bot_should_move(r));
        r.game.players[0].disconnected = false;
        assert!(core::bot_should_move(r));
    });
    let server = TestServer::start().await;
    manual_bots(&server);
    let a = server.player(1).await;
    create(&server, &a, json!({"ai_players": ["easy"], "challenge_mode": true})).await;
    let b = server.player(2).await;
    b.call("join_room", json!({"code": a.code()})).await;
    a.call("start_game", Value::Null).await;
    with_room(&server, |r| {
        r.game.current_player_idx = 1; // a robot jön
        let tiles = vec![scrabble::board::Placed::new(7, 7, scrabble::tiles::Tile::from_str("A").unwrap(), false)];
        r.game.pending_challenge = Some(scrabble::challenge::Challenge::new(tiles, vec![], vec![], 1, 0, vec![]));
        assert!(!core::bot_should_move(r));
        r.game.pending_challenge = None;
    });
}

#[tokio::test(flavor = "multi_thread")]
async fn a_full_game_with_a_passing_human_finishes_and_is_saved() {
    let server = TestServer::start().await;
    set_setting(&server, "bot_think_multiplier", json!(0.0));
    relax_socket_limits(&server.app, &["pass_turn"]);
    let client = solo_started(&server, json!({"ai_players": ["hard"]})).await;
    for _ in 0..400 {
        if with_room(&server, |r| r.game.finished) {
            break;
        }
        if client.my_turn() {
            client.emit0("pass_turn");
        }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        // a robot gyorsan lép; a kérésenkénti korlát miatt ritkítva passzolunk
    }
    assert!(with_room(&server, |r| r.game.finished));
    client.settle().await;
    assert!(with_room(&server, |r| r.result_saved));
    assert!(with_room(&server, |r| r.game.winner().is_some() || !r.game.winners.is_empty()));
}

#[tokio::test(flavor = "multi_thread")]
async fn bots_in_challenge_games_start_votes_for_the_humans() {
    let server = TestServer::start().await;
    manual_bots(&server);
    let a = server.player(1).await;
    create(&server, &a, json!({"ai_players": ["hard"], "challenge_mode": true})).await;
    let b = server.player(2).await;
    b.call("join_room", json!({"code": a.code()})).await;
    a.call("start_game", Value::Null).await;
    with_room(&server, |r| {
        r.game.current_player_idx = 1; // sorrend: A, robot, B
        r.game.turn_number += 1;
    });
    assert!(play(&server, None, None).await);
    with_room(&server, |r| {
        if r.game.pending_challenge.is_some() {
            let humans: std::collections::HashSet<String> = r.game.human_players().iter().map(|p| p.id.clone()).collect();
            assert_eq!(r.game.voter_ids(), humans);
        } else {
            let kind = r.game.history().last().unwrap()["type"].as_str().unwrap().to_string();
            assert!(kind == "pass" || kind == "exchange");
        }
    });
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_bot_turn_follows_the_resolved_vote() {
    let server = TestServer::start().await;
    manual_bots(&server);
    let a = server.player(1).await;
    create(&server, &a, json!({"ai_players": ["easy"], "challenge_mode": true})).await;
    let b = server.player(2).await;
    b.call("join_room", json!({"code": a.code()})).await;
    a.call("start_game", Value::Null).await;
    // a kezdő az A játékos (sorrend: A, robot, B) — a kezét beállítjuk
    let first = current_name(&server);
    assert_eq!(first, "Jatekos1");
    set_hand(&server, "Jatekos1", &["A", "L", "M", "A", "K", "Ö", "R"]);
    a.emit("place_tiles", word_tiles(7, 7, &["A", "L", "M", "A"]));
    a.settle().await;
    assert!(with_room(&server, |r| r.game.pending_challenge.is_some()));
    let before = with_room(&server, |r| r.bot_turn_id());
    b.call("accept_words", Value::Null).await;
    assert!(with_room(&server, |r| r.game.pending_challenge.is_none()));
    assert!(with_room(&server, |r| r.game.current_player().unwrap().is_bot));
    assert_ne!(with_room(&server, |r| r.bot_turn_id()), before, "a következő robotlépés ütemeződött");
}

#[tokio::test(flavor = "multi_thread")]
async fn waiting_rooms_with_only_bots_are_removed() {
    let server = TestServer::start().await;
    manual_bots(&server);
    let c = server.player(1).await;
    create(&server, &c, json!({"ai_players": ["easy", "hard"]})).await;
    c.call("leave_room", Value::Null).await;
    assert_eq!(room_count(&server), 0);
    // kapcsolat megszakadásakor türelmi idő, lejártakor megszűnik
    let c = server.player(2).await;
    create(&server, &c, json!({"ai_players": ["easy"]})).await;
    c.close();
    c.settle().await;
    assert_eq!(room_count(&server), 1, "türelmi idő: a játékos visszatérhet");
    let token = server.app.state.lock().disconnected_players.keys().next().unwrap().clone();
    {
        let mut st = server.app.state.lock();
        scrabble::server::events::finalize_player_disconnect(&server.app, &mut st, &token);
    }
    assert_eq!(room_count(&server), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn restore_lobbies_only_expect_humans() {
    let server = TestServer::start().await;
    manual_bots(&server);
    let uid = server.ensure_user(1);
    let c = server.player(1).await;
    create(&server, &c, json!({"ai_players": ["hard"]})).await;
    c.call("start_game", Value::Null).await;
    let room_id = c.room();
    with_room(&server, |r| r.manually_saved = true);
    {
        let mut st = server.app.state.lock();
        core::save_game_to_db(&server.app, &mut st, &room_id);
    }
    c.call("leave_room", Value::Null).await;
    let saved = server.app.db.get_user_active_games(uid, None).unwrap();
    assert!(!saved.is_empty());
    use scrabble::db::RowExt;
    let events = c.call("restore_game", json!({"game_id": saved[0].int("game_id")})).await;
    let joined = find(&events, "room_joined").unwrap();
    assert_eq!(joined["is_restore_lobby"], true);
    assert_eq!(joined["expected_players"], json!(["Jatekos1"]));
    assert_eq!(with_room(&server, |r| r.max_players), 2);
    c.call("start_game", Value::Null).await;
    with_room(&server, |r| {
        assert_eq!(r.game.players.iter().map(|p| p.is_bot).collect::<Vec<_>>(), vec![false, true]);
        assert!(!r.game.players[1].disconnected);
        assert!(r.late_join_names.is_empty());
    });
}

#[tokio::test(flavor = "multi_thread")]
async fn rejected_bot_placements_are_not_repeated() {
    let server = TestServer::start().await;
    manual_bots(&server);
    let a = server.player(1).await;
    create(&server, &a, json!({"ai_players": ["hard"], "challenge_mode": true})).await;
    let b = server.player(2).await;
    b.call("join_room", json!({"code": a.code()})).await;
    a.call("start_game", Value::Null).await;
    with_room(&server, |r| {
        r.game.current_player_idx = 1;
        r.game.turn_number += 1;
    });
    assert!(play(&server, None, None).await);
    let pending = with_room(&server, |r| r.game.pending_challenge.as_ref().map(|c| scrabble::game::placement_key(&c.tiles_placed)));
    let Some(rejected) = pending else { return }; // a robot passzolt / cserélt: nincs mit elutasítani
    a.emit0("reject_words");
    b.emit0("reject_words");
    a.settle().await;
    assert!(with_room(&server, |r| r.game.rejected_placements.contains(&rejected)));
    assert!(with_room(&server, |r| r.game.current_player().unwrap().is_bot));
    assert!(play(&server, None, None).await);
    let again = with_room(&server, |r| r.game.pending_challenge.as_ref().map(|c| scrabble::game::placement_key(&c.tiles_placed)));
    if let Some(again) = again {
        assert_ne!(again, rejected);
    }
}

// ---------------------------------------------------------------------------------------------------
// tipp
// ---------------------------------------------------------------------------------------------------

async fn solo_with_alma(server: &TestServer, extra: Value) -> Sio {
    let client = server.player(1).await;
    let mut opts = json!({"ai_players": ["easy"]});
    for (k, v) in extra.as_object().unwrap() {
        opts[k] = v.clone();
    }
    create(server, &client, opts).await;
    client.call("start_game", Value::Null).await;
    set_hand(server, "Jatekos1", &["A", "L", "M", "A", "K", "Ö", "R"]);
    client
}

async fn hint(client: &Sio) -> Value {
    client.emit0("request_hint");
    client.wait("hint_result", 20).await.expect("hint_result")
}

#[tokio::test(flavor = "multi_thread")]
async fn hints_in_a_solo_game() {
    let server = TestServer::start().await;
    manual_bots(&server);
    let client = solo_with_alma(&server, json!({})).await;
    let result = hint(&client).await;
    assert_eq!(result["success"], true);
    let moves = result["moves"].as_array().unwrap().clone();
    assert!(!moves.is_empty());
    for m in &moves {
        assert!(m["score"].as_i64().unwrap() > 0 && !m["tiles"].as_array().unwrap().is_empty() && !m["words"].as_array().unwrap().is_empty());
    }
    // a javasolt lépés lerakható, pontja egyezik
    client.emit("place_tiles", json!({"tiles": moves[0]["tiles"]}));
    let placed = client.wait("action_result", 20).await.unwrap();
    assert_eq!(placed["success"], true);
    assert_eq!(placed["score"], moves[0]["score"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn hint_refusals() {
    let server = TestServer::start().await;
    manual_bots(&server);
    // más ember is van a szobában
    let a = server.player(1).await;
    create(&server, &a, json!({"ai_players": ["easy"]})).await;
    let b = server.player(2).await;
    b.call("join_room", json!({"code": a.code()})).await;
    a.call("start_game", Value::Null).await;
    let result = hint(&a).await;
    assert_eq!(result["success"], false);
    assert!(result["message"].as_str().unwrap().contains("egyedül"));
    // nem a saját kör
    let server = TestServer::start().await;
    manual_bots(&server);
    let client = solo_with_alma(&server, json!({})).await;
    with_room(&server, |r| r.game.current_player_idx = 1);
    assert_eq!(hint(&client).await["success"], false);
    // indítás előtt
    let server = TestServer::start().await;
    let c = server.player(1).await;
    create(&server, &c, json!({"ai_players": ["easy"]})).await;
    assert_eq!(hint(&c).await["success"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn hints_are_rate_limited() {
    let server = TestServer::start().await;
    manual_bots(&server);
    let client = solo_with_alma(&server, json!({"hint_limit": 10})).await;
    for _ in 0..5 {
        client.emit0("request_hint");
    }
    client.settle_long().await;
    let results = client.peek("hint_result");
    assert!(results.iter().any(|r| r["message"].as_str().unwrap_or("").contains("Túl sok")), "{results:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_hint_limit_is_per_room_and_validated() {
    for (input, expected) in [
        (json!({}), 3),
        (json!({"hint_limit": 0}), 0),
        (json!({"hint_limit": 1}), 1),
        (json!({"hint_limit": 5}), 5),
        (json!({"hint_limit": 10}), 10),
        (json!({"hint_limit": "5"}), 5),
        (json!({"hint_limit": 2}), 3),
        (json!({"hint_limit": -1}), 3),
        (json!({"hint_limit": 100}), 3),
        (json!({"hint_limit": "abc"}), 3),
        (json!({"hint_limit": null}), 3),
        (json!({"hint_limit": []}), 3),
        (json!({"hint_limit": 3.5}), 3),
    ] {
        let server = TestServer::start().await;
        manual_bots(&server);
        let c = server.player(1).await;
        let mut opts = json!({"ai_players": ["easy"]});
        for (k, v) in input.as_object().unwrap() {
            opts[k] = v.clone();
        }
        let events = create(&server, &c, opts).await;
        assert_eq!(with_room(&server, |r| r.game.hint_limit), expected, "{input}");
        assert_eq!(find(&events, "room_joined").unwrap()["hint_limit"], expected);
    }
    // a csatlakozó is megkapja
    let server = TestServer::start().await;
    let owner = server.player(1).await;
    create(&server, &owner, json!({"hint_limit": 5})).await;
    let other = server.player(2).await;
    let events = other.call("join_room", json!({"code": owner.code()})).await;
    assert_eq!(find(&events, "room_joined").unwrap()["hint_limit"], 5);
}

#[tokio::test(flavor = "multi_thread")]
async fn hints_are_counted_and_run_out() {
    let server = TestServer::start().await;
    manual_bots(&server);
    let client = solo_with_alma(&server, json!({"hint_limit": 0})).await;
    let result = hint(&client).await;
    assert_eq!(result["success"], false);
    assert!(result["message"].as_str().unwrap().contains("ki vannak kapcsolva"));
    assert_eq!(with_room(&server, |r| r.game.hints_used), 0);

    let server = TestServer::start().await;
    manual_bots(&server);
    let client = solo_with_alma(&server, json!({"hint_limit": 1})).await;
    let first = hint(&client).await;
    assert!(first["success"] == true && !first["moves"].as_array().unwrap().is_empty());
    assert_eq!(first["hints_left"], 0);
    assert_eq!(with_room(&server, |r| r.game.hints_left()), 0);
    let second = hint(&client).await;
    assert_eq!(second["success"], false);
    assert!(second["message"].as_str().unwrap().contains("Elfogytak"));
    assert_eq!(second["hints_left"], 0);

    // az állapot frissül, és a limit játékonként él
    let server = TestServer::start().await;
    manual_bots(&server);
    let client = solo_with_alma(&server, json!({"hint_limit": 3})).await;
    hint(&client).await;
    client.settle().await;
    assert_eq!(client.peek("game_state").last().unwrap()["hints_left"], 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_hint_limit_survives_a_restore() {
    let server = TestServer::start().await;
    manual_bots(&server);
    let uid = server.ensure_user(1);
    let c = solo_with_alma(&server, json!({"hint_limit": 5})).await;
    hint(&c).await;
    assert_eq!(with_room(&server, |r| r.game.hints_used), 1);
    let room_id = c.room();
    with_room(&server, |r| r.manually_saved = true);
    {
        let mut st = server.app.state.lock();
        core::save_game_to_db(&server.app, &mut st, &room_id);
    }
    c.call("leave_room", Value::Null).await;
    use scrabble::db::RowExt;
    let saved = server.app.db.get_user_active_games(uid, None).unwrap();
    let events = c.call("restore_game", json!({"game_id": saved[0].int("game_id")})).await;
    assert_eq!(find(&events, "room_joined").unwrap()["hint_limit"], 5);
    c.call("start_game", Value::Null).await;
    with_room(&server, |r| assert_eq!((r.game.hint_limit, r.game.hints_used), (5, 1)));
}

// ---------------------------------------------------------------------------------------------------
// előnézet
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn the_preview_event() {
    let server = TestServer::start().await;
    manual_bots(&server);
    let client = solo_with_alma(&server, json!({})).await;
    let tiles = word_tiles(7, 7, &["A", "L", "M", "A"]);
    client.emit("preview_move", tiles.clone());
    let preview = client.wait("move_preview", 10).await.unwrap();
    assert_eq!(preview["valid"], true);
    assert_eq!(preview["words"][0]["word"], "ALMA");
    client.emit("place_tiles", tiles);
    let result = client.wait("action_result", 20).await.unwrap();
    assert_eq!(result["score"], preview["score"]);
    // értelmetlen kérés
    let server = TestServer::start().await;
    manual_bots(&server);
    let client = solo_with_alma(&server, json!({})).await;
    client.emit("preview_move", json!({"tiles": [{"row": 99, "col": 0, "letter": "A"}]}));
    assert_eq!(client.wait("move_preview", 10).await.unwrap()["valid"], false);
    // szoba nélkül csend
    let server = TestServer::start().await;
    let lone = server.player(1).await;
    assert!(find(&lone.call("preview_move", json!({"tiles": []})).await, "move_preview").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_preview_is_rate_limited_silently() {
    let server = TestServer::start().await;
    manual_bots(&server);
    let client = solo_with_alma(&server, json!({})).await;
    let tiles = word_tiles(7, 7, &["A", "L"]);
    for _ in 0..45 {
        client.emit("preview_move", tiles.clone());
    }
    client.settle_long().await;
    client.settle_long().await;
    let events = client.take();
    assert!(events.iter().filter(|(n, _)| n == "move_preview").count() <= 30);
    assert!(errors(&events).is_empty());
}
