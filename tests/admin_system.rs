//! Admin panel: áttekintés, biztonság (belépési napló, IP tiltás, kódok, munkamenetek), rendszer és statisztika (a
//! Python `test_admin_system.py` megfelelője). A környezeti változókra érzékeny részek az `admin_system_env.rs`-ben, a
//! konzolkimenet gyűjtése az `admin_log_capture.rs`-ben vannak.

mod common;
use common::admin::*;
use common::push::PushSink;
use common::smtp::FakeSmtp;
use common::*;
use scrabble::admin::system::LOG_BUFFER;
use scrabble::app::AnalysisJob;
use scrabble::db::{GamePlayerData, StoredMove};
use scrabble::util;
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

fn exec(e: &Env, sql: &str) {
    e.server.app.db.with(|tx| tx.execute_batch(sql)).unwrap();
}

// ===================================================================================================
// Áttekintés
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_overview_structure() {
    let e = env().await;
    let data = e.api.admin_get("/overview").await.json();
    for key in ["live", "today", "server", "services", "alerts", "versions", "maintenance"] {
        assert!(data.get(key).is_some(), "{key}");
    }
    assert_eq!((data["live"]["online_users"].clone(), data["live"]["rooms_total"].clone()), (json!(0), json!(0)));
    assert_eq!(data["services"]["dictionary"], true);
    assert!(data["server"]["uptime"].as_i64().unwrap() >= 0);
    assert!(data["server"]["db_size"].as_i64().unwrap() > 0);
    assert_eq!(data["today"]["registrations"], 1); // az admin fiók
}

#[tokio::test(flavor = "multi_thread")]
async fn live_counters() {
    let e = env().await;
    set_setting(&e.server, "bot_think_multiplier", json!(5.0));
    let a = e.server.create_user("a@example.com", "Anna");
    let anna = sio_user(&e.server, a, "Anna").await;
    let bela = e.server.guest("Béla").await;
    live_room(&e.server, &anna, &[&bela], json!({"challenge_mode": true})).await;
    let solo = e.server.guest("Cili").await;
    live_room(&e.server, &solo, &[], json!({"ai_players": [3], "name": "Robotos", "max_players": 2})).await;
    let watcher = e.server.guest("Néző").await;
    let first_room = e.server.app.state.lock().rooms_in_creation_order()[0].id.clone();
    watcher.call("spectate_room", json!({"room_id": first_room})).await;
    let live = e.api.admin_get("/overview").await.json()["live"].clone();
    assert_eq!((live["online_users"].clone(), live["online_guests"].clone(), live["sockets"].clone()), (json!(1), json!(3), json!(4)));
    assert_eq!(
        (live["rooms_total"].clone(), live["rooms_active"].clone(), live["games_running"].clone(), live["games_with_bots"].clone()),
        (json!(2), json!(2), json!(2), json!(1))
    );
    assert_eq!((live["rooms_public"].clone(), live["spectators"].clone(), live["grace_players"].clone()), (json!(2), json!(1), json!(0)));
}

#[tokio::test(flavor = "multi_thread")]
async fn grace_and_votes_are_counted() {
    let e = env().await;
    let (anna, bela) = (e.server.guest("Anna").await, e.server.guest("Béla").await);
    live_room(&e.server, &anna, &[&bela], json!({"challenge_mode": true})).await;
    let tiles: Vec<Value> = with_room(&e.server, |room| {
        let player = room.game.players.iter().find(|p| p.name == "Anna").unwrap();
        player
            .hand
            .iter()
            .filter(|t| !t.is_blank())
            .take(2)
            .enumerate()
            .map(|(i, t)| json!({"row": 7, "col": 7 + i, "letter": t.as_str(), "is_blank": false}))
            .collect()
    });
    anna.call("place_tiles", json!({"tiles": tiles})).await;
    bela.close();
    bela.settle_long().await;
    let live = e.api.admin_get("/overview").await.json()["live"].clone();
    assert_eq!((live["pending_votes"].clone(), live["grace_players"].clone()), (json!(1), json!(1)));
}

#[tokio::test(flavor = "multi_thread")]
async fn today_counters() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    finished_game(&e.server, "r1", &[(Some(a), "Anna", 10), (None, "Vendég", 5)], false, &[], 0);
    e.server.app.db.save_word_review(a, "valami", false).unwrap();
    let today = e.api.admin_get("/overview").await.json()["today"].clone();
    assert_eq!((today["registrations"].clone(), today["finished_games"].clone()), (json!(2), json!(1)));
    assert_eq!((today["review_decisions"].clone(), today["review_rejections"].clone()), (json!(1), json!(1)));
    assert!(today["rejected_words"].as_i64().unwrap() >= 600);
}

async fn alert_codes(e: &Env) -> std::collections::HashSet<String> {
    arr(&e.api.admin_get("/overview").await.json()["alerts"]).iter().map(|a| a["code"].as_str().unwrap().to_string()).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn service_alerts() {
    let e = env().await;
    let codes = alert_codes(&e).await;
    assert!(codes.contains("smtp_missing") && codes.contains("backup_old"), "{codes:?}");
    let alerts = arr(&e.api.admin_get("/overview").await.json()["alerts"]);
    let level = |code: &str| alerts.iter().find(|a| a["code"] == code).unwrap()["level"].clone();
    assert_eq!(level("smtp_missing"), "yellow");
    assert!(!codes.contains("dictionary_down"));
}

#[tokio::test(flavor = "multi_thread")]
async fn no_backup_alert_with_a_fresh_backup() {
    let e = env().await;
    assert!(alert_codes(&e).await.contains("backup_old"));
    assert_eq!(e.api.admin_post("/system/backup", json!({})).await.status, 200);
    assert!(!alert_codes(&e).await.contains("backup_old"));
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_login_alerts() {
    let e = env().await;
    for _ in 0..10 {
        e.server.app.db.record_login_event("x@example.com", false, Some("203.0.113.5"), Some("UA"), None, "");
    }
    let alerts = arr(&e.api.admin_get("/overview").await.json()["alerts"]);
    let alert = alerts.iter().find(|a| a["code"] == "failed_logins").expect("riasztás");
    assert!(alert["detail"].as_str().unwrap().contains("203.0.113.5"));
    assert!(alert["count"].as_i64().unwrap() >= 10);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_review_spike_alert() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    for i in 0..25 {
        e.server.app.db.save_word_review(a, &format!("szo{i}"), false).unwrap();
    }
    assert!(alert_codes(&e).await.contains("review_spike"));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_overdue_async_alert() {
    let e = env().await;
    *e.server.app.async_starter.lock() = Some(0);
    let a = e.server.create_user("a@example.com", "Anna");
    let b = e.server.create_user("b@example.com", "Béla");
    assert!(e.server.app.db.send_friend_request(a, b).0);
    assert!(e.server.app.db.accept_friend_request(b, a).0);
    let anna = sio_user(&e.server, a, "Anna").await;
    anna.call("create_async_game", json!({"name": "X", "friend_ids": [b], "turn_hours": 24})).await;
    assert!(!alert_codes(&e).await.contains("async_overdue"));
    {
        let mut st = e.server.app.state.lock();
        let room = st.rooms.values_mut().find(|r| r.is_async).unwrap();
        room.game.turn_deadline = Some(util::now() - 3600.0);
    }
    assert!(alert_codes(&e).await.contains("async_overdue"));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_maintenance_state_is_included() {
    let e = env().await;
    e.api.admin_post("/maintenance", json!({"enabled": true, "message_hu": "Karbantartás"})).await;
    assert_eq!(e.api.admin_get("/overview").await.json()["maintenance"]["message_hu"], "Karbantartás");
}

#[tokio::test(flavor = "multi_thread")]
async fn charts() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    finished_game(&e.server, "r1", &[(Some(a), "Anna", 10), (None, "Vendég", 5)], false, &[], 0);
    finished_game(&e.server, "r2", &[(Some(a), "Anna", 10), (None, "Robi", 5)], true, &[], 0);
    let data = e.api.admin_get("/charts?days=14").await.json();
    assert_eq!(data["days"].as_array().unwrap().len(), 14);
    let last = |key: &str| data[key].as_array().unwrap().last().unwrap()["value"].clone();
    assert_eq!(last("registrations"), 2);
    assert_eq!((last("games_human"), last("games_bots")), (json!(1), json!(1)));
    assert!(last("active_users").as_i64().unwrap() >= 1);
    assert_eq!(e.api.admin_get("/charts?days=1").await.status, 400);
}

// ===================================================================================================
// Belépési napló
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn login_events_are_recorded() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    let anon = e.server.http();
    anon.request("POST", "/api/auth/login", Some(json!({"email": "a@example.com", "password": "rossz"})), &[("User-Agent", "Teszt/1")]).await;
    anon.post("/api/auth/login", json!({"email": "a@example.com", "password": PASSWORD})).await;
    let items = arr(&e.api.admin_get("/security/logins?user=a@example.com").await.json()["items"]);
    assert_eq!(
        items.iter().map(|i| (i["success"].clone(), i["reason"].clone())).collect::<Vec<_>>(),
        vec![(json!(true), json!("")), (json!(false), json!("bad_credentials"))]
    );
    assert_eq!((items[0]["user_id"].clone(), items[1]["user_agent"].clone()), (json!(a), json!("Teszt/1")));
    let user = e.server.app.db.get_user_by_id(a).unwrap().unwrap();
    assert!(user.last_login_at.is_some());
    assert_eq!(user.last_login_ip.as_deref(), Some("127.0.0.1"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_banned_attempt_is_recorded_with_the_reason() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    e.api.sudo().await;
    e.api.admin_post(&format!("/users/{a}/ban"), json!({"until": "1d", "reason": REASON})).await;
    e.server.http().post("/api/auth/login", json!({"email": "a@example.com", "password": PASSWORD})).await;
    let items = arr(&e.api.admin_get("/security/logins?success=0").await.json()["items"]);
    assert_eq!(items.iter().map(|i| i["reason"].clone()).collect::<Vec<_>>(), vec![json!("banned")]);
}

#[tokio::test(flavor = "multi_thread")]
async fn registration_is_a_login_too() {
    let e = env().await;
    let code = e.server.app.db.create_verification_code("uj@example.com").unwrap();
    assert!(e.server.app.db.verify_code("uj@example.com", &code).0);
    e.server.http().post("/api/auth/register", json!({"email": "uj@example.com", "password": PASSWORD, "display_name": "Új"})).await;
    let items = arr(&e.api.admin_get("/security/logins?user=uj@example.com").await.json()["items"]);
    assert_eq!((items[0]["reason"].clone(), items[0]["success"].clone()), (json!("register"), json!(true)));
}

#[tokio::test(flavor = "multi_thread")]
async fn sessions_keep_the_ip_and_the_browser() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    e.server
        .http()
        .request(
            "POST",
            "/api/auth/login",
            Some(json!({"email": "a@example.com", "password": PASSWORD})),
            &[("User-Agent", "Teszt/2"), ("X-Forwarded-For", "198.51.100.4")],
        )
        .await;
    let sessions = e.api.admin_get(&format!("/users/{a}")).await.json()["sessions"].clone();
    assert_eq!((sessions[0]["ip"].clone(), sessions[0]["user_agent"].clone()), (json!("198.51.100.4"), json!("Teszt/2")));
}

#[tokio::test(flavor = "multi_thread")]
async fn suspicious_patterns_are_flagged() {
    let e = env().await;
    for _ in 0..5 {
        e.server.app.db.record_login_event("a@example.com", false, Some("203.0.113.5"), Some("UA"), None, "");
    }
    for name in ["b", "c", "d"] {
        e.server.app.db.record_login_event(&format!("{name}@example.com"), false, Some("203.0.113.5"), Some("UA"), None, "");
    }
    let data = e.api.admin_get("/security/logins").await.json();
    assert!(arr(&data["suspicious"]["ips"]).contains(&json!("203.0.113.5")));
    assert!(arr(&data["suspicious"]["accounts"]).contains(&json!("a@example.com")));
    assert!(arr(&data["suspicious"]["spread_ips"]).contains(&json!("203.0.113.5")));
    let flags: std::collections::HashSet<String> = arr(&data["items"]).iter().flat_map(|i| arr(&i["flags"])).map(|f| f.as_str().unwrap().to_string()).collect();
    for flag in ["ip_failures", "account_failures", "many_accounts"] {
        assert!(flags.contains(flag), "{flag}: {flags:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn login_filters_and_csv() {
    let e = env().await;
    e.server.app.db.record_login_event("a@example.com", false, Some("203.0.113.5"), Some("UA"), None, "bad_credentials");
    e.server.app.db.record_login_event("b@example.com", true, Some("198.51.100.9"), Some("UA"), None, "");
    let emails = |q: &'static str| {
        let api = e.api.clone();
        async move {
            arr(&api.admin_get(&format!("/security/logins?{q}")).await.json()["items"])
                .iter()
                .map(|i| i["email"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(emails("ip=203.0.113").await, vec!["a@example.com"]);
    // a próbához bejelentkezett admin saját belépése is sikeres
    assert_eq!(emails("success=1").await.into_iter().filter(|e| e != ADMIN_EMAIL).collect::<Vec<_>>(), vec!["b@example.com"]);
    assert_eq!(emails("q=bad_cred").await, vec!["a@example.com"]);
    assert!(emails("since=2999-01-01").await.is_empty());
    assert_eq!(e.api.admin_get(&format!("/security/logins{}", qs(&[("since", "hibás")]))).await.status, 400);
    let text = e.api.admin_get("/security/logins?format=csv").await.text();
    assert!(text.starts_with("id,created_at,success,email"), "{text}");
    assert!(!audit_of(&e.server, "view.logins_export").is_empty());
}

// ===================================================================================================
// IP tiltás
// ===================================================================================================

fn from_ip(ip: &str) -> Vec<(&'static str, String)> {
    vec![("X-Forwarded-For", ip.to_string())]
}

async fn get_as(http: &Http, path: &str, ip: &str) -> u16 {
    http.request("GET", path, None, &[("X-Forwarded-For", ip)]).await.status
}

#[tokio::test(flavor = "multi_thread")]
async fn a_ban_blocks_http_and_sockets_at_once() {
    let e = env().await;
    e.api.sudo().await;
    assert_eq!(e.api.admin_post("/security/ip-bans", json!({"ip": "198.51.100.7", "reason": REASON})).await.status, 200);
    let anon = e.server.http();
    assert_eq!(get_as(&anon, "/", "198.51.100.7").await, 403);
    assert_eq!(get_as(&anon, "/api/announcements", "198.51.100.7").await, 403);
    assert_eq!(get_as(&anon, "/admin", "198.51.100.7").await, 403); // semmi nem árulkodik
    assert_eq!(anon.get("/api/leaderboard").await.status, 200); // másnak működik
    assert!(Sio::handshake_refused(&e.server, &[("X-Forwarded-For", "198.51.100.7")]).await);
    assert!(!Sio::handshake_refused(&e.server, &[("X-Forwarded-For", "198.51.100.8")]).await);
    let other = Sio::connect(&e.server).await;
    other.settle().await;
    assert!(!other.is_closed());
    let _ = from_ip("");
}

#[tokio::test(flavor = "multi_thread")]
async fn cidr_expiry_and_removal() {
    let e = env().await;
    e.api.sudo().await;
    let ban_id = e.api.admin_post("/security/ip-bans", json!({"ip": "198.51.100.0/24", "until": "1h", "reason": REASON})).await.json()["id"].clone();
    let anon = e.server.http();
    assert_eq!(get_as(&anon, "/", "198.51.100.200").await, 403);
    assert_eq!(get_as(&anon, "/", "198.51.101.1").await, 200);
    let item = e.api.admin_get("/security/ip-bans").await.json()["items"][0].clone();
    assert_eq!((item["ip"].clone(), item["active"].clone(), item["admin_name"].clone()), (json!("198.51.100.0/24"), json!(true), json!("Admin")));
    exec(&e, "UPDATE ip_bans SET expires_at = '2000-01-01 00:00:00'");
    e.server.app.ip_bans.invalidate();
    assert_eq!(get_as(&anon, "/", "198.51.100.200").await, 200);
    assert_eq!(e.api.admin_delete(&format!("/security/ip-bans/{ban_id}"), json!({"reason": REASON})).await.status, 200);
    assert_eq!(e.api.admin_get("/security/ip-bans").await.json()["items"], json!([]));
    assert_eq!(e.api.admin_delete(&format!("/security/ip-bans/{ban_id}"), json!({"reason": REASON})).await.status, 404);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_ban_applies_immediately() {
    let e = env().await;
    e.api.sudo().await;
    let ban_id = e.api.admin_post("/security/ip-bans", json!({"ip": "198.51.100.7", "reason": REASON})).await.json()["id"].clone();
    assert_eq!(get_as(&e.server.http(), "/", "198.51.100.7").await, 403);
    e.api.admin_delete(&format!("/security/ip-bans/{ban_id}"), json!({"reason": REASON})).await;
    assert_eq!(get_as(&e.server.http(), "/", "198.51.100.7").await, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn your_own_address_cannot_be_banned() {
    let e = env().await;
    e.api.sudo().await;
    for ip in ["127.0.0.1", "127.0.0.0/8", "0.0.0.0/0"] {
        assert_eq!(e.api.admin_post("/security/ip-bans", json!({"ip": ip, "reason": REASON})).await.status, 409, "{ip}");
    }
    assert_eq!(e.api.admin_get("/security/ip-bans").await.json()["items"], json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_invalid_ip_address() {
    let e = env().await;
    e.api.sudo().await;
    for ip in [json!(""), json!("nem-ip"), json!("300.1.1.1"), json!("1.2.3.4/99"), json!(null)] {
        assert_eq!(e.api.admin_post("/security/ip-bans", json!({"ip": ip, "reason": REASON})).await.status, 400, "{ip}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ip_ban_needs_sudo_and_a_reason() {
    let e = env().await;
    assert_eq!(e.api.admin_post("/security/ip-bans", json!({"ip": "198.51.100.7", "reason": REASON})).await.status, 401);
    e.api.sudo().await;
    assert_eq!(e.api.admin_post("/security/ip-bans", json!({"ip": "198.51.100.7"})).await.status, 400);
    assert!(audit_of(&e.server, "security.ip_ban").is_empty());
}

// ===================================================================================================
// Forgalomkorlát-állapot
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn limited_addresses_are_listed_and_can_be_unblocked() {
    let e = env().await;
    let anon = e.server.http();
    e.api.admin_patch("/settings", json!({"changes": {"rate_limits_http": {"announcements": [2, 60]}}, "reason": REASON})).await;
    for _ in 0..3 {
        get_as(&anon, "/api/announcements", "198.51.100.30").await;
    }
    let data = e.api.admin_get("/security/rate-limits").await.json();
    let row = arr(&data["ips"]).into_iter().find(|r| r["ip"] == "198.51.100.30").expect("a korlátozott cím");
    assert_eq!((row["action"].clone(), row["count"].clone()), (json!("announcements"), json!(2)));
    let retry = row["retry_in"].as_f64().unwrap();
    assert!(retry > 0.0 && retry <= 60.0, "{retry}");
    assert_eq!(data["limits"]["http"]["announcements"], json!({"default": [60, 60], "current": [2, 60], "overridden": true}));
    assert_eq!(data["limits"]["socket"]["send_chat"]["current"], json!([10, 10]));
    assert_eq!(e.api.admin_post("/security/rate-limits/unblock", json!({"ip": "198.51.100.30", "reason": REASON})).await.status, 200);
    assert_eq!(get_as(&anon, "/api/announcements", "198.51.100.30").await, 200);
    assert_eq!(e.api.admin_post("/security/rate-limits/unblock", json!({"ip": "198.51.100.99", "reason": REASON})).await.status, 404);
}

#[tokio::test(flavor = "multi_thread")]
async fn limited_connections_are_listed() {
    let e = env().await;
    let client = e.server.guest("Anna").await;
    for _ in 0..12 {
        client.emit("get_rooms", Value::Null);
    }
    client.settle().await;
    let data = e.api.admin_get("/security/rate-limits").await.json();
    assert!(arr(&data["sids"]).iter().any(|r| r["event"] == "get_rooms"), "{}", data["sids"]);
}

// ===================================================================================================
// Ellenőrző kódok
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn pending_codes_are_visible_and_the_view_is_logged() {
    let e = env().await;
    let code = e.server.app.db.create_verification_code("uj@example.com").unwrap();
    let data = e.api.admin_get("/security/codes").await.json();
    assert_eq!(arr(&data["items"]).iter().map(|c| (c["email"].clone(), c["code"].clone())).collect::<Vec<_>>(), vec![(json!("uj@example.com"), json!(code))]);
    assert_eq!(audit_of(&e.server, "view.codes")[0]["details"], json!({"count": 1}));
}

#[tokio::test(flavor = "multi_thread")]
async fn used_and_expired_codes_are_hidden() {
    let e = env().await;
    e.server.app.db.create_verification_code("a@example.com").unwrap();
    e.server.app.db.create_verification_code("a@example.com").unwrap(); // az előző érvénytelen lesz
    e.server.app.db.create_verification_code("b@example.com").unwrap();
    exec(&e, "UPDATE verification_codes SET expires_at = '2000-01-01 00:00:00' WHERE email = 'b@example.com'");
    let emails: Vec<Value> = arr(&e.api.admin_get("/security/codes").await.json()["items"]).iter().map(|c| c["email"].clone()).collect();
    assert_eq!(emails, vec![json!("a@example.com")]);
}

#[tokio::test(flavor = "multi_thread")]
async fn invalidating_a_code() {
    let e = env().await;
    let code = e.server.app.db.create_verification_code("uj@example.com").unwrap();
    let code_id = e.api.admin_get("/security/codes").await.json()["items"][0]["id"].clone();
    assert_eq!(e.api.admin_delete(&format!("/security/codes/{code_id}"), json!({"reason": REASON})).await.status, 200);
    assert!(!e.server.app.db.verify_code("uj@example.com", &code).0);
    assert_eq!(e.api.admin_delete(&format!("/security/codes/{code_id}"), json!({"reason": REASON})).await.status, 404);
}

// ===================================================================================================
// Munkamenetek
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_session_listing_has_no_tokens() {
    let e = env().await;
    let (_, user) = make_user(&e.server, "a@example.com", "Anna").await;
    let token = user.session_token().unwrap();
    let data = arr(&e.api.admin_get("/security/sessions").await.json()["items"]);
    let names: std::collections::HashSet<String> = data.iter().map(|s| s["display_name"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, ["Admin".to_string(), "Anna".to_string()].into_iter().collect());
    assert!(!json!(data).to_string().contains(&token));
    assert_eq!(data.iter().find(|s| s["display_name"] == "Admin").unwrap()["current"], true);
    let only = |q: &str| {
        let api = e.api.clone();
        let q = q.to_string();
        async move {
            arr(&api.admin_get(&format!("/security/sessions?{q}")).await.json()["items"])
                .iter()
                .map(|s| s["display_name"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(only("q=anna").await, vec!["Anna"]);
    assert_eq!(only("admin=1").await, vec!["Admin"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn revoking_one_session_but_not_your_own() {
    let e = env().await;
    let (_, user) = make_user(&e.server, "a@example.com", "Anna").await;
    let token = user.session_token().unwrap();
    let sessions = arr(&e.api.admin_get("/security/sessions").await.json()["items"]);
    let mine = sessions.iter().find(|s| s["current"] == true).unwrap();
    let theirs = sessions.iter().find(|s| s["current"] != true).unwrap();
    assert_eq!(e.api.admin_delete(&format!("/security/sessions/{}", mine["id"]), json!({"reason": REASON})).await.status, 409);
    assert_eq!(e.api.admin_delete(&format!("/security/sessions/{}", theirs["id"]), json!({"reason": REASON})).await.status, 200);
    assert!(e.server.app.db.validate_session(Some(&token)).is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn revoking_everyone_keeps_the_admin_and_drops_the_sockets() {
    let e = env().await;
    let (a, user) = make_user(&e.server, "a@example.com", "Anna").await;
    let token = user.session_token().unwrap();
    let anna = sio_user(&e.server, a, "Anna").await;
    assert_eq!(e.api.admin_post("/security/sessions/revoke-all", json!({"reason": REASON})).await.status, 401);
    e.api.sudo().await;
    let data = e.api.admin_post("/security/sessions/revoke-all", json!({"reason": REASON})).await.json();
    assert_eq!(data["removed"], 1);
    assert!(e.server.app.db.validate_session(Some(&token)).is_none());
    assert_eq!(e.api.get("/api/admin/session").await.status, 200);
    anna.settle_long().await;
    assert!(anna.is_closed());
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_the_other_admin_sessions() {
    let e = env().await;
    let admin_id = e.server.user_id(ADMIN_EMAIL);
    let second = e.server.app.db.create_session(admin_id, None, None).unwrap();
    e.server.app.db.touch_admin_session(&second);
    let (_, user) = make_user(&e.server, "a@example.com", "Anna").await;
    let token = user.session_token().unwrap();
    assert_eq!(e.api.admin_post("/security/sessions/close-admins", json!({"reason": REASON})).await.json()["removed"], 1);
    assert!(e.server.app.db.validate_session(Some(&second)).is_none());
    assert!(e.server.app.db.validate_session(Some(&token)).is_some());
    assert_eq!(e.api.get("/api/admin/session").await.status, 200);
}

// ===================================================================================================
// Rendszer
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_system_overview() {
    let e = env().await;
    let data = e.api.admin_get("/system").await.json();
    assert!(!data["server"]["python"].as_str().unwrap().is_empty());
    assert_eq!(data["services"]["dictionary"], true);
    let versions = &data["versions"];
    assert!(!versions["git"].is_null() && !versions["asset_version"].is_null() && !versions["tests"].is_null(), "{versions}");
    assert!(versions["packages"]["axum"].is_string());
    let tables: std::collections::HashMap<String, i64> =
        arr(&data["database"]["tables"]).iter().map(|t| (t["name"].as_str().unwrap().to_string(), t["rows"].as_i64().unwrap())).collect();
    assert_eq!(tables["users"], 1);
    assert!(tables.contains_key("admin_audit") && data["database"]["size"].as_i64().unwrap() > 0);
    let jobs: std::collections::HashSet<String> = arr(&data["jobs"]).iter().map(|j| j["name"].as_str().unwrap().to_string()).collect();
    assert!(jobs.contains("async_sweeper") && jobs.contains("admin_broadcaster"), "{jobs:?}");
    assert_eq!(data["mail"]["source"], "environment");
    assert!(data["mail"].get("password").is_none() && data.get("tunnel").is_some());
}

// ===================================================================================================
// Naplók
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_ring_buffer_collects_and_filters() {
    let e = env().await;
    LOG_BUFFER.add("INFO", "teszt.admin", "információ-sor-a", None, None, None);
    LOG_BUFFER.add("WARNING", "teszt.admin", "figyelmeztetés-sor-a", None, None, None);
    LOG_BUFFER.add("ERROR", "teszt.admin", "baj van-a", Some("ValueError"), Some("tests/admin_system.rs:1"), None);
    let items = arr(&e.api.admin_get("/system/logs?q=-sor-a").await.json()["items"]);
    assert_eq!(items.iter().map(|i| i["level"].clone()).collect::<Vec<_>>(), vec![json!("INFO"), json!("WARNING")]);
    let errors = e.api.admin_get("/system/logs?level=ERROR&q=baj%20van-a").await.json();
    assert_eq!(errors["items"][0]["exc_type"], "ValueError");
    assert!(errors["items"][0]["location"].as_str().unwrap().contains("admin_system.rs"));
    assert_eq!(e.api.admin_get("/system/logs?level=NINCS").await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn follow_with_since_id_and_limit() {
    let e = env().await;
    LOG_BUFFER.add("INFO", "teszt.admin", "első-követés", None, None, None);
    let first = arr(&e.api.admin_get(&format!("/system/logs{}", qs(&[("q", "-követés")]))).await.json()["items"]);
    LOG_BUFFER.add("INFO", "teszt.admin", "második-követés", None, None, None);
    let since = first.last().unwrap()["id"].as_u64().unwrap();
    let newer = arr(&e.api.admin_get(&format!("/system/logs{}", qs(&[("q", "-követés"), ("since_id", &since.to_string())]))).await.json()["items"]);
    assert_eq!(newer.iter().map(|i| i["message"].clone()).collect::<Vec<_>>(), vec![json!("második-követés")]);
    assert_eq!(arr(&e.api.admin_get("/system/logs?limit=1").await.json()["items"]).len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn errors_are_grouped() {
    let e = env().await;
    for _ in 0..3 {
        LOG_BUFFER.add("ERROR", "teszt.admin", "csoport", Some("KeyError"), Some("tests/admin_system.rs:csoportosítás"), None);
    }
    let groups = arr(&e.api.admin_get("/system/logs").await.json()["groups"]);
    let group = groups.iter().find(|g| g["exc_type"] == "KeyError" && g["location"].as_str().unwrap_or("").contains("csoportosítás")).expect("csoport");
    assert!(group["count"].as_i64().unwrap() >= 3);
}

// ===================================================================================================
// Adatbázis
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_download_needs_sudo_and_is_a_consistent_snapshot() {
    let e = env().await;
    e.server.create_user("a@example.com", "Anna");
    assert_eq!(e.api.admin_get("/system/backup").await.status, 401);
    e.api.sudo().await;
    let response = e.api.admin_get("/system/backup").await;
    assert_eq!(response.status, 200);
    assert!(response.header("Content-Disposition").unwrap().contains("attachment"));
    let target = e.server.dir.join("letoltott.db");
    std::fs::write(&target, &response.body).unwrap();
    let conn = rusqlite::Connection::open(&target).unwrap();
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
    assert_eq!(conn.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0)).unwrap(), "ok");
    assert!(audit_of(&e.server, "system.backup_download")[0]["details"]["size"].as_i64().unwrap() > 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backup_on_the_server_keeps_the_last_ones() {
    let e = env().await;
    e.api.admin_patch("/settings", json!({"changes": {"backup_keep": 2}, "reason": REASON})).await;
    for _ in 0..3 {
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        assert_eq!(e.api.admin_post("/system/backup", json!({})).await.status, 200);
    }
    let names: Vec<String> =
        arr(&e.api.admin_get("/system").await.json()["database"]["backups"]).iter().map(|b| b["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(names.len(), 2);
    let mut sorted = names.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(names, sorted);
    assert!(scrabble::admin::system::last_backup_time(&e.server.app).is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn vacuum() {
    let e = env().await;
    let data = e.api.admin_post("/system/vacuum", json!({"reason": REASON})).await.json();
    assert!(data["before"].as_i64().unwrap() > 0 && data["after"].as_i64().unwrap() > 0);
    assert!(!audit_of(&e.server, "system.vacuum").is_empty());
    assert_eq!(e.api.admin_post("/system/vacuum", json!({})).await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn cleanup_preview_and_run() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    exec(
        &e,
        &format!(
            "INSERT INTO sessions (user_id, token, expires_at) VALUES ({a}, 'lejart', '2000-01-01 00:00:00'); \
             INSERT INTO verification_codes (email, code, expires_at, used) VALUES ('x@e.hu', '1', '2000-01-01 00:00:00', 0); \
             INSERT INTO verified_emails (email, expires_at) VALUES ('x@e.hu', '2000-01-01 00:00:00'); \
             INSERT INTO ip_bans (ip, expires_at) VALUES ('1.2.3.4', '2000-01-01 00:00:00'); \
             INSERT INTO chat_log (room_id, message, created_at) VALUES ('r', 'régi', '2000-01-01 00:00:00')"
        ),
    );
    let old = e.server.app.db.save_game("x1", "Régi", "{}", false, &[], "", None, false, false).unwrap();
    e.server.app.db.abandon_game_by_id(old);
    exec(&e, &format!("UPDATE saved_games SET updated_at = '2020-01-01 00:00:00' WHERE id = {old}"));
    let counts = e.api.admin_get("/system/cleanup?days=30").await.json()["counts"].clone();
    assert_eq!(counts, json!({"sessions": 1, "codes": 1, "verified_emails": 1, "abandoned_games": 1, "chat_log": 1, "ip_bans": 1, "login_events": 0}));
    assert_eq!(e.api.admin_post("/system/cleanup", json!({"items": ["sessions"], "reason": REASON})).await.status, 401);
    e.api.sudo().await;
    assert_eq!(e.api.admin_post("/system/cleanup", json!({"items": ["nincs"], "reason": REASON})).await.status, 400);
    let removed =
        e.api.admin_post("/system/cleanup", json!({"items": ["sessions", "codes", "abandoned_games"], "days": 30, "reason": REASON})).await.json()["removed"]
            .clone();
    assert_eq!(removed, json!({"sessions": 1, "codes": 1, "abandoned_games": 1}));
    let counts = e.api.admin_get("/system/cleanup?days=30").await.json()["counts"].clone();
    assert_eq!(
        (counts["sessions"].clone(), counts["codes"].clone(), counts["abandoned_games"].clone(), counts["chat_log"].clone()),
        (json!(0), json!(0), json!(0), json!(1))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_scheduled_maintenance() {
    let e = env().await;
    let app = &e.server.app;
    assert!(scrabble::admin::system::run_scheduled_maintenance(app).is_empty());
    e.api.admin_patch("/settings", json!({"changes": {"backup_daily": true, "chat_log_enabled": true, "chat_log_days": 1}, "reason": REASON})).await;
    exec(&e, "INSERT INTO chat_log (room_id, message, created_at) VALUES ('r', 'régi', '2000-01-01 00:00:00')");
    assert_eq!(scrabble::admin::system::run_scheduled_maintenance(app), vec!["backup", "chat_log"]);
    assert!(scrabble::admin::system::run_scheduled_maintenance(app).is_empty()); // a mai mentés már megvan
    assert_eq!(scrabble::admin::system::list_backups(app).len(), 1);
}

// ===================================================================================================
// Folyamatok és eszközök
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn job_status() {
    let e = env().await;
    e.server.app.job_ran("async_sweeper", true, Some("2".into()));
    e.server.app.job_ran("async_sweeper", false, Some("hiba".into()));
    let jobs = arr(&e.api.admin_get("/system").await.json()["jobs"]);
    let job = jobs.iter().find(|j| j["name"] == "async_sweeper").unwrap();
    assert!(job["runs"].as_i64().unwrap() >= 2 && job["errors"].as_i64().unwrap() >= 1);
    assert_eq!(job["ok"], false);
    assert!(!job["last_run"].is_null());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_analysis_queue() {
    let e = env().await;
    e.server.app.analysis_jobs.lock().insert(5, AnalysisJob::Running { done: 2, total: 9 });
    e.server.app.analysis_jobs.lock().insert(6, AnalysisJob::Failed { at: util::now() });
    let queue = arr(&e.api.admin_get("/system").await.json()["analysis"]);
    assert!(queue.contains(&json!({"game_id": 5, "done": 2, "total": 9, "error": false})), "{queue:?}");
    assert!(queue.iter().any(|q| q["game_id"] == 6 && q["error"] == true));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_tunnel_restart_needs_sudo_and_a_tunnel() {
    let e = env().await;
    assert_eq!(e.api.admin_post("/system/tunnel/restart", json!({"reason": REASON})).await.status, 401);
    e.api.sudo().await;
    // tunnel nélkül (a tesztszerver helyi) nincs mit újraindítani
    assert_eq!(e.api.admin_post("/system/tunnel/restart", json!({"reason": REASON})).await.status, 409);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_smtp_test() {
    let e = env().await;
    // beállítás nélkül a konzolra megy
    let data = e.api.admin_post("/system/smtp-test", json!({})).await.json();
    assert_eq!(data["console"], true);
    let smtp = FakeSmtp::start();
    use_smtp(&e.server, &smtp);
    assert_eq!(e.api.admin_post("/system/smtp-test", json!({})).await.json(), json!({"success": true, "sent": true, "console": false}));
    let mails = smtp.wait_for(1, 5).await;
    assert_eq!(mails[0].to, vec![ADMIN_EMAIL.to_string()]);
    smtp.reject_recipients_with(Some("550 nincs ilyen felhasználó"));
    assert_eq!(e.api.admin_post("/system/smtp-test", json!({})).await.status, 502);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_push_test_to_myself() {
    let e = env().await;
    let sink = PushSink::start();
    assert_eq!(e.api.admin_post("/system/push-test", json!({})).await.status, 409);
    let admin_id = e.server.user_id(ADMIN_EMAIL);
    let sub = sink.subscription("boss-device");
    e.server.app.db.save_push_subscription(admin_id, &sub.endpoint, &sub.p256dh, &sub.auth, "hu").unwrap();
    assert_eq!(e.api.admin_post("/system/push-test", json!({})).await.json()["sent"], 1);
    assert_eq!(sink.wait_for(1, 5).await.len(), 1);
}

// ===================================================================================================
// Statisztika
// ===================================================================================================

async fn stats(e: &Env, metric: &str, days: i64) -> Value {
    let response = e.api.admin_get(&format!("/stats?metric={metric}&range={days}")).await;
    assert_eq!(response.status, 200, "{}", response.text());
    response.json()
}

#[tokio::test(flavor = "multi_thread")]
async fn stats_validation() {
    let e = env().await;
    assert_eq!(e.api.admin_get("/stats?metric=nincs&range=30").await.status, 400);
    assert_eq!(e.api.admin_get("/stats?metric=games&range=5").await.status, 400);
    assert_eq!(e.api.admin_get("/stats").await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn registrations_and_activity() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    let data = stats(&e, "registrations", 7).await;
    assert_eq!(data["days"].as_array().unwrap().len(), 7);
    // az admin és Anna is ma regisztrált
    assert_eq!((data["series"].as_array().unwrap().last().unwrap()["value"].clone(), data["total_users"].clone()), (json!(2), json!(2)));
    assert_eq!(data["table"]["columns"], json!(["day", "registrations"]));
    e.server.app.db.record_login_event("a@example.com", true, Some("1.2.3.4"), Some("UA"), Some(a), "");
    let active = stats(&e, "active", 30).await;
    // Anna és a bejelentkezett admin
    assert_eq!((active["dau"].clone(), active["wau"].clone(), active["mau"].clone()), (json!(2), json!(2), json!(2)));
}

#[tokio::test(flavor = "multi_thread")]
async fn retention() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    let b = e.server.create_user("b@example.com", "Béla");
    exec(
        &e,
        &format!(
            "UPDATE users SET created_at = datetime('now', '-10 days') WHERE id IN ({a}, {b}); \
             INSERT INTO login_events (user_id, email, success, created_at) VALUES ({a}, 'a', 1, datetime('now', '-9 days')); \
             INSERT INTO login_events (user_id, email, success, created_at) VALUES ({a}, 'a', 1, datetime('now', '-3 days'))"
        ),
    );
    let rows: std::collections::HashMap<i64, Value> =
        arr(&stats(&e, "retention", 30).await["retention"]).into_iter().map(|r| (r["day"].as_i64().unwrap(), r)).collect();
    assert_eq!((rows[&1]["eligible"].clone(), rows[&1]["returned"].clone()), (json!(2), json!(1)));
    assert_eq!(rows[&1]["share"].as_f64(), Some(50.0));
    assert_eq!(rows[&7]["returned"], 1);
    assert_eq!(rows[&30]["eligible"], 0);
    assert!(rows[&30]["share"].is_null());
}

#[tokio::test(flavor = "multi_thread")]
async fn games_statistics() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    finished_game(&e.server, "r1", &[(Some(a), "Anna", 10), (None, "Vendég", 5)], false, &[], 6);
    finished_game(&e.server, "r2", &[(Some(a), "Anna", 10), (None, "Robi", 5)], true, &[], 2);
    e.server.app.db.save_game("x", "Abandon", "{}", false, &[], "", None, false, false).unwrap();
    e.server.app.db.abandon_game("x");
    let data = stats(&e, "games", 30).await;
    assert_eq!(data["types"], json!({"human": 1, "bots": 1, "async": 0, "daily": 0}));
    assert_eq!(data["avg_moves"].as_f64(), Some(4.0));
    assert_eq!((data["finished"].clone(), data["abandoned"].clone()), (json!(2), json!(1)));
    assert_eq!(data["completion_rate"].as_f64(), Some(66.7));
    let last_row = data["table"]["rows"].as_array().unwrap().last().unwrap().as_array().unwrap().clone();
    assert_eq!(&last_row[1..], &[json!(1), json!(1)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn bot_levels() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    let bot_game = |room: &str, level: Value, human_wins: bool| {
        let state = json!({"players": [{"name": "Anna", "is_bot": false}, {"name": "Robi", "is_bot": true, "difficulty": level}]});
        let players = [
            GamePlayerData { player_name: "Anna".into(), user_id: Some(a), score: 50, is_winner: human_wins, resigned: false },
            GamePlayerData { player_name: "Robi".into(), user_id: None, score: 40, is_winner: !human_wins, resigned: false },
        ];
        e.server.app.db.finish_game(room, &state.to_string(), &players, "Játék", true).unwrap();
    };
    for (i, won) in [true, false, true].iter().enumerate() {
        bot_game(&format!("a{i}"), json!(5), *won);
    }
    bot_game("b", json!("auto"), false);
    let levels: std::collections::HashMap<String, Value> =
        arr(&stats(&e, "bots", 30).await["levels"]).into_iter().map(|l| (l["level"].to_string().trim_matches('"').to_string(), l)).collect();
    assert_eq!((levels["5"]["games"].clone(), levels["5"]["human_wins"].clone()), (json!(3), json!(2)));
    assert_eq!(levels["5"]["human_win_rate"].as_f64(), Some(66.7));
    assert_eq!(levels["5"]["strength"].as_f64(), Some(12.7));
    assert_eq!((levels["auto"]["games"].clone(), levels["1"]["games"].clone()), (json!(1), json!(0)));
}

#[tokio::test(flavor = "multi_thread")]
async fn words_and_moves() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    let game_id = finished_game(&e.server, "r1", &[(Some(a), "Anna", 10), (None, "Vendég", 5)], false, &[], 0);
    for (n, (words, score, tiles)) in [(json!(["ALMA"]), 20, 4), (json!(["ALMA", "ÁL"]), 30, 5), (json!(["KÖRTE"]), 120, 7)].iter().enumerate() {
        let details = json!({"words": words, "score": score, "tiles": vec![json!({}); *tiles]});
        e.server
            .app
            .db
            .add_game_move(
                game_id,
                &StoredMove {
                    move_number: n as i64 + 1,
                    player_name: "Anna".into(),
                    action_type: "place".into(),
                    details_json: Some(details.to_string()),
                    board_snapshot_json: Some("{}".into()),
                },
            )
            .unwrap();
    }
    let data = stats(&e, "words", 30).await;
    assert_eq!(data["top_words"][0], json!({"word": "ALMA", "count": 2}));
    assert_eq!(data["bingos"], 1);
    assert_eq!((data["top_moves"][0]["score"].clone(), data["top_moves"][0]["words"].clone()), (json!(120), json!(["KÖRTE"])));
    assert_eq!(data["table"]["rows"][0], json!(["ALMA", 2]));
}

#[tokio::test(flavor = "multi_thread")]
async fn challenges_statistics() {
    let e = env().await;
    let game_id = e.server.app.db.save_game("c1", "Megtámadós", "{}", true, &[], "", None, false, false).unwrap();
    for (n, kind) in ["challenge_accept", "challenge_accept", "challenge_reject", "place"].iter().enumerate() {
        e.server
            .app
            .db
            .add_game_move(
                game_id,
                &StoredMove {
                    move_number: n as i64 + 1,
                    player_name: "Anna".into(),
                    action_type: kind.to_string(),
                    details_json: Some("{}".into()),
                    board_snapshot_json: Some("{}".into()),
                },
            )
            .unwrap();
    }
    let data = stats(&e, "challenges", 30).await;
    assert_eq!((data["games"].clone(), data["accepted"].clone(), data["rejected"].clone()), (json!(1), json!(2), json!(1)));
    assert_eq!(data["rejection_rate"].as_f64(), Some(33.3));
}

#[tokio::test(flavor = "multi_thread")]
async fn practice_usage_counters() {
    let e = env().await;
    let (_, user) = make_user(&e.server, "a@example.com", "Anna").await;
    user.get("/api/practice/quiz").await;
    user.get("/api/practice/quiz").await;
    user.get("/api/practice/rack?kind=hunt").await;
    user.get("/api/practice/word-review").await;
    let data = stats(&e, "practice", 30).await;
    assert_eq!((data["totals"]["quiz"].clone(), data["totals"]["rack"].clone(), data["totals"]["word_review"].clone()), (json!(2), json!(1), json!(1)));
    assert_eq!(data["series"]["quiz"].as_array().unwrap().last().unwrap()["value"], 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_review_series() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    e.server.app.db.save_word_review(a, "egy", false).unwrap();
    e.server.app.db.save_word_review(a, "ketto", true).unwrap();
    let data = stats(&e, "review", 30).await;
    let last = |key: &str| data[key].as_array().unwrap().last().unwrap()["value"].clone();
    assert_eq!((last("decisions"), last("rejections")), (json!(2), json!(1)));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_heatmap() {
    let e = env().await;
    let game_id = e.server.app.db.save_game("h1", "Hő", "{}", false, &[], "", None, false, false).unwrap();
    exec(
        &e,
        &format!(
            "INSERT INTO game_moves (game_id, move_number, player_name, action_type, created_at) VALUES ({game_id}, 1, 'Anna', 'pass', '2026-10-05 08:30:00')"
        ),
    ); // hétfő
    let grid = stats(&e, "heatmap", 365).await;
    assert_eq!(grid["grid"].as_array().unwrap().len(), 7);
    assert!(grid["grid"].as_array().unwrap().iter().all(|row| row.as_array().unwrap().len() == 24));
    assert!(grid["max"].as_i64().unwrap() >= 1);
    let timezone = grid["timezone"].as_str().unwrap();
    assert!(["Europe/Budapest", "UTC"].contains(&timezone), "{timezone}");
    let hour = if timezone == "Europe/Budapest" { 10 } else { 8 }; // októberben UTC+2
    assert_eq!(grid["grid"][0][hour], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn stats_csv_export() {
    let e = env().await;
    e.server.create_user("a@example.com", "Anna");
    let text = e.api.admin_get("/stats?metric=registrations&range=7&format=csv").await.text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "day,registrations");
    assert_eq!(lines.len(), 8);
    assert!(lines.last().unwrap().ends_with(",2"), "{lines:?}");
}
