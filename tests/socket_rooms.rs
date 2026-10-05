//! Socket.IO: azonosítás, szobák, csatlakozás, chat (a Python `test_server_socket.py` megfelelője).

mod common;
use common::*;
use serde_json::{Value, json};

fn errors(events: &[(String, Value)]) -> Vec<String> {
    events.iter().filter(|(n, _)| n == "error").filter_map(|(_, d)| d["message"].as_str().map(|s| s.to_string())).collect()
}

// ---------------------------------------------------------------------------------------------------
// set_name
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn guest_name_is_registered() {
    let server = TestServer::start().await;
    let guest = server.guest("TestPlayer").await;
    let st = server.app.state.lock();
    assert!(st.player_names.values().any(|n| n == "TestPlayer"));
    drop(st);
    guest.close();
}

#[tokio::test(flavor = "multi_thread")]
async fn registered_name_comes_from_the_account() {
    let server = TestServer::start().await;
    let uid = server.create_user("regplayer@example.com", "RegPlayer");
    let sio = Sio::connect(&server).await;
    sio.emit("set_name", json!({"name": "Hamis Név", "is_guest": false, "auth_token": server.socket_token(uid)}));
    sio.settle().await;
    let st = server.app.state.lock();
    let sid = st.player_names.iter().find(|(_, n)| n.as_str() == "RegPlayer").map(|(s, _)| s.clone()).expect("a név a fiókból jön");
    assert!(!st.player_auth[&sid].is_guest);
    assert_eq!(st.player_auth[&sid].user_id, Some(uid));
}

#[tokio::test(flavor = "multi_thread")]
async fn forged_user_id_without_token_is_a_guest() {
    let server = TestServer::start().await;
    let uid = server.create_user("victim@example.com", "Victim");
    let sio = Sio::connect(&server).await;
    let events = sio.call("set_name", json!({"name": "Attacker", "is_guest": false, "user_id": uid})).await;
    let st = server.app.state.lock();
    let sid = st.player_names.iter().find(|(_, n)| n.as_str() == "Attacker").map(|(s, _)| s.clone()).unwrap();
    assert!(st.player_auth[&sid].is_guest);
    assert_eq!(st.player_auth[&sid].user_id, None);
    assert!(!st.is_user_online(uid));
    assert!(events.iter().any(|(n, _)| n == "error"));
}

#[tokio::test(flavor = "multi_thread")]
async fn token_of_another_user_does_not_transfer_identity() {
    let server = TestServer::start().await;
    let mine = server.create_user("mine@example.com", "Mine");
    let other = server.create_user("other@example.com", "Other");
    let sio = Sio::connect(&server).await;
    sio.emit("set_name", json!({"name": "X", "is_guest": false, "user_id": other, "auth_token": server.socket_token(mine)}));
    sio.settle().await;
    let st = server.app.state.lock();
    let sid = st.player_names.iter().find(|(_, n)| n.as_str() == "Mine").map(|(s, _)| s.clone()).unwrap();
    assert_eq!(st.player_auth[&sid].user_id, Some(mine));
    assert!(!st.is_user_online(other));
}

#[tokio::test(flavor = "multi_thread")]
async fn tampered_token_is_a_guest() {
    let server = TestServer::start().await;
    let uid = server.create_user("tamper@example.com", "Tamper");
    let token = server.socket_token(uid);
    let bad = format!("{}abc", &token[..token.len() - 3]);
    let sio = Sio::connect(&server).await;
    sio.emit("set_name", json!({"name": "Tamper", "is_guest": false, "auth_token": bad}));
    sio.settle().await;
    assert!(!server.app.state.lock().is_user_online(uid));
}

#[tokio::test(flavor = "multi_thread")]
async fn empty_name_becomes_nameless() {
    let server = TestServer::start().await;
    let sio = Sio::connect(&server).await;
    sio.emit("set_name", json!({"name": "", "is_guest": true}));
    sio.settle().await;
    assert!(server.app.state.lock().player_names.values().any(|n| n == "Névtelen"));
}

#[tokio::test(flavor = "multi_thread")]
async fn set_name_is_rate_limited() {
    let server = TestServer::start().await;
    let sio = Sio::connect(&server).await;
    for _ in 0..7 {
        sio.emit("set_name", json!({"name": "Gyors", "is_guest": true}));
    }
    sio.settle().await;
    let errs = errors(&sio.take());
    assert_eq!(errs.len(), 2, "a 6. és 7. kérés korlátozva: {errs:?}");
    assert!(errs[0].contains("Túl sok kérés"));
}

// ---------------------------------------------------------------------------------------------------
// get_rooms / create_room
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn get_rooms_empty_lists() {
    let server = TestServer::start().await;
    let g = server.guest("G").await;
    let events = g.call("get_rooms", Value::Null).await;
    assert_eq!(find(&events, "rooms_list"), Some(json!([])));
    assert_eq!(find(&events, "live_games"), Some(json!([])));
}

#[tokio::test(flavor = "multi_thread")]
async fn guest_and_registered_can_create_rooms() {
    let server = TestServer::start().await;
    let g = server.guest("GuestUser").await;
    let events = g.call("create_room", json!({"name": "TestRoom", "max_players": 4})).await;
    let joined = find(&events, "room_joined").expect("room_joined");
    assert_eq!(joined["room_name"], "TestRoom");
    assert_eq!(joined["is_owner"], true);
    let p = server.player(1).await;
    let events = p.call("create_room", json!({"name": "TestRoom2", "max_players": 4})).await;
    assert_eq!(find(&events, "room_joined").unwrap()["room_name"], "TestRoom2");
}

#[tokio::test(flavor = "multi_thread")]
async fn create_room_generates_a_six_digit_code_for_the_owner_only() {
    let server = TestServer::start().await;
    let owner = server.player(1).await;
    let events = owner.call("create_room", json!({"name": "CodeVis", "max_players": 4})).await;
    let code = find(&events, "room_code").unwrap()["code"].as_str().unwrap().to_string();
    assert_eq!(code.len(), 6);
    assert!(code.chars().all(|c| c.is_ascii_digit()));
    assert_eq!(events.iter().filter(|(n, _)| n == "room_code").count(), 1);
    let joiner = server.player(2).await;
    let events = joiner.call("join_room", json!({"code": code})).await;
    assert!(find(&events, "room_code").is_none(), "csak a tulajdonos kapja meg a kódot");
}

#[tokio::test(flavor = "multi_thread")]
async fn create_room_defaults_and_clamps() {
    let server = TestServer::start().await;
    let p = server.player(1).await;
    let events = p.call("create_room", json!({"name": "", "max_players": 10})).await;
    assert_eq!(find(&events, "room_joined").unwrap()["room_name"], "Szoba");
    let st = server.app.state.lock();
    assert_eq!(st.rooms.values().next().unwrap().max_players, 4);
}

#[tokio::test(flavor = "multi_thread")]
async fn create_room_validates_options() {
    let server = TestServer::start().await;
    let p = server.player(1).await;
    // érvénytelen időlimit → hiba, szoba nincs
    let events = p.call("create_room", json!({"name": "T", "max_players": 2, "turn_time_limit": 45})).await;
    assert!(find(&events, "room_joined").is_none() || room_count(&server) == 1);
    let events = p.call("create_room", json!({"name": "T2", "max_players": 2})).await;
    assert!(errors(&events).iter().all(|m| !m.is_empty()));
}

#[tokio::test(flavor = "multi_thread")]
async fn second_room_while_in_one_is_refused() {
    let server = TestServer::start().await;
    let p = server.player(1).await;
    p.call("create_room", json!({"name": "Egy", "max_players": 2})).await;
    let events = p.call("create_room", json!({"name": "Kettő", "max_players": 2})).await;
    assert_eq!(errors(&events), vec!["Már egy szobában vagy. Előbb lépj ki belőle."]);
    assert_eq!(room_count(&server), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn create_room_is_rate_limited() {
    let server = TestServer::start().await;
    let p = server.player(1).await;
    for i in 0..4 {
        p.emit("create_room", json!({"name": format!("R{i}"), "max_players": 2}));
    }
    p.settle().await;
    let errs = errors(&p.take());
    assert!(errs.iter().any(|m| m == "Túl sok szoba létrehozás, várj egy kicsit."), "{errs:?}");
}

// ---------------------------------------------------------------------------------------------------
// join_room
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn join_by_code_and_by_id() {
    let server = TestServer::start().await;
    let owner = server.player(1).await;
    owner.call("create_room", json!({"name": "JoinTest", "max_players": 4})).await;
    let c2 = server.guest("Joiner").await;
    let events = c2.call("join_room", json!({"code": owner.code()})).await;
    let joined = find(&events, "room_joined").unwrap();
    assert_eq!(joined["room_name"], "JoinTest");
    assert_eq!(joined["is_owner"], false);
    let c3 = server.guest("Joiner2").await;
    let events = c3.call("join_room", json!({"room_id": owner.room()})).await;
    assert!(find(&events, "room_joined").is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn join_errors() {
    let server = TestServer::start().await;
    let owner = server.player(1).await;
    owner.call("create_room", json!({"name": "FullRoom", "max_players": 2})).await;
    let g = server.guest("G").await;
    assert_eq!(errors(&g.call("join_room", json!({"code": "999999"})).await), vec!["Nincs ilyen kódú szoba."]);
    let events = g.call("join_room", json!({"code": "abc"})).await;
    assert!(errors(&events)[0].contains("Érvénytelen"), "{events:?}");
    assert_eq!(errors(&g.call("join_room", json!({})).await), vec!["Szoba kód vagy azonosító szükséges."]);
    assert!(!errors(&g.call("join_room", json!({"room_id": "nonexist"})).await).is_empty());
    // megtelt
    let g2 = server.guest("G2").await;
    g2.call("join_room", json!({"code": owner.code()})).await;
    let g3 = server.guest("P3").await;
    let events = g3.call("join_room", json!({"code": owner.code()})).await;
    assert!(errors(&events)[0].contains("megtelt"));
    // már szobában
    let events = g2.call("join_room", json!({"code": owner.code()})).await;
    assert_eq!(errors(&events), vec!["Már egy szobában vagy. Előbb lépj ki belőle."]);
}

#[tokio::test(flavor = "multi_thread")]
async fn join_is_rate_limited() {
    let server = TestServer::start().await;
    let g = server.guest("G").await;
    for _ in 0..6 {
        g.emit("join_room", json!({"code": "999999"}));
    }
    g.settle().await;
    let errs = errors(&g.take());
    assert_eq!(errs.iter().filter(|m| m.as_str() == "Túl sok kérés, várj egy kicsit.").count(), 1, "{errs:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn private_rooms() {
    let server = TestServer::start().await;
    let owner = server.player(1).await;
    let events = owner.call("create_room", json!({"name": "PrivateRoom", "max_players": 4, "is_private": true})).await;
    assert_eq!(find(&events, "room_joined").unwrap()["is_private"], true);
    // a listában nincs, azonosítóval nem, kóddal igen
    let events = owner.call("get_rooms", Value::Null).await;
    assert_eq!(find(&events, "rooms_list"), Some(json!([])));
    let c2 = server.guest("Blocked").await;
    let events = c2.call("join_room", json!({"room_id": owner.room()})).await;
    assert!(errors(&events)[0].to_lowercase().contains("privát"));
    let events = c2.call("join_room", json!({"code": owner.code()})).await;
    let joined = find(&events, "room_joined").unwrap();
    assert_eq!(joined["room_name"], "PrivateRoom");
    assert_eq!(joined["is_private"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn public_rooms_are_listed_without_codes() {
    let server = TestServer::start().await;
    let owner = server.player(1).await;
    owner.call("create_room", json!({"name": "VisibleRoom", "max_players": 4, "is_private": false, "challenge_mode": true})).await;
    let events = owner.call("get_rooms", Value::Null).await;
    let rooms = find(&events, "rooms_list").unwrap();
    assert_eq!(rooms.as_array().unwrap().len(), 1);
    assert_eq!(rooms[0]["name"], "VisibleRoom");
    assert_eq!(rooms[0]["challenge_mode"], true);
    assert!(rooms[0].get("join_code").is_none() && rooms[0].get("code").is_none());
    let events = owner.call("get_rooms", Value::Null).await;
    assert!(find(&events, "live_games").is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn challenge_mode_is_reported() {
    let server = TestServer::start().await;
    let owner = server.player(1).await;
    let events = owner.call("create_room", json!({"name": "ChalRoom", "max_players": 4, "challenge_mode": true})).await;
    assert_eq!(find(&events, "room_joined").unwrap()["challenge_mode"], true);
    let c2 = server.guest("P2").await;
    let events = c2.call("join_room", json!({"code": owner.code()})).await;
    assert_eq!(find(&events, "room_joined").unwrap()["challenge_mode"], true);
    let p3 = server.player(3).await;
    let events = p3.call("create_room", json!({"name": "NormalRoom", "max_players": 4})).await;
    assert_eq!(find(&events, "room_joined").unwrap()["challenge_mode"], false);
    assert_eq!(find(&events, "room_joined").unwrap()["is_private"], false);
}

// ---------------------------------------------------------------------------------------------------
// leave_room / start_game
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn leaving_the_last_player_deletes_room_and_code() {
    let server = TestServer::start().await;
    let p = server.player(1).await;
    p.call("create_room", json!({"name": "LeaveTest", "max_players": 4})).await;
    assert_eq!(server.app.state.lock().join_codes.len(), 1);
    p.emit("send_chat", json!({"message": "Test"}));
    p.settle().await;
    let events = p.call("leave_room", Value::Null).await;
    assert!(find(&events, "room_left").is_some());
    let st = server.app.state.lock();
    assert!(st.rooms.is_empty());
    assert!(st.join_codes.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn owner_leaving_transfers_ownership() {
    let server = TestServer::start().await;
    let owner = server.player(1).await;
    owner.call("create_room", json!({"name": "OwnerTest", "max_players": 4})).await;
    let c2 = server.player(2).await;
    c2.call("join_room", json!({"code": owner.code()})).await;
    owner.call("leave_room", Value::Null).await;
    let st = server.app.state.lock();
    assert_eq!(st.rooms.len(), 1);
    assert_eq!(st.rooms.values().next().unwrap().owner_name, "Jatekos2");
}

#[tokio::test(flavor = "multi_thread")]
async fn start_game_rules() {
    let server = TestServer::start().await;
    let owner = server.player(1).await;
    owner.call("create_room", json!({"name": "StartTest", "max_players": 4})).await;
    let c2 = server.player(2).await;
    c2.call("join_room", json!({"code": owner.code()})).await;
    let events = c2.call("start_game", Value::Null).await;
    assert_eq!(errors(&events), vec!["Csak a szoba tulajdonosa indíthatja a játékot."]);
    let events = owner.call("start_game", Value::Null).await;
    assert!(find(&events, "game_started").is_some());
    let state = owner.state();
    assert_eq!(state["started"], true);
    assert_eq!(state["tiles_remaining"], 86);
    assert_eq!(state["players"].as_array().unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn solo_game_can_start() {
    let server = TestServer::start().await;
    let owner = server.player(1).await;
    owner.call("create_room", json!({"name": "Egyedül", "max_players": 4})).await;
    let events = owner.call("start_game", Value::Null).await;
    assert!(find(&events, "game_started").is_some());
    // nincs függő szavazás: elfogadni / elutasítani nem lehet
    let events = owner.call("accept_words", Value::Null).await;
    assert_eq!(find(&events, "action_result").unwrap()["success"], false);
    let events = owner.call("reject_words", Value::Null).await;
    assert_eq!(find(&events, "action_result").unwrap()["success"], false);
}

// ---------------------------------------------------------------------------------------------------
// chat
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn chat_reaches_everybody_in_the_room() {
    let server = TestServer::start().await;
    let owner = server.player(1).await;
    owner.call("create_room", json!({"name": "ChatAll", "max_players": 4})).await;
    let c2 = server.guest("P2Chat").await;
    c2.call("join_room", json!({"code": owner.code()})).await;
    owner.take();
    let events = owner.call("send_chat", json!({"message": "Hi all!"})).await;
    let chat = find(&events, "chat_message").unwrap();
    assert_eq!(chat["name"], "Jatekos1");
    assert_eq!(chat["message"], "Hi all!");
    assert_eq!(find(&c2.take(), "chat_message").unwrap()["message"], "Hi all!");
    let st = server.app.state.lock();
    assert_eq!(st.rooms.values().next().unwrap().chat_messages.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_chat_messages_are_dropped() {
    let server = TestServer::start().await;
    let p = server.player(1).await;
    // szobán kívül
    assert!(find(&p.call("send_chat", json!({"message": "Lost!"})).await, "chat_message").is_none());
    p.call("create_room", json!({"name": "ChatRoom", "max_players": 4})).await;
    for bad in ["", "   ", &"x".repeat(201)] {
        assert!(find(&p.call("send_chat", json!({"message": bad})).await, "chat_message").is_none(), "{bad:?}");
    }
    assert!(find(&p.call("send_chat", json!({"message": "x".repeat(200)})).await, "chat_message").is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn chat_is_rate_limited_and_stored() {
    let server = TestServer::start().await;
    let p = server.player(1).await;
    p.call("create_room", json!({"name": "ChatStore", "max_players": 4})).await;
    for i in 0..12 {
        p.emit("send_chat", json!({"message": format!("üzenet {i}")}));
    }
    p.settle_long().await;
    let events = p.take();
    assert_eq!(events.iter().filter(|(n, _)| n == "chat_message").count(), 10);
    assert_eq!(errors(&events).len(), 2);
    assert_eq!(errors(&events)[0], "Túl sok kérés, várj egy kicsit.");
    let st = server.app.state.lock();
    assert_eq!(st.rooms.values().next().unwrap().chat_messages.len(), 10);
}

// ---------------------------------------------------------------------------------------------------
// játék közbeni műveletek (egyszerű esetek)
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn placing_out_of_turn_or_nothing_fails() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "GameRoom", "max_players": 4})).await;
    let players = [&a, &b];
    let cur = current(&players);
    let other = others(&players, cur)[0];
    let events = other.call("place_tiles", word_tiles(7, 7, &["A"])).await;
    let result = find(&events, "action_result").unwrap();
    assert_eq!(result["success"], false);
    assert_eq!(result["message"], "Nem te következel.");
    let events = cur.call("place_tiles", json!({"tiles": []})).await;
    assert_eq!(find(&events, "action_result").unwrap()["success"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_real_word_is_placed_and_scored() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "GameRoom", "max_players": 4})).await;
    let players = [&a, &b];
    let cur = current(&players);
    let name = current_name(&server);
    set_hand(&server, &name, &["A", "L", "M", "A", "K", "E", "T"]);
    cur.emit("place_tiles", word_tiles(7, 5, &["A", "L", "M", "A"]));
    let result = cur.wait("action_result", 20).await.expect("action_result");
    assert_eq!(result["success"], true, "{result}");
    assert_eq!(result["own_turn"], true);
    assert!(result["score"].as_i64().unwrap() > 0);
    // a lerakott betűk pótlódtak a zsákból
    assert_eq!(hand_of(&server, &name).len(), 7);
}

// ---------------------------------------------------------------------------------------------------
// csere, passz, nézetek
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn exchange_and_pass_follow_the_turn_rules() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "GameRoom", "max_players": 4})).await;
    let players = [&a, &b];
    let cur = current(&players);
    let other = others(&players, cur)[0];
    // nem soros
    assert_eq!(find(&other.call("exchange_tiles", json!({"indices": [0]})).await, "action_result").unwrap()["success"], false);
    assert_eq!(find(&other.call("pass_turn", Value::Null).await, "action_result").unwrap()["success"], false);
    // érvénytelen index, üres lista
    let bad = find(&cur.call("exchange_tiles", json!({"indices": [99]})).await, "action_result").unwrap();
    assert_eq!(bad["success"], false);
    let none = find(&cur.call("exchange_tiles", json!({"indices": []})).await, "action_result").unwrap();
    assert_eq!(none["message"], "Legalább egy zsetont ki kell választani.");
    // sikeres csere: a kéz 7 zseton marad, a kör továbbmegy
    let before = cur.state()["turn_number"].as_i64().unwrap();
    let ok = find(&cur.call("exchange_tiles", json!({"indices": [0, 1]})).await, "action_result").unwrap();
    assert_eq!(ok["success"], true, "{ok}");
    assert_eq!(cur.state()["turn_number"].as_i64().unwrap(), before + 1);
    assert_eq!(cur.hand().len(), 7);
    // a másik passzol
    let passed = find(&other.call("pass_turn", Value::Null).await, "action_result").unwrap();
    assert_eq!(passed["success"], true);
    assert_eq!(passed["message"], "Passz.");
}

#[tokio::test(flavor = "multi_thread")]
async fn actions_outside_a_room_are_ignored() {
    let server = TestServer::start().await;
    let lost = server.guest("LostPlayer").await;
    let events = lost.call("place_tiles", word_tiles(7, 7, &["A"])).await;
    assert!(find(&events, "action_result").is_none());
    assert!(lost.call("pass_turn", Value::Null).await.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_state_is_broadcast_after_a_move() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "GameRoom", "max_players": 4})).await;
    let players = [&a, &b];
    let cur = current(&players);
    let other = others(&players, cur)[0];
    let name = current_name(&server);
    set_hand(&server, &name, &["A", "L", "M", "A", "K", "E", "T"]);
    cur.emit("place_tiles", word_tiles(7, 5, &["A", "L", "M", "A"]));
    cur.wait("action_result", 20).await.expect("result");
    other.settle().await;
    let state = other.peek("game_state");
    assert!(!state.is_empty());
    assert_eq!(state.last().unwrap()["board"][7][5]["letter"], "A");
}

// ---------------------------------------------------------------------------------------------------
// a tulajdonos kilépése
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn owner_leaving_an_active_game_disbands_the_room() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "DisbandTest", "max_players": 4})).await;
    assert_eq!(room_count(&server), 1);
    a.call("leave_room", Value::Null).await;
    assert_eq!(room_count(&server), 0);
    let events = b.take();
    let disbanded = find(&events, "room_disbanded").expect("room_disbanded");
    assert!(disbanded["message"].as_str().unwrap().contains("tulajdonosa kilépett"));
    assert!(find(&events, "room_left").is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_non_owner_leaving_an_active_game_keeps_the_room() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "NoDisband", "max_players": 4})).await;
    b.call("leave_room", Value::Null).await;
    assert_eq!(room_count(&server), 1);
    drop(a);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_new_owner_gets_the_code_and_can_start() {
    let server = TestServer::start().await;
    let owner = server.player(1).await;
    owner.call("create_room", json!({"name": "CodeTransfer", "max_players": 4})).await;
    let c2 = server.guest("NewLeader").await;
    c2.call("join_room", json!({"code": owner.code()})).await;
    owner.call("leave_room", Value::Null).await;
    let events = c2.take();
    assert_eq!(events.iter().filter(|(n, _)| n == "room_code").count(), 1, "az új tulajdonos megkapja a kódot");
    let events = c2.call("start_game", Value::Null).await;
    assert!(errors(&events).is_empty(), "{events:?}");
    assert!(find(&events, "game_started").is_some());
}

// ---------------------------------------------------------------------------------------------------
// körönkénti időlimit
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn turn_time_limit_is_validated_and_listed() {
    let server = TestServer::start().await;
    let p = server.player(1).await;
    let events = p.call("create_room", json!({"name": "TimedRoom", "max_players": 2, "turn_time_limit": 180})).await;
    assert_eq!(find(&events, "room_joined").unwrap()["turn_time_limit"], 180);
    let rooms = find(&events, "rooms_list").unwrap();
    assert!(rooms.as_array().unwrap().iter().any(|r| r["turn_time_limit"] == 180));
    p.call("leave_room", Value::Null).await;
    let events = p.call("create_room", json!({"name": "BadTimer", "max_players": 2, "turn_time_limit": 999})).await;
    assert_eq!(find(&events, "room_joined").unwrap()["turn_time_limit"], 0);
    p.call("leave_room", Value::Null).await;
    let events = p.call("create_room", json!({"name": "NoTimer", "max_players": 2})).await;
    assert_eq!(find(&events, "room_joined").unwrap()["turn_time_limit"], 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_state_carries_the_turn_timer() {
    let server = TestServer::start().await;
    let (a, _b) = started_pair(&server, json!({"name": "TimedRoom", "max_players": 2, "turn_time_limit": 60})).await;
    let state = a.state();
    assert_eq!(state["turn_time_limit"], 60);
    assert!(state["turn_timer_expires_at"].as_f64().unwrap() > 0.0);
}

#[tokio::test(flavor = "multi_thread")]
async fn withdrawing_a_pending_placement_over_the_socket() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "WithdrawRoom", "max_players": 2, "challenge_mode": true})).await;
    let players = [&a, &b];
    let cur = current(&players);
    let other = others(&players, cur)[0];
    let name = current_name(&server);
    set_hand(&server, &name, &["A", "L", "M", "A", "K", "E", "T"]);
    cur.emit("place_tiles", word_tiles(7, 5, &["A", "L", "M", "A"]));
    cur.wait("action_result", 20).await.expect("result");
    other.settle().await;
    assert!(with_room(&server, |r| r.game.pending_challenge.is_some()));
    // nem a lerakó nem vonhatja vissza
    let result = find(&other.call("withdraw_words", Value::Null).await, "action_result").unwrap();
    assert_eq!(result["success"], false);
    assert!(with_room(&server, |r| r.game.pending_challenge.is_some()));
    // a lerakó igen: a betűk visszakerülnek
    let result = find(&cur.call("withdraw_words", Value::Null).await, "action_result").unwrap();
    assert_eq!(result["success"], true);
    assert_eq!(result["own_turn"], true);
    assert!(with_room(&server, |r| r.game.pending_challenge.is_none()));
    let mut hand = hand_of(&server, &name);
    hand.sort();
    assert_eq!(hand, vec!["A", "A", "E", "K", "L", "M", "T"]);
    let states = other.peek("game_state");
    assert_eq!(states.last().unwrap()["pending_challenge"], Value::Null);
    // a visszavont lerakásra már nem lehet szavazni
    let result = find(&other.call("accept_words", Value::Null).await, "action_result").unwrap();
    assert_eq!(result["success"], false);
}
