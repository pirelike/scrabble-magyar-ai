//! Admin panel: kommunikáció (közlemény, karbantartási mód, push, e-mail) és a futásidejű beállítások (a Python
//! `test_admin_comm.py` megfelelője).

mod common;
use common::admin::*;
use common::push::PushSink;
use common::smtp::FakeSmtp;
use common::*;
use scrabble::admin::AdminContext;
use serde_json::{Value, json};

const REASON: &str = "Teszt indoklás";

struct Env {
    server: TestServer,
    api: Http,
}

async fn env() -> Env {
    let server = TestServer::start().await;
    let api = server.admin_http().await;
    Env { server, api }
}

fn arr(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

fn texts(items: &[Value]) -> Vec<String> {
    items.iter().map(|i| i["text_hu"].as_str().unwrap_or("").to_string()).collect()
}

async fn public(http: &Http) -> Vec<Value> {
    arr(&http.get("/api/announcements").await.json()["items"])
}

async fn patch_settings(e: &Env, changes: Value) -> Resp {
    e.api.admin_patch("/settings", json!({"changes": changes, "reason": REASON})).await
}

fn error_messages(events: &[(String, Value)]) -> Vec<String> {
    events.iter().filter(|(n, _)| n == "error").filter_map(|(_, d)| d["message"].as_str().map(|s| s.to_string())).collect()
}

fn has_event(events: &[(String, Value)], name: &str) -> bool {
    events.iter().any(|(n, _)| n == name)
}

// ===================================================================================================
// Közlemények
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn create_and_show_publicly() {
    let e = env().await;
    let data = json!({"text_hu": "Szombaton frissítünk.", "text_en": "Update on Saturday.", "kind": "warning"});
    let new_id = e.api.admin_post("/announcements", data).await.json()["id"].clone();
    let items = public(&e.server.http()).await;
    assert_eq!(items.len(), 1);
    assert_eq!(
        (items[0]["id"].clone(), items[0]["kind"].clone(), items[0]["text_hu"].clone(), items[0]["text_en"].clone()),
        (new_id.clone(), json!("warning"), json!("Szombaton frissítünk."), json!("Update on Saturday."))
    );
    assert_eq!(e.api.admin_get("/announcements").await.json()["items"][0]["status"], "active");
    let row = audit_of(&e.server, "comm.announcement_create")[0]["details"].clone();
    assert_eq!((row["text_hu"].clone(), row["announcement_id"].clone()), (json!("Szombaton frissítünk."), new_id));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_english_text_falls_back_to_hungarian() {
    let e = env().await;
    e.api.admin_post("/announcements", json!({"text_hu": "Csak magyarul."})).await;
    assert_eq!(public(&e.server.http()).await[0]["text_en"], "Csak magyarul.");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_validity_window() {
    let e = env().await;
    e.api.admin_post("/announcements", json!({"text_hu": "Jövőbeli", "starts_at": "2999-01-01"})).await;
    e.api.admin_post("/announcements", json!({"text_hu": "Lejárt", "ends_at": "2000-01-01"})).await;
    e.api.admin_post("/announcements", json!({"text_hu": "Most érvényes", "starts_at": "2000-01-01", "ends_at": "2999-01-01"})).await;
    assert_eq!(texts(&public(&e.server.http()).await), vec!["Most érvényes"]);
    let statuses: std::collections::HashMap<String, String> = arr(&e.api.admin_get("/announcements").await.json()["items"])
        .iter()
        .map(|i| (i["text_hu"].as_str().unwrap().to_string(), i["status"].as_str().unwrap().to_string()))
        .collect();
    assert_eq!(
        statuses,
        std::collections::HashMap::from([
            ("Jövőbeli".to_string(), "scheduled".to_string()),
            ("Lejárt".to_string(), "expired".to_string()),
            ("Most érvényes".to_string(), "active".to_string())
        ])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_audience() {
    let e = env().await;
    e.api.admin_post("/announcements", json!({"text_hu": "Mindenkinek", "audience": "all"})).await;
    e.api.admin_post("/announcements", json!({"text_hu": "Bejelentkezetteknek", "audience": "registered"})).await;
    e.api.admin_post("/announcements", json!({"text_hu": "Vendégeknek", "audience": "guests"})).await;
    let (_, user) = make_user(&e.server, "a@example.com", "Anna").await;
    assert_eq!(texts(&public(&e.server.http()).await), vec!["Mindenkinek", "Vendégeknek"]);
    assert_eq!(texts(&public(&user).await), vec!["Mindenkinek", "Bejelentkezetteknek"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn edit_and_revoke() {
    let e = env().await;
    let id = e.api.admin_post("/announcements", json!({"text_hu": "Régi"})).await.json()["id"].clone();
    assert_eq!(e.api.admin_patch(&format!("/announcements/{id}"), json!({"text_hu": "Új", "kind": "info"})).await.status, 200);
    assert_eq!(public(&e.server.http()).await[0]["text_hu"], "Új");
    let row = audit_of(&e.server, "comm.announcement_update")[0]["details"].clone();
    assert_eq!((row["before"]["text_hu"].clone(), row["after"]["text_hu"].clone()), (json!("Régi"), json!("Új")));
    assert_eq!(e.api.admin_delete(&format!("/announcements/{id}"), json!({})).await.status, 200);
    assert!(public(&e.server.http()).await.is_empty());
    assert_eq!(e.api.admin_delete(&format!("/announcements/{id}"), json!({})).await.status, 409);
    assert_eq!(e.api.admin_patch(&format!("/announcements/{id}"), json!({"text_hu": "Vissza", "active": true})).await.status, 200);
    assert_eq!(texts(&public(&e.server.http()).await), vec!["Vissza"]);
    assert_eq!(e.api.admin_delete("/announcements/999", json!({})).await.status, 404);
}

#[tokio::test(flavor = "multi_thread")]
async fn announcement_validation() {
    let cases = [
        json!({}),
        json!({"text_hu": "  "}),
        json!({"text_hu": "x".repeat(501)}),
        json!({"text_hu": "ok", "kind": "riadó"}),
        json!({"text_hu": "ok", "audience": "senki"}),
        json!({"text_hu": "ok", "starts_at": "holnap"}),
        json!({"text_hu": "ok", "starts_at": "2999-01-02", "ends_at": "2999-01-01"}),
    ];
    let e = env().await;
    for data in cases {
        assert_eq!(e.api.admin_post("/announcements", data.clone()).await.status, 400, "{data}");
        assert_eq!(e.api.admin_get("/announcements").await.json()["items"], json!([]), "{data}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_live_event() {
    let e = env().await;
    let client = e.server.guest("Anna").await;
    e.api.admin_post("/announcements", json!({"text_hu": "Élő", "audience": "all"})).await;
    client.settle().await;
    let events: Vec<Value> = client.take().into_iter().filter(|(n, _)| n == "announcement").map(|(_, d)| d).collect();
    assert_eq!(texts(&arr(&events[0]["guests"])), vec!["Élő"]);
    assert_eq!(events[0]["registered"][0]["text_hu"], "Élő");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_announcement_endpoint_is_public_and_rate_limited() {
    let e = env().await;
    let anon = e.server.http();
    assert_eq!(anon.get("/api/announcements").await.status, 200);
    restore_default_limits(&e.server.app);
    let mut statuses = Vec::new();
    for _ in 0..61 {
        statuses.push(anon.get("/api/announcements").await.status);
    }
    assert!(statuses[..60].iter().all(|s| *s == 200), "{statuses:?}");
    assert_eq!(statuses[60], 429);
}

// ===================================================================================================
// Karbantartási mód
// ===================================================================================================

const MAINTENANCE_MESSAGE: &str = "A szerver karbantartás alatt van, új játék most nem indítható.";

#[tokio::test(flavor = "multi_thread")]
async fn maintenance_blocks_new_games_but_not_the_running_ones() {
    let e = env().await;
    let (anna, bela) = (e.server.guest("Anna").await, e.server.guest("Béla").await);
    live_room(&e.server, &anna, &[&bela], json!({})).await;
    let response = e.api.admin_post("/maintenance", json!({"enabled": true, "message_hu": "Karbantartás!", "message_en": "Maintenance!"})).await;
    assert_eq!(response.status, 200, "{}", response.text());
    let cili = e.server.guest("Cili").await;
    let got = cili.call("create_room", json!({"name": "Új"})).await;
    assert_eq!(error_messages(&got), vec![MAINTENANCE_MESSAGE]);
    assert_eq!(room_count(&e.server), 1);
    anna.call("pass_turn", Value::Null).await; // a futó játék folytatódik
    assert_eq!(with_room(&e.server, |room| room.game.turn_number), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn other_ways_to_start_a_game_are_blocked_too() {
    let e = env().await;
    let a_id = e.server.create_user("a@example.com", "Anna");
    let client = sio_user(&e.server, a_id, "Anna").await;
    e.api.admin_post("/maintenance", json!({"enabled": true, "message_hu": "Karbantartás!"})).await;
    for (event, payload) in [("start_daily", Value::Null), ("create_async_game", json!({"friend_ids": [1]})), ("restore_game", json!({"game_id": 1}))] {
        let got = client.call(event, payload).await;
        assert_eq!(error_messages(&got), vec![MAINTENANCE_MESSAGE], "{event}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_admin_is_exempt() {
    let e = env().await;
    e.api.admin_post("/maintenance", json!({"enabled": true, "message_hu": "Karbantartás!"})).await;
    let boss = sio_user(&e.server, e.server.user_id(ADMIN_EMAIL), "Admin").await;
    let got = boss.call("create_room", json!({"name": "Admin szoba"})).await;
    assert!(has_event(&got, "room_joined"), "{got:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_banner_with_a_countdown() {
    let e = env().await;
    let state = e
        .api
        .admin_post("/maintenance", json!({"enabled": true, "message_hu": "Újraindulás.", "message_en": "Restarting.", "until": "2999-01-01 10:00"}))
        .await
        .json();
    assert!(state["enabled"] == true && state["active"] == true, "{state}");
    let banner = public(&e.server.http()).await[0].clone();
    assert_eq!((banner["id"].clone(), banner["kind"].clone()), (json!("maintenance"), json!("maintenance")));
    assert_eq!((banner["text_en"].clone(), banner["ends_at"].clone()), (json!("Restarting."), json!("2999-01-01T10:00:00Z")));
}

#[tokio::test(flavor = "multi_thread")]
async fn maintenance_ends_automatically() {
    let e = env().await;
    e.api.admin_post("/maintenance", json!({"enabled": true, "message_hu": "Hamarosan vége", "until": "2999-01-01"})).await;
    let mut value = e.server.app.settings.get(scrabble::settings::MAINTENANCE_KEY);
    value["until"] = json!("2000-01-01 00:00:00");
    let admin_id = e.server.user_id(ADMIN_EMAIL);
    e.server
        .app
        .db
        .with(|tx| {
            e.server.app.settings.store(tx, scrabble::settings::MAINTENANCE_KEY, &value, Some(admin_id)).map(|_| ()).map_err(|_| rusqlite::Error::InvalidQuery)
        })
        .unwrap();
    e.server.app.settings.invalidate();
    assert!(e.server.app.settings.maintenance().is_none());
    assert!(public(&e.server.http()).await.is_empty());
    let cili = e.server.guest("Cili").await;
    let got = cili.call("create_room", json!({"name": "Új"})).await;
    assert!(has_event(&got, "room_joined"), "{got:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn switching_maintenance_off() {
    let e = env().await;
    e.api.admin_post("/maintenance", json!({"enabled": true, "message_hu": "Karbantartás!"})).await;
    assert_eq!(e.api.admin_post("/maintenance", json!({"enabled": false})).await.json()["active"], false);
    assert!(public(&e.server.http()).await.is_empty());
    let rows = audit_of(&e.server, "comm.maintenance");
    assert_eq!(rows.iter().map(|r| r["details"]["after"]["enabled"].clone()).collect::<Vec<_>>(), vec![json!(false), json!(true)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn maintenance_validation() {
    let e = env().await;
    for data in [json!({}), json!({"enabled": "igen"}), json!({"enabled": true}), json!({"enabled": true, "message_hu": "ok", "until": "2000-01-01"})] {
        assert_eq!(e.api.admin_post("/maintenance", data.clone()).await.status, 400, "{data}");
    }
}

// ===================================================================================================
// Push
// ===================================================================================================

struct Devices {
    a: i64,
    b: i64,
    sink_a: PushSink,
    sink_b: PushSink,
}

fn add_device(e: &Env, uid: i64, sink: &PushSink, path: &str) {
    let sub = sink.subscription(path);
    e.server.app.db.save_push_subscription(uid, &sub.endpoint, &sub.p256dh, &sub.auth, "hu").unwrap();
}

async fn devices(e: &Env) -> Devices {
    let a = e.server.create_user("a@example.com", "Anna");
    let b = e.server.create_user("b@example.com", "Béla");
    e.server.create_user("c@example.com", "Cili"); // nincs feliratkozott eszköze
    let (sink_a, sink_b) = (PushSink::start(), PushSink::start());
    add_device(e, a, &sink_a, "a-device-1");
    add_device(e, a, &sink_a, "a-device-2");
    add_device(e, b, &sink_b, "b-device-1");
    Devices { a, b, sink_a, sink_b }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_push_preview() {
    let e = env().await;
    devices(&e).await;
    let data = e.api.admin_post("/push/preview", json!({"target": "group", "group": "all", "title": "Cím", "body": "Szöveg"})).await.json();
    assert_eq!((data["recipients"].clone(), data["devices"].clone()), (json!(2), json!(3)));
    let mut sample: Vec<String> = arr(&data["sample"]).iter().map(|s| s.as_str().unwrap().to_string()).collect();
    sample.sort();
    assert_eq!(sample, vec!["Anna", "Béla"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn sending_to_a_user() {
    let e = env().await;
    let d = devices(&e).await;
    let data = e.api.admin_post("/push", json!({"target": "user", "user_id": d.a, "title": "Cím", "body": "Szöveg"})).await.json();
    assert_eq!((data["recipients"].clone(), data["sent"].clone(), data["failed"].clone(), data["removed"].clone()), (json!(1), json!(2), json!(0), json!(0)));
    let got = d.sink_a.wait_for(2, 5).await;
    assert_eq!(got.len(), 2);
    let mut paths: Vec<String> = got.iter().map(|r| r.path.clone()).collect();
    paths.sort();
    assert_eq!(paths, vec!["/a-device-1", "/a-device-2"]);
    assert!(got.iter().all(|r| r.payload["title"] == "Cím" && r.payload["body"] == "Szöveg"));
    assert!(d.sink_b.received().is_empty());
    assert_eq!(audit_of(&e.server, "comm.push")[0]["details"], json!({"title": "Cím", "body": "Szöveg", "recipients": 1}));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_group_send_needs_the_confirmation() {
    let e = env().await;
    let d = devices(&e).await;
    let body = json!({"target": "group", "group": "all", "title": "Cím", "body": "Szöveg"});
    let response = e.api.admin_post("/push", body.clone()).await;
    assert_eq!(response.status, 400);
    assert_eq!(response.json()["needs_confirm"], true);
    assert!(d.sink_a.stays_silent().await && d.sink_b.stays_silent().await);
    let mut confirmed = body.clone();
    confirmed["confirm"] = json!(true);
    assert_eq!(e.api.admin_post("/push", confirmed).await.json()["sent"], 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_result_counts_failures_and_removed_subscriptions() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    let b = e.server.create_user("b@example.com", "Béla");
    let (ok, gone, broken) = (PushSink::start(), PushSink::start(), PushSink::start());
    gone.set_status(410);
    broken.set_status(500);
    add_device(&e, a, &ok, "a-device-1");
    add_device(&e, a, &gone, "a-device-2");
    add_device(&e, b, &broken, "b-device-1");
    let data = e.api.admin_post("/push", json!({"target": "group", "group": "all", "title": "T", "body": "B", "confirm": true})).await.json();
    assert_eq!((data["sent"].clone(), data["removed"].clone(), data["failed"].clone()), (json!(1), json!(1), json!(1)));
    assert_eq!(e.server.app.db.count_push_subscriptions(a) + e.server.app.db.count_push_subscriptions(b), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn nobody_to_send_to_and_validation() {
    let e = env().await;
    devices(&e).await;
    let c = e.server.user_id("c@example.com");
    assert_eq!(e.api.admin_post("/push", json!({"target": "user", "user_id": c, "title": "T", "body": "B"})).await.status, 409);
    assert_eq!(e.api.admin_post("/push", json!({"target": "user", "user_id": 999, "title": "T", "body": "B"})).await.status, 404);
    assert_eq!(e.api.admin_post("/push", json!({"target": "senki", "title": "T", "body": "B"})).await.status, 400);
    assert_eq!(e.api.admin_post("/push", json!({"target": "group", "group": "all", "title": "", "body": "B"})).await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_online_group() {
    let e = env().await;
    let d = devices(&e).await;
    let _anna = sio_user(&e.server, d.a, "Anna").await;
    let data = e.api.admin_post("/push/preview", json!({"target": "group", "group": "online", "title": "T", "body": "B"})).await.json();
    assert_eq!(data["sample"], json!(["Anna"]));
}

#[tokio::test(flavor = "multi_thread")]
async fn banned_users_are_not_recipients() {
    let e = env().await;
    let d = devices(&e).await;
    e.api.sudo().await;
    e.api.admin_post(&format!("/users/{}/ban", d.a), json!({"until": "1d", "reason": REASON})).await;
    let data = e.api.admin_post("/push/preview", json!({"target": "group", "group": "all", "title": "T", "body": "B"})).await.json();
    assert_eq!(data["sample"], json!(["Béla"]));
    let _ = d.b;
}

// ===================================================================================================
// E-mail
// ===================================================================================================

async fn env_with_smtp() -> (Env, FakeSmtp) {
    let e = env().await;
    let smtp = FakeSmtp::start();
    use_smtp(&e.server, &smtp);
    (e, smtp)
}

#[tokio::test(flavor = "multi_thread")]
async fn email_to_a_user() {
    let (e, smtp) = env_with_smtp().await;
    let a = e.server.create_user("a@example.com", "Anna");
    let data = e.api.admin_post("/email", json!({"user_id": a, "subject": "Üdv", "body": "Szöveg."})).await.json();
    assert_eq!(data, json!({"success": true, "sent": true, "console": false}));
    let mails = smtp.wait_for(1, 5).await;
    assert_eq!(mails[0].to, vec!["a@example.com".to_string()]);
    assert_eq!(mails[0].subject(), "Üdv");
    let body = mails[0].body();
    assert!(body.starts_with("Szia Anna!") && body.contains("Szöveg."), "{body}");
    assert_eq!(audit_of(&e.server, "comm.email")[0]["details"], json!({"subject": "Üdv", "body": "Szöveg."}));
}

#[tokio::test(flavor = "multi_thread")]
async fn without_smtp_it_goes_to_the_console() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    let data = e.api.admin_post("/email", json!({"user_id": a, "subject": "Üdv", "body": "Szöveg."})).await.json();
    assert_eq!((data["sent"].clone(), data["console"].clone()), (json!(false), json!(true)));
}

#[tokio::test(flavor = "multi_thread")]
async fn email_validation() {
    let (e, smtp) = env_with_smtp().await;
    let a = e.server.create_user("a@example.com", "Anna");
    for body in [
        json!({"subject": "", "body": "x"}),
        json!({"subject": "x", "body": ""}),
        json!({"subject": "két\nsor", "body": "x"}),
        json!({"subject": "x", "body": "y".repeat(5001)}),
    ] {
        let mut data = body.clone();
        data["user_id"] = json!(a);
        assert_eq!(e.api.admin_post("/email", data).await.status, 400);
    }
    assert!(smtp.mails().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_user_or_a_missing_id() {
    let (e, _smtp) = env_with_smtp().await;
    assert_eq!(e.api.admin_post("/email", json!({"user_id": 999, "subject": "x", "body": "y"})).await.status, 404);
    assert_eq!(e.api.admin_post("/email", json!({"subject": "x", "body": "y"})).await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn bulk_needs_sudo_confirmation_and_respects_the_rate_limit() {
    let (e, smtp) = env_with_smtp().await;
    scrabble::admin::comm::reset_bulk_cooldown();
    e.server.create_user("a@example.com", "Anna");
    e.server.create_user("b@example.com", "Béla");
    let body = json!({"group": "all", "subject": "Hírlevél", "body": "Szöveg", "reason": REASON});
    assert_eq!(e.api.admin_post("/email/bulk", body.clone()).await.status, 401);
    e.api.sudo().await;
    let response = e.api.admin_post("/email/bulk", body.clone()).await;
    assert_eq!(response.status, 400);
    assert_eq!(response.json()["needs_confirm"], true);
    assert!(smtp.mails().is_empty());
    let mut confirmed = body.clone();
    confirmed["confirm"] = json!(true);
    let data = e.api.admin_post("/email/bulk", confirmed.clone()).await.json();
    assert_eq!((data["recipients"].clone(), data["sent"].clone()), (json!(3), json!(3))); // az admin is felhasználó
    let mails = smtp.wait_for(3, 10).await;
    let recipients: std::collections::HashSet<String> = mails.iter().flat_map(|m| m.to.clone()).collect();
    assert_eq!(recipients, std::collections::HashSet::from([ADMIN_EMAIL.to_string(), "a@example.com".to_string(), "b@example.com".to_string()]));
    assert_eq!(e.api.admin_post("/email/bulk", confirmed).await.status, 429);
    assert_eq!(audit_of(&e.server, "comm.email_bulk")[0]["details"]["recipients"], 3);
    let _ = AdminContext { admin_user_id: 0, ip: String::new(), user_agent: String::new() };
}

#[tokio::test(flavor = "multi_thread")]
async fn the_bulk_preview() {
    let e = env().await;
    e.server.create_user("a@example.com", "Anna");
    assert_eq!(e.api.admin_get("/email/preview?group=all").await.json()["recipients"], 2);
    assert_eq!(e.api.admin_get("/email/preview?group=online").await.status, 400);
}

// ===================================================================================================
// Futásidejű beállítások
// ===================================================================================================

async fn settings_items(e: &Env) -> std::collections::HashMap<String, Value> {
    let data = e.api.admin_get("/settings").await.json();
    arr(&data["items"]).into_iter().map(|i| (i["key"].as_str().unwrap().to_string(), i)).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn listing_the_settings() {
    let e = env().await;
    let data = e.api.admin_get("/settings").await.json();
    let items: std::collections::HashMap<String, Value> = arr(&data["items"]).into_iter().map(|i| (i["key"].as_str().unwrap().to_string(), i)).collect();
    let grace = &items["grace_disconnect"];
    for (key, expected) in [
        ("value", json!(120)),
        ("default", json!(120)),
        ("overridden", json!(false)),
        ("min", json!(15)),
        ("max", json!(1800)),
        ("type", json!("int")),
        ("group", json!("timing")),
    ] {
        assert_eq!(grace[key], expected, "{key}");
    }
    for key in ["changed_by", "changed_at", "changed_by_name"] {
        assert!(grace[key].is_null(), "{key}");
    }
    assert_eq!(items["room_creation"]["choices"], json!(["everyone", "registered", "none"]));
    assert!(!items.contains_key("maintenance"));
    let groups: Vec<String> = arr(&data["groups"]).iter().map(|g| g.as_str().unwrap().to_string()).collect();
    assert!(["access", "bots", "timing"].iter().all(|g| groups.contains(&g.to_string())), "{groups:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn change_reset_and_the_audit_trail() {
    let e = env().await;
    assert_eq!(patch_settings(&e, json!({"grace_disconnect": 60, "chat_max_length": 100})).await.status, 200);
    let items = settings_items(&e).await;
    let grace = &items["grace_disconnect"];
    assert_eq!((grace["value"].clone(), grace["overridden"].clone(), grace["default"].clone()), (json!(60), json!(true), json!(120)));
    assert_eq!(grace["changed_by_name"], "Admin");
    assert!(!grace["changed_at"].is_null());
    let row = audit_of(&e.server, "settings.update")[0]["details"].clone();
    assert_eq!(row["before"], json!({"chat_max_length": 200, "grace_disconnect": 120}));
    assert_eq!(row["after"], json!({"chat_max_length": 100, "grace_disconnect": 60}));
    assert_eq!(patch_settings(&e, json!({"grace_disconnect": null})).await.status, 200);
    let grace = settings_items(&e).await["grace_disconnect"].clone();
    assert_eq!((grace["value"].clone(), grace["overridden"].clone(), grace["changed_by"].clone()), (json!(120), json!(false), Value::Null));
}

#[tokio::test(flavor = "multi_thread")]
async fn validation_is_all_or_nothing() {
    let cases = [
        json!({"grace_disconnect": 5}),
        json!({"grace_disconnect": "60"}),
        json!({"grace_disconnect": true}),
        json!({"registration_open": 1}),
        json!({"room_creation": "bárki"}),
        json!({"default_hint_limit": 4}),
        json!({"bot_think_multiplier": 9.0}),
        json!({"nincs_ilyen": 1}),
        json!({"maintenance": {"enabled": true}}),
        json!({"rate_limits_http": {"login": [0, 5]}}),
        json!({"rate_limits_http": "sok"}),
        json!({"rate_limits_socket": {"create_room": [3]}}),
    ];
    let e = env().await;
    for changes in cases {
        let mut all = changes.clone();
        all["chat_max_length"] = json!(100);
        assert_eq!(patch_settings(&e, all).await.status, 400, "{changes}");
        assert_eq!(settings_items(&e).await["chat_max_length"]["overridden"], false, "{changes}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reason_and_a_change_are_required() {
    let e = env().await;
    assert_eq!(e.api.admin_patch("/settings", json!({"changes": {"chat_max_length": 100}})).await.status, 400);
    assert_eq!(patch_settings(&e, json!({})).await.status, 400);
    assert_eq!(e.api.admin_patch("/settings", json!({"reason": REASON})).await.status, 400);
}

// --- hatás futás közben ---

#[tokio::test(flavor = "multi_thread")]
async fn registration_can_be_closed() {
    let e = env().await;
    patch_settings(&e, json!({"registration_open": false})).await;
    let anon = e.server.http();
    let register = json!({"email": "uj@example.com", "password": PASSWORD, "display_name": "Új"});
    for (path, body) in [("/api/auth/request-code", json!({"email": "uj@example.com"})), ("/api/auth/register", register)] {
        let response = anon.post(path, body).await;
        assert_eq!(response.status, 403, "{path}");
        assert_eq!(response.json()["message"], "A regisztráció jelenleg le van zárva.");
    }
    patch_settings(&e, json!({"registration_open": null})).await;
    assert_eq!(anon.post("/api/auth/request-code", json!({"email": "uj@example.com"})).await.status, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn guest_mode_can_be_turned_off() {
    let e = env().await;
    patch_settings(&e, json!({"guest_allowed": false})).await;
    let client = Sio::connect(&e.server).await;
    let got = client.call("set_name", json!({"name": "Vendég", "is_guest": true})).await;
    assert_eq!(error_messages(&got), vec!["A vendég mód jelenleg ki van kapcsolva."]);
    assert!(e.server.app.state.lock().player_names.is_empty());
    let a = e.server.create_user("a@example.com", "Anna");
    let _registered = sio_user(&e.server, a, "Anna").await; // a regisztrált belépés nem érintett
    assert_eq!(e.server.app.state.lock().player_names.values().cloned().collect::<Vec<_>>(), vec!["Anna"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_room_creation_policy() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    patch_settings(&e, json!({"room_creation": "registered"})).await;
    let guest = e.server.guest("Vendég").await;
    let got = guest.call("create_room", json!({"name": "X"})).await;
    assert_eq!(error_messages(&got), vec!["Szobát csak regisztrált felhasználó hozhat létre."]);
    let member = sio_user(&e.server, a, "Anna").await;
    assert!(has_event(&member.call("create_room", json!({"name": "X"})).await, "room_joined"));
    patch_settings(&e, json!({"room_creation": "none"})).await;
    let other = e.server.create_user("b@example.com", "Béla");
    let blocked = sio_user(&e.server, other, "Béla").await;
    let got = blocked.call("create_room", json!({"name": "Y"})).await;
    assert_eq!(error_messages(&got), vec!["Új szoba létrehozása jelenleg le van tiltva."]);
}

fn bots_in_the_last_room(server: &TestServer) -> usize {
    let st = server.app.state.lock();
    let room = st.rooms.values().max_by(|a, b| a.created_at.partial_cmp(&b.created_at).unwrap()).unwrap();
    room.game.players.iter().filter(|p| p.is_bot).count()
}

#[tokio::test(flavor = "multi_thread")]
async fn bot_settings() {
    let e = env().await;
    set_setting(&e.server, "bot_think_multiplier", json!(5.0));
    patch_settings(&e, json!({"max_bots": 1})).await;
    let client = e.server.guest("Anna").await;
    client.call("create_room", json!({"name": "X", "max_players": 4, "ai_players": [3, 5, 7]})).await;
    assert_eq!(bots_in_the_last_room(&e.server), 1);
    client.call("leave_room", Value::Null).await;
    patch_settings(&e, json!({"bots_enabled": false})).await;
    client.call("create_room", json!({"name": "Y", "max_players": 4, "ai_players": [3]})).await;
    assert_eq!(bots_in_the_last_room(&e.server), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_default_hint_limit() {
    let e = env().await;
    patch_settings(&e, json!({"default_hint_limit": 10})).await;
    let client = e.server.guest("Anna").await;
    let got = client.call("create_room", json!({"name": "X"})).await;
    let joined = got.iter().find(|(n, _)| n == "room_joined").map(|(_, d)| d.clone()).unwrap();
    assert_eq!(joined["hint_limit"], 10);
}

#[tokio::test(flavor = "multi_thread")]
async fn feature_switches() {
    let e = env().await;
    let (a, user) = make_user(&e.server, "a@example.com", "Anna").await;
    patch_settings(&e, json!({"feature_practice": false, "feature_word_review": false, "feature_daily": false, "feature_async": false})).await;
    assert_eq!(user.get("/api/practice/quiz").await.status, 503);
    assert_eq!(user.get("/api/practice/short-words?length=2").await.status, 503);
    assert_eq!(user.get("/api/practice/word-review").await.status, 503);
    assert_eq!(user.get("/api/daily").await.status, 503);
    assert_eq!(user.get("/api/async/games").await.status, 503);
    let client = sio_user(&e.server, a, "Anna").await;
    let got = client.call("start_daily", Value::Null).await;
    assert_eq!(error_messages(&got), vec!["Ez a funkció jelenleg ki van kapcsolva."]);
    patch_settings(&e, json!({"feature_practice": null, "feature_word_review": null})).await;
    assert_eq!(user.get("/api/practice/quiz").await.status, 200);
    assert_eq!(user.get("/api/practice/word-review").await.status, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_grace_periods() {
    let e = env().await;
    assert_eq!(scrabble::server::events::grace_seconds(&e.server.app, true, true), 120);
    patch_settings(&e, json!({"grace_disconnect": 45, "grace_waiting_owner": 90})).await;
    let app = &e.server.app;
    // aktív játékban mindenkinek, a várakozó szoba nem-tulajdonosának a rövidebb; a várakozó szoba tulajdonosának a hosszabb
    assert_eq!(scrabble::server::events::grace_seconds(app, true, true), 45);
    assert_eq!(scrabble::server::events::grace_seconds(app, true, false), 45);
    assert_eq!(scrabble::server::events::grace_seconds(app, false, false), 45);
    assert_eq!(scrabble::server::events::grace_seconds(app, false, true), 90);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_chat_length_limit() {
    let e = env().await;
    patch_settings(&e, json!({"chat_max_length": 20})).await;
    let (anna, bela) = (e.server.guest("Anna").await, e.server.guest("Béla").await);
    live_room(&e.server, &anna, &[&bela], json!({})).await;
    anna.emit("send_chat", json!({"message": "x".repeat(21)}));
    anna.emit("send_chat", json!({"message": "x".repeat(20)}));
    bela.settle().await;
    let lengths: Vec<usize> = bela.take().into_iter().filter(|(n, _)| n == "chat_message").map(|(_, d)| d["message"].as_str().unwrap().len()).collect();
    assert_eq!(lengths, vec![20]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_chat_rate_limit() {
    let e = env().await;
    patch_settings(&e, json!({"chat_rate_count": 2, "chat_rate_window": 60})).await;
    let (anna, bela) = (e.server.guest("Anna").await, e.server.guest("Béla").await);
    live_room(&e.server, &anna, &[&bela], json!({})).await;
    for text in ["egy", "kettő", "három"] {
        anna.emit("send_chat", json!({"message": text})); // a harmadik már a sebességkorlát alatt
    }
    bela.settle().await;
    let messages: Vec<Value> = bela.take().into_iter().filter(|(n, _)| n == "chat_message").map(|(_, d)| d["message"].clone()).collect();
    assert_eq!(messages, vec![json!("egy"), json!("kettő")]);
}

#[tokio::test(flavor = "multi_thread")]
async fn rate_limit_overrides() {
    let e = env().await;
    patch_settings(&e, json!({"rate_limits_http": {"announcements": [2, 60]}})).await;
    let anon = e.server.http();
    e.server.app.limiter.clear_ip("127.0.0.1");
    let mut statuses = Vec::new();
    for _ in 0..3 {
        statuses.push(anon.get("/api/announcements").await.status);
    }
    assert_eq!(statuses, vec![200, 200, 429]);
    patch_settings(&e, json!({"rate_limits_http": null})).await;
    e.server.app.limiter.clear_ip("127.0.0.1");
    let mut statuses = Vec::new();
    for _ in 0..3 {
        statuses.push(anon.get("/api/announcements").await.status);
    }
    assert_eq!(statuses, vec![200, 200, 200]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_spectator_limit() {
    let e = env().await;
    patch_settings(&e, json!({"max_spectators": 1})).await;
    let (anna, bela) = (e.server.guest("Anna").await, e.server.guest("Béla").await);
    let (id, _) = live_room(&e.server, &anna, &[&bela], json!({})).await;
    let watchers = [e.server.guest("Néző0").await, e.server.guest("Néző1").await];
    let first = watchers[0].call("spectate_room", json!({"room_id": id})).await;
    let second = watchers[1].call("spectate_room", json!({"room_id": id})).await;
    assert_eq!(with_room(&e.server, |room| room.spectators.len()), 1);
    // az első néző belépett, a második nem fért be
    assert!(error_messages(&first).is_empty(), "{first:?}");
    assert_eq!(error_messages(&second), vec!["A szobának nem fér több megfigyelője."]);
}
