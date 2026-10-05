//! Admin panel: a levelező szerver (SMTP) beállítása, kapcsolat-próbája és visszaállítása (a Python
//! `test_admin_mail.py` megfelelője). A küldést valódi, helyi hamis SMTP kiszolgáló fogadja (`FakeSmtp`).

mod common;
use common::admin::*;
use common::smtp::FakeSmtp;
use common::*;
use scrabble::mail::KEY;
use serde_json::{Value, json};

const REASON: &str = "Új levelező szolgáltató";

fn good() -> Value {
    json!({
        "host": "smtp.example.org", "port": 587, "security": "starttls", "username": "mailer",
        "password": "titkos-jelszo-123", "from_address": "noreply@example.org", "from_name": "Magyar Scrabble", "verify_tls": true
    })
}

fn merged(overrides: Value) -> Value {
    let mut body = good();
    for (k, v) in overrides.as_object().unwrap() {
        body[k] = v.clone();
    }
    body
}

async fn save(api: &Http, overrides: Value) -> Resp {
    let mut body = merged(overrides);
    body["reason"] = json!(REASON);
    api.admin_patch("/system/mail", body).await
}

async fn start() -> (TestServer, Http) {
    let server = TestServer::start().await;
    let api = server.admin_http().await;
    (server, api)
}

fn store(server: &TestServer, cfg: &Value) {
    server.app.db.with(|tx| server.app.mailer.save(tx, cfg, 7)).unwrap();
}

fn audit(server: &TestServer, action: &str) -> Vec<Value> {
    audit_of(server, action)
}

// ===================================================================================================
// Tárolás és érvényes beállítás
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn without_an_override_the_environment_applies() {
    let server = TestServer::start_with(|c| {
        c.smtp_host = "smtp.env.example".into();
        c.smtp_user = "envuser".into();
        c.smtp_password = "envpass".into();
        c.smtp_from = "env@example.org".into();
    })
    .await;
    let cfg = server.app.mailer.current();
    assert_eq!((cfg.source, cfg.configured, cfg.security.as_str()), ("environment", true, "starttls"));
    assert_eq!((cfg.host.as_str(), server.app.mailer.from_address().as_str()), ("smtp.env.example", "env@example.org"));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_saved_override_wins_and_can_be_cleared() {
    let server = TestServer::start_with(|c| {
        c.smtp_user = "envuser".into();
        c.smtp_password = "envpass".into();
        c.smtp_from = "env@example.org".into();
    })
    .await;
    store(&server, &good());
    let cfg = server.app.mailer.current();
    assert_eq!((cfg.source, cfg.host.as_str(), cfg.configured), ("database", "smtp.example.org", true));
    assert!(server.app.db.with(|tx| server.app.mailer.clear(tx)).unwrap());
    assert!(!server.app.db.with(|tx| server.app.mailer.clear(tx)).unwrap());
    assert_eq!(server.app.mailer.current().source, "environment");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_username_without_a_password_is_not_configured() {
    let (server, _api) = start().await;
    store(&server, &merged(json!({"password": ""})));
    assert!(!server.app.mailer.is_configured());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_relay_without_login_is_configured() {
    let (server, _api) = start().await;
    store(&server, &merged(json!({"username": "", "password": "", "security": "none", "port": 25})));
    assert!(server.app.mailer.is_configured());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_damaged_row_falls_back_to_the_environment() {
    let (server, _api) = start().await;
    let wrong_security = merged(json!({"security": "ssh"})).to_string();
    let string_port = merged(json!({"port": "587"})).to_string();
    for raw in ["nem json", "[]", "{\"host\": \"x\"}", wrong_security.as_str(), string_port.as_str()] {
        server.app.db.set_setting(KEY, raw).unwrap();
        assert!(server.app.mailer.stored().is_none(), "{raw}");
        assert_eq!(server.app.mailer.current().source, "environment", "{raw}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_public_view_has_no_password() {
    let (server, _api) = start().await;
    store(&server, &good());
    let view = server.app.mailer.public_view();
    assert_eq!(view["password_set"], true);
    assert!(view.get("password").is_none() && view["environment"].get("password").is_none());
    assert!(!view.to_string().contains("titkos-jelszo-123"));
}

// ===================================================================================================
// Lekérdezés és mentés
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn get_before_any_change() {
    let (_server, api) = start().await;
    let mail = api.admin_get("/system/mail").await.json()["mail"].clone();
    assert_eq!((mail["source"].clone(), mail["configured"].clone()), (json!("environment"), json!(false)));
    assert_eq!(mail["defaults"], json!({"starttls": 587, "ssl": 465, "none": 25}));
}

#[tokio::test(flavor = "multi_thread")]
async fn saving_needs_sudo_and_a_reason() {
    let (server, api) = start().await;
    assert_eq!(save(&api, json!({})).await.status, 401);
    api.sudo().await;
    let no_reason = api.admin_patch("/system/mail", good()).await;
    assert_eq!(no_reason.status, 400);
    assert_eq!(no_reason.json()["field"], "reason");
    assert!(server.app.mailer.stored().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn save_applies_immediately_and_never_returns_the_password() {
    let (server, api) = start().await;
    api.sudo().await;
    let response = save(&api, json!({})).await;
    assert_eq!(response.status, 200, "{}", response.text());
    assert!(!response.text().contains("titkos-jelszo-123"));
    let mail = response.json()["mail"].clone();
    assert_eq!((mail["source"].clone(), mail["configured"].clone(), mail["password_set"].clone()), (json!("database"), json!(true), json!(true)));
    assert_eq!(mail["host"], "smtp.example.org");
    assert!(mail["updated_by_name"].is_string());
    assert_eq!(server.app.mailer.current().password, "titkos-jelszo-123");
    assert!(!api.admin_get("/system/mail").await.text().contains("titkos-jelszo-123"));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_audit_log_has_no_secret() {
    let (server, api) = start().await;
    api.sudo().await;
    save(&api, json!({})).await;
    let entry = audit(&server, "system.mail_update")[0].clone();
    assert!(!entry.to_string().contains("titkos-jelszo-123"));
    assert_eq!((entry["details"]["after"]["password_set"].clone(), entry["details"]["password_changed"].clone()), (json!(true), json!(true)));
    assert_eq!((entry["details"]["before"]["source"].clone(), entry["reason"].clone()), (json!("environment"), json!(REASON)));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_blank_password_keeps_the_saved_one_for_the_same_server() {
    let (server, api) = start().await;
    api.sudo().await;
    save(&api, json!({})).await;
    assert_eq!(save(&api, json!({"password": null, "from_name": "Új név"})).await.status, 200);
    let cfg = server.app.mailer.current();
    assert_eq!((cfg.password.as_str(), cfg.from_name.as_str()), ("titkos-jelszo-123", "Új név"));
    assert_eq!(audit(&server, "system.mail_update")[0]["details"]["password_changed"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_changed_server_needs_the_password_again() {
    let (server, api) = start().await;
    api.sudo().await;
    save(&api, json!({})).await;
    let response = save(&api, json!({"password": null, "host": "evil.example.net"})).await;
    assert_eq!((response.status, response.json()["field"].clone()), (400, json!("password")));
    assert_eq!(server.app.mailer.current().host, "smtp.example.org");
    assert_eq!(save(&api, json!({"password": null, "username": "masik"})).await.status, 400);
    assert_eq!(save(&api, json!({"password": "uj-jelszo", "host": "other.example.org"})).await.status, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_username_no_password_is_kept() {
    let (server, api) = start().await;
    api.sudo().await;
    assert_eq!(save(&api, json!({"username": "", "password": "ignorált", "security": "none", "port": 25})).await.status, 200);
    assert_eq!(server.app.mailer.current().password, "");
    assert!(server.app.mailer.is_configured());
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_values_are_refused() {
    let (server, api) = start().await;
    api.sudo().await;
    let long_host = "a".repeat(300);
    let long_name = "n".repeat(81);
    let cases: Vec<(&str, Value)> = vec![
        ("host", json!("")),
        ("host", json!("nem érvényes host")),
        ("host", json!(long_host)),
        ("host", json!("host;rm -rf")),
        ("host", Value::Null),
        ("port", json!(0)),
        ("port", json!(70000)),
        ("port", json!("587")),
        ("port", json!(true)),
        ("port", Value::Null),
        ("security", json!("tls")),
        ("security", Value::Null),
        ("username", json!("x\ny")),
        ("password", json!("a\nb")),
        ("password", json!(5)),
        ("from_address", json!("")),
        ("from_address", json!("nem-cim")),
        ("from_address", json!("a@b.hu\nBcc: x@y.hu")),
        ("from_name", json!("Név\r\nBcc: x@y.hu")),
        ("from_name", json!(long_name)),
        ("verify_tls", json!("igen")),
    ];
    for (field, value) in cases {
        let response = save(&api, json!({ field: value.clone() })).await;
        assert_eq!(response.status, 400, "{field}={value}: {}", response.text());
        assert_eq!(response.json()["field"], field, "{field}={value}");
        assert!(server.app.mailer.stored().is_none(), "{field}={value}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_username_without_a_password_is_refused() {
    let (_server, api) = start().await;
    api.sudo().await;
    let response = save(&api, json!({"password": ""})).await;
    assert_eq!((response.status, response.json()["field"].clone()), (400, json!("password")));
}

#[tokio::test(flavor = "multi_thread")]
async fn ipv4_and_ipv6_hosts_are_accepted() {
    let (_server, api) = start().await;
    api.sudo().await;
    assert_eq!(save(&api, json!({"host": "192.168.1.10"})).await.status, 200);
    assert_eq!(save(&api, json!({"host": "::1", "password": "x"})).await.status, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_overview_alert_goes_away() {
    let (_server, api) = start().await;
    let codes = |api: Http| async move {
        api.admin_get("/overview").await.json()["alerts"].as_array().unwrap().iter().map(|a| a["code"].as_str().unwrap().to_string()).collect::<Vec<_>>()
    };
    assert!(codes(api.clone()).await.contains(&"smtp_missing".to_string()));
    api.sudo().await;
    save(&api, json!({})).await;
    assert!(!codes(api.clone()).await.contains(&"smtp_missing".to_string()));
    assert_eq!(api.admin_get("/overview").await.json()["services"]["smtp"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_config_table_shows_the_effective_values() {
    let (_server, api) = start().await;
    api.sudo().await;
    save(&api, json!({})).await;
    let system = api.admin_get("/system").await.json();
    let items: std::collections::HashMap<String, Value> =
        system["config"].as_array().unwrap().iter().map(|i| (i["key"].as_str().unwrap().to_string(), i.clone())).collect();
    assert_eq!((items["SMTP_HOST"]["value"].clone(), items["SMTP_SOURCE"]["value"].clone()), (json!("smtp.example.org"), json!("database")));
    assert_eq!(items["SMTP_PASSWORD"]["secret"], true);
    assert!(!system["config"].to_string().contains("titkos-jelszo-123"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_saved_configuration_is_used_for_sending() {
    let (server, api) = start().await;
    let smtp = FakeSmtp::start();
    smtp.require_login("mailer", "titkos-jelszo-123");
    api.sudo().await;
    assert_eq!(save(&api, json!({"host": "127.0.0.1", "port": smtp.port(), "security": "none"})).await.status, 200);
    let mailer = server.app.mailer.clone();
    let sent = tokio::task::spawn_blocking(move || mailer.send_plain_email("x@example.com", "T", "S", true)).await.unwrap();
    assert_eq!(sent, (true, None));
    assert_eq!(smtp.logins(), vec!["mailer"]);
    let mails = smtp.mails();
    assert_eq!(mails.len(), 1);
    assert_eq!(mails[0].from, "noreply@example.org");
    // a feladó neve a From fejlécben (a levelező idézőjelbe teheti), a boríték feladója a bare cím
    let from = mails[0].header("From").unwrap();
    assert!(from.contains("Magyar Scrabble") && from.ends_with("<noreply@example.org>"), "{from}");
}

// ===================================================================================================
// Visszaállítás
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn reset_needs_sudo() {
    let (_server, api) = start().await;
    assert_eq!(api.admin_delete("/system/mail", json!({"reason": REASON})).await.status, 401);
}

#[tokio::test(flavor = "multi_thread")]
async fn reset_needs_a_reason() {
    let (server, api) = start().await;
    api.sudo().await;
    save(&api, json!({})).await;
    assert_eq!(api.admin_delete("/system/mail", json!({})).await.status, 400);
    assert!(server.app.mailer.stored().is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn reset_returns_to_the_environment() {
    let (server, api) = start().await;
    api.sudo().await;
    save(&api, json!({})).await;
    let response = api.admin_delete("/system/mail", json!({"reason": REASON})).await;
    assert_eq!(response.status, 200);
    assert_eq!(response.json()["mail"]["source"], "environment");
    assert!(server.app.mailer.stored().is_none());
    assert_eq!(audit(&server, "system.mail_reset")[0]["details"]["before"]["source"], "database");
}

#[tokio::test(flavor = "multi_thread")]
async fn reset_without_a_saved_setting_is_a_conflict() {
    let (_server, api) = start().await;
    api.sudo().await;
    assert_eq!(api.admin_delete("/system/mail", json!({"reason": REASON})).await.status, 409);
}

// ===================================================================================================
// Kapcsolat-próba
// ===================================================================================================

fn local(smtp: &FakeSmtp, overrides: Value) -> Value {
    let mut cfg = merged(json!({"host": "127.0.0.1", "port": smtp.port(), "security": "none"}));
    for (k, v) in overrides.as_object().unwrap() {
        cfg[k] = v.clone();
    }
    cfg
}

#[tokio::test(flavor = "multi_thread")]
async fn check_needs_sudo() {
    let (_server, api) = start().await;
    assert_eq!(api.admin_post("/system/mail/check", good()).await.status, 401);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_successful_check_does_not_save_or_send_anything() {
    let (server, api) = start().await;
    let smtp = FakeSmtp::start();
    smtp.require_login("mailer", "titkos-jelszo-123");
    api.sudo().await;
    let data = api.admin_post("/system/mail/check", local(&smtp, json!({}))).await.json();
    assert_eq!((data["ok"].clone(), data["code"].clone(), data["sent_to"].clone()), (json!(true), json!("ok"), Value::Null));
    assert_eq!(smtp.logins(), vec!["mailer"]);
    assert!(smtp.mails().is_empty());
    assert!(server.app.mailer.stored().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_test_message_goes_to_the_admin_only() {
    let (_server, api) = start().await;
    let smtp = FakeSmtp::start();
    api.sudo().await;
    let mut body = local(&smtp, json!({"username": "", "password": ""}));
    body["send"] = json!(true);
    body["recipient"] = json!("masvalaki@example.com");
    let data = api.admin_post("/system/mail/check", body).await.json();
    assert_eq!((data["ok"].clone(), data["sent_to"].clone()), (json!(true), json!(ADMIN_EMAIL)));
    let mails = smtp.wait_for(1, 5).await;
    assert_eq!(mails.len(), 1);
    assert_eq!(mails[0].to, vec![ADMIN_EMAIL.to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failure_returns_a_code_and_is_logged_without_secrets() {
    let (server, api) = start().await;
    let smtp = FakeSmtp::start();
    smtp.require_login("mailer", "mas-jelszo");
    api.sudo().await;
    let data = api.admin_post("/system/mail/check", local(&smtp, json!({}))).await.json();
    assert_eq!((data["ok"].clone(), data["code"].clone(), data["sent_to"].clone()), (json!(false), json!("auth"), Value::Null));
    let entry = audit(&server, "system.mail_check")[0].clone();
    assert_eq!(entry["details"]["code"], "auth");
    assert!(!entry.to_string().contains("titkos-jelszo-123"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_closed_port_is_reported_as_refused() {
    let (_server, api) = start().await;
    let closed = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    api.sudo().await;
    let data = api.admin_post("/system/mail/check", merged(json!({"host": "127.0.0.1", "port": closed, "security": "none"}))).await.json();
    assert_eq!((data["ok"].clone(), data["code"].clone()), (json!(false), json!("refused")));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_draft_is_validated() {
    let (_server, api) = start().await;
    api.sudo().await;
    assert_eq!(api.admin_post("/system/mail/check", merged(json!({"host": ""}))).await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stored_password_is_not_sent_to_a_different_host() {
    let (_server, api) = start().await;
    let smtp = FakeSmtp::start();
    smtp.require_login("mailer", "titkos-jelszo-123");
    api.sudo().await;
    assert_eq!(save(&api, json!({"host": "127.0.0.1", "port": smtp.port(), "security": "none"})).await.status, 200);
    let elsewhere = api.admin_post("/system/mail/check", local(&smtp, json!({"password": null, "host": "elsewhere.example.net"}))).await;
    assert_eq!(elsewhere.status, 400);
    assert!(smtp.logins().is_empty());
    let same = api.admin_post("/system/mail/check", local(&smtp, json!({"password": null}))).await;
    assert_eq!(same.status, 200);
    assert_eq!(same.json()["ok"], true);
    assert_eq!(smtp.logins(), vec!["mailer"]); // a mentett jelszóval jelentkezett be
}

#[tokio::test(flavor = "multi_thread")]
async fn the_saved_configuration_test_reports_the_failure_code() {
    let (_server, api) = start().await;
    let smtp = FakeSmtp::start();
    smtp.require_login("mailer", "mas-jelszo");
    api.sudo().await;
    assert_eq!(save(&api, json!({"host": "127.0.0.1", "port": smtp.port(), "security": "none"})).await.status, 200);
    let response = api.admin_post("/system/smtp-test", json!({})).await;
    assert_eq!(response.status, 502);
    assert_eq!(response.json()["code"], "auth");
    assert!(!response.text().contains("hamis-smtp"), "a kiszolgáló szövege nem mehet a felületre");
}
