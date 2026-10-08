//! E-mail küldés: verifikációs és egyszerű levelek a mentett levelező beállítással, a fejlécek védelme és a
//! kapcsolat-próba hibakódjai (a Python `test_email_service.py` megfelelője). A kiszolgáló a helyi `FakeSmtp`.

mod common;
use common::smtp::FakeSmtp;
use common::*;
use scrabble::mail::{MailConfig, check_connection};
use serde_json::{Value, json};
use std::sync::Arc;

fn config_for(smtp: &FakeSmtp, overrides: Value) -> Value {
    let mut cfg = json!({
        "host": "127.0.0.1", "port": smtp.port(), "security": "none", "username": "", "password": "",
        "from_address": "noreply@example.org", "from_name": "Magyar Scrabble", "verify_tls": true
    });
    for (k, v) in overrides.as_object().unwrap() {
        cfg[k] = v.clone();
    }
    cfg
}

fn store(server: &TestServer, cfg: &Value) {
    server.app.db.with(|tx| server.app.mailer.save(tx, cfg, 1)).unwrap();
}

/// A levél küldése a háttérszálon (a küldés blokkoló). Visszatér: (elküldve-e, hibakód).
async fn send(server: &TestServer, to: &str, subject: &str, body: &str) -> (bool, Option<String>) {
    let mailer = server.app.mailer.clone();
    let (to, subject, body) = (to.to_string(), subject.to_string(), body.to_string());
    tokio::task::spawn_blocking(move || mailer.send_plain_email(&to, &subject, &body, true)).await.unwrap()
}

fn mail_config(cfg: &Value) -> MailConfig {
    MailConfig {
        host: cfg["host"].as_str().unwrap().to_string(),
        port: cfg["port"].as_i64().unwrap() as u16,
        security: cfg["security"].as_str().unwrap().to_string(),
        username: cfg["username"].as_str().unwrap().to_string(),
        password: cfg["password"].as_str().unwrap().to_string(),
        from_address: cfg["from_address"].as_str().unwrap().to_string(),
        from_name: cfg["from_name"].as_str().unwrap().to_string(),
        verify_tls: cfg["verify_tls"].as_bool().unwrap(),
        source: "database",
        configured: true,
    }
}

async fn check(cfg: &Value, recipient: Option<&str>) -> (bool, &'static str) {
    let cfg = mail_config(cfg);
    let recipient = recipient.map(|r| r.to_string());
    tokio::task::spawn_blocking(move || check_connection(&cfg, recipient.as_deref())).await.unwrap()
}

// ===================================================================================================
// Verifikációs levél
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn without_smtp_nothing_is_sent_and_the_code_goes_to_the_console() {
    let server = TestServer::start().await;
    assert!(!server.app.mailer.is_configured());
    assert_eq!(send(&server, "a@example.com", "Tárgy", "Szöveg").await, (false, Some("smtp_not_configured".to_string())));
    // a verifikációs levél sem megy sehova (nem hibázik)
    let mailer: Arc<scrabble::mail::Mailer> = server.app.mailer.clone();
    tokio::task::spawn_blocking(move || mailer.send_verification_email("test@example.com", "123456")).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_saved_configuration_turns_mail_on_without_environment() {
    let server = TestServer::start().await;
    let smtp = FakeSmtp::start();
    store(&server, &config_for(&smtp, json!({})));
    assert!(server.app.mailer.is_configured());
    let mailer = server.app.mailer.clone();
    tokio::task::spawn_blocking(move || mailer.send_verification_email("test@example.com", "654321")).await.unwrap();
    let mails = smtp.wait_for(1, 5).await;
    assert_eq!(mails.len(), 1, "a verifikációs levél a háttérszálon elmegy");
    assert_eq!(mails[0].to, vec!["test@example.com".to_string()]);
    assert_eq!(mails[0].subject(), "Magyar Scrabble - Verifikációs kód");
    assert!(mails[0].raw.contains("654321"), "{}", mails[0].raw);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_verification_send_does_not_panic() {
    let server = TestServer::start().await;
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    store(
        &server,
        &json!({
            "host": "127.0.0.1", "port": closed, "security": "none", "username": "", "password": "",
            "from_address": "noreply@example.org", "from_name": "", "verify_tls": true
        }),
    );
    let mailer = server.app.mailer.clone();
    tokio::task::spawn_blocking(move || mailer.send_verification_email("test@example.com", "111222")).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await; // a háttérszál hibája a konzolra megy
}

// ===================================================================================================
// A fejlécek
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_sender_name_goes_into_the_from_header_only() {
    let server = TestServer::start().await;
    let smtp = FakeSmtp::start();
    store(&server, &config_for(&smtp, json!({})));
    assert_eq!(send(&server, "recipient@example.com", "Tárgy", "Szöveg").await, (true, None));
    let mail = &smtp.mails()[0];
    let from = mail.header("From").unwrap();
    assert!(from.contains("Magyar Scrabble") && from.ends_with("<noreply@example.org>"), "{from}");
    assert_eq!(mail.from, "noreply@example.org"); // a boríték feladója a bare cím
    assert_eq!(mail.to, vec!["recipient@example.com".to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn date_and_message_id_headers_are_present() {
    let server = TestServer::start().await;
    let smtp = FakeSmtp::start();
    store(&server, &config_for(&smtp, json!({})));
    send(&server, "recipient@example.com", "T", "S").await;
    let mail = &smtp.mails()[0];
    assert!(mail.header("Date").is_some());
    let id = mail.header("Message-ID").unwrap();
    assert!(id.starts_with('<') && id.ends_with("@example.org>"), "{id}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_non_ascii_sender_name_and_subject_are_encoded() {
    let server = TestServer::start().await;
    let smtp = FakeSmtp::start();
    store(&server, &config_for(&smtp, json!({"from_name": "Scrabble Klub Győr"})));
    send(&server, "recipient@example.com", "Tárgy: árvíztűrő", "Szöveg: ő").await;
    let mail = &smtp.mails()[0];
    assert!(mail.raw.split("\r\n\r\n").next().unwrap().contains("=?utf-8?"), "{}", mail.raw);
    assert_eq!(mail.subject(), "Tárgy: árvíztűrő");
    assert_eq!(mail.body().trim_end(), "Szöveg: ő");
    assert!(mail.header("From").unwrap().ends_with("<noreply@example.org>"));
}

#[tokio::test(flavor = "multi_thread")]
async fn header_injection_is_neutralised_even_in_a_damaged_setting() {
    let server = TestServer::start().await;
    let smtp = FakeSmtp::start();
    // az ellenőrzést megkerülő (kézzel írt) tárolt érték és tárgy
    store(&server, &config_for(&smtp, json!({"from_name": "Név\r\nBcc: x@y.hu"})));
    send(&server, "recipient@example.com", "Tárgy\r\nBcc: z@y.hu", "Szöveg").await;
    let mail = &smtp.mails()[0];
    assert!(mail.header("Bcc").is_none(), "{}", mail.raw);
    assert_eq!(mail.to, vec!["recipient@example.com".to_string()]);
}

// ===================================================================================================
// Bejelentkezés, titkosítás, azonnali hatás
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn login_is_used_only_with_a_username() {
    let server = TestServer::start().await;
    let smtp = FakeSmtp::start();
    smtp.require_login("mailer", "pw-1");
    store(&server, &config_for(&smtp, json!({"username": "mailer", "password": "pw-1"})));
    assert_eq!(send(&server, "a@example.com", "T", "S").await, (true, None));
    assert_eq!(smtp.logins(), vec!["mailer"]);
    // bejelentkezés nélküli levelező: nincs AUTH
    let open = FakeSmtp::start();
    store(&server, &config_for(&open, json!({})));
    assert_eq!(send(&server, "a@example.com", "T", "S").await, (true, None));
    assert!(open.logins().is_empty());
    assert_eq!(open.mails().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_change_applies_without_a_restart() {
    let server = TestServer::start().await;
    let (first, second) = (FakeSmtp::start(), FakeSmtp::start());
    store(&server, &config_for(&first, json!({})));
    send(&server, "a@example.com", "T", "S").await;
    store(&server, &config_for(&second, json!({})));
    send(&server, "a@example.com", "T", "S").await;
    assert_eq!((first.mails().len(), second.mails().len()), (1, 1));
}

#[tokio::test(flavor = "multi_thread")]
async fn without_wait_the_send_runs_in_the_background() {
    let server = TestServer::start().await;
    let smtp = FakeSmtp::start();
    store(&server, &config_for(&smtp, json!({})));
    let mailer = server.app.mailer.clone();
    let result = tokio::task::spawn_blocking(move || mailer.send_plain_email("a@example.com", "T", "S", false)).await.unwrap();
    assert_eq!(result, (true, None));
    assert_eq!(smtp.wait_for(1, 5).await.len(), 1);
}

// ===================================================================================================
// Biztonságos hibakódok
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn wait_returns_a_safe_error_code() {
    let server = TestServer::start().await;
    let smtp = FakeSmtp::start();
    smtp.require_login("mailer", "helyes");
    store(&server, &config_for(&smtp, json!({"username": "mailer", "password": "rossz"})));
    assert_eq!(send(&server, "a@example.com", "T", "S").await, (false, Some("auth".to_string()))); // a kiszolgáló szövege nem megy tovább
}

#[tokio::test(flavor = "multi_thread")]
async fn error_codes_by_failure() {
    // elutasított címzett
    let smtp = FakeSmtp::start();
    smtp.reject_recipients_with(Some("550 5.1.1 no such user"));
    assert_eq!(check(&config_for(&smtp, json!({})), Some("a@example.com")).await, (false, "recipient"));
    // elutasított feladó
    let smtp = FakeSmtp::start();
    smtp.reject_sender_with(Some("553 5.7.1 sender rejected"));
    assert_eq!(check(&config_for(&smtp, json!({})), Some("a@example.com")).await, (false, "sender"));
    // elutasított tartalom
    let smtp = FakeSmtp::start();
    smtp.reject_data_with(Some("554 5.7.1 spam"));
    assert_eq!(check(&config_for(&smtp, json!({})), Some("a@example.com")).await, (false, "protocol"));
    // az üdvözlés után bontott kapcsolat
    let smtp = FakeSmtp::start();
    smtp.hang_up_after_greeting(true);
    assert_eq!(check(&config_for(&smtp, json!({})), None).await, (false, "disconnected"));
    // titkosítás nélküli kiszolgáló „ssl” módban: nem TLS-t kap
    let smtp = FakeSmtp::start();
    let (ok, code) = check(&config_for(&smtp, json!({"security": "ssl"})), None).await;
    assert!(!ok && code == "tls", "{code}");
    // STARTTLS-t nem tudó kiszolgáló
    let smtp = FakeSmtp::start();
    let (ok, code) = check(&config_for(&smtp, json!({"security": "starttls"})), None).await;
    assert!(!ok && ["unsupported", "tls"].contains(&code), "{code}");
    // zárt port
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let cfg = json!({"host": "127.0.0.1", "port": closed, "security": "none", "username": "", "password": "", "from_address": "a@example.org", "from_name": "", "verify_tls": true});
    assert_eq!(check(&cfg, None).await, (false, "refused"));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_host_is_reported_as_dns() {
    let cfg = json!({"host": "nem-letezo.invalid", "port": 25, "security": "none", "username": "", "password": "", "from_address": "a@example.org", "from_name": "", "verify_tls": true});
    let (ok, code) = check(&cfg, None).await;
    assert!(!ok && ["dns", "connect"].contains(&code), "{code}");
}

// ===================================================================================================
// Kapcsolat-próba
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_check_connects_and_logs_in_without_sending() {
    let smtp = FakeSmtp::start();
    smtp.require_login("u", "p");
    let cfg = config_for(&smtp, json!({"username": "u", "password": "p"}));
    assert_eq!(check(&cfg, None).await, (true, "ok"));
    assert_eq!(smtp.logins(), vec!["u"]);
    assert!(smtp.mails().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_check_sends_a_test_message_to_the_recipient() {
    let smtp = FakeSmtp::start();
    let cfg = config_for(&smtp, json!({}));
    assert_eq!(check(&cfg, Some("boss@example.com")).await, (true, "ok"));
    let mails = smtp.mails();
    assert_eq!(mails.len(), 1);
    assert_eq!((mails[0].from.as_str(), mails[0].to.clone()), ("noreply@example.org", vec!["boss@example.com".to_string()]));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_login_failure_is_reported_as_auth() {
    let smtp = FakeSmtp::start();
    smtp.require_login("u", "jo");
    assert_eq!(check(&config_for(&smtp, json!({"username": "u", "password": "rossz"})), None).await, (false, "auth"));
}
