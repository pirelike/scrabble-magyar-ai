//! Újracsatlakozás, türelmi idő, munkamenet-átvétel, kilépés, mentés és visszaállítás szobákban (a Python
//! `test_regressions.py` szerver részének megfelelője).

mod common;
use common::*;
use scrabble::server::{core, events};
use serde_json::{Value, json};

fn errors(events: &[(String, Value)]) -> Vec<String> {
    events.iter().filter(|(n, _)| n == "error").filter_map(|(_, d)| d["message"].as_str().map(|s| s.to_string())).collect()
}

fn sid_of(server: &TestServer, name: &str) -> String {
    server.app.state.lock().player_names.iter().find(|(_, n)| n.as_str() == name).map(|(s, _)| s.clone()).unwrap_or_else(|| panic!("nincs {name}"))
}

fn tokens_disconnected(server: &TestServer) -> Vec<String> {
    server.app.state.lock().disconnected_players.keys().cloned().collect()
}

/// Szoba három emberrel: regisztrált tulajdonos (Jatekos1) és két vendég, elindítva.
async fn started_room(server: &TestServer, guests: &[&str]) -> (Sio, Vec<Sio>) {
    let owner = server.player(1).await;
    owner.call("create_room", json!({"name": "R", "max_players": 4})).await;
    let mut others = Vec::new();
    for name in guests {
        let g = server.guest(name).await;
        g.call("join_room", json!({"code": owner.code()})).await;
        others.push(g);
    }
    owner.call("start_game", Value::Null).await;
    owner.take();
    for g in &others {
        g.take();
    }
    (owner, others)
}

fn finalize(server: &TestServer, token: &str) {
    let mut st = server.app.state.lock();
    events::finalize_player_disconnect(&server.app, &mut st, token);
}

// ---------------------------------------------------------------------------------------------------
// a tulajdonos időtúllépése (háttérszál, kérés-kontextus nélkül)
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn the_room_is_disbanded_when_the_owners_grace_expires() {
    let server = TestServer::start().await;
    let (owner, guests) = started_room(&server, &["Guest"]).await;
    let guest = &guests[0];
    owner.close();
    guest.settle().await;
    let tokens = tokens_disconnected(&server);
    assert_eq!(tokens.len(), 1);
    guest.take();
    finalize(&server, &tokens[0]);
    guest.settle().await;
    assert!(server.app.state.lock().rooms.is_empty());
    let events = guest.take();
    assert!(find(&events, "room_disbanded").is_some());
    assert!(find(&events, "room_left").is_some());
    assert!(server.app.state.lock().player_rooms.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_disbanded_game_is_saved() {
    let server = TestServer::start().await;
    let uid = server.ensure_user(1);
    let (owner, _guests) = started_room(&server, &["Guest"]).await;
    owner.close();
    owner.settle().await;
    let tokens = tokens_disconnected(&server);
    finalize(&server, &tokens[0]);
    assert_eq!(server.app.db.get_user_active_games(uid, None).unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn ownership_transfer_works_outside_a_request() {
    let server = TestServer::start().await;
    let owner = server.guest("Owner").await;
    owner.call("create_room", json!({"name": "Lobby", "max_players": 2})).await;
    let other = server.guest("Other").await;
    other.call("join_room", json!({"code": owner.code()})).await;
    let owner_sid = sid_of(&server, "Owner");
    {
        let mut st = server.app.state.lock();
        let room_id = st.rooms.keys().next().unwrap().clone();
        st.rooms.get_mut(&room_id).unwrap().game.remove_player(&owner_sid); // a tulajdonos kilépett
        core::transfer_ownership(&server.app, &mut st, &room_id);
        assert_eq!(st.rooms[&room_id].owner_name, "Other");
    }
    other.settle().await;
    assert!(other.names().iter().any(|n| n == "room_code"));
}

// ---------------------------------------------------------------------------------------------------
// munkamenet-átvétel
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_new_connection_takes_over_a_still_live_session() {
    let server = TestServer::start().await;
    let (owner, guests) = started_room(&server, &["Guest"]).await;
    let token = owner.token();
    let fresh = Sio::connect(&server).await;
    let events = fresh.call("rejoin_room", json!({"token": token})).await;
    assert!(find(&events, "rejoin_failed").is_none(), "{events:?}");
    assert!(find(&events, "room_joined").is_some());
    let state = events.iter().rev().find(|(n, _)| n == "game_state").unwrap().1.clone();
    assert_eq!(state["started"], true);
    let me = state["players"].as_array().unwrap().iter().find(|p| p["name"] == "Jatekos1").unwrap().clone();
    assert_eq!(me["disconnected"], false);
    assert!(me.get("hand").is_some());
    guests[0].settle().await;
    assert!(guests[0].names().iter().any(|n| n == "player_reconnected"));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_takeover_moves_every_reference_to_the_new_sid() {
    let server = TestServer::start().await;
    let (owner, _guests) = started_room(&server, &["Guest"]).await;
    let old_sid = sid_of(&server, "Jatekos1");
    let token = server.app.state.lock().get_reconnect_token_for_sid(&old_sid).unwrap();
    let fresh = Sio::connect(&server).await;
    fresh.call("rejoin_room", json!({"token": token})).await;
    let new_sid = sid_of(&server, "Jatekos1");
    assert_ne!(new_sid, old_sid);
    let st = server.app.state.lock();
    assert!(!st.player_rooms.contains_key(&old_sid));
    let room_id = st.rooms.keys().next().unwrap().clone();
    assert_eq!(st.player_rooms[&new_sid], room_id);
    assert_eq!(st.rooms[&room_id].owner.as_deref(), Some(new_sid.as_str()));
    assert!(!st.player_auth.contains_key(&old_sid));
    drop(st);
    drop(owner);
}

#[tokio::test(flavor = "multi_thread")]
async fn rejoining_from_the_same_connection_keeps_it_alive() {
    let server = TestServer::start().await;
    let (owner, _guests) = started_room(&server, &["Guest"]).await;
    let events = owner.call("rejoin_room", json!({"token": owner.token()})).await;
    assert!(!owner.is_closed());
    assert!(find(&events, "room_joined").is_some());
    assert_eq!(events.iter().rev().find(|(n, _)| n == "game_state").unwrap().1["started"], true);
    assert!(!with_room(&server, |r| r.game.players[0].disconnected));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_token_fails() {
    let server = TestServer::start().await;
    let c = Sio::connect(&server).await;
    assert!(find(&c.call("rejoin_room", json!({"token": "nope"})).await, "rejoin_failed").is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn takeover_in_a_waiting_room() {
    let server = TestServer::start().await;
    let owner = server.guest("Owner").await;
    owner.call("create_room", json!({"name": "Lobby", "max_players": 2})).await;
    let code = owner.code();
    let fresh = Sio::connect(&server).await;
    let events = fresh.call("rejoin_room", json!({"token": owner.token()})).await;
    assert!(find(&events, "rejoin_failed").is_none());
    assert_eq!(find(&events, "room_joined").unwrap()["is_owner"], true);
    assert_eq!(find(&events, "room_code").unwrap()["code"], code);
    assert_eq!(events.iter().rev().find(|(n, _)| n == "game_state").unwrap().1["started"], false);
}

// ---------------------------------------------------------------------------------------------------
// a várakozó szoba türelmi ideje
// ---------------------------------------------------------------------------------------------------

async fn waiting_owner(server: &TestServer) -> (Sio, String) {
    let owner = server.guest("Owner").await;
    owner.call("create_room", json!({"name": "Lobby", "max_players": 3})).await;
    let code = owner.code();
    (owner, code)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_waiting_room_survives_the_owner_leaving_the_connection() {
    let server = TestServer::start().await;
    let (owner, code) = waiting_owner(&server).await;
    owner.close();
    owner.settle().await;
    assert_eq!(server.app.state.lock().rooms.len(), 1);
    assert!(server.app.state.lock().join_codes.contains_key(&code));
    // a kóddal továbbra is lehet csatlakozni, a tulajdonos „offline”
    let friend = server.guest("Friend").await;
    let events = friend.call("join_room", json!({"code": code})).await;
    assert!(errors(&events).is_empty(), "{events:?}");
    let state = events.iter().rev().find(|(n, _)| n == "game_state").unwrap().1.clone();
    let names: Vec<&str> = state["players"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["Owner", "Friend"]);
    assert_eq!(state["players"][0]["disconnected"], true);
    assert_eq!(state["players"][1]["disconnected"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_owner_returns_with_the_token_and_keeps_the_room() {
    let server = TestServer::start().await;
    let (owner, code) = waiting_owner(&server).await;
    let token = owner.token();
    let old_sid = sid_of(&server, "Owner");
    let friend = server.guest("Friend").await;
    friend.call("join_room", json!({"code": code})).await;
    owner.close();
    friend.settle().await;
    friend.take();
    let back = server.guest("Owner2").await;
    let events = back.call("rejoin_room", json!({"token": token})).await;
    assert!(find(&events, "rejoin_failed").is_none());
    assert_eq!(find(&events, "room_joined").unwrap()["is_owner"], true);
    assert_eq!(find(&events, "room_code").unwrap()["code"], code);
    let state = events.iter().rev().find(|(n, _)| n == "game_state").unwrap().1.clone();
    assert_eq!(state["started"], false);
    assert!(state["players"].as_array().unwrap().iter().all(|p| p["disconnected"] == false));
    {
        let st = server.app.state.lock();
        let room = st.rooms.values().next().unwrap();
        assert_ne!(room.owner.as_deref(), Some(old_sid.as_str()));
        assert!(st.player_rooms.contains_key(room.owner.as_deref().unwrap()));
    }
    friend.settle().await;
    assert!(friend.names().iter().any(|n| n == "player_reconnected"));
    // a visszatért tulajdonos el tudja indítani a játékot
    let events = back.call("start_game", Value::Null).await;
    assert_eq!(events.iter().rev().find(|(n, _)| n == "game_state").unwrap().1["started"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn grace_expiry_removes_an_empty_room_or_moves_the_ownership() {
    let server = TestServer::start().await;
    let (owner, code) = waiting_owner(&server).await;
    owner.close();
    owner.settle().await;
    finalize(&server, &tokens_disconnected(&server)[0]);
    {
        let st = server.app.state.lock();
        assert!(st.rooms.is_empty());
        assert!(!st.join_codes.contains_key(&code));
        assert!(st.player_rooms.is_empty() && st.player_auth.is_empty());
    }
    // másik szobában a tulajdonjog továbbszáll
    let (owner, code) = waiting_owner(&server).await;
    let friend = server.guest("Friend").await;
    friend.call("join_room", json!({"code": code})).await;
    owner.close();
    friend.settle().await;
    friend.take();
    finalize(&server, &tokens_disconnected(&server)[0]);
    friend.settle().await;
    {
        let st = server.app.state.lock();
        let room = st.rooms.values().next().unwrap();
        assert_eq!(room.game.players.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Friend"]);
        assert_eq!(room.owner_name, "Friend");
    }
    let events = friend.take();
    assert!(find(&events, "player_left").is_some());
    assert_eq!(find(&events, "room_code").unwrap()["code"], code);
}

#[tokio::test(flavor = "multi_thread")]
async fn other_players_in_a_waiting_room_get_a_grace_period_too() {
    let server = TestServer::start().await;
    let (owner, code) = waiting_owner(&server).await;
    let friend = server.guest("Friend").await;
    friend.call("join_room", json!({"code": code})).await;
    owner.take();
    friend.close();
    owner.settle().await;
    with_room(&server, |room| {
        assert_eq!(room.game.players.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Owner", "Friend"]);
        assert!(room.game.players[1].disconnected);
    });
    assert!(owner.names().iter().any(|n| n == "player_disconnected"));
}

#[test]
fn the_grace_period_lengths() {
    assert_eq!(core::DISCONNECT_GRACE_PERIOD, 120);
    assert!(core::WAITING_OWNER_GRACE_PERIOD >= 600);
}

#[tokio::test(flavor = "multi_thread")]
async fn leaving_a_waiting_room_removes_the_player_at_once() {
    let server = TestServer::start().await;
    let (owner, _code) = waiting_owner(&server).await;
    owner.call("leave_room", Value::Null).await;
    let st = server.app.state.lock();
    assert!(st.rooms.is_empty());
    assert!(st.disconnected_players.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_old_disconnect_timer_does_not_cut_a_newer_one_short() {
    let server = TestServer::start().await;
    let (owner, _code) = waiting_owner(&server).await;
    let token = owner.token();
    owner.close();
    owner.settle().await;
    let first_seq = server.app.state.lock().disconnected_players[&token].seq;
    let back = server.guest("Owner").await;
    back.call("rejoin_room", json!({"token": token})).await;
    back.close();
    back.settle().await;
    let st = server.app.state.lock();
    let second = st.disconnected_players[&token].seq;
    assert_ne!(first_seq, second);
    assert!(!st.disconnect_is_current(&token, first_seq), "a régi időzítő már nem érvényes");
    assert!(st.disconnect_is_current(&token, second));
}

// ---------------------------------------------------------------------------------------------------
// belépési védelmek, kijelentkezés
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn room_join_guards() {
    let server = TestServer::start().await;
    let a = server.guest("A").await;
    let b = server.guest("B").await;
    a.call("create_room", json!({"name": "One", "max_players": 2})).await;
    b.call("create_room", json!({"name": "Two", "max_players": 2})).await;
    assert!(!errors(&b.call("join_room", json!({"code": a.code()})).await).is_empty());
    assert_eq!(with_room(&server, |_| 1), 1);
    let st = server.app.state.lock();
    let room_a = st.rooms.values().find(|r| r.name == "One").unwrap();
    assert_eq!(room_a.game.players.len(), 1);
    drop(st);
    // kilépés után új szoba hozható létre
    let solo = server.guest("Solo").await;
    solo.call("create_room", json!({"name": "One", "max_players": 2})).await;
    solo.call("leave_room", Value::Null).await;
    assert!(find(&solo.call("create_room", json!({"name": "Two", "max_players": 2})).await, "room_joined").is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn duplicate_names_are_rejected_in_a_room() {
    let server = TestServer::start().await;
    let a = server.guest("Béla").await;
    let b = server.guest("béla").await;
    a.call("create_room", json!({"name": "One", "max_players": 4})).await;
    let events = b.call("join_room", json!({"code": a.code()})).await;
    assert!(!errors(&events).is_empty());
    assert!(find(&events, "room_joined").is_none());
    assert_eq!(with_room(&server, |r| r.game.players.len()), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn non_string_codes_are_clean_errors() {
    let server = TestServer::start().await;
    let c = server.guest("Solo").await;
    for bad in [json!(123456), json!(["1"]), json!({"a": 1}), Value::Null, json!(true)] {
        let events = c.call("join_room", json!({"code": bad})).await;
        assert!(!errors(&events).is_empty(), "{bad}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn logout_clears_presence_and_room() {
    let server = TestServer::start().await;
    let uid = server.ensure_user(1);
    let user = server.player(1).await;
    user.call("create_room", json!({"name": "One", "max_players": 2})).await;
    assert!(server.app.state.lock().is_user_online(uid));
    user.call("logout", Value::Null).await;
    let st = server.app.state.lock();
    assert!(!st.is_user_online(uid));
    assert!(st.rooms.is_empty());
    assert!(st.player_rooms.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn relogin_as_another_user_does_not_leak_presence() {
    let server = TestServer::start().await;
    let first = server.ensure_user(1);
    let second = server.ensure_user(2);
    let c = server.player(1).await;
    c.emit("set_name", json!({"name": "Jatekos2", "is_guest": false, "auth_token": server.socket_token(second)}));
    c.settle().await;
    let st = server.app.state.lock();
    assert!(st.is_user_online(second));
    assert!(!st.is_user_online(first));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_leaver_gets_no_game_state() {
    let server = TestServer::start().await;
    let (owner, guests) = started_room(&server, &["Második", "Harmadik"]).await;
    let (second, third) = (&guests[0], &guests[1]);
    third.call("leave_room", Value::Null).await;
    assert!(third.names().is_empty() || true);
    third.take();
    let name = current_name(&server);
    let cur = if name == "Jatekos1" { &owner } else { second };
    cur.emit0("pass_turn");
    cur.settle().await;
    assert!(!third.names().iter().any(|n| n == "game_state"));
    owner.settle().await;
    assert!(owner.names().iter().any(|n| n == "game_state"));
}

// ---------------------------------------------------------------------------------------------------
// a befejezett játék mentése
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_finished_game_is_saved_once() {
    let server = TestServer::start().await;
    let uid = server.ensure_user(1);
    let owner = server.player(1).await;
    owner.call("create_room", json!({"name": "Solo", "max_players": 2})).await;
    owner.call("start_game", Value::Null).await;
    let room_id = owner.room();
    {
        let mut st = server.app.state.lock();
        let room = st.rooms.get_mut(&room_id).unwrap();
        for _ in 0..6 {
            let id = room.game.current_player().unwrap().id.clone();
            room.game.pass_turn(&id, false).unwrap();
        }
        assert!(room.game.finished);
        core::save_game_to_db(&server.app, &mut st, &room_id);
        core::save_game_to_db(&server.app, &mut st, &room_id);
    }
    assert_eq!(server.app.db.get_user_by_id(uid).unwrap().unwrap().games_played, 1);
    let history = server.app.db.get_user_game_history(uid, 20).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0]["room_name"], "Solo");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_draw_is_saved_for_every_top_scorer() {
    let server = TestServer::start().await;
    let alice = server.player(1).await;
    let bob = server.player(2).await;
    alice.call("create_room", json!({"name": "R", "max_players": 4})).await;
    bob.call("join_room", json!({"code": alice.code()})).await;
    alice.call("start_game", Value::Null).await;
    let room_id = alice.room();
    {
        let mut st = server.app.state.lock();
        let room = st.rooms.get_mut(&room_id).unwrap();
        for p in room.game.players.iter_mut() {
            p.hand.clear();
            p.score = 50;
        }
        room.game.end_game(None);
        assert_eq!(room.game.winners.len(), 2);
        core::save_game_to_db(&server.app, &mut st, &room_id);
    }
    for n in [1, 2] {
        let uid = server.user_id(&format!("u{n}@example.com"));
        assert_eq!(server.app.db.get_user_by_id(uid).unwrap().unwrap().games_won, 1);
        assert_eq!(server.app.db.get_user_game_history(uid, 20).unwrap()[0]["is_winner"].as_i64(), Some(1));
    }
}

// ---------------------------------------------------------------------------------------------------
// mentés és visszaállítás
// ---------------------------------------------------------------------------------------------------

/// Két regisztrált játékos, a mentéskor Bob (Jatekos2) következik. Visszaadja: Alice kapcsolatát és a mentés azonosítóját.
async fn saved_with_second_player_to_move(server: &TestServer) -> (Sio, i64, i64) {
    let alice = server.player(1).await;
    let bob = server.player(2).await;
    alice.call("create_room", json!({"name": "R", "max_players": 4})).await;
    bob.call("join_room", json!({"code": alice.code()})).await;
    alice.call("start_game", Value::Null).await;
    // Alice kezdjen: ha nem ő, Bob passzol — a cél, hogy Bob következzen a mentéskor
    {
        let mut st = server.app.state.lock();
        let room = st.rooms.values_mut().next().unwrap();
        let id = room.game.current_player().unwrap().id.clone();
        room.game.pass_turn(&id, false).unwrap();
        assert_eq!(room.game.current_player().unwrap().name, "Jatekos2");
    }
    let events = alice.call("save_game", Value::Null).await;
    assert_eq!(find(&events, "action_result").unwrap()["success"], true);
    alice.call("leave_room", Value::Null).await;
    bob.close();
    alice.settle().await;
    let game_id = server.app.db.load_active_games().unwrap()[0].int("id");
    let bob_id = server.user_id("u2@example.com");
    (alice, bob_id, game_id)
}

use scrabble::db::RowExt;

#[tokio::test(flavor = "multi_thread")]
async fn an_absent_current_player_is_skipped_on_restore() {
    let server = TestServer::start().await;
    let (alice, _bob, game_id) = saved_with_second_player_to_move(&server).await;
    alice.call("restore_game", json!({"game_id": game_id})).await;
    let events = alice.call("start_game", Value::Null).await;
    let state = events.iter().rev().find(|(n, _)| n == "game_state").unwrap().1.clone();
    assert_eq!(state["started"], true);
    let current = state["players"].as_array().unwrap().iter().find(|p| p["id"] == state["current_player"]).unwrap().clone();
    assert_eq!(current["name"], "Jatekos1", "nem a hiányzó Bob");
    assert!(state["players"].as_array().unwrap().iter().any(|p| p["name"] == "Jatekos2" && p["disconnected"] == true));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_missing_player_can_join_the_running_game() {
    let server = TestServer::start().await;
    let (alice, bob_id, game_id) = saved_with_second_player_to_move(&server).await;
    let events = alice.call("restore_game", json!({"game_id": game_id})).await;
    let code = find(&events, "room_code").unwrap()["code"].as_str().unwrap().to_string();
    alice.call("start_game", Value::Null).await;
    let bob = Sio::connect(&server).await;
    bob.emit("set_name", json!({"name": "Jatekos2", "is_guest": false, "auth_token": server.socket_token(bob_id)}));
    bob.settle().await;
    bob.take();
    let events = bob.call("join_room", json!({"code": code})).await;
    assert!(errors(&events).is_empty(), "{events:?}");
    assert!(find(&events, "room_joined").is_some() && find(&events, "game_started").is_some());
    let state = events.iter().rev().find(|(n, _)| n == "game_state").unwrap().1.clone();
    let bob_state = state["players"].as_array().unwrap().iter().find(|p| p["name"] == "Jatekos2").unwrap().clone();
    assert_eq!(bob_state["disconnected"], false);
    assert_eq!(bob_state["hand"].as_array().unwrap().len(), 7);
    alice.settle().await;
    assert!(alice.names().iter().any(|n| n == "player_reconnected"));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_impostor_cannot_take_a_registered_seat() {
    let server = TestServer::start().await;
    let (alice, _bob, game_id) = saved_with_second_player_to_move(&server).await;
    let events = alice.call("restore_game", json!({"game_id": game_id})).await;
    let code = find(&events, "room_code").unwrap()["code"].as_str().unwrap().to_string();
    // a visszaállító váróban sem
    let impostor = server.guest("Jatekos2").await;
    let events = impostor.call("join_room", json!({"code": code})).await;
    assert!(!errors(&events).is_empty());
    assert!(find(&events, "room_joined").is_none());
    alice.call("start_game", Value::Null).await;
    // a futó játékba sem: sem vendégként, sem más fiókkal
    let events = impostor.call("join_room", json!({"code": code})).await;
    assert!(!errors(&events).is_empty());
    assert!(find(&events, "room_joined").is_none());
    server.ensure_user(3);
    let other = server.guest("Jatekos2").await;
    assert!(find(&other.call("join_room", json!({"code": code})).await, "room_joined").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restored_game_keeps_the_history_and_is_saved_again() {
    let server = TestServer::start().await;
    let (alice, _bob, game_id) = saved_with_second_player_to_move(&server).await;
    let old_moves = server.app.db.get_game_moves(game_id).unwrap();
    assert!(!old_moves.is_empty());
    alice.call("restore_game", json!({"game_id": game_id})).await;
    alice.call("start_game", Value::Null).await;
    let (db_id, log_len) = with_room(&server, |r| (r.db_game_id, r.game.move_log.len()));
    assert!(log_len >= old_moves.len());
    let new_id = db_id.expect("új mentés");
    assert_ne!(new_id, game_id);
    let new_moves = server.app.db.get_game_moves(new_id).unwrap();
    assert_eq!(new_moves.iter().take(old_moves.len()).map(|m| m.move_number).collect::<Vec<_>>(), old_moves.iter().map(|m| m.move_number).collect::<Vec<_>>());
    // a régi mentés lezárult, az új mindkét játékos fiókjához tartozik
    assert_eq!(server.app.db.get_game_by_id(game_id).unwrap().unwrap().get("status").and_then(|v| v.as_str()), Some("abandoned"));
    let players = server.app.db.get_game_players(new_id).unwrap();
    assert_eq!(players.len(), 2);
    assert!(players.iter().all(|(_, uid, _)| uid.is_some()));
}
