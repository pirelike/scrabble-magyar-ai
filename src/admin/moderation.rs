//! Admin panel: moderáció — tiltott szavak, chat napló, bejelentések, nevek átnézése.
//!
//! A játékszerver is használja: a tiltott szavak szűrése (`contains_banned`, `filter_chat`), a chat tartós
//! naplózása (`log_chat`) és a játékosok bejelentései (`create_report`) ide futnak be.

use crate::admin::*;
use crate::app::App;
use crate::db::{Db, Row, RowExt, fetch_all, fetch_one};
use crate::room::Room;
use parking_lot::Mutex;
use rusqlite::types::Value as SqlValue;
use serde_json::{Value, json};
use std::collections::HashMap;

pub const MAX_REPORT_REASON: usize = 300;
pub const MAX_REPORT_MESSAGE: usize = 200;
pub const SNAPSHOT_CHAT_LINES: usize = 15;
pub const REPORT_KINDS: [&str; 2] = ["player", "chat"];
pub const REPORT_STATUSES: [&str; 3] = ["new", "handled", "rejected"];

/// A tiltott szavak gyorsítótára (kis-nagybetű és ékezet nélküli alakban).
#[derive(Default)]
pub struct BannedWords {
    cache: Mutex<Option<Vec<String>>>,
}

impl BannedWords {
    pub fn invalidate(&self) {
        *self.cache.lock() = None;
    }

    pub fn words(&self, db: &Db) -> Vec<String> {
        let mut cache = self.cache.lock();
        if let Some(words) = cache.as_ref() {
            return words.clone();
        }
        let words: Vec<String> = db
            .with(|tx| {
                let rows = crate::db::fetch_all(tx, "SELECT word FROM banned_words ORDER BY word", [])?;
                Ok(rows.iter().map(|r| r.get("word").and_then(|w| w.as_str()).unwrap_or("").to_string()).collect())
            })
            .unwrap_or_default();
        *cache = Some(words.clone());
        words
    }
}

/// A szöveg ékezet- és kisbetű-független alakja + minden karakterének eredeti helye.
fn fold_with_map(text: &str) -> (Vec<char>, Vec<usize>) {
    let mut chars = Vec::new();
    let mut index = Vec::new();
    for (i, ch) in text.chars().enumerate() {
        for folded in fold(&ch.to_string()).chars() {
            chars.push(folded);
            index.push(i);
        }
    }
    (chars, index)
}

/// A szövegben talált tiltott szavak helyei: [(kezdet, vég)] az eredeti szövegben (átfedés nélkül).
pub fn find_banned(words: &[String], text: &str) -> Vec<(usize, usize)> {
    if words.is_empty() {
        return Vec::new();
    }
    let (folded, index) = fold_with_map(text);
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for word in words {
        let needle: Vec<char> = word.chars().collect();
        if needle.is_empty() || needle.len() > folded.len() {
            continue;
        }
        for start in 0..=(folded.len() - needle.len()) {
            if folded[start..start + needle.len()] == needle[..] {
                spans.push((index[start], index[start + needle.len() - 1] + 1));
            }
        }
    }
    spans.sort();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in spans {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}

pub fn contains_banned(words: &[String], text: &str) -> bool {
    !find_banned(words, text).is_empty()
}

/// A tiltott szavak kicsillagozva. Visszatér: (szöveg, talált-e).
pub fn mask_banned(words: &[String], text: &str) -> (String, bool) {
    let spans = find_banned(words, text);
    if spans.is_empty() {
        return (text.to_string(), false);
    }
    let mut chars: Vec<char> = text.chars().collect();
    for (start, end) in spans {
        for ch in chars.iter_mut().take(end).skip(start) {
            *ch = '*';
        }
    }
    (chars.into_iter().collect(), true)
}

/// A chat üzenet szűrése a beállított kezelés szerint. Visszatér: (üzenet vagy None = eldobva, jelölt-e).
pub fn filter_chat(words: &[String], drop_instead_of_mask: bool, message: &str) -> (Option<String>, bool) {
    let (masked, flagged) = mask_banned(words, message);
    if !flagged {
        return (Some(message.to_string()), false);
    }
    if drop_instead_of_mask { (None, true) } else { (Some(masked), true) }
}

/// A chat üzenet tartós naplózása (csak ha a beállítás be van kapcsolva; a hiba nem akadályozza a chatet).
pub fn log_chat(db: &Db, enabled: bool, room: &Room, user_id: Option<i64>, name: &str, message: &str) {
    if !enabled {
        return;
    }
    let result = db.with(|tx| {
        tx.execute(
            "INSERT INTO chat_log (room_id, room_name, user_id, name, message) VALUES (?, ?, ?, ?, ?)",
            rusqlite::params![room.id, room.name, user_id, name, message],
        )
        .map(|_| ())
    });
    if let Err(e) = result {
        println!("[chat] A chat napló nem írható: {e}");
    }
}

/// Egy bejelentés rögzítése a bejelentett tartalom pillanatképével. Visszatér: az azonosító, vagy None, ha a
/// bejelentés érvénytelen.
#[allow(clippy::too_many_arguments)]
pub fn create_report(
    db: &Db,
    reporter_id: i64,
    reporter_name: &str,
    reported_user_id: Option<i64>,
    reported_name: &str,
    kind: &Value,
    message: &Value,
    reason: &Value,
    room: &Room,
) -> Option<i64> {
    let kind = kind.as_str().filter(|k| REPORT_KINDS.contains(k))?;
    let reason = reason.as_str().map(|r| r.trim()).unwrap_or("");
    if reason.chars().count() > MAX_REPORT_REASON {
        return None;
    }
    let mut text = message.as_str().map(|m| m.trim().to_string()).unwrap_or_default();
    if kind == "chat" && (text.is_empty() || text.chars().count() > MAX_REPORT_MESSAGE) {
        return None;
    }
    if kind == "player" {
        text.clear();
    }
    let chat_len = room.chat_messages.len();
    let snapshot = json!({
        "players": room.game.players.iter().map(|p| json!({"name": p.name, "is_bot": p.is_bot, "score": p.score})).collect::<Vec<_>>(),
        "chat": &room.chat_messages[chat_len.saturating_sub(SNAPSHOT_CHAT_LINES)..],
        "turn": room.game.turn_number,
    });
    db.with(|tx| {
        tx.execute(
            "INSERT INTO reports (reporter_user_id, reporter_name, reported_user_id, reported_name, kind, message, \
             reason, room_id, room_name, snapshot_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![reporter_id, reporter_name, reported_user_id, reported_name, kind, text, reason, room.id, room.name, snapshot.to_string()],
        )?;
        Ok(tx.last_insert_rowid())
    })
    .ok()
}

/// Push értesítés az adminoknak egy új bejelentésről (háttérben; push híján nem történik semmi).
pub fn notify_admins_of_report(app: &std::sync::Arc<crate::app::App>, room_name: &str) {
    let mut emails: Vec<&String> = app.config.admin_emails.iter().collect();
    emails.sort();
    for email in emails {
        if let Ok(Some(user)) = app.db.get_user_by_email(email) {
            app.push.notify_background(user.id, "report", room_name);
        }
    }
}

pub const MIN_BANNED_WORD: usize = 2;
pub const MAX_BANNED_WORD: usize = 40;
pub const CHAT_LOG_LIMIT_MAX: i64 = 500;

// ===== Tiltott szavak (admin) =====

pub fn list_banned_words(app: &App) -> AdminResult<Vec<Value>> {
    let rows = txn(&app.db, |tx| {
        Ok(fetch_all(
            tx,
            "SELECT w.word, w.created_at, u.display_name AS admin_name FROM banned_words w LEFT JOIN users u ON u.id = w.created_by ORDER BY w.word",
            [],
        )?)
    })?;
    Ok(rows.into_iter().map(Value::Object).collect())
}

pub fn add_banned_word(app: &App, ctx: &AdminContext, word: Option<&Value>, reason: Option<&Value>) -> AdminResult<String> {
    let text = fold(&clean_text(word, "word", MAX_BANNED_WORD, true)?);
    if text.chars().count() < MIN_BANNED_WORD {
        return Err(AdminError::field("A tiltott szó legalább 2 karakter legyen.", 400, "word"));
    }
    action(&app.db, ctx, "mod.word_add", Some("word"), Some(text.clone()), reason, json!({}), true, |act| {
        if fetch_one(act.tx, "SELECT 1 AS x FROM banned_words WHERE word = ?", [&text])?.is_some() {
            return Err(AdminError::field("Ez a szó már szerepel a listán.", 409, "word"));
        }
        act.tx.execute("INSERT INTO banned_words (word, created_by) VALUES (?, ?)", rusqlite::params![text, ctx.admin_user_id])?;
        Ok(())
    })?;
    app.banned_words.invalidate();
    Ok(text)
}

pub fn remove_banned_word(app: &App, ctx: &AdminContext, word: Option<&Value>, reason: Option<&Value>) -> AdminResult<()> {
    let text = fold(&clean_text(word, "word", MAX_BANNED_WORD, true)?);
    action(&app.db, ctx, "mod.word_remove", Some("word"), Some(text.clone()), reason, json!({}), true, |act| {
        if act.tx.execute("DELETE FROM banned_words WHERE word = ?", [&text])? == 0 {
            return Err(AdminError::new("A szó nem szerepel a listán.", 404));
        }
        Ok(())
    })?;
    app.banned_words.invalidate();
    Ok(())
}

// ===== Chat napló =====

/// A tartósan naplózott chat üzenetek (szűrés: q, room, user, since, until). Naplózva: `view.chat`.
pub fn query_chat_log(app: &App, ctx: &AdminContext, args: &HashMap<String, String>) -> AdminResult<Value> {
    let mut where_: Vec<String> = Vec::new();
    let mut params: Vec<SqlValue> = Vec::new();
    let arg = |k: &str| args.get(k).map(|s| s.as_str()).filter(|s| !s.is_empty());
    let q = args.get("q").map(|q| q.trim()).unwrap_or("");
    if !q.is_empty() {
        where_.push("c.message LIKE ? ESCAPE '\\'".into());
        params.push(SqlValue::Text(like(q)));
    }
    if let Some(room) = arg("room") {
        where_.push("(c.room_id = ? OR c.room_name LIKE ? ESCAPE '\\')".into());
        params.push(SqlValue::Text(room.to_string()));
        params.push(SqlValue::Text(like(room)));
    }
    let user = args.get("user").map(|u| u.trim()).unwrap_or("");
    if !user.is_empty() {
        if user.chars().all(|c| c.is_ascii_digit()) {
            where_.push("c.user_id = ?".into());
            params.push(SqlValue::Integer(user.parse().unwrap_or(0)));
        } else {
            where_.push("c.name LIKE ? ESCAPE '\\'".into());
            params.push(SqlValue::Text(like(user)));
        }
    }
    if let Some(since) = arg("since") {
        where_.push("c.created_at >= ?".into());
        params.push(SqlValue::Text(parse_bound(since, false)?.0));
    }
    if let Some(until) = arg("until") {
        let (stamp, exclusive) = parse_bound(until, true)?;
        where_.push(if exclusive { "c.created_at < ?" } else { "c.created_at <= ?" }.into());
        params.push(SqlValue::Text(stamp));
    }
    let (limit, offset) = page_args(args, 100, CHAT_LOG_LIMIT_MAX);
    let clause = if where_.is_empty() { String::new() } else { format!("WHERE {}", where_.join(" AND ")) };
    let filters: serde_json::Map<String, Value> =
        ["q", "room", "user", "since", "until"].iter().filter_map(|k| arg(k).map(|v| (k.to_string(), json!(v)))).collect();
    let (total, rows) = txn(&app.db, |tx| {
        let total = fetch_one(tx, &format!("SELECT COUNT(*) AS n FROM chat_log c {clause}"), rusqlite::params_from_iter(params.clone()))?
            .map(|r| r.int("n"))
            .unwrap_or(0);
        let mut page = params.clone();
        page.push(SqlValue::Integer(limit));
        page.push(SqlValue::Integer(offset));
        let rows = fetch_all(tx, &format!("SELECT c.* FROM chat_log c {clause} ORDER BY c.id DESC LIMIT ? OFFSET ?"), rusqlite::params_from_iter(page))?;
        record(tx, ctx, "view.chat", Some("chat"), None, &json!({"filters": filters}))?;
        Ok((total, rows))
    })?;
    let banned = app.banned_word_list();
    let items: Vec<Value> = rows
        .into_iter()
        .map(|mut item| {
            let flagged = contains_banned(&banned, &item.text("message"));
            item.insert("flagged".into(), json!(flagged));
            Value::Object(item)
        })
        .collect();
    Ok(json!({"items": items, "total": total, "limit": limit, "offset": offset}))
}

// ===== Bejelentések (admin) =====

fn report_item(row: &Row, detail: bool) -> Value {
    let mut item = json!({
        "id": row.int("id"), "status": row.text("status"), "kind": row.text("kind"), "created_at": row.text("created_at"),
        "reporter_user_id": row.get("reporter_user_id"), "reporter_name": row.get("reporter_name"),
        "reported_user_id": row.get("reported_user_id"), "reported_name": row.get("reported_name"),
        "message": row.get("message"), "reason": row.get("reason"), "room_id": row.get("room_id"), "room_name": row.get("room_name"),
        "handled_by": row.get("handled_by"), "handled_at": row.get("handled_at"), "handler_note": row.get("handler_note"),
    });
    if detail {
        item["snapshot"] = row.opt_text("snapshot_json").and_then(|s| serde_json::from_str::<Value>(&s).ok()).unwrap_or(Value::Null);
    }
    item
}

pub fn list_reports(app: &App, args: &HashMap<String, String>) -> AdminResult<Value> {
    let mut where_: Vec<String> = Vec::new();
    let mut params: Vec<SqlValue> = Vec::new();
    if let Some(status) = args.get("status").map(|s| s.as_str()).filter(|s| !s.is_empty()) {
        if !REPORT_STATUSES.contains(&status) {
            return Err(AdminError::new("Érvénytelen szűrő.", 400));
        }
        where_.push("status = ?".into());
        params.push(SqlValue::Text(status.to_string()));
    }
    let (limit, offset) = page_args(args, 50, 200);
    let clause = if where_.is_empty() { String::new() } else { format!("WHERE {}", where_.join(" AND ")) };
    let (total, new_count, rows) = txn(&app.db, |tx| {
        let total =
            fetch_one(tx, &format!("SELECT COUNT(*) AS n FROM reports {clause}"), rusqlite::params_from_iter(params.clone()))?.map(|r| r.int("n")).unwrap_or(0);
        let new_count = fetch_one(tx, "SELECT COUNT(*) AS n FROM reports WHERE status = 'new'", [])?.map(|r| r.int("n")).unwrap_or(0);
        let mut page = params.clone();
        page.push(SqlValue::Integer(limit));
        page.push(SqlValue::Integer(offset));
        let rows = fetch_all(tx, &format!("SELECT * FROM reports {clause} ORDER BY id DESC LIMIT ? OFFSET ?"), rusqlite::params_from_iter(page))?;
        Ok((total, new_count, rows))
    })?;
    Ok(json!({"items": rows.iter().map(|r| report_item(r, false)).collect::<Vec<_>>(), "total": total, "new": new_count, "limit": limit, "offset": offset}))
}

/// Egy bejelentés a pillanatképpel (a megtekintés naplózva: `view.report`).
pub fn get_report(app: &App, ctx: &AdminContext, report_id: i64) -> AdminResult<Value> {
    let row = txn(&app.db, |tx| {
        let row = fetch_one(tx, "SELECT * FROM reports WHERE id = ?", [report_id])?.ok_or_else(|| AdminError::new("A bejelentés nem található.", 404))?;
        record(tx, ctx, "view.report", Some("report"), Some(&report_id.to_string()), &Value::Null)?;
        Ok(row)
    })?;
    Ok(report_item(&row, true))
}

/// A bejelentés kezelése: új / kezelt / elutasított állapot, indoklással.
pub fn update_report(app: &App, ctx: &AdminContext, report_id: i64, status: Option<&Value>, note: Option<&Value>) -> AdminResult<()> {
    let Some(status) = str_of(status).filter(|s| REPORT_STATUSES.contains(s)) else {
        return Err(AdminError::field("Érvénytelen állapot.", 400, "status"));
    };
    action(&app.db, ctx, "mod.report_update", Some("report"), Some(report_id.to_string()), note, json!({}), true, |act| {
        let row =
            fetch_one(act.tx, "SELECT status FROM reports WHERE id = ?", [report_id])?.ok_or_else(|| AdminError::new("A bejelentés nem található.", 404))?;
        let handled = status != "new";
        act.tx.execute(
            "UPDATE reports SET status = ?, handled_by = ?, handled_at = ?, handler_note = ? WHERE id = ?",
            rusqlite::params![
                status,
                if handled { Some(ctx.admin_user_id) } else { None },
                if handled { Some(util::now_ts()) } else { None },
                normalize_reason(note)?,
                report_id
            ],
        )?;
        act.details.insert("before".into(), json!({"status": row.text("status")}));
        act.details.insert("after".into(), json!({"status": status}));
        Ok(())
    })
}

// ===== Nevek átnézése =====

/// Legutóbb regisztrált és átnevezett nevek (tiltott szóra jelölve): az átnevezés a felhasználó oldaláról egy
/// kattintással elvégezhető.
pub fn recent_names(app: &App, args: &HashMap<String, String>) -> AdminResult<Value> {
    let limit = int_arg(args.get("limit").map(|s| s.as_str()), 50, 1, 200);
    let banned = app.banned_word_list();
    let mut items: Vec<Value> = Vec::new();
    txn(&app.db, |tx| {
        for r in fetch_all(tx, "SELECT id, display_name, created_at FROM users WHERE deleted_at IS NULL ORDER BY id DESC LIMIT ?", [limit])? {
            items.push(json!({"user_id": r.int("id"), "display_name": r.text("display_name"), "at": r.text("created_at"), "kind": "registered", "flagged": contains_banned(&banned, &r.text("display_name"))}));
        }
        for r in fetch_all(
            tx,
            "SELECT a.target_id, a.created_at, a.details_json FROM admin_audit a WHERE a.action = 'user.rename' ORDER BY a.id DESC LIMIT ?",
            [limit],
        )? {
            let after = r
                .opt_text("details_json")
                .and_then(|d| serde_json::from_str::<Value>(&d).ok())
                .and_then(|d| d["after"]["display_name"].as_str().map(|s| s.to_string()));
            let target = r.opt_text("target_id").unwrap_or_default();
            if let (Some(after), true) = (after, !target.is_empty() && target.chars().all(|c| c.is_ascii_digit())) {
                items.push(json!({"user_id": target.parse::<i64>().unwrap_or(0), "display_name": after, "at": r.text("created_at"), "kind": "renamed", "flagged": contains_banned(&banned, &after)}));
            }
        }
        Ok(())
    })?;
    items.sort_by(|a, b| b["at"].as_str().unwrap_or("").cmp(a["at"].as_str().unwrap_or("")));
    items.truncate(limit as usize);
    Ok(json!({"items": items}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn banned_words_ignore_case_and_accents() {
        let w = words(&["rossz", "csunya"]);
        assert!(contains_banned(&w, "Te ROSSZ vagy"));
        assert!(contains_banned(&w, "csúnya"));
        assert!(!contains_banned(&w, "szép"));
        assert!(!contains_banned(&[], "rossz"));
    }

    #[test]
    fn masking_preserves_the_rest() {
        let w = words(&["rossz", "csunya"]);
        assert_eq!(mask_banned(&w, "Te ROSSZ és csúnya"), ("Te ***** és ******".to_string(), true));
        assert_eq!(mask_banned(&w, "minden rendben"), ("minden rendben".to_string(), false));
        assert_eq!(mask_banned(&words(&["ab", "bc"]), "xabcx").0, "x***x", "az átfedő találatok összeolvadnak");
    }

    #[test]
    fn chat_filter_modes() {
        let w = words(&["rossz"]);
        assert_eq!(filter_chat(&w, false, "rossz"), (Some("*****".to_string()), true));
        assert_eq!(filter_chat(&w, true, "rossz"), (None, true));
        assert_eq!(filter_chat(&w, true, "jó"), (Some("jó".to_string()), false));
    }
}
