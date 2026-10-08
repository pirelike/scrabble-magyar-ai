//! Admin panel: a konfiguráció a környezeti változókból — a titkok sosem látszanak (a Python
//! `test_admin_system.py` `TestSystem` osztályának környezet-érzékeny tesztjei).
//!
//! A környezeti változók a folyamat egészére hatnak, ezért ezek a tesztek külön binárisban, egymás után futnak.

mod common;
use common::*;
use serde_json::{Value, json};

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn set_env(key: &str, value: Option<&str>) {
    // a tesztek zárja alatt hívódik: más szál nem olvassa egyszerre
    unsafe {
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }
}

fn items(system: &Value) -> std::collections::HashMap<String, Value> {
    system["config"].as_array().unwrap().iter().map(|i| (i["key"].as_str().unwrap().to_string(), i.clone())).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn config_hides_the_secrets() {
    let _serial = SERIAL.lock().await;
    let server = TestServer::start_with(|c| c.smtp_password = "titkos-jelszo".into()).await;
    let api = server.admin_http().await;
    set_env("SECRET_KEY", Some("nagyon-titkos"));
    set_env("VAPID_PRIVATE_KEY", Some("vapid-titok"));
    let text = api.admin_get("/system").await.text();
    assert!(!text.contains("titkos-jelszo") && !text.contains("nagyon-titkos") && !text.contains("vapid-titok"));
    let items = items(&api.admin_get("/system").await.json());
    for key in ["SMTP_PASSWORD", "SECRET_KEY", "VAPID_PRIVATE_KEY"] {
        assert_eq!(items[key], json!({"key": key, "value": "••••", "secret": true, "set": true}), "{key}");
    }
    assert_eq!(items["ADMIN_EMAILS"]["value"], json!([ADMIN_EMAIL]));
    assert_eq!(items["SMTP_HOST"]["secret"], false);
    set_env("SECRET_KEY", None);
    set_env("VAPID_PRIVATE_KEY", None);
}

#[tokio::test(flavor = "multi_thread")]
async fn unset_secrets_say_so() {
    let _serial = SERIAL.lock().await;
    let server = TestServer::start().await;
    let api = server.admin_http().await;
    set_env("SECRET_KEY", None);
    let items = items(&api.admin_get("/system").await.json());
    assert_eq!(items["SMTP_PASSWORD"]["set"], false);
    assert_eq!(items["SECRET_KEY"]["value"], "");
}
