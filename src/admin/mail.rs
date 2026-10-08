//! Admin panel: a levelező szerver (SMTP) beállítása, kapcsolat-próbája és visszaállítása.
//!
//! A tárolás és a most érvényes beállítás a `mail.rs`-ben van; itt az ellenőrzés, a naplózás és a próba. A jelszó soha
//! nem kerül a válaszba és a naplóba (csak az, hogy változott-e). A módosítás a beállítást azonnal, újraindítás nélkül
//! érvényesíti: a levélküldés minden küldésnél a friss értéket olvassa.

use super::*;
use crate::app::App;
use crate::db::{RowExt, fetch_one};
use crate::mail::{MailConfig, SECURITY_MODES, check_connection};
use once_cell::sync::Lazy;
use regex::Regex;
use std::net::IpAddr;

static HOST_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?(\.[A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?)*$").expect("érvényes regex"));
static EMAIL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$").expect("érvényes regex"));
pub const MAX_HOST: usize = 253;
pub const MAX_USERNAME: usize = 254;
pub const MAX_PASSWORD: usize = 256;
pub const MAX_FROM_NAME: usize = 80;

/// Fejléc-injektálás ellen: sortörés és vezérlőkarakter nem lehet a mezőben.
fn single_line(text: String, field: &str) -> AdminResult<String> {
    if text.chars().any(|c| (c as u32) < 32 || c as u32 == 127) {
        return Err(AdminError::field("Érvénytelen karakter a mezőben.", 400, field));
    }
    Ok(text)
}

fn host_of(value: Option<&Value>) -> AdminResult<String> {
    let host = single_line(clean_text(value, "host", MAX_HOST, true)?, "host")?;
    if HOST_RE.is_match(&host) {
        return Ok(host);
    }
    let stripped = host.trim_matches(['[', ']']);
    if stripped.parse::<IpAddr>().is_ok() {
        return Ok(stripped.to_string()); // IPv6 cím
    }
    Err(AdminError::field("Érvénytelen kiszolgáló.", 400, "host"))
}

/// A kérésből ellenőrzött, teljes beállítás. A jelszó hiányában (`null`) a most érvényes marad (ha a kiszolgáló és a
/// felhasználónév nem változott); felhasználónév nélkül nincs jelszó.
pub fn build(app: &App, data: &Value) -> AdminResult<Value> {
    if !data.is_object() {
        return Err(AdminError::new("Érvénytelen kérés.", 400));
    }
    let current = app.mailer.current();
    let host = host_of(data.get("host"))?;
    let port = int_field(data.get("port"), "port", 1, 65535)?;
    let Some(security) = data.get("security").and_then(|s| s.as_str()).filter(|s| SECURITY_MODES.contains(s)) else {
        return Err(AdminError::field("Érvénytelen titkosítási mód.", 400, "security"));
    };
    let username = single_line(clean_text(data.get("username"), "username", MAX_USERNAME, false)?, "username")?;
    let password_value = data.get("password").filter(|p| !p.is_null());
    if let Some(p) = password_value
        && !p.as_str().is_some_and(|s| s.chars().count() <= MAX_PASSWORD)
    {
        return Err(AdminError::field("Érvénytelen jelszó.", 400, "password"));
    }
    let password: String = if username.is_empty() {
        String::new()
    } else {
        match password_value.and_then(|p| p.as_str()) {
            None => {
                // Nincs megadva: a mostani marad, de csak ugyanahhoz a kiszolgálóhoz és felhasználóhoz (a mentett jelszó
                // így nem küldhető el egy másik címre anélkül, hogy valaki újra megadná)
                if host != current.host || username != current.username {
                    return Err(AdminError::field("A kiszolgáló vagy a felhasználónév változott: add meg újra a jelszót.", 400, "password"));
                }
                current.password.clone()
            }
            Some(p) => single_line(p.to_string(), "password")?,
        }
    };
    if !username.is_empty() && password.is_empty() {
        return Err(AdminError::field("A felhasználónévhez jelszó is kell.", 400, "password"));
    }
    let from_address = single_line(clean_text(data.get("from_address"), "from_address", 254, true)?, "from_address")?;
    if !EMAIL_RE.is_match(&from_address) {
        return Err(AdminError::field("Érvénytelen feladó e-mail cím.", 400, "from_address"));
    }
    let from_name = single_line(clean_text(data.get("from_name"), "from_name", MAX_FROM_NAME, false)?, "from_name")?;
    let verify_tls = bool_field(Some(data.get("verify_tls").unwrap_or(&json!(true))), "verify_tls")?;
    Ok(json!({
        "host": host, "port": port, "security": security, "username": username, "password": password,
        "from_address": from_address, "from_name": from_name, "verify_tls": verify_tls,
    }))
}

/// A beállítás naplózható (jelszó nélküli) képe.
fn audit_view(cfg: &Value, source: Option<&str>) -> Value {
    json!({
        "host": cfg["host"], "port": cfg["port"], "security": cfg["security"], "username": cfg["username"],
        "password_set": cfg["password"].as_str().is_some_and(|p| !p.is_empty()), "from_address": cfg["from_address"],
        "from_name": cfg["from_name"], "verify_tls": cfg["verify_tls"].as_bool().unwrap_or(true), "source": source,
    })
}

fn config_json(cfg: &MailConfig) -> Value {
    json!({
        "host": cfg.host, "port": cfg.port, "security": cfg.security, "username": cfg.username, "password": cfg.password,
        "from_address": cfg.from_address, "from_name": cfg.from_name, "verify_tls": cfg.verify_tls,
    })
}

/// A felületnek szánt beállítás (jelszó nélkül), a módosító nevével.
pub fn view(app: &App) -> AdminResult<Value> {
    let mut data = app.mailer.public_view();
    data["updated_by_name"] = Value::Null;
    if let Some(admin_id) = data["updated_by"].as_i64() {
        let row = txn(&app.db, |tx| Ok(fetch_one(tx, "SELECT display_name FROM users WHERE id = ?", [admin_id])?))?;
        data["updated_by_name"] = row.map(|r| json!(r.text("display_name"))).unwrap_or(Value::Null);
    }
    Ok(data)
}

/// A levelező szerver beállításának mentése (kötelező indoklással, naplózva).
pub fn update(app: &App, ctx: &AdminContext, data: &Value) -> AdminResult<Value> {
    let cfg = build(app, data)?;
    let before = app.mailer.current();
    action(&app.db, ctx, "system.mail_update", Some("system"), Some("mail".to_string()), data.get("reason"), json!({}), true, |act| {
        app.mailer.save(act.tx, &cfg, ctx.admin_user_id)?;
        act.details.insert("before".into(), audit_view(&config_json(&before), Some(before.source)));
        act.details.insert("after".into(), audit_view(&cfg, Some("database")));
        act.details.insert("password_changed".into(), json!(cfg["password"].as_str().unwrap_or("") != before.password));
        Ok(())
    })?;
    view(app)
}

/// A mentett beállítás törlése: a környezeti változók értéke lép érvénybe.
pub fn reset(app: &App, ctx: &AdminContext, reason: Option<&Value>) -> AdminResult<Value> {
    if app.mailer.stored().is_none() {
        return Err(AdminError::new("Nincs mentett beállítás: a környezeti változók érvényesek.", 409));
    }
    let before = app.mailer.current();
    action(&app.db, ctx, "system.mail_reset", Some("system"), Some("mail".to_string()), reason, json!({}), true, |act| {
        app.mailer.clear(act.tx)?;
        act.details.insert("before".into(), audit_view(&config_json(&before), Some(before.source)));
        Ok(())
    })?;
    view(app)
}

/// Kapcsolat-próba a megadott (még nem mentett) beállítással; `recipient` esetén teszt levéllel. A próba a naplóba kerül
/// (indoklás nélkül, jelszó nélkül). Visszatér: {ok, code, sent_to}.
pub fn check(app: &App, ctx: &AdminContext, data: &Value, recipient: Option<&str>) -> AdminResult<Value> {
    let cfg = build(app, data)?;
    let send_to = if data.get("send") == Some(&json!(true)) { recipient.filter(|r| !r.is_empty()) } else { None };
    let mail_cfg = MailConfig {
        host: cfg["host"].as_str().unwrap_or("").to_string(),
        port: cfg["port"].as_i64().unwrap_or(0) as u16,
        security: cfg["security"].as_str().unwrap_or("starttls").to_string(),
        username: cfg["username"].as_str().unwrap_or("").to_string(),
        password: cfg["password"].as_str().unwrap_or("").to_string(),
        from_address: cfg["from_address"].as_str().unwrap_or("").to_string(),
        from_name: cfg["from_name"].as_str().unwrap_or("").to_string(),
        verify_tls: cfg["verify_tls"].as_bool().unwrap_or(true),
        source: "database",
        configured: true,
    };
    let (ok, code) = check_connection(&mail_cfg, send_to);
    app.db.with(|tx| {
        record(
            tx,
            ctx,
            "system.mail_check",
            Some("system"),
            Some("mail"),
            &json!({"host": cfg["host"], "port": cfg["port"], "security": cfg["security"], "sent": send_to.is_some(), "ok": ok, "code": code}),
        )
        .map(|_| ())
    })?;
    Ok(json!({"ok": ok, "code": code, "sent_to": if ok { json!(send_to) } else { Value::Null }}))
}
