//! Megfigyelő (spectator) mód (a Python `test_spectator.py` megfelelője).

mod common;
use common::*;
use serde_json::{Value, json};

fn errors(events: &[(String, Value)]) -> Vec<String> {
    events.iter().filter(|(n, _)| n == "error").filter_map(|(_, d)| d["message"].as_str().map(|s| s.to_string())).collect()
}

/// Folyamatban lévő két játékosos szoba (Jatekos1 a tulajdonos): (a, b, szoba azonosító).
async fn live_room(server: &TestServer, private: bool) -> (Sio, Sio, String) {
    live_room_with(server, 1, 2, private).await
}

async fn live_room_with(server: &TestServer, n1: u32, n2: u32, private: bool) -> (Sio, Sio, String) {
    let a = server.player(n1).await;
    a.call("create_room", json!({"name": "Élő", "max_players": 2, "is_private": private})).await;
    let b = server.player(n2).await;
    b.call("join_room", json!({"code": a.code()})).await;
    a.call("start_game", Value::Null).await;
    a.take();
    b.take();
    let room = a.room();
    (a, b, room)
}

fn finish_room(server: &TestServer) {
    with_room(server, |r| r.game.finished = true);
}

// ---------------------------------------------------------------------------------------------------
// élő játékok listája
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn only_started_public_games_are_listed() {
    let server = TestServer::start().await;
    let a = server.player(1).await;
    a.call("create_room", json!({"name": "Vár"})).await;
    assert!(server.app.state.lock().get_live_games().is_empty(), "váró szoba");
    a.call("leave_room", Value::Null).await;
    let (a, b, room_id) = live_room(&server, false).await;
    {
        let st = server.app.state.lock();
        let live = st.get_live_games();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0]["id"], room_id);
        assert_eq!(live[0]["name"], "Élő");
        assert_eq!(live[0]["spectators"], 0);
        let names: Vec<&str> = live[0]["players"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"Jatekos1") && names.contains(&"Jatekos2"));
    }
    finish_room(&server);
    assert!(server.app.state.lock().get_live_games().is_empty(), "befejezett");
    drop((a, b));
}

#[tokio::test(flavor = "multi_thread")]
async fn private_games_are_not_listed() {
    let server = TestServer::start().await;
    let _room = live_room(&server, true).await;
    assert!(server.app.state.lock().get_live_games().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_rooms_and_game_start_send_the_live_games() {
    let server = TestServer::start().await;
    let watcher = server.player(3).await;
    let a = server.player(1).await;
    a.call("create_room", json!({"name": "Élő"})).await;
    watcher.take();
    a.call("start_game", Value::Null).await;
    assert!(watcher.names().iter().any(|n| n == "live_games"), "indításkor mindenki megkapja");
    let events = watcher.call("get_rooms", Value::Null).await;
    assert_eq!(find(&events, "live_games").unwrap()[0]["id"], a.room());
}

// ---------------------------------------------------------------------------------------------------
// megfigyelés
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn spectating_a_public_room() {
    let server = TestServer::start().await;
    let (a, _b, room_id) = live_room(&server, false).await;
    let s = server.player(3).await;
    let events = s.call("spectate_room", json!({"room_id": room_id})).await;
    let names = event_names(&events);
    assert!(names.contains(&"spectate_joined") && names.contains(&"game_state"));
    assert_eq!(find(&events, "spectate_joined").unwrap()["room_name"], "Élő");
    assert_eq!(with_room(&server, |r| r.spectators.len()), 1);
    // a néző állapota kéz nélküli, a játékosoké nem „néző”
    let state = events.iter().rev().find(|(n, _)| n == "game_state").unwrap().1.clone();
    assert_eq!(state["spectator"], true);
    assert!(state["players"].as_array().unwrap().iter().all(|p| p.get("hand").is_none() && p["hand_count"] == 7));
    let player_state = a.state();
    assert!(player_state.get("spectator").is_none());
    assert_eq!(player_state["spectator_count"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_spectator_follows_the_game_and_the_chat() {
    let server = TestServer::start().await;
    let (a, b, room_id) = live_room(&server, false).await;
    a.emit("send_chat", json!({"message": "régi"}));
    a.settle().await;
    let s = server.player(3).await;
    let events = s.call("spectate_room", json!({"room_id": room_id})).await;
    let joined = find(&events, "spectate_joined").unwrap();
    let history: Vec<&str> = joined["chat_messages"].as_array().unwrap().iter().map(|m| m["message"].as_str().unwrap()).collect();
    assert_eq!(history, vec!["régi"]);
    let players = [&a, &b];
    let cur = current(&players);
    cur.call("pass_turn", Value::Null).await;
    let states = s.peek("game_state");
    assert_eq!(states.last().unwrap()["history"].as_array().unwrap().last().unwrap()["type"], "pass");
    a.call("send_chat", json!({"message": "Sziasztok"})).await;
    assert_eq!(find(&s.take(), "chat_message").unwrap()["message"], "Sziasztok");
}

#[tokio::test(flavor = "multi_thread")]
async fn private_rooms_need_the_code() {
    let server = TestServer::start().await;
    let (a, _b, room_id) = live_room(&server, true).await;
    let s = server.player(3).await;
    let events = s.call("spectate_room", json!({"room_id": room_id})).await;
    assert!(errors(&events).iter().any(|m| m.contains("privát")));
    let events = s.call("spectate_room", json!({"code": a.code()})).await;
    assert!(event_names(&events).contains(&"spectate_joined"));
}

#[tokio::test(flavor = "multi_thread")]
async fn spectate_error_cases() {
    let server = TestServer::start().await;
    let a = server.player(1).await;
    a.call("create_room", json!({"name": "Vár"})).await;
    let s = server.player(3).await;
    assert!(errors(&s.call("spectate_room", json!({"room_id": a.room()})).await).iter().any(|m| m.contains("folyamatban")));
    assert!(errors(&s.call("spectate_room", json!({"room_id": "nincs"})).await).iter().any(|m| m.contains("nem létezik")));
    assert!(errors(&s.call("spectate_room", json!({"code": "12"})).await).iter().any(|m| m.contains("Érvénytelen kód")));
    assert!(errors(&s.call("spectate_room", json!({"code": "999999"})).await).iter().any(|m| m.contains("Nincs ilyen kódú")));
    assert!(!errors(&s.call("spectate_room", json!({})).await).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn finished_games_cannot_be_watched() {
    let server = TestServer::start().await;
    let (_a, _b, room_id) = live_room(&server, false).await;
    finish_room(&server);
    let s = server.player(3).await;
    assert!(!errors(&s.call("spectate_room", json!({"room_id": room_id})).await).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn players_and_spectators_cannot_watch_twice() {
    let server = TestServer::start().await;
    let (a, _b, room_id) = live_room_with(&server, 1, 2, false).await;
    let (_c, _d, other) = live_room_with(&server, 3, 4, false).await;
    // játékos nem figyelhet meg másik szobát
    assert!(errors(&a.call("spectate_room", json!({"room_id": other})).await).iter().any(|m| m.contains("Már egy szobában")));
    let s = server.player(5).await;
    s.call("spectate_room", json!({"room_id": room_id})).await;
    assert!(errors(&s.call("spectate_room", json!({"room_id": room_id})).await).iter().any(|m| m.contains("Már egy szobában")));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_spectator_limit_is_a_setting() {
    let server = TestServer::start().await;
    server.app.db.with(|tx| server.app.settings.store(tx, "max_spectators", &json!(1), None).map_err(|e| rusqlite::Error::InvalidParameterName(e.0))).unwrap();
    server.app.apply_runtime_settings();
    let (_a, _b, room_id) = live_room(&server, false).await;
    let s1 = server.player(3).await;
    s1.call("spectate_room", json!({"room_id": room_id})).await;
    let s2 = server.player(4).await;
    assert!(errors(&s2.call("spectate_room", json!({"room_id": room_id})).await).iter().any(|m| m.contains("megfigyelő")));
}

#[tokio::test(flavor = "multi_thread")]
async fn spectators_cannot_act() {
    let server = TestServer::start().await;
    let (_a, _b, room_id) = live_room(&server, false).await;
    let s = server.player(3).await;
    s.call("spectate_room", json!({"room_id": room_id})).await;
    let before = with_room(&server, |r| r.game.turn_number);
    s.emit0("pass_turn");
    s.emit("place_tiles", word_tiles(7, 7, &["A", "L"]));
    s.emit("send_chat", json!({"message": "szia"}));
    s.emit0("request_hint");
    s.settle().await;
    assert_eq!(with_room(&server, |r| r.game.turn_number), before);
    assert!(with_room(&server, |r| r.chat_messages.is_empty()));
}

// ---------------------------------------------------------------------------------------------------
// kilépés és a szoba életciklusa
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn leaving_the_spectator_role() {
    let server = TestServer::start().await;
    let (a, _b, room_id) = live_room(&server, false).await;
    let s = server.player(3).await;
    s.call("spectate_room", json!({"room_id": room_id})).await;
    a.take();
    let events = s.call("leave_spectate", Value::Null).await;
    assert!(event_names(&events).contains(&"spectate_left"));
    assert!(with_room(&server, |r| r.spectators.is_empty()));
    assert!(server.app.state.lock().spectator_rooms.is_empty());
    assert_eq!(a.peek("game_state").last().unwrap()["spectator_count"], 0);
    // másodszor nem történik semmi
    assert!(s.call("leave_spectate", Value::Null).await.is_empty());
    // és szobát lehet létrehozni
    assert!(event_names(&s.call("create_room", json!({"name": "Saját"})).await).contains(&"room_joined"));
}

#[tokio::test(flavor = "multi_thread")]
async fn disconnect_and_logout_remove_the_spectator() {
    let server = TestServer::start().await;
    let (_a, _b, room_id) = live_room(&server, false).await;
    let s1 = server.player(3).await;
    s1.call("spectate_room", json!({"room_id": room_id})).await;
    s1.close();
    let s2 = server.player(4).await;
    s2.call("spectate_room", json!({"room_id": room_id})).await;
    s2.call("logout", Value::Null).await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(with_room(&server, |r| r.spectators.is_empty()));
    assert!(server.app.state.lock().spectator_rooms.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn spectators_are_told_when_the_room_goes_away() {
    let server = TestServer::start().await;
    let (a, _b, room_id) = live_room(&server, false).await;
    let s = server.player(3).await;
    s.call("spectate_room", json!({"room_id": room_id})).await;
    a.call("leave_room", Value::Null).await;
    assert!(event_names(&s.take()).contains(&"room_disbanded"));
    assert!(server.app.state.lock().rooms.is_empty());
    assert!(server.app.state.lock().spectator_rooms.is_empty());
    // takarítással is
    let (_c, _d, room2) = live_room_with(&server, 1, 2, false).await;
    let s2 = server.player(4).await;
    s2.call("spectate_room", json!({"room_id": room2})).await;
    {
        let mut st = server.app.state.lock();
        scrabble::server::core::cleanup_room(&server.app, &mut st, &room2);
    }
    s2.settle().await;
    assert!(event_names(&s2.take()).contains(&"room_disbanded"));
    assert!(server.app.state.lock().spectator_rooms.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_final_state_reaches_the_spectator() {
    let server = TestServer::start().await;
    let (a, b, room_id) = live_room(&server, false).await;
    let s = server.player(3).await;
    s.call("spectate_room", json!({"room_id": room_id})).await;
    let players = [&a, &b];
    for _ in 0..8 {
        if with_room(&server, |r| r.game.finished) {
            break;
        }
        let cur = current(&players);
        cur.emit0("pass_turn");
        cur.settle().await;
    }
    s.settle().await;
    assert_eq!(s.peek("game_state").last().unwrap()["finished"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_spectator_keeps_the_bots_going() {
    let server = TestServer::start().await;
    let a = server.player(1).await;
    a.call("create_room", json!({"name": "Bot", "ai_players": ["easy", "hard"], "is_private": false})).await;
    a.call("start_game", Value::Null).await;
    with_room(&server, |room| {
        room.game.players[0].disconnected = true;
        room.game.current_player_idx = 1;
        assert!(!scrabble::server::core::bot_should_move(room), "ember nélkül nem lépnek a robotok");
        room.spectators.insert("x".into(), "Néző".into());
        assert!(scrabble::server::core::bot_should_move(room), "néző jelenlétében igen");
    });
}
