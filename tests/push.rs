//! Web Push: üzenetek, feliratkozások, VAPID kulcsok, küldés (hamis push szolgáltatással), HTTP végpontok és a
//! „Te jössz!” értesítés (a Python `test_push.py` megfelelője; a környezeti változós részek a `push_env.rs`-ben).

mod common;
use common::push::{PushSink, verify_vapid};
use common::*;
use scrabble::db::Db;
use scrabble::push::{self, Push};
use scrabble::server::core;
use serde_json::{Value, json};
use std::sync::Arc;

const ENDPOINT: &str = "https://push.example.com/send/abcdef123456";
const P256DH: &str = "BNcRdreALRFXTkOOUHK1EtK2wtaz5Ry4YfYCA_0QTpQtUbVlUls0VJXg7A8u-Ts1XbjhazAkj7I99e8QcYP7DkM";
const AUTH_KEY: &str = "tBHItJI5svbpez7KI4CCXg";

fn user(server: &TestServer, email: &str, name: &str) -> i64 {
    server.create_user(email, name)
}

fn save(server: &TestServer, uid: i64, endpoint: &str, lang: &str) {
    server.app.db.save_push_subscription(uid, endpoint, P256DH, AUTH_KEY, lang).unwrap();
}

fn subscribe(server: &TestServer, uid: i64, sink: &PushSink, path: &str, lang: &str) -> String {
    let sub = sink.subscription(path);
    server.app.db.save_push_subscription(uid, &sub.endpoint, &sub.p256dh, &sub.auth, lang).unwrap();
    sub.endpoint
}

// ===================================================================================================
// Üzenetek
// ===================================================================================================

#[test]
fn messages_in_hungarian_and_english() {
    let hu = push::build_message("turn", Some("hu"), "Kedd esti", "r1");
    let en = push::build_message("turn", Some("en"), "Tuesday", "r1");
    assert_eq!(hu["body"], "Te jössz! · Kedd esti");
    assert_eq!(hu["title"], "Magyar Scrabble");
    assert_eq!(en["body"], "It's your turn! · Tuesday");
    assert_eq!(hu["url"], "/");
    assert_eq!(hu["tag"], "turn-r1");
}

#[test]
fn an_unknown_language_falls_back_to_hungarian() {
    assert!(push::build_message("turn", Some("de"), "X", "")["body"].as_str().unwrap().starts_with("Te jössz"));
    assert!(push::build_message("turn", None, "X", "")["body"].as_str().unwrap().starts_with("Te jössz"));
}

// ===================================================================================================
// Feliratkozások
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn subscriptions_are_saved_and_listed() {
    let server = TestServer::start().await;
    let uid = user(&server, "a@example.com", "Anna");
    save(&server, uid, ENDPOINT, "en");
    let subs = server.app.db.get_push_subscriptions(uid);
    assert_eq!(subs.len(), 1);
    use scrabble::db::RowExt;
    assert_eq!((subs[0].text("endpoint"), subs[0].text("p256dh"), subs[0].text("auth"), subs[0].text("lang")), (ENDPOINT.into(), P256DH.into(), AUTH_KEY.into(), "en".into()));
    assert_eq!(server.app.db.count_push_subscriptions(uid), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn resubscribing_updates_instead_of_duplicating() {
    let server = TestServer::start().await;
    let uid = user(&server, "a@example.com", "Anna");
    save(&server, uid, ENDPOINT, "hu");
    save(&server, uid, ENDPOINT, "en");
    assert_eq!(server.app.db.count_push_subscriptions(uid), 1);
    use scrabble::db::RowExt;
    assert_eq!(server.app.db.get_push_subscriptions(uid)[0].text("lang"), "en");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_endpoint_moves_to_the_newest_account_on_the_device() {
    let server = TestServer::start().await;
    let (a, b) = (user(&server, "a@example.com", "A"), user(&server, "b@example.com", "B"));
    save(&server, a, ENDPOINT, "hu");
    save(&server, b, ENDPOINT, "hu");
    assert_eq!(server.app.db.count_push_subscriptions(a), 0);
    assert_eq!(server.app.db.count_push_subscriptions(b), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn deleting_is_scoped_to_the_owner() {
    let server = TestServer::start().await;
    let (a, b) = (user(&server, "a@example.com", "A"), user(&server, "b@example.com", "B"));
    save(&server, a, ENDPOINT, "hu");
    assert_eq!(server.app.db.delete_push_subscription(ENDPOINT, Some(b)), 0);
    assert_eq!(server.app.db.delete_push_subscription(ENDPOINT, Some(a)), 1);
    assert_eq!(server.app.db.count_push_subscriptions(a), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn subscriptions_follow_the_user_deletion() {
    let server = TestServer::start().await;
    let uid = user(&server, "a@example.com", "Anna");
    save(&server, uid, ENDPOINT, "hu");
    server.app.db.with(|tx| tx.execute("DELETE FROM users WHERE id = ?", [uid]).map(|_| ())).unwrap();
    assert!(server.app.db.get_push_subscriptions(uid).is_empty());
}

// ===================================================================================================
// VAPID kulcsok
// ===================================================================================================

fn temp_push() -> (Arc<Db>, Push, std::path::PathBuf) {
    let dir = temp_dir("push");
    let db = Arc::new(Db::open(dir.join("p.db").to_str().unwrap()).unwrap());
    db.init().unwrap();
    let push = Push::new(db.clone(), Box::new(|| "scrabble@example.com".to_string()));
    (db, push, dir)
}

#[test]
fn the_vapid_key_is_generated_once_and_persisted() {
    let (db, push, dir) = temp_push();
    let first = push.public_key();
    assert!(first.len() > 80 && !first.contains('='), "{first}");
    assert!(db.get_setting("vapid_private_pem").unwrap().starts_with("-----BEGIN"));
    // újraindítás: ugyanaz a kulcs
    let again = Push::new(db.clone(), Box::new(|| "x@example.com".to_string()));
    assert_eq!(again.public_key(), first);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_subject_defaults_to_the_sender_address() {
    let (_db, push, dir) = temp_push();
    assert_eq!(push.subject(), "mailto:scrabble@example.com");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_subject_has_a_fallback_without_a_sender() {
    let dir = temp_dir("push");
    let db = Arc::new(Db::open(dir.join("p.db").to_str().unwrap()).unwrap());
    db.init().unwrap();
    let push = Push::new(db, Box::new(String::new));
    assert!(push.subject().starts_with("mailto:"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_vapid_header_verifies_with_the_public_key() {
    let (_db, push, dir) = temp_push();
    let header = push.authorization("https://push.example.com/send/x", 4_000_000_000).unwrap();
    let claims = verify_vapid(&header).expect("érvényes aláírás");
    assert_eq!(claims["aud"], "https://push.example.com");
    assert_eq!(claims["exp"], 4_000_000_000u64);
    assert_eq!(claims["sub"], "mailto:scrabble@example.com");
    assert!(header.ends_with(&format!("k={}", push.public_key())));
    // érvénytelen végpont: nincs fejléc
    assert!(push.authorization("nem url", 1).is_none());
    let _ = std::fs::remove_dir_all(dir);
}

// ===================================================================================================
// Küldés
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn every_device_is_notified_in_its_language() {
    let server = TestServer::start().await;
    let sink = PushSink::start();
    let uid = user(&server, "a@example.com", "Anna");
    subscribe(&server, uid, &sink, "send/one", "hu");
    subscribe(&server, uid, &sink, "send/two", "en");
    assert_eq!(server.app.push.notify_user(uid, "turn", "Szoba", "r1"), 2);
    let got = sink.wait_for(2, 5).await;
    assert_eq!(got.len(), 2);
    let bodies: std::collections::BTreeSet<String> = sink.bodies().into_iter().collect();
    let expected: std::collections::BTreeSet<String> = ["Te jössz! · Szoba", "It's your turn! · Szoba"].iter().map(|s| s.to_string()).collect();
    assert_eq!(bodies, expected);
    for request in &got {
        assert_eq!(request.header("content-encoding"), Some("aes128gcm"));
        assert_eq!(request.header("ttl"), Some(push::TTL_SECONDS.to_string().as_str()));
        let claims = verify_vapid(request.header("authorization").unwrap()).expect("érvényes VAPID aláírás");
        assert!(claims["sub"].as_str().unwrap().starts_with("mailto:"));
        assert!(claims["aud"].as_str().unwrap().starts_with("http://127.0.0.1:"));
        assert_eq!(request.payload["tag"], "turn-r1");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn expired_subscriptions_are_removed() {
    for status in [404u16, 410] {
        let server = TestServer::start().await;
        let sink = PushSink::start();
        sink.set_status(status);
        let uid = user(&server, "a@example.com", "Anna");
        subscribe(&server, uid, &sink, "send/x", "hu");
        assert_eq!(server.app.push.notify_user(uid, "turn", "X", "r"), 0);
        assert_eq!(server.app.db.count_push_subscriptions(uid), 0, "{status}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn temporary_errors_keep_the_subscription() {
    let server = TestServer::start().await;
    let sink = PushSink::start();
    sink.set_status(500);
    let uid = user(&server, "a@example.com", "Anna");
    subscribe(&server, uid, &sink, "send/x", "hu");
    assert_eq!(server.app.push.notify_user(uid, "turn", "X", "r"), 0);
    assert_eq!(server.app.db.count_push_subscriptions(uid), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn network_errors_never_panic_and_keep_the_subscription() {
    let server = TestServer::start().await;
    let uid = user(&server, "a@example.com", "Anna");
    // a végponton senki sem figyel
    let sink = PushSink::start();
    let sub = sink.subscription("send/x");
    drop(sink);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    server.app.db.save_push_subscription(uid, &sub.endpoint, &sub.p256dh, &sub.auth, "hu").unwrap();
    assert_eq!(server.app.push.notify_user(uid, "turn", "X", "r"), 0);
    assert_eq!(server.app.db.count_push_subscriptions(uid), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_invalid_subscription_key_is_skipped() {
    let server = TestServer::start().await;
    let uid = user(&server, "a@example.com", "Anna");
    save(&server, uid, "http://127.0.0.1:9/x", "hu"); // a kulcs nem érvényes pont
    assert_eq!(server.app.push.notify_user(uid, "turn", "X", "r"), 0);
    assert_eq!(server.app.db.count_push_subscriptions(uid), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn notify_turn_without_a_subscription_does_nothing() {
    let server = TestServer::start().await;
    let uid = user(&server, "a@example.com", "Anna");
    assert!(!server.app.push.notify_turn(uid, "Szoba", "r1", "turn"));
}

#[tokio::test(flavor = "multi_thread")]
async fn notify_turn_runs_in_the_background() {
    let server = TestServer::start().await;
    let sink = PushSink::start();
    let uid = user(&server, "a@example.com", "Anna");
    subscribe(&server, uid, &sink, "send/x", "hu");
    assert!(server.app.push.notify_turn(uid, "Szoba", "r1", "turn"));
    let got = sink.wait_for(1, 5).await;
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].payload["body"], "Te jössz! · Szoba");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_test_message_counts_sent_removed_and_failed() {
    let server = TestServer::start().await;
    let sink = PushSink::start();
    let uid = user(&server, "a@example.com", "Anna");
    subscribe(&server, uid, &sink, "send/x", "en");
    let result = server.app.push.send_test(uid);
    assert_eq!(result, json!({"sent": 1, "removed": 0, "failed": 0}));
    assert_eq!(sink.payloads()[0]["body"], "Test notification: notifications are working.");
    sink.set_status(500);
    assert_eq!(server.app.push.send_test(uid), json!({"sent": 0, "removed": 0, "failed": 1}));
    sink.set_status(410);
    assert_eq!(server.app.push.send_test(uid), json!({"sent": 0, "removed": 1, "failed": 0}));
    assert_eq!(server.app.db.count_push_subscriptions(uid), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_custom_message_carries_title_body_and_url() {
    let server = TestServer::start().await;
    let sink = PushSink::start();
    let uid = user(&server, "a@example.com", "Anna");
    subscribe(&server, uid, &sink, "send/x", "hu");
    assert_eq!(server.app.push.send_custom(uid, "Cím", "Szöveg", "/valami"), json!({"sent": 1, "removed": 0, "failed": 0}));
    let payload = sink.payloads().remove(0);
    assert_eq!((payload["title"].clone(), payload["body"].clone(), payload["url"].clone()), (json!("Cím"), json!("Szöveg"), json!("/valami")));
}

// ===================================================================================================
// HTTP végpontok
// ===================================================================================================

fn subscription_body(lang: Option<&str>) -> Value {
    let mut body = json!({"endpoint": ENDPOINT, "keys": {"p256dh": P256DH, "auth": AUTH_KEY}});
    if let Some(lang) = lang {
        body["lang"] = json!(lang);
    }
    body
}

#[tokio::test(flavor = "multi_thread")]
async fn the_public_key_requires_login() {
    let server = TestServer::start().await;
    assert_eq!(server.http().get("/api/push/public-key").await.status, 401);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_public_key_is_served_with_the_subscription_state() {
    let server = TestServer::start().await;
    let uid = server.ensure_user(1);
    let http = server.http_as("u1@example.com").await;
    let data = http.get("/api/push/public-key").await.json();
    assert_eq!(data["available"], true);
    assert_eq!(data["public_key"], json!(server.app.push.public_key()));
    assert_eq!(data["subscribed"], false);
    save(&server, uid, ENDPOINT, "hu");
    assert_eq!(http.get("/api/push/public-key").await.json()["subscribed"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn subscribing_and_unsubscribing() {
    let server = TestServer::start().await;
    let uid = server.ensure_user(1);
    let http = server.http_as("u1@example.com").await;
    assert_eq!(http.post("/api/push/subscribe", subscription_body(Some("en"))).await.json()["success"], true);
    use scrabble::db::RowExt;
    assert_eq!(server.app.db.get_push_subscriptions(uid)[0].text("lang"), "en");
    assert_eq!(http.post("/api/push/unsubscribe", json!({"endpoint": ENDPOINT})).await.json()["success"], true);
    assert_eq!(server.app.db.count_push_subscriptions(uid), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_language_is_stored_as_hungarian() {
    let server = TestServer::start().await;
    let uid = server.ensure_user(1);
    let http = server.http_as("u1@example.com").await;
    http.post("/api/push/subscribe", subscription_body(Some("xx"))).await;
    use scrabble::db::RowExt;
    assert_eq!(server.app.db.get_push_subscriptions(uid)[0].text("lang"), "hu");
}

#[tokio::test(flavor = "multi_thread")]
async fn subscription_validation() {
    let bodies = [
        json!({}),
        json!({"endpoint": "http://insecure.example.com/x", "keys": {"p256dh": P256DH, "auth": AUTH_KEY}}),
        json!({"endpoint": ENDPOINT}),
        json!({"endpoint": ENDPOINT, "keys": {"p256dh": "rovid", "auth": AUTH_KEY}}),
        json!({"endpoint": ENDPOINT, "keys": {"p256dh": P256DH, "auth": 5}}),
        json!({"endpoint": 5, "keys": {}}),
        json!({"endpoint": ENDPOINT, "keys": "nem dict"}),
    ];
    let server = TestServer::start().await;
    let uid = server.ensure_user(1);
    let http = server.http_as("u1@example.com").await;
    for body in bodies {
        assert_eq!(http.post("/api/push/subscribe", body.clone()).await.status, 400, "{body}");
    }
    assert_eq!(server.app.db.count_push_subscriptions(uid), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_endpoints_require_login_and_are_rate_limited() {
    let server = TestServer::start().await;
    let anonymous = server.http();
    assert_eq!(anonymous.post("/api/push/subscribe", subscription_body(None)).await.status, 401);
    assert_eq!(anonymous.post("/api/push/unsubscribe", json!({"endpoint": ENDPOINT})).await.status, 401);
    server.ensure_user(1);
    let http = server.http_as("u1@example.com").await;
    restore_default_limits(&server.app);
    let mut last = 0;
    for _ in 0..21 {
        last = http.post("/api/push/subscribe", subscription_body(None)).await.status;
    }
    assert_eq!(last, 429);
}

#[tokio::test(flavor = "multi_thread")]
async fn unsubscribing_cannot_remove_someone_elses_subscription() {
    let server = TestServer::start().await;
    let a = server.ensure_user(1);
    server.ensure_user(2);
    save(&server, a, ENDPOINT, "hu");
    let http = server.http_as("u2@example.com").await;
    http.post("/api/push/unsubscribe", json!({"endpoint": ENDPOINT})).await;
    assert_eq!(server.app.db.count_push_subscriptions(a), 1);
}

// ===================================================================================================
// A szerver: „Te jössz!”
// ===================================================================================================

/// A soron lévő és a várakozó játékos (Sio, user_id).
struct Turn<'a> {
    mover: &'a Sio,
    waiting: &'a Sio,
    waiting_id: i64,
    mover_id: i64,
}

fn turn<'a>(server: &TestServer, a: &'a Sio, b: &'a Sio) -> Turn<'a> {
    let mover = current(&[a, b]);
    let waiting = if std::ptr::eq(mover, a) { b } else { a };
    let id_of = |sio: &Sio| server.user_id(&format!("u{}@example.com", sio.name_n));
    Turn { mover, waiting, waiting_id: id_of(waiting), mover_id: id_of(mover) }
}

fn room_id(server: &TestServer) -> String {
    server.app.state.lock().rooms.keys().next().cloned().unwrap()
}

fn emit_states(server: &TestServer) {
    let id = room_id(server);
    let mut st = server.app.state.lock();
    core::emit_all_states(&server.app, &mut st, &id);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_disconnected_player_is_notified_once() {
    let server = TestServer::start().await;
    let sink = PushSink::start();
    let (a, b) = started_pair(&server, json!({"name": "Esti játék", "max_players": 2})).await;
    let t = turn(&server, &a, &b);
    subscribe(&server, t.waiting_id, &sink, "send/x", "hu");
    t.mover.emit0("pass_turn"); // a kör átmegy, a másik játékos online és látható
    t.mover.settle().await;
    assert!(sink.stays_silent().await);
    with_room(&server, |room| {
        let idx = room.game.current_player_idx;
        room.game.players[idx].disconnected = true;
    }); // közben lecsatlakozik
    emit_states(&server);
    assert_eq!(sink.wait_for(1, 5).await.len(), 1);
    assert_eq!(sink.bodies(), vec!["Te jössz! · Esti játék".to_string()]);
    emit_states(&server); // ugyanaz a kör: nincs újabb értesítés
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert_eq!(sink.received().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_online_visible_player_gets_no_push() {
    let server = TestServer::start().await;
    let sink = PushSink::start();
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2})).await;
    let t = turn(&server, &a, &b);
    subscribe(&server, t.waiting_id, &sink, "send/x", "hu");
    t.mover.emit0("pass_turn");
    t.mover.settle().await;
    assert!(sink.stays_silent().await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hidden_tab_triggers_the_push() {
    let server = TestServer::start().await;
    let sink = PushSink::start();
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2})).await;
    let t = turn(&server, &a, &b);
    subscribe(&server, t.waiting_id, &sink, "send/x", "hu");
    t.waiting.emit("set_visibility", json!({"hidden": true})); // még nem az ő köre
    t.waiting.settle().await;
    assert!(sink.stays_silent().await);
    t.mover.emit0("pass_turn"); // most ő jön, és a lapja rejtett
    assert_eq!(sink.wait_for(1, 5).await.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn hiding_during_your_own_turn_pushes_immediately_once() {
    let server = TestServer::start().await;
    let sink = PushSink::start();
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2})).await;
    let t = turn(&server, &a, &b);
    subscribe(&server, t.mover_id, &sink, "send/x", "hu");
    t.mover.emit("set_visibility", json!({"hidden": true}));
    assert_eq!(sink.wait_for(1, 5).await.len(), 1);
    t.mover.emit("set_visibility", json!({"hidden": true})); // ugyanarra a körre nem ismétel
    t.mover.settle().await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(sink.received().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn returning_to_the_tab_cancels_the_hidden_state() {
    let server = TestServer::start().await;
    let sink = PushSink::start();
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2})).await;
    let t = turn(&server, &a, &b);
    subscribe(&server, t.waiting_id, &sink, "send/x", "hu");
    t.waiting.emit("set_visibility", json!({"hidden": true}));
    t.waiting.emit("set_visibility", json!({"hidden": false}));
    t.waiting.settle().await;
    assert!(server.app.state.lock().hidden_sids.is_empty());
    t.mover.emit0("pass_turn");
    t.mover.settle().await;
    assert!(sink.stays_silent().await);
}

#[tokio::test(flavor = "multi_thread")]
async fn no_subscription_means_no_push_and_the_turn_is_not_marked() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "R", "max_players": 2})).await;
    let t = turn(&server, &a, &b);
    t.waiting.emit("set_visibility", json!({"hidden": true}));
    t.waiting.settle().await;
    t.mover.emit0("pass_turn");
    t.mover.settle().await;
    assert_eq!(with_room(&server, |room| room.pushed_turn), -1); // nem jelöljük meg a kört, ha nem ment ki értesítés
}

#[tokio::test(flavor = "multi_thread")]
async fn guests_and_bots_are_never_notified() {
    let server = TestServer::start().await;
    let sink = PushSink::start();
    let guest = server.guest("Vendég").await;
    guest.call("create_room", json!({"name": "R", "max_players": 2, "ai_players": [3]})).await;
    guest.call("start_game", Value::Null).await;
    guest.emit("set_visibility", json!({"hidden": true}));
    guest.settle().await;
    emit_states(&server);
    assert!(sink.stays_silent().await);
    assert_eq!(with_room(&server, |room| room.pushed_turn), -1);
}

#[tokio::test(flavor = "multi_thread")]
async fn puzzle_rooms_are_never_notified() {
    let server = TestServer::start().await;
    let sink = PushSink::start();
    let uid = server.ensure_user(1);
    subscribe(&server, uid, &sink, "send/x", "hu");
    let board: Vec<Vec<Value>> = vec![vec![Value::Null; 15]; 15];
    server.app.db.save_daily_puzzle(&scrabble::daily::today_str(), &json!(board), &vec!["A".to_string(); 7], 10, &json!({"tiles": [], "words": []})).unwrap();
    let client = server.player(1).await;
    client.call("start_daily", Value::Null).await;
    client.emit("set_visibility", json!({"hidden": true}));
    client.settle().await;
    assert!(sink.stays_silent().await);
}

#[tokio::test(flavor = "multi_thread")]
async fn disconnecting_forgets_the_hidden_state() {
    let server = TestServer::start().await;
    let (a, _b) = started_pair(&server, json!({"name": "R", "max_players": 2})).await;
    a.emit("set_visibility", json!({"hidden": true}));
    a.settle().await;
    assert!(!server.app.state.lock().hidden_sids.is_empty());
    a.close();
    a.settle_long().await;
    assert!(server.app.state.lock().hidden_sids.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn malformed_visibility_events_are_ignored() {
    let server = TestServer::start().await;
    let (a, _b) = started_pair(&server, json!({"name": "R", "max_players": 2})).await;
    a.emit("set_visibility", json!("nem dict"));
    a.emit("set_visibility", json!({"hidden": "igen"}));
    a.settle().await;
    assert!(server.app.state.lock().hidden_sids.is_empty());
}
