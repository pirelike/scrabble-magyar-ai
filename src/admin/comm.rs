//! Admin panel: kommunikáció — közlemények (banner), karbantartási mód, push üzenetek, e-mail.
//!
//! A közlemény és a karbantartási mód nyilvános oldalról is olvasható (`active_announcements`); a küldés (push,
//! e-mail) a felhasználók beállításait és az SMTP / push elérhetőségét tiszteletben tartja.

use crate::app::App;
use crate::db::{RowExt, fetch_all, placeholders};
use crate::util;
use rusqlite::params_from_iter;
use serde_json::{Value, json};

/// A `2026-10-05 10:00:00` időbélyeg ISO alakja (UTC jelzéssel), a kliens `Date`-je így értelmezi.
pub fn utc_iso(stamp: Option<&str>) -> Value {
    match stamp.filter(|s| !s.is_empty()) {
        Some(s) => json!(format!("{}Z", s.replace(' ', "T"))),
        None => Value::Null,
    }
}

/// A most érvényes közlemények a nyilvános felületnek (a karbantartási móddal együtt). `registered`: a kérő
/// bejelentkezett-e (célcsoport szerinti szűréshez).
pub fn active_announcements(app: &App, registered: bool) -> Vec<Value> {
    let now = util::now_ts();
    let audiences: [&str; 2] = if registered { ["all", "registered"] } else { ["all", "guests"] };
    let sql = format!(
        "SELECT * FROM announcements WHERE active = 1 AND (starts_at IS NULL OR starts_at <= ?) \
         AND (ends_at IS NULL OR ends_at > ?) AND audience IN ({}) ORDER BY id",
        placeholders(audiences.len())
    );
    let mut args: Vec<String> = vec![now.clone(), now];
    args.extend(audiences.iter().map(|a| a.to_string()));
    let rows = app.db.with(|tx| fetch_all(tx, &sql, params_from_iter(args))).unwrap_or_default();
    let mut items: Vec<Value> = rows
        .iter()
        .map(|r| {
            let text_hu = r.text("text_hu");
            let text_en = r.opt_text("text_en").filter(|t| !t.is_empty()).unwrap_or_else(|| text_hu.clone());
            json!({
                "id": r.int("id"), "kind": r.text("kind"), "text_hu": text_hu, "text_en": text_en,
                "stamp": r.text("updated_at"), "ends_at": utc_iso(r.opt_text("ends_at").as_deref()),
            })
        })
        .collect();
    if let Some(maintenance) = app.settings.maintenance() {
        let message_hu = maintenance.get("message_hu").and_then(|m| m.as_str()).unwrap_or("").to_string();
        let message_en = maintenance.get("message_en").and_then(|m| m.as_str()).filter(|m| !m.is_empty()).map(|m| m.to_string()).unwrap_or_else(|| message_hu.clone());
        items.insert(
            0,
            json!({
                "id": "maintenance", "kind": "maintenance", "text_hu": message_hu, "text_en": message_en,
                "stamp": maintenance.get("started_at").and_then(|s| s.as_str()).unwrap_or(""),
                "ends_at": utc_iso(maintenance.get("until").and_then(|u| u.as_str())),
            }),
        );
    }
    items
}

// ===================================================================================================
// Közlemények, karbantartási mód, push, e-mail (admin)
// ===================================================================================================

use super::*;
use crate::db::{Row, fetch_one};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use rusqlite::types::Value as SqlValue;
use std::collections::HashSet;
use std::sync::Arc;

pub const ANNOUNCEMENT_KINDS: [&str; 3] = ["info", "warning", "maintenance"];
pub const AUDIENCES: [&str; 3] = ["all", "registered", "guests"];
pub const MAX_TEXT: usize = 500;
pub const MAX_TITLE: usize = 80;
pub const MAX_EMAIL_SUBJECT: usize = 150;
pub const MAX_EMAIL_BODY: usize = 5000;
pub const MAX_PUSH_RECIPIENTS: usize = 2000;
pub const MAX_EMAIL_RECIPIENTS: usize = 500;
/// mp: tömeges e-mailt ennyi időnként lehet küldeni
pub const BULK_EMAIL_COOLDOWN: f64 = 60.0;
pub const EMAIL_GROUPS: [&str; 2] = ["recent", "all"];
pub const RECENT_DAYS: i64 = 14;

static BULK_LAST: Lazy<Mutex<f64>> = Lazy::new(|| Mutex::new(0.0));

// ===== Közlemények =====

struct CleanAnnouncement {
    text_hu: String,
    text_en: String,
    kind: String,
    audience: String,
    starts_at: Option<String>,
    ends_at: Option<String>,
}

impl CleanAnnouncement {
    fn to_json(&self) -> Value {
        json!({
            "text_hu": self.text_hu, "text_en": self.text_en, "kind": self.kind, "audience": self.audience,
            "starts_at": self.starts_at, "ends_at": self.ends_at,
        })
    }
}

fn stamp(value: Option<&Value>, field: &str) -> AdminResult<Option<String>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.is_empty() => Ok(None),
        Some(Value::String(s)) => match parse_bound(s, false) {
            Ok((stamp, _)) => Ok(Some(stamp)),
            Err(mut e) => {
                e.extra.insert("field".into(), json!(field));
                Err(e)
            }
        },
        Some(_) => Err(AdminError::field("Érvénytelen dátum (ÉÉÉÉ-HH-NN formátum kell).", 400, field)),
    }
}

fn clean_announcement(data: &Value) -> AdminResult<CleanAnnouncement> {
    let text_hu = clean_text(data.get("text_hu"), "text_hu", MAX_TEXT, true)?;
    let text_en = clean_text(data.get("text_en"), "text_en", MAX_TEXT, false)?;
    let kind = data.get("kind").cloned().unwrap_or(json!("info"));
    let audience = data.get("audience").cloned().unwrap_or(json!("all"));
    let Some(kind) = kind.as_str().filter(|k| ANNOUNCEMENT_KINDS.contains(k)) else {
        return Err(AdminError::field("Érvénytelen típus.", 400, "kind"));
    };
    let Some(audience) = audience.as_str().filter(|a| AUDIENCES.contains(a)) else {
        return Err(AdminError::field("Érvénytelen célcsoport.", 400, "audience"));
    };
    let starts = stamp(data.get("starts_at"), "starts_at")?;
    let ends = stamp(data.get("ends_at"), "ends_at")?;
    if let (Some(s), Some(e)) = (&starts, &ends) {
        if e <= s {
            return Err(AdminError::field("A vég nem lehet a kezdet előtt.", 400, "ends_at"));
        }
    }
    Ok(CleanAnnouncement { text_hu, text_en, kind: kind.to_string(), audience: audience.to_string(), starts_at: starts, ends_at: ends })
}

fn announcement_status(row: &Row, now: &str) -> &'static str {
    if !row.flag("active") {
        return "revoked";
    }
    if row.opt_text("starts_at").is_some_and(|s| s.as_str() > now) {
        return "scheduled";
    }
    if row.opt_text("ends_at").is_some_and(|e| e.as_str() <= now) {
        return "expired";
    }
    "active"
}

fn announcement_item(row: &Row, now: &str) -> Value {
    json!({
        "id": row.int("id"), "text_hu": row.text("text_hu"), "text_en": row.get("text_en"), "kind": row.text("kind"),
        "audience": row.text("audience"), "starts_at": row.get("starts_at"), "ends_at": row.get("ends_at"),
        "active": row.flag("active"), "status": announcement_status(row, now), "created_by": row.get("created_by"),
        "created_at": row.text("created_at"), "updated_at": row.text("updated_at"),
    })
}

pub fn list_announcements(app: &App) -> AdminResult<Vec<Value>> {
    let now = util::now_ts();
    let rows = txn(&app.db, |tx| Ok(fetch_all(tx, "SELECT * FROM announcements ORDER BY id DESC LIMIT 200", [])?))?;
    Ok(rows.iter().map(|r| announcement_item(r, &now)).collect())
}

pub fn create_announcement(app: &App, ctx: &AdminContext, data: &Value) -> AdminResult<i64> {
    let clean = clean_announcement(data)?;
    action(&app.db, ctx, "comm.announcement_create", Some("announcement"), None, data.get("reason"), clean.to_json(), false, |act| {
        act.tx.execute(
            "INSERT INTO announcements (text_hu, text_en, kind, audience, starts_at, ends_at, created_by) VALUES (?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![clean.text_hu, clean.text_en, clean.kind, clean.audience, clean.starts_at, clean.ends_at, ctx.admin_user_id],
        )?;
        let id = act.tx.last_insert_rowid();
        act.details.insert("announcement_id".into(), json!(id));
        Ok(id)
    })
}

pub fn update_announcement(app: &App, ctx: &AdminContext, announcement_id: i64, data: &Value) -> AdminResult<()> {
    let clean = clean_announcement(data)?;
    let active = bool_field(Some(data.get("active").unwrap_or(&json!(true))), "active")?;
    action(&app.db, ctx, "comm.announcement_update", Some("announcement"), Some(announcement_id.to_string()), data.get("reason"), json!({}), false, |act| {
        let row = fetch_one(act.tx, "SELECT * FROM announcements WHERE id = ?", [announcement_id])?.ok_or_else(|| AdminError::new("A közlemény nem található.", 404))?;
        act.tx.execute(
            "UPDATE announcements SET text_hu = ?, text_en = ?, kind = ?, audience = ?, starts_at = ?, ends_at = ?, active = ?, updated_at = datetime('now') WHERE id = ?",
            rusqlite::params![clean.text_hu, clean.text_en, clean.kind, clean.audience, clean.starts_at, clean.ends_at, active as i64, announcement_id],
        )?;
        let mut before = Map::new();
        for key in ["text_hu", "text_en", "kind", "audience", "starts_at", "ends_at", "active"] {
            before.insert(key.to_string(), row.get(key).cloned().unwrap_or(Value::Null));
        }
        let mut after = clean.to_json();
        after["active"] = json!(active);
        act.details.insert("before".into(), Value::Object(before));
        act.details.insert("after".into(), after);
        Ok(())
    })
}

pub fn revoke_announcement(app: &App, ctx: &AdminContext, announcement_id: i64) -> AdminResult<()> {
    action(&app.db, ctx, "comm.announcement_revoke", Some("announcement"), Some(announcement_id.to_string()), None, json!({}), false, |act| {
        let row = fetch_one(act.tx, "SELECT active FROM announcements WHERE id = ?", [announcement_id])?.ok_or_else(|| AdminError::new("A közlemény nem található.", 404))?;
        if !row.flag("active") {
            return Err(AdminError::new("A közlemény már vissza van vonva.", 409));
        }
        act.tx.execute("UPDATE announcements SET active = 0, updated_at = datetime('now') WHERE id = ?", [announcement_id])?;
        Ok(())
    })
}

// ===== Karbantartási mód =====

pub fn maintenance_state(app: &App) -> Value {
    let value = app.settings.get(crate::settings::MAINTENANCE_KEY);
    let text = |k: &str| value.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    json!({
        "enabled": value.get("enabled").is_some_and(util::truthy), "active": app.settings.maintenance().is_some(),
        "message_hu": text("message_hu"), "message_en": text("message_en"),
        "until": value.get("until").cloned().unwrap_or(Value::Null), "started_at": value.get("started_at").cloned().unwrap_or(Value::Null),
    })
}

/// A karbantartási mód be- és kikapcsolása. Bekapcsolva nem hozható létre új szoba / levelezős játék / napi
/// próbálkozás, a futó játékok folytatódnak; az admin kivétel. Az `until` után magától véget ér.
pub fn set_maintenance(app: &App, ctx: &AdminContext, data: &Value) -> AdminResult<Value> {
    let enabled = bool_field(data.get("enabled"), "enabled")?;
    let message_hu = clean_text(data.get("message_hu"), "message_hu", MAX_TEXT, enabled)?;
    let message_en = clean_text(data.get("message_en"), "message_en", MAX_TEXT, false)?;
    let until = stamp(data.get("until"), "until")?;
    if enabled && until.as_ref().is_some_and(|u| *u <= util::now_ts()) {
        return Err(AdminError::field("A lejárat nem lehet a múltban.", 400, "until"));
    }
    let before = maintenance_state(app);
    let value = json!({
        "enabled": enabled, "message_hu": message_hu, "message_en": message_en, "until": until,
        "started_at": if enabled { json!(util::now_ts()) } else { Value::Null },
    });
    action(&app.db, ctx, "comm.maintenance", Some("setting"), Some(crate::settings::MAINTENANCE_KEY.to_string()), data.get("reason"), json!({}), false, |act| {
        app.settings.store(act.tx, crate::settings::MAINTENANCE_KEY, &value, Some(ctx.admin_user_id)).map_err(|e| AdminError::new(&e.0, 400))?;
        act.details.insert("before".into(), json!({"enabled": before["enabled"], "message_hu": before["message_hu"], "until": before["until"]}));
        act.details.insert("after".into(), json!({"enabled": enabled, "message_hu": message_hu, "until": until}));
        Ok(())
    })?;
    app.settings.invalidate();
    Ok(maintenance_state(app))
}

// ===== Címzettek =====

type Recipient = (i64, String, String);

/// A csoport felhasználói: [(id, név, e-mail)]. Törölt és kitiltott fiók nem címzett.
fn group_users(app: &App, group: &str, online: &HashSet<i64>, with_push: bool) -> AdminResult<Vec<Recipient>> {
    let now = util::now_ts();
    let mut where_ = "u.deleted_at IS NULL AND COALESCE(u.banned_until, '') <= ?".to_string();
    let mut params: Vec<SqlValue> = vec![SqlValue::Text(now)];
    match group {
        "recent" => {
            where_.push_str(" AND COALESCE(u.last_login_at, u.created_at) >= ?");
            params.push(SqlValue::Text(util::format_ts(util::utcnow() - chrono::Duration::days(RECENT_DAYS))));
        }
        "online" => {
            let mut ids: Vec<i64> = online.iter().copied().collect();
            ids.sort();
            if ids.is_empty() {
                where_.push_str(" AND 0");
            } else {
                where_.push_str(&format!(" AND u.id IN ({})", crate::db::placeholders(ids.len())));
            }
            params.extend(ids.into_iter().map(SqlValue::Integer));
        }
        "all" => {}
        _ => return Err(AdminError::field("Érvénytelen célcsoport.", 400, "group")),
    }
    if with_push {
        where_.push_str(" AND EXISTS (SELECT 1 FROM push_subscriptions p WHERE p.user_id = u.id)");
    }
    let rows = txn(&app.db, |tx| {
        Ok(fetch_all(tx, &format!("SELECT u.id, u.display_name, u.email FROM users u WHERE {where_} ORDER BY u.id"), params_from_iter(params))?)
    })?;
    Ok(rows.iter().map(|r| (r.int("id"), r.text("display_name"), r.text("email"))).collect())
}

fn user_row(app: &App, user_id: i64) -> AdminResult<Row> {
    let row = txn(&app.db, |tx| Ok(fetch_one(tx, "SELECT id, display_name, email, deleted_at FROM users WHERE id = ?", [user_id])?))?;
    match row {
        Some(row) if row.opt_text("deleted_at").is_none() => Ok(row),
        _ => Err(AdminError::new("Felhasználó nem található.", 404)),
    }
}

// ===== Push =====

fn clean_push(data: &Value) -> AdminResult<(String, String)> {
    Ok((clean_text(data.get("title"), "title", MAX_TITLE, true)?, clean_text(data.get("body"), "body", MAX_TEXT, true)?))
}

/// A push címzettjei: [(user_id, név)] — csak akinek van feliratkozott eszköze.
pub fn push_recipients(app: &App, target: Option<&str>, user_id: Option<i64>, group: Option<&str>, online: &HashSet<i64>) -> AdminResult<Vec<(i64, String)>> {
    let users: Vec<Recipient> = match target {
        Some("user") => {
            let row = user_row(app, user_id.unwrap_or(0))?;
            if app.db.count_push_subscriptions(row.int("id")) > 0 { vec![(row.int("id"), row.text("display_name"), row.text("email"))] } else { vec![] }
        }
        Some("group") => group_users(app, group.unwrap_or(""), online, true)?,
        _ => return Err(AdminError::field("Érvénytelen célcsoport.", 400, "target")),
    };
    Ok(users.into_iter().map(|(uid, name, _)| (uid, name)).take(MAX_PUSH_RECIPIENTS).collect())
}

/// Előnézet: mit kapnának és hányan (a küldés nélkül).
pub fn push_preview(app: &App, data: &Value, online: &HashSet<i64>) -> AdminResult<Value> {
    let (title, body) = clean_push(data)?;
    let recipients = push_recipients(app, data.get("target").and_then(|t| t.as_str()), data.get("user_id").and_then(|u| u.as_i64()), data.get("group").and_then(|g| g.as_str()), online)?;
    let devices: i64 = recipients.iter().map(|(uid, _)| app.db.count_push_subscriptions(*uid)).sum();
    Ok(json!({
        "title": title, "body": body, "recipients": recipients.len(), "devices": devices, "available": app.push.is_available(),
        "sample": recipients.iter().take(10).map(|(_, n)| n.clone()).collect::<Vec<_>>(),
    }))
}

/// Push üzenet küldése egy felhasználónak vagy csoportnak. Csoportnál megerősítés (`confirm`) kell. Visszatér: az
/// eredmény (sikeres / sikertelen / törölt feliratkozás).
pub fn push_send(app: &App, ctx: &AdminContext, data: &Value, online: &HashSet<i64>) -> AdminResult<Value> {
    if !app.push.is_available() {
        return Err(AdminError::new("A push értesítések ezen a szerveren nem érhetők el.", 503));
    }
    let (title, body) = clean_push(data)?;
    let target = data.get("target").and_then(|t| t.as_str());
    if target == Some("group") && data.get("confirm") != Some(&json!(true)) {
        return Err(AdminError::new("Csoportos küldéshez megerősítés kell.", 400).with("needs_confirm", json!(true)));
    }
    let recipients = push_recipients(app, target, data.get("user_id").and_then(|u| u.as_i64()), data.get("group").and_then(|g| g.as_str()), online)?;
    if recipients.is_empty() {
        return Err(AdminError::new("Nincs címzett (nincs feliratkozott eszköz).", 409));
    }
    let target_id = data.get("user_id").filter(|v| util::truthy(v)).or_else(|| data.get("group")).map(|v| match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    });
    app.db.with(|tx| {
        record(tx, ctx, "comm.push", Some(if target == Some("user") { "user" } else { "group" }), target_id.as_deref(), &json!({"title": title, "body": body, "recipients": recipients.len()}))
            .map(|_| ())
    })?;
    let (mut sent, mut removed, mut failed) = (0, 0, 0);
    for (uid, _) in &recipients {
        let result = app.push.send_custom(*uid, &title, &body, "/");
        sent += result["sent"].as_i64().unwrap_or(0);
        removed += result["removed"].as_i64().unwrap_or(0);
        failed += result["failed"].as_i64().unwrap_or(0);
    }
    Ok(json!({"recipients": recipients.len(), "sent": sent, "removed": removed, "failed": failed}))
}

/// Teszt üzenet a felhasználó összes eszközére.
pub fn push_test(app: &App, ctx: &AdminContext, user_id: i64) -> AdminResult<Value> {
    if !app.push.is_available() {
        return Err(AdminError::new("A push értesítések ezen a szerveren nem érhetők el.", 503));
    }
    let row = user_row(app, user_id)?;
    if app.db.count_push_subscriptions(user_id) == 0 {
        return Err(AdminError::new("A felhasználónak nincs feliratkozott eszköze.", 409));
    }
    app.db.with(|tx| record(tx, ctx, "user.push_test", Some("user"), Some(&user_id.to_string()), &Value::Null).map(|_| ()))?;
    let mut result = app.push.send_test(user_id);
    result["name"] = json!(row.text("display_name"));
    Ok(result)
}

// ===== E-mail =====

fn email_body(name: &str, body: &str) -> String {
    format!("Szia {name}!\n\n{body}\n\nÜdvözlettel,\nMagyar Scrabble")
}

fn clean_email(data: &Value) -> AdminResult<(String, String)> {
    let subject = clean_text(data.get("subject"), "subject", MAX_EMAIL_SUBJECT, true)?;
    let body = clean_text(data.get("body"), "body", MAX_EMAIL_BODY, true)?;
    if subject.contains(['\n', '\r']) {
        return Err(AdminError::field("Érvénytelen tárgy.", 400, "subject"));
    }
    Ok((subject, body))
}

/// E-mail egy felhasználónak (SMTP-n, sablonnal). SMTP nélkül a konzolra kerül. Visszatér: {'sent', 'console'}.
pub fn email_user(app: &Arc<App>, ctx: &AdminContext, user_id: i64, data: &Value) -> AdminResult<Value> {
    let (subject, body) = clean_email(data)?;
    let row = user_row(app, user_id)?;
    app.db.with(|tx| record(tx, ctx, "comm.email", Some("user"), Some(&user_id.to_string()), &json!({"subject": subject, "body": body})).map(|_| ()))?;
    let (ok, error) = app.mailer.send_plain_email(&row.text("email"), &subject, &email_body(&row.text("display_name"), &body), true);
    if error.as_deref().is_some_and(|e| e != "smtp_not_configured") {
        return Err(AdminError::new("Az e-mail küldése nem sikerült.", 502));
    }
    Ok(json!({"sent": ok, "console": !app.mailer.is_configured()}))
}

pub fn email_preview(app: &App, group: Option<&str>) -> AdminResult<Value> {
    let Some(group) = group.filter(|g| EMAIL_GROUPS.contains(g)) else {
        return Err(AdminError::field("Érvénytelen célcsoport.", 400, "group"));
    };
    let users = group_users(app, group, &HashSet::new(), false)?;
    Ok(json!({"recipients": users.len(), "limit": MAX_EMAIL_RECIPIENTS, "sample": users.iter().take(10).map(|(_, n, _)| n.clone()).collect::<Vec<_>>()}))
}

/// Tömeges e-mail: csak megerősítéssel (`confirm`), legfeljebb percenként egy, legfeljebb 500 címzett.
pub fn email_bulk(app: &Arc<App>, ctx: &AdminContext, data: &Value) -> AdminResult<Value> {
    let Some(group) = data.get("group").and_then(|g| g.as_str()).filter(|g| EMAIL_GROUPS.contains(g)) else {
        return Err(AdminError::field("Érvénytelen célcsoport.", 400, "group"));
    };
    let (subject, body) = clean_email(data)?;
    if data.get("confirm") != Some(&json!(true)) {
        return Err(AdminError::new("Tömeges küldéshez megerősítés kell.", 400).with("needs_confirm", json!(true)));
    }
    let users = group_users(app, group, &HashSet::new(), false)?;
    if users.is_empty() {
        return Err(AdminError::new("Nincs címzett.", 409));
    }
    if users.len() > MAX_EMAIL_RECIPIENTS {
        return Err(AdminError::new("Túl sok címzett egyszerre.", 400));
    }
    let now = util::now();
    {
        let last = BULK_LAST.lock();
        if now - *last < BULK_EMAIL_COOLDOWN {
            return Err(AdminError::new("Tömeges e-mailt legfeljebb percenként egyet lehet küldeni.", 429));
        }
    }
    app.db.with(|tx| record(tx, ctx, "comm.email_bulk", Some("group"), Some(group), &json!({"subject": subject, "body": body, "recipients": users.len()})).map(|_| ()))?;
    *BULK_LAST.lock() = now;
    let (mut sent, mut failed) = (0, 0);
    for (_, name, email) in &users {
        let (ok, error) = app.mailer.send_plain_email(email, &subject, &email_body(name, &body), false);
        if ok {
            sent += 1;
        } else if error.as_deref() != Some("smtp_not_configured") {
            failed += 1;
        }
    }
    Ok(json!({"recipients": users.len(), "sent": sent, "failed": failed, "console": !app.mailer.is_configured()}))
}

/// A tömeges e-mail korlátjának nullázása (tesztekhez).
pub fn reset_bulk_cooldown() {
    *BULK_LAST.lock() = 0.0;
}
