//! Barátok: HTTP végpontok, Socket.IO kérések és meghívók, online állapot (a Python `test_friends.py` szerver része;
//! az adatbázis-szintű részt a `db_tests.rs` fedi).

mod common;
use common::*;
use serde_json::{Value, json};

fn befriend(server: &TestServer, a: i64, b: i64) {
    assert!(server.app.db.send_friend_request(a, b).0);
    assert!(server.app.db.accept_friend_request(b, a).0);
}

fn named(events: &[(String, Value)], name: &str) -> Vec<Value> {
    events.iter().filter(|(n, _)| n == name).map(|(_, d)| d.clone()).collect()
}

fn succeeded(events: &[(String, Value)], name: &str) -> bool {
    named(events, name).iter().any(|d| d["success"] == json!(true))
}

fn is_online(server: &TestServer, uid: i64) -> bool {
    server.app.state.lock().is_user_online(uid)
}

// ===================================================================================================
// HTTP
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_friend_list_is_empty_for_a_new_user() {
    let server = TestServer::start().await;
    server.ensure_user(1);
    let http = server.http_as("u1@example.com").await;
    let resp = http.get("/api/auth/friends").await;
    assert_eq!(resp.status, 200);
    assert_eq!(resp.json()["friends"], json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_friend_list_requires_login() {
    let server = TestServer::start().await;
    assert_eq!(server.http().get("/api/auth/friends").await.status, 401);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_friend_list_has_friends_pending_and_sent_requests() {
    let server = TestServer::start().await;
    let (a, b, c, d) = (server.ensure_user(1), server.ensure_user(2), server.ensure_user(3), server.ensure_user(4));
    befriend(&server, a, b);
    assert!(server.app.db.send_friend_request(c, a).0); // c kérte a-t
    assert!(server.app.db.send_friend_request(a, d).0); // a kérte d-t
    let http = server.http_as("u1@example.com").await;
    let data = http.get("/api/auth/friends").await.json();
    assert_eq!(data["friends"][0]["online"], false);
    assert_eq!(data["friends"].as_array().unwrap().iter().map(|f| f["id"].as_i64().unwrap()).collect::<Vec<_>>(), vec![b]);
    assert_eq!(data["pending_requests"].as_array().unwrap().iter().map(|f| f["id"].as_i64().unwrap()).collect::<Vec<_>>(), vec![c]);
    assert_eq!(data["sent_requests"].as_array().unwrap().iter().map(|f| f["id"].as_i64().unwrap()).collect::<Vec<_>>(), vec![d]);
}

#[tokio::test(flavor = "multi_thread")]
async fn user_search_finds_other_users() {
    let server = TestServer::start().await;
    server.create_user("s1@test.com", "Search1");
    server.create_user("s2@test.com", "Search2");
    let http = server.http_as("s1@test.com").await;
    let resp = http.get("/api/auth/search-users?q=Search").await;
    assert_eq!(resp.status, 200);
    let users = resp.json()["users"].clone();
    assert_eq!(users.as_array().unwrap().len(), 1);
    assert_eq!(users[0]["display_name"], "Search2");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_short_search_query_returns_nothing() {
    let server = TestServer::start().await;
    server.create_user("s3@test.com", "Search3");
    server.create_user("s4@test.com", "Search4");
    let http = server.http_as("s3@test.com").await;
    let resp = http.get("/api/auth/search-users?q=S").await;
    assert_eq!(resp.status, 200);
    assert!(resp.json()["users"].as_array().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn user_search_requires_login() {
    let server = TestServer::start().await;
    assert_eq!(server.http().get("/api/auth/search-users?q=Search").await.status, 401);
}

// ===================================================================================================
// Socket.IO: barátkérések
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn sending_a_friend_request_notifies_both_sides() {
    let server = TestServer::start().await;
    let a = server.player(1).await;
    let b = server.player(2).await;
    let b_id = server.user_id("u2@example.com");
    let got = a.call("send_friend_request", json!({"friend_id": b_id})).await;
    assert!(succeeded(&got, "friend_request_result"), "{got:?}");
    b.settle().await;
    assert!(!named(&b.take(), "friend_request_received").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guest_cannot_send_a_friend_request() {
    let server = TestServer::start().await;
    let b_id = server.ensure_user(2);
    let guest = server.guest("Vendég").await;
    let got = guest.call("send_friend_request", json!({"friend_id": b_id})).await;
    assert!(!succeeded(&got, "friend_request_result"), "{got:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn accepting_a_request_notifies_the_requester() {
    let server = TestServer::start().await;
    let (a_id, b_id) = (server.ensure_user(1), server.ensure_user(2));
    assert!(server.app.db.send_friend_request(a_id, b_id).0);
    let a = server.player(1).await;
    let b = server.player(2).await;
    let got = b.call("accept_friend_request", json!({"requester_id": a_id})).await;
    assert!(succeeded(&got, "friend_request_result"), "{got:?}");
    a.settle().await;
    assert!(!named(&a.take(), "friend_request_accepted").is_empty());
    assert_eq!(server.app.db.get_friends(a_id).len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn declining_a_request() {
    let server = TestServer::start().await;
    let (a_id, b_id) = (server.ensure_user(1), server.ensure_user(2));
    assert!(server.app.db.send_friend_request(a_id, b_id).0);
    let b = server.player(2).await;
    let got = b.call("decline_friend_request", json!({"requester_id": a_id})).await;
    assert!(succeeded(&got, "friend_request_result"), "{got:?}");
    assert!(server.app.db.get_pending_requests(b_id).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_a_friend() {
    let server = TestServer::start().await;
    let (a_id, b_id) = (server.ensure_user(1), server.ensure_user(2));
    befriend(&server, a_id, b_id);
    let a = server.player(1).await;
    let got = a.call("remove_friend", json!({"friend_id": b_id})).await;
    assert!(succeeded(&got, "friend_request_result"), "{got:?}");
    assert!(server.app.db.get_friends(a_id).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn malformed_friend_events_do_not_break_anything() {
    let server = TestServer::start().await;
    let a = server.player(1).await;
    relax_socket_limits(&server.app, &["send_friend_request", "accept_friend_request", "decline_friend_request", "remove_friend"]);
    for event in ["send_friend_request", "accept_friend_request", "decline_friend_request", "remove_friend"] {
        for payload in [json!("nem dict"), json!({}), json!({"friend_id": "x", "requester_id": "x"}), json!({"friend_id": [1], "requester_id": null})] {
            let got = a.call(event, payload.clone()).await;
            assert!(!succeeded(&got, "friend_request_result"), "{event} {payload}: {got:?}");
        }
    }
    assert!(!a.is_closed());
}

// ===================================================================================================
// Meghívók
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn inviting_a_friend_to_the_room() {
    let server = TestServer::start().await;
    let (a_id, b_id) = (server.ensure_user(1), server.ensure_user(2));
    befriend(&server, a_id, b_id);
    let a = server.player(1).await;
    let b = server.player(2).await;
    a.call("create_room", json!({"name": "testroom", "max_players": 4})).await;
    let got = a.call("invite_to_room", json!({"friend_id": b_id})).await;
    assert!(succeeded(&got, "invite_sent"), "{got:?}");
    let invite = b.wait("game_invite", 5).await.expect("meghívó");
    assert!(invite["invite_id"].is_string() || invite["invite_id"].is_number());
    assert!(b.peek("game_invite").is_empty(), "egyetlen meghívó érkezik");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_non_friend_cannot_be_invited() {
    let server = TestServer::start().await;
    let b_id = server.ensure_user(2);
    let a = server.player(1).await;
    a.call("create_room", json!({"name": "testroom", "max_players": 4})).await;
    let got = a.call("invite_to_room", json!({"friend_id": b_id})).await;
    let result = named(&got, "invite_sent").remove(0);
    assert_eq!(result["success"], false);
    assert!(result["message"].as_str().unwrap().to_lowercase().contains("barát"));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_invite_can_be_accepted() {
    let server = TestServer::start().await;
    let (a_id, b_id) = (server.ensure_user(1), server.ensure_user(2));
    befriend(&server, a_id, b_id);
    let a = server.player(1).await;
    let b = server.player(2).await;
    a.call("create_room", json!({"name": "testroom", "max_players": 4})).await;
    a.call("invite_to_room", json!({"friend_id": b_id})).await;
    let invite_id = b.wait("game_invite", 5).await.expect("meghívó")["invite_id"].clone();
    let got = b.call("respond_invite", json!({"invite_id": invite_id, "accept": true})).await;
    assert!(!named(&got, "invite_accepted").is_empty(), "{got:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_invite_can_be_declined_and_cannot_be_used_twice() {
    let server = TestServer::start().await;
    let (a_id, b_id) = (server.ensure_user(1), server.ensure_user(2));
    befriend(&server, a_id, b_id);
    let a = server.player(1).await;
    let b = server.player(2).await;
    a.call("create_room", json!({"name": "testroom", "max_players": 4})).await;
    a.call("invite_to_room", json!({"friend_id": b_id})).await;
    let invite_id = b.wait("game_invite", 5).await.expect("meghívó")["invite_id"].clone();
    let got = b.call("respond_invite", json!({"invite_id": invite_id, "accept": false})).await;
    assert!(named(&got, "invite_accepted").is_empty(), "{got:?}");
    let got = b.call("respond_invite", json!({"invite_id": invite_id, "accept": true})).await;
    assert!(named(&got, "invite_accepted").is_empty(), "az elutasított meghívó nem használható újra: {got:?}");
}

// ===================================================================================================
// Online állapot
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_online_status_follows_the_connection() {
    let server = TestServer::start().await;
    let uid = server.ensure_user(1);
    assert!(!is_online(&server, uid));
    let a = server.player(1).await;
    assert!(is_online(&server, uid));
    let sid = server.app.state.lock().get_user_sids(uid)[0].clone();
    server.app.state.lock().unregister_player(&sid);
    assert!(!is_online(&server, uid));
    let _ = a;
}

#[tokio::test(flavor = "multi_thread")]
async fn several_connections_of_one_user() {
    let server = TestServer::start().await;
    let uid = server.ensure_user(1);
    let _a = server.player(1).await;
    let _b = server.player(1).await;
    let sids = server.app.state.lock().get_user_sids(uid);
    assert_eq!(sids.len(), 2);
    server.app.state.lock().unregister_player(&sids[0]);
    assert!(is_online(&server, uid), "a másik kapcsolat miatt még online");
    server.app.state.lock().unregister_player(&sids[1]);
    assert!(!is_online(&server, uid));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_online_status_is_cleared_on_a_real_disconnect() {
    // regresszió: böngészőzárás után a játékos örökre online-nak látszott, és meghívható maradt
    let server = TestServer::start().await;
    let uid = server.ensure_user(1);
    let a = server.player(1).await;
    assert!(is_online(&server, uid));
    a.close();
    a.settle_long().await;
    assert!(!is_online(&server, uid));
    assert!(server.app.state.lock().get_user_sids(uid).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_online_status_is_restored_on_rejoin() {
    // regresszió: a visszacsatlakozó játékos offline-nak látszott a barátai számára a játék hátralévő ideje alatt
    let server = TestServer::start().await;
    let (c1, c2) = started_pair(&server, json!({"name": "RejoinRoom", "max_players": 2})).await;
    let uid = server.user_id("u1@example.com");
    let token = c1.token();
    assert!(is_online(&server, uid), "a játék elején online");
    c1.close();
    c1.settle_long().await;
    assert!(!is_online(&server, uid), "a lecsatlakozás után offline");
    let c1_new = Sio::connect(&server).await;
    c1_new.emit("rejoin_room", json!({"token": token}));
    c1_new.settle_long().await;
    let got = c1_new.take();
    assert!(got.iter().any(|(n, _)| n == "room_joined"), "{:?}", got.iter().map(|(n, _)| n).collect::<Vec<_>>());
    assert!(is_online(&server, uid), "a visszacsatlakozás után újra online");
    let _ = c2;
}

#[tokio::test(flavor = "multi_thread")]
async fn friends_see_the_presence_change_on_login_and_logout() {
    let server = TestServer::start().await;
    let (a_id, b_id) = (server.ensure_user(1), server.ensure_user(2));
    befriend(&server, a_id, b_id);
    let observer = server.player(1).await;
    observer.take();
    let subject = server.player(2).await;
    observer.settle().await;
    let events = observer.take();
    assert!(named(&events, "friend_presence_changed").iter().any(|e| e["friend_id"] == b_id && e["online"] == json!(true)), "{events:?}");
    subject.close();
    observer.settle_long().await;
    let events = observer.take();
    assert!(named(&events, "friend_presence_changed").iter().any(|e| e["friend_id"] == b_id && e["online"] == json!(false)), "{events:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn strangers_do_not_see_the_presence_change() {
    let server = TestServer::start().await;
    let observer = server.player(1).await;
    observer.take();
    let _subject = server.player(2).await;
    observer.settle().await;
    assert!(named(&observer.take(), "friend_presence_changed").is_empty());
}
