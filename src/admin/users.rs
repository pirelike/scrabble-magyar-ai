//! Admin panel: felhasználók — lista, részletek és a felhasználói műveletek.
//!
//! A modul a HTTP kéréstől és a futó szerver állapotától független (az online állapotot és a socket-műveleteket a
//! hívó adja / végzi a tranzakció UTÁN). Minden módosító művelet `action`-nel fut: kötelező indoklás, a napló a
//! művelettel egy tranzakcióban íródik. Védelmek: admin fiók nem tiltható ki, nem törölhető, az e-mail címe és a
//! jelszava nem módosítható a panelről (az adminság csak a szerver környezeti változójával változik).

use super::*;
use crate::achievements;
use crate::app::App;
use crate::db::{Row, RowExt, fetch_all, fetch_one, placeholders, until_active};
use crate::password::generate_password_hash;
use once_cell::sync::Lazy;
use rand::RngExt;
use regex::Regex;
use rusqlite::params_from_iter;
use rusqlite::types::Value as SqlValue;
use std::collections::{HashMap, HashSet};

pub static NAME_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[\w\sáéíóöőúüűÁÉÍÓÖŐÚÜŰ._-]{1,20}$").expect("érvényes regex"));
pub static EMAIL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$").expect("érvényes regex"));
pub const MAX_NOTE_LEN: usize = 1000;
pub const MIN_RATING: i64 = 100;
pub const MAX_RATING: i64 = 4000;
const LIST_SORTS: [(&str, &str); 7] = [
    ("id", "u.id"),
    ("name", "py_fold(u.display_name)"),
    ("email", "u.email_lower"),
    ("created_at", "u.created_at"),
    ("last_login_at", "COALESCE(u.last_login_at, '')"),
    ("games_played", "u.games_played"),
    ("rating", "u.rating"),
];
const STATUSES: [&str; 5] = ["active", "banned", "muted", "deleted", "review_blocked"];
const TEMP_PASSWORD_LENGTH: usize = 12;
const USER_AGENT_SHOW: usize = 120;
pub const USER_CSV_FIELDS: [&str; 10] =
    ["id", "display_name", "email", "created_at", "last_login_at", "games_played", "games_won", "rating", "status", "push_devices"];

fn deleted_name(id: i64) -> String {
    format!("Törölt felhasználó #{id}")
}

fn deleted_email(id: i64) -> String {
    format!("deleted-{id}@deleted.invalid")
}

// ===== Közös =====

fn is_admin_email(app: &App, email: &str) -> bool {
    app.is_admin_email(email)
}

fn row_status(row: &Row) -> &'static str {
    if row.opt_text("deleted_at").is_some() {
        return "deleted";
    }
    if crate::db::ban_from_parts(row.opt_text("banned_until").as_deref(), row.opt_text("ban_reason").as_deref()).is_some() {
        return "banned";
    }
    if until_active(row.opt_text("chat_muted_until").as_deref()) {
        return "muted";
    }
    "active"
}

fn user_summary(app: &App, row: &Row, online: &HashSet<i64>) -> Value {
    let ban = crate::db::ban_from_parts(row.opt_text("banned_until").as_deref(), row.opt_text("ban_reason").as_deref());
    let muted = until_active(row.opt_text("chat_muted_until").as_deref());
    json!({
        "id": row.int("id"), "display_name": row.text("display_name"), "email": row.text("email"),
        "created_at": row.text("created_at"), "last_login_at": row.get("last_login_at").cloned().unwrap_or(Value::Null),
        "online": online.contains(&row.int("id")), "games_played": row.int("games_played"), "games_won": row.int("games_won"),
        "rating": row.get("rating").cloned().unwrap_or(json!(crate::elo::INITIAL_RATING)), "rated_games": row.int("rated_games"),
        "status": row_status(row), "banned": ban.is_some(), "ban": ban.map(|b| b.to_json()), "muted": muted,
        "muted_until": if muted { row.get("chat_muted_until").cloned().unwrap_or(Value::Null) } else { Value::Null },
        "review_blocked": row.flag("review_blocked"), "deleted": row.opt_text("deleted_at").is_some(),
        "is_admin": is_admin_email(app, &row.text("email_lower")),
        "push_devices": row.get("push_devices").cloned().unwrap_or(Value::Null),
    })
}

/// A felhasználó sora, vagy 404.
pub fn get_row(tx: &Transaction, user_id: i64) -> AdminResult<Row> {
    fetch_one(tx, "SELECT * FROM users WHERE id = ?", [user_id])?.ok_or_else(|| AdminError::new("Felhasználó nem található.", 404))
}

/// Admin és törölt fiókon nem végezhető romboló / személyes művelet.
fn require_manageable(app: &App, row: &Row, deleted_ok: bool) -> AdminResult<()> {
    if is_admin_email(app, &row.text("email_lower")) {
        return Err(AdminError::new("Admin fiókon ez a művelet nem végezhető.", 403));
    }
    if row.opt_text("deleted_at").is_some() && !deleted_ok {
        return Err(AdminError::new("A fiók törölve van.", 409));
    }
    Ok(())
}

pub fn sanitize_name(value: Option<&Value>) -> Option<String> {
    let name = value?.as_str()?.trim();
    (!name.is_empty() && name.chars().count() <= 20 && NAME_RE.is_match(name)).then(|| name.to_string())
}

// ===== Lista =====

fn arg<'a>(args: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    args.get(key).map(|s| s.as_str()).filter(|s| !s.is_empty())
}

/// Szűrt, rendezett, lapozott felhasználólista. `args`: q, status, online, admin, from, to, min_games,
/// inactive_days, sort, order, limit, offset.
pub fn list_users(app: &App, args: &HashMap<String, String>, online: &HashSet<i64>, paging: Option<(i64, i64)>) -> AdminResult<Value> {
    let mut where_: Vec<String> = Vec::new();
    let mut params: Vec<SqlValue> = Vec::new();
    let q = args.get("q").map(|q| q.trim()).unwrap_or("");
    if !q.is_empty() {
        let mut clauses = vec!["py_fold(u.display_name) LIKE ? ESCAPE '\\'".to_string(), "u.email_lower LIKE ? ESCAPE '\\'".to_string()];
        params.push(SqlValue::Text(like_contains(q)));
        params.push(SqlValue::Text(like(&q.to_lowercase())));
        if q.chars().all(|c| c.is_ascii_digit()) {
            clauses.push("u.id = ?".into());
            params.push(SqlValue::Integer(q.parse().unwrap_or(0)));
        }
        where_.push(format!("({})", clauses.join(" OR ")));
    }
    if let Some(status) = arg(args, "status") {
        if !STATUSES.contains(&status) {
            return Err(AdminError::new("Érvénytelen szűrő.", 400));
        }
        let now = util::now_ts();
        let (clause, count) = match status {
            "active" => ("u.deleted_at IS NULL AND COALESCE(u.banned_until, '') <= ? AND COALESCE(u.chat_muted_until, '') <= ?", 2),
            "banned" => ("u.deleted_at IS NULL AND COALESCE(u.banned_until, '') > ?", 1),
            "muted" => ("u.deleted_at IS NULL AND COALESCE(u.chat_muted_until, '') > ?", 1),
            "deleted" => ("u.deleted_at IS NOT NULL", 0),
            _ => ("u.review_blocked = 1", 0),
        };
        where_.push(clause.to_string());
        for _ in 0..count {
            params.push(SqlValue::Text(now.clone()));
        }
    }
    if matches!(arg(args, "online"), Some("1") | Some("true")) {
        let mut ids: Vec<i64> = online.iter().copied().collect();
        ids.sort();
        where_.push(if ids.is_empty() { "0".to_string() } else { format!("u.id IN ({})", placeholders(ids.len())) });
        params.extend(ids.into_iter().map(SqlValue::Integer));
    }
    if matches!(arg(args, "admin"), Some("1") | Some("true")) {
        let mut emails: Vec<String> = app.config.admin_emails.iter().cloned().collect();
        emails.sort();
        where_.push(if emails.is_empty() { "0".to_string() } else { format!("u.email_lower IN ({})", placeholders(emails.len())) });
        params.extend(emails.into_iter().map(SqlValue::Text));
    }
    if let Some(from) = arg(args, "from") {
        let (stamp, _) = parse_bound(from, false)?;
        where_.push("u.created_at >= ?".into());
        params.push(SqlValue::Text(stamp));
    }
    if let Some(to) = arg(args, "to") {
        let (stamp, exclusive) = parse_bound(to, true)?;
        where_.push(if exclusive { "u.created_at < ?" } else { "u.created_at <= ?" }.into());
        params.push(SqlValue::Text(stamp));
    }
    if let Some(min_games) = arg(args, "min_games") {
        where_.push("u.games_played >= ?".into());
        params.push(SqlValue::Integer(int_arg(Some(min_games), 0, 0, 1_000_000)));
    }
    if let Some(days) = arg(args, "inactive_days") {
        let days = int_arg(Some(days), 0, 1, 3650);
        where_.push("COALESCE(u.last_login_at, u.created_at) < ?".into());
        params.push(SqlValue::Text(util::format_ts(util::utcnow() - chrono::Duration::days(days))));
    }
    let (limit, offset) = paging.unwrap_or_else(|| page_args(args, 50, 200));
    let clause = if where_.is_empty() { String::new() } else { format!("WHERE {}", where_.join(" AND ")) };
    let sort = args.get("sort").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let default_order = if sort.is_none() { "desc" } else { "asc" };
    let order = order_by(sort, Some(args.get("order").map(|o| o.as_str()).unwrap_or(default_order)), &LIST_SORTS, "id");
    let (total, rows) = txn(&app.db, |tx| {
        let total = fetch_one(tx, &format!("SELECT COUNT(*) AS n FROM users u {clause}"), params_from_iter(params.clone()))?.map(|r| r.int("n")).unwrap_or(0);
        let mut page = params.clone();
        page.push(SqlValue::Integer(limit));
        page.push(SqlValue::Integer(offset));
        let rows = fetch_all(
            tx,
            &format!(
                "SELECT u.*, (SELECT COUNT(*) FROM push_subscriptions p WHERE p.user_id = u.id) AS push_devices \
                 FROM users u {clause} ORDER BY {order}, u.id DESC LIMIT ? OFFSET ?"
            ),
            params_from_iter(page),
        )?;
        Ok((total, rows))
    })?;
    Ok(json!({"items": rows.iter().map(|r| user_summary(app, r, online)).collect::<Vec<_>>(), "total": total, "limit": limit, "offset": offset}))
}

/// Rövid találatok a globális keresőhöz (név / e-mail / id).
pub fn search_brief(app: &App, q: &str, limit: i64) -> AdminResult<Vec<Value>> {
    let q = q.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let mut clauses = vec!["py_fold(display_name) LIKE ? ESCAPE '\\'".to_string(), "email_lower LIKE ? ESCAPE '\\'".to_string()];
    let mut params = vec![SqlValue::Text(like_contains(q)), SqlValue::Text(like(&q.to_lowercase()))];
    if q.chars().all(|c| c.is_ascii_digit()) {
        clauses.push("id = ?".into());
        params.push(SqlValue::Integer(q.parse().unwrap_or(0)));
    }
    params.push(SqlValue::Integer(limit));
    let rows = txn(&app.db, |tx| {
        Ok(fetch_all(
            tx,
            &format!("SELECT id, display_name, email, deleted_at FROM users WHERE {} ORDER BY id DESC LIMIT ?", clauses.join(" OR ")),
            params_from_iter(params),
        )?)
    })?;
    Ok(rows
        .iter()
        .map(|r| json!({"id": r.int("id"), "display_name": r.text("display_name"), "email": r.text("email"), "deleted": r.opt_text("deleted_at").is_some()}))
        .collect())
}

// ===== Részletek =====

fn host_of(endpoint: &str) -> String {
    let rest = endpoint.split_once("://").map(|(_, r)| r).unwrap_or("");
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let netloc = &rest[..end];
    if netloc.is_empty() { "?".to_string() } else { netloc.to_string() }
}

fn rating_history(tx: &Transaction, user_id: i64) -> AdminResult<Vec<Value>> {
    let mut history: Vec<Value> = Vec::new();
    for r in fetch_all(
        tx,
        "SELECT sg.updated_at AS at, gp.rating_before AS before, gp.rating_after AS after, gp.game_id AS game_id \
         FROM game_players gp JOIN saved_games sg ON sg.id = gp.game_id \
         WHERE gp.user_id = ? AND gp.rating_after IS NOT NULL ORDER BY sg.updated_at, gp.id",
        [user_id],
    )? {
        history.push(json!({"at": r["at"], "before": r["before"], "after": r["after"], "kind": "game", "game_id": r["game_id"]}));
    }
    for r in fetch_all(tx, "SELECT created_at, rating_before, rating_after, reason FROM rating_adjustments WHERE user_id = ?", [user_id])? {
        history.push(json!({"at": r["created_at"], "before": r["rating_before"], "after": r["rating_after"], "kind": "adjustment", "reason": r["reason"]}));
    }
    history.sort_by_key(|item| item["at"].as_str().unwrap_or("").to_string());
    Ok(history)
}

fn rows_json(rows: Vec<Row>) -> Value {
    Value::Array(rows.into_iter().map(Value::Object).collect())
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// A felhasználó teljes képe (személyes adat: a megtekintés naplózódik `view.user` néven).
pub fn user_detail(app: &App, ctx: &AdminContext, user_id: i64, online: bool) -> AdminResult<Value> {
    let history = audit::query_audit(
        &app.db,
        &audit::AuditFilters { target_type: Some("user".into()), target_id: Some(user_id.to_string()), ..Default::default() },
        Some("30"),
        None,
        AUDIT_MAX_LIMIT,
    )?["items"]
        .clone();
    let mut data = txn(&app.db, |tx| {
        let row = get_row(tx, user_id)?;
        let online_set: HashSet<i64> = if online { HashSet::from([user_id]) } else { HashSet::new() };
        let mut account = user_summary(app, &row, &online_set);
        let played = row.int("games_played");
        account["last_login_ip"] = row.get("last_login_ip").cloned().unwrap_or(Value::Null);
        account["deleted_at"] = row.get("deleted_at").cloned().unwrap_or(Value::Null);
        account["total_score"] = json!(row.int("total_score"));
        account["ban_reason"] = row.get("ban_reason").cloned().unwrap_or(Value::Null);
        account["win_rate"] = json!(if played > 0 { round1(row.int("games_won") as f64 / played as f64 * 100.0) } else { 0.0 });
        account["avg_score"] = json!(if played > 0 { round1(row.int("total_score") as f64 / played as f64) } else { 0.0 });

        let sessions: Vec<Value> = fetch_all(tx, "SELECT * FROM sessions WHERE user_id = ? ORDER BY id DESC", [user_id])?
            .iter()
            .map(|r| {
                json!({
                    "id": r["id"], "created_at": r["created_at"], "expires_at": r["expires_at"], "ip": r["ip"],
                    "user_agent": r.text("user_agent").chars().take(USER_AGENT_SHOW).collect::<String>(), "last_seen": r["last_seen"],
                })
            })
            .collect();
        let push: Vec<Value> = fetch_all(tx, "SELECT id, lang, created_at, endpoint FROM push_subscriptions WHERE user_id = ? ORDER BY id", [user_id])?
            .iter()
            .map(|r| json!({"id": r["id"], "lang": r["lang"], "created_at": r["created_at"], "host": host_of(&r.text("endpoint"))}))
            .collect();
        let pending_in = fetch_all(
            tx,
            "SELECT u.id, u.display_name, f.created_at FROM friendships f JOIN users u ON u.id = f.user_id WHERE f.friend_id = ? AND f.status = 'pending'",
            [user_id],
        )?;
        let pending_out = fetch_all(
            tx,
            "SELECT u.id, u.display_name, f.created_at FROM friendships f JOIN users u ON u.id = f.friend_id WHERE f.user_id = ? AND f.status = 'pending'",
            [user_id],
        )?;
        let friends = fetch_all(
            tx,
            "SELECT u.id, u.display_name FROM friendships f JOIN users u ON (u.id = f.user_id OR u.id = f.friend_id) AND u.id != ? \
             WHERE (f.user_id = ? OR f.friend_id = ?) AND f.status = 'accepted' ORDER BY u.display_name",
            [user_id, user_id, user_id],
        )?;
        let badges = fetch_all(tx, "SELECT badge, game_id, earned_at FROM achievements WHERE user_id = ? ORDER BY earned_at, rowid", [user_id])?;
        let daily = fetch_all(
            tx,
            "SELECT puzzle_date, best_score, attempts, revealed FROM daily_scores WHERE user_id = ? ORDER BY puzzle_date DESC LIMIT 60",
            [user_id],
        )?;
        let review = fetch_one(tx, "SELECT COUNT(*) AS total, COALESCE(SUM(1 - verdict), 0) AS invalid FROM word_reviews WHERE user_id = ?", [user_id])?.unwrap_or_default();
        let (total, invalid) = (review.int("total"), review.int("invalid"));
        let recent: Vec<Value> = fetch_all(tx, "SELECT word, verdict, created_at FROM word_reviews WHERE user_id = ? ORDER BY created_at DESC, rowid DESC LIMIT 50", [user_id])?
            .iter()
            .map(|r| json!({"word": r["word"], "valid": r.flag("verdict"), "at": r["created_at"]}))
            .collect();
        let reviews = json!({"total": total, "invalid": invalid, "invalid_share": if total > 0 { round1(invalid as f64 / total as f64 * 100.0) } else { 0.0 }, "recent": recent});
        let games = fetch_all(
            tx,
            "SELECT sg.id AS id, sg.room_name AS room_name, sg.status AS status, sg.is_async AS is_async, sg.has_bots AS has_bots, \
             sg.created_at AS created_at, sg.updated_at AS updated_at, gp.final_score AS score, gp.is_winner AS is_winner, gp.player_name AS player_name \
             FROM game_players gp JOIN saved_games sg ON sg.id = gp.game_id WHERE gp.user_id = ? ORDER BY sg.id DESC LIMIT 200",
            [user_id],
        )?;
        let notes = fetch_all(
            tx,
            "SELECT n.id, n.note, n.created_at, n.admin_user_id, u.display_name AS admin_name FROM user_admin_notes n \
             LEFT JOIN users u ON u.id = n.admin_user_id WHERE n.user_id = ? ORDER BY n.id DESC",
            [user_id],
        )?;
        let logins: Vec<Value> = fetch_all(
            tx,
            "SELECT id, ip, user_agent, success, reason, created_at FROM login_events WHERE user_id = ? ORDER BY id DESC LIMIT 20",
            [user_id],
        )?
        .into_iter()
        .map(|mut r| {
            let agent: String = r.text("user_agent").chars().take(USER_AGENT_SHOW).collect();
            r.insert("user_agent".into(), json!(agent));
            Value::Object(r)
        })
        .collect();
        let rating_history = rating_history(tx, user_id)?;
        record(tx, ctx, "view.user", Some("user"), Some(&user_id.to_string()), &Value::Null)?;
        Ok(json!({
            "user": account, "sessions": sessions, "push": push, "friends": rows_json(friends), "pending_in": rows_json(pending_in),
            "pending_out": rows_json(pending_out), "badges": rows_json(badges), "daily": rows_json(daily), "reviews": reviews,
            "games": rows_json(games), "notes": rows_json(notes), "logins": logins, "rating_history": rating_history,
        }))
    })?;
    data["admin_history"] = history;
    data["available_badges"] = json!(achievements::BADGES);
    Ok(data)
}

/// A profil oldal tartalma, ahogy a felhasználó látja (csak olvasható; naplózva).
pub fn profile_view(app: &App, ctx: &AdminContext, user_id: i64) -> AdminResult<Value> {
    let row = txn(&app.db, |tx| {
        let row = get_row(tx, user_id)?;
        record(tx, ctx, "view.user_profile", Some("user"), Some(&user_id.to_string()), &Value::Null)?;
        Ok(row)
    })?;
    let history = app.db.get_user_game_history(user_id, 20)?;
    let (played, won) = (row.int("games_played"), row.int("games_won"));
    Ok(json!({
        "display_name": row.text("display_name"), "badges": app.db.get_user_achievements(user_id),
        "stats": {
            "games_played": played, "games_won": won,
            "win_rate": if played > 0 { round1(won as f64 / played as f64 * 100.0) } else { 0.0 },
            "avg_score": if played > 0 { round1(row.int("total_score") as f64 / played as f64) } else { 0.0 },
            "total_score": row.int("total_score"), "rating": row.get("rating").cloned().unwrap_or(Value::Null), "rated_games": row.int("rated_games"),
        },
        "history": history.iter().map(|h| {
            let change = match (h["rating_before"].as_i64(), h["rating_after"].as_i64()) {
                (Some(b), Some(a)) => json!(a - b),
                _ => Value::Null,
            };
            json!({
                "game_id": h["game_id"], "room_name": h["room_name"], "created_at": h["created_at"], "final_score": h["final_score"],
                "is_winner": util::truthy(&h["is_winner"]), "rating_change": change, "opponents": h["opponents"],
            })
        }).collect::<Vec<_>>(),
    }))
}

// ===== Átnevezés =====

/// A mentett állapotban a játékos nevének cseréje; None, ha nem volt mit cserélni.
fn patch_state_names(state_json: &str, old: &str, new: &str) -> Option<String> {
    let mut data: Value = serde_json::from_str(state_json).ok()?;
    let mut changed = false;
    if let Some(players) = data.get_mut("players").and_then(|p| p.as_array_mut()) {
        for player in players {
            if player.get("name").and_then(|n| n.as_str()) == Some(old) {
                player["name"] = json!(new);
                changed = true;
            }
        }
    }
    if let Some(names) = data.get_mut("winner_names").and_then(|n| n.as_array_mut()) {
        if names.iter().any(|n| n.as_str() == Some(old)) {
            for name in names.iter_mut() {
                if name.as_str() == Some(old) {
                    *name = json!(new);
                }
            }
            changed = true;
        }
    }
    changed.then(|| data.to_string())
}

/// A felhasználó nevének átírása a játékaiban (`game_players`, lépésnapló, tulajdonos, mentett állapot).
/// `only_active`: csak a folyamatban lévő játékokban (átnevezés); különben mindegyikben (anonimizálás).
pub fn rename_in_games(tx: &Transaction, user_id: i64, old: &str, new: &str, only_active: bool) -> AdminResult<Vec<i64>> {
    let status_clause = if only_active { "AND sg.status = 'active'" } else { "" };
    let games: Vec<i64> = fetch_all(
        tx,
        &format!("SELECT DISTINCT gp.game_id AS game_id FROM game_players gp JOIN saved_games sg ON sg.id = gp.game_id WHERE gp.user_id = ? {status_clause}"),
        [user_id],
    )?
    .iter()
    .map(|r| r.int("game_id"))
    .collect();
    for game_id in &games {
        tx.execute("UPDATE game_players SET player_name = ? WHERE game_id = ? AND user_id = ?", rusqlite::params![new, game_id, user_id])?;
        tx.execute("UPDATE game_moves SET player_name = ? WHERE game_id = ? AND player_name = ?", rusqlite::params![new, game_id, old])?;
        tx.execute("UPDATE saved_games SET owner_name = ? WHERE id = ? AND owner_name = ?", rusqlite::params![new, game_id, old])?;
        let state_json: String = tx.query_row("SELECT state_json FROM saved_games WHERE id = ?", [game_id], |r| r.get(0))?;
        if let Some(patched) = patch_state_names(&state_json, old, new) {
            tx.execute("UPDATE saved_games SET state_json = ? WHERE id = ?", rusqlite::params![patched, game_id])?;
        }
    }
    Ok(games)
}

/// Megjelenítési név módosítása a regisztrációval azonos szabályokkal. Visszatér: (régi, új, érintett játékok).
pub fn rename(app: &App, ctx: &AdminContext, user_id: i64, new_name: Option<&Value>, reason: Option<&Value>) -> AdminResult<(String, String, Vec<i64>)> {
    let Some(name) = sanitize_name(new_name) else {
        return Err(AdminError::field("Érvénytelen megjelenítési név (1-20 karakter, betűk és számok).", 400, "display_name"));
    };
    action(&app.db, ctx, "user.rename", Some("user"), Some(user_id.to_string()), reason, json!({}), true, |act| {
        let row = get_row(act.tx, user_id)?;
        require_manageable(app, &row, false)?;
        let old = row.text("display_name");
        if old == name {
            return Err(AdminError::field("A név nem változott.", 409, "display_name"));
        }
        let games = rename_in_games(act.tx, user_id, &old, &name, true)?;
        act.tx.execute("UPDATE users SET display_name = ? WHERE id = ?", rusqlite::params![name, user_id])?;
        act.details.insert("before".into(), json!({"display_name": old}));
        act.details.insert("after".into(), json!({"display_name": name}));
        act.details.insert("games".into(), json!(games));
        Ok((old, name.clone(), games))
    })
}

// ===== E-mail, jelszó =====

/// E-mail cím módosítása (egyediség-ellenőrzéssel). Visszatér: a régi cím (az értesítéshez).
pub fn change_email(app: &App, ctx: &AdminContext, user_id: i64, new_email: Option<&Value>, reason: Option<&Value>) -> AdminResult<String> {
    let email = str_of(new_email).map(|e| e.trim().to_string()).unwrap_or_default();
    if !EMAIL_RE.is_match(&email) || email.chars().count() > 254 {
        return Err(AdminError::field("Érvénytelen email cím.", 400, "email"));
    }
    let lowered = email.to_lowercase();
    if app.config.admin_emails.contains(&lowered) {
        return Err(AdminError::field("Ez a cím védett, más fiók nem veheti fel.", 403, "email"));
    }
    action(&app.db, ctx, "user.email", Some("user"), Some(user_id.to_string()), reason, json!({}), true, |act| {
        let row = get_row(act.tx, user_id)?;
        require_manageable(app, &row, false)?;
        if lowered == row.text("email_lower") {
            return Err(AdminError::field("Az e-mail cím nem változott.", 409, "email"));
        }
        if fetch_one(act.tx, "SELECT 1 AS x FROM users WHERE email_lower = ?", [&lowered])?.is_some() {
            return Err(AdminError::field("Ez az email cím már regisztrálva van.", 409, "email"));
        }
        act.tx.execute("UPDATE users SET email = ?, email_lower = ? WHERE id = ?", rusqlite::params![email, lowered, user_id])?;
        act.details.insert("before".into(), json!({"email": row.text("email")}));
        act.details.insert("after".into(), json!({"email": email}));
        Ok(row.text("email"))
    })
}

pub fn temp_password() -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::rng();
    (0..TEMP_PASSWORD_LENGTH).map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char).collect()
}

/// Ideiglenes jelszó beállítása, minden munkamenet érvénytelenítése. Visszatér: (jelszó, e-mail cím, sessionök). A
/// jelszó a naplóba nem kerül.
pub fn reset_password(app: &App, ctx: &AdminContext, user_id: i64, reason: Option<&Value>) -> AdminResult<(String, String, usize)> {
    let password = temp_password();
    let hash = generate_password_hash(&password);
    let (email, killed) = action(&app.db, ctx, "user.reset_password", Some("user"), Some(user_id.to_string()), reason, json!({}), true, |act| {
        let row = get_row(act.tx, user_id)?;
        require_manageable(app, &row, false)?;
        act.tx.execute("UPDATE users SET password_hash = ? WHERE id = ?", rusqlite::params![hash, user_id])?;
        let killed = act.tx.execute("DELETE FROM sessions WHERE user_id = ?", [user_id])?;
        act.details.insert("sessions_closed".into(), json!(killed));
        Ok((row.text("email"), killed))
    })?;
    Ok((password, email, killed))
}

/// Kijelentkeztetés mindenhonnan: a munkamenetek törlése. A socketek bontása a hívó dolga (a tranzakció után).
pub fn logout_all(app: &App, ctx: &AdminContext, user_id: i64, reason: Option<&Value>) -> AdminResult<usize> {
    action(&app.db, ctx, "user.logout_all", Some("user"), Some(user_id.to_string()), reason, json!({}), true, |act| {
        let row = get_row(act.tx, user_id)?;
        require_manageable(app, &row, false)?;
        let killed = act.tx.execute("DELETE FROM sessions WHERE user_id = ?", [user_id])?;
        act.details.insert("sessions_closed".into(), json!(killed));
        Ok(killed)
    })
}

// ===== Kitiltás, némítás, tiltások =====

/// Kitiltás időtartammal (1h / 1d / 7d / 30d / pontos időpont / None = végleges). Visszatér: a lejárat.
pub fn ban(app: &App, ctx: &AdminContext, user_id: i64, until: Option<&Value>, reason: Option<&Value>) -> AdminResult<String> {
    let stamp = parse_until(until, true)?;
    action(&app.db, ctx, "user.ban", Some("user"), Some(user_id.to_string()), reason, json!({}), true, |act| {
        let row = get_row(act.tx, user_id)?;
        require_manageable(app, &row, false)?;
        act.tx.execute(
            "UPDATE users SET banned_until = ?, ban_reason = ? WHERE id = ?",
            rusqlite::params![stamp, normalize_reason(reason)?, user_id],
        )?;
        let killed = act.tx.execute("DELETE FROM sessions WHERE user_id = ?", [user_id])?;
        act.details.insert("before".into(), json!({"banned_until": row.get("banned_until"), "ban_reason": row.get("ban_reason")}));
        act.details.insert("after".into(), json!({"banned_until": stamp, "permanent": stamp == crate::db::PERMANENT_UNTIL}));
        act.details.insert("sessions_closed".into(), json!(killed));
        Ok(stamp.clone())
    })
}

pub fn unban(app: &App, ctx: &AdminContext, user_id: i64, reason: Option<&Value>) -> AdminResult<()> {
    action(&app.db, ctx, "user.unban", Some("user"), Some(user_id.to_string()), reason, json!({}), true, |act| {
        let row = get_row(act.tx, user_id)?;
        if row.opt_text("banned_until").is_none() {
            return Err(AdminError::new("A felhasználó nincs kitiltva.", 409));
        }
        act.tx.execute("UPDATE users SET banned_until = NULL, ban_reason = NULL WHERE id = ?", [user_id])?;
        act.details.insert("before".into(), json!({"banned_until": row.get("banned_until"), "ban_reason": row.get("ban_reason")}));
        act.details.insert("after".into(), json!({"banned_until": null}));
        Ok(())
    })
}

/// Chat némítás időtartammal. Visszatér: a lejárat.
pub fn mute(app: &App, ctx: &AdminContext, user_id: i64, until: Option<&Value>, reason: Option<&Value>) -> AdminResult<String> {
    let stamp = parse_until(until, true)?;
    action(&app.db, ctx, "user.mute", Some("user"), Some(user_id.to_string()), reason, json!({}), true, |act| {
        let row = get_row(act.tx, user_id)?;
        require_manageable(app, &row, false)?;
        act.tx.execute("UPDATE users SET chat_muted_until = ? WHERE id = ?", rusqlite::params![stamp, user_id])?;
        act.details.insert("before".into(), json!({"chat_muted_until": row.get("chat_muted_until")}));
        act.details.insert("after".into(), json!({"chat_muted_until": stamp}));
        Ok(stamp.clone())
    })
}

pub fn unmute(app: &App, ctx: &AdminContext, user_id: i64, reason: Option<&Value>) -> AdminResult<()> {
    action(&app.db, ctx, "user.unmute", Some("user"), Some(user_id.to_string()), reason, json!({}), true, |act| {
        let row = get_row(act.tx, user_id)?;
        if row.opt_text("chat_muted_until").is_none() {
            return Err(AdminError::new("A felhasználó nincs némítva.", 409));
        }
        act.tx.execute("UPDATE users SET chat_muted_until = NULL WHERE id = ?", [user_id])?;
        act.details.insert("before".into(), json!({"chat_muted_until": row.get("chat_muted_until")}));
        act.details.insert("after".into(), json!({"chat_muted_until": null}));
        Ok(())
    })
}

/// Szótár-építő tiltása / engedélyezése. A letiltott szavazatai nem számítanak (a kizárásokat újra kell számolni).
pub fn set_review_blocked(app: &App, ctx: &AdminContext, user_id: i64, blocked: Option<&Value>, reason: Option<&Value>) -> AdminResult<()> {
    let blocked = bool_field(blocked, "blocked")?;
    let name = if blocked { "user.review_block" } else { "user.review_unblock" };
    action(&app.db, ctx, name, Some("user"), Some(user_id.to_string()), reason, json!({}), true, |act| {
        let row = get_row(act.tx, user_id)?;
        require_manageable(app, &row, false)?;
        act.tx.execute("UPDATE users SET review_blocked = ? WHERE id = ?", rusqlite::params![blocked as i64, user_id])?;
        act.details.insert("before".into(), json!({"review_blocked": row.flag("review_blocked")}));
        act.details.insert("after".into(), json!({"review_blocked": blocked}));
        Ok(())
    })
}

/// A felhasználó szótár-építős szavazatainak törlése (mind, vagy egy időponttól). Visszatér: a törölt szavak.
pub fn revert_reviews(app: &App, ctx: &AdminContext, user_id: i64, since: Option<&Value>, reason: Option<&Value>) -> AdminResult<Vec<String>> {
    let stamp = match str_of(since).filter(|s| !s.is_empty()) {
        Some(s) => Some(parse_bound(s, false)?.0),
        None => None,
    };
    action(&app.db, ctx, "user.reviews_revert", Some("user"), Some(user_id.to_string()), reason, json!({}), true, |act| {
        let row = get_row(act.tx, user_id)?;
        require_manageable(app, &row, false)?;
        let mut sql = "FROM word_reviews WHERE user_id = ?".to_string();
        let mut params: Vec<SqlValue> = vec![SqlValue::Integer(user_id)];
        if let Some(stamp) = &stamp {
            sql.push_str(" AND created_at >= ?");
            params.push(SqlValue::Text(stamp.clone()));
        }
        let words: Vec<String> = fetch_all(act.tx, &format!("SELECT word {sql}"), params_from_iter(params.clone()))?.iter().map(|r| r.text("word")).collect();
        act.tx.execute(&format!("DELETE {sql}"), params_from_iter(params))?;
        act.details.insert("count".into(), json!(words.len()));
        act.details.insert("since".into(), json!(stamp));
        act.details.insert("words".into(), json!(words.iter().take(200).collect::<Vec<_>>()));
        Ok(words)
    })
}

// ===== Értékszám, statisztika, kitüntetés =====

/// Kézi értékszám-módosítás: külön sor az értékszám-előzményben (`rating_adjustments`).
pub fn set_rating(app: &App, ctx: &AdminContext, user_id: i64, rating: Option<&Value>, reason: Option<&Value>) -> AdminResult<()> {
    let rating = int_field(rating, "rating", MIN_RATING, MAX_RATING)?;
    action(&app.db, ctx, "user.rating", Some("user"), Some(user_id.to_string()), reason, json!({}), true, |act| {
        let row = get_row(act.tx, user_id)?;
        require_manageable(app, &row, false)?;
        let current = row.opt_int("rating").unwrap_or(crate::elo::INITIAL_RATING);
        if current == rating {
            return Err(AdminError::field("Az értékszám nem változott.", 409, "rating"));
        }
        act.tx.execute("UPDATE users SET rating = ? WHERE id = ?", rusqlite::params![rating, user_id])?;
        act.tx.execute(
            "INSERT INTO rating_adjustments (user_id, admin_user_id, rating_before, rating_after, reason) VALUES (?, ?, ?, ?, ?)",
            rusqlite::params![user_id, ctx.admin_user_id, current, rating, normalize_reason(reason)?],
        )?;
        act.details.insert("before".into(), json!({"rating": current}));
        act.details.insert("after".into(), json!({"rating": rating}));
        Ok(())
    })
}

/// A számlálók a `game_players`-ből (befejezett játékok): (játszott, nyert, összpont).
pub fn compute_counters(tx: &Transaction, user_id: i64) -> AdminResult<(i64, i64, i64)> {
    let row = fetch_one(
        tx,
        "SELECT COUNT(*) AS played, COALESCE(SUM(gp.is_winner), 0) AS won, COALESCE(SUM(gp.final_score), 0) AS score \
         FROM game_players gp JOIN saved_games sg ON sg.id = gp.game_id AND sg.status = 'finished' WHERE gp.user_id = ?",
        [user_id],
    )?
    .unwrap_or_default();
    Ok((row.int("played"), row.int("won"), row.int("score")))
}

/// Egy felhasználó számlálóinak újraszámolása. Visszatér: ((előtte), (utána)).
pub fn recompute_user_counters(tx: &Transaction, user_id: i64) -> AdminResult<((i64, i64, i64), (i64, i64, i64))> {
    let row = fetch_one(tx, "SELECT games_played, games_won, total_score FROM users WHERE id = ?", [user_id])?.unwrap_or_default();
    let (played, won, score) = compute_counters(tx, user_id)?;
    tx.execute("UPDATE users SET games_played = ?, games_won = ?, total_score = ? WHERE id = ?", rusqlite::params![played, won, score, user_id])?;
    Ok(((row.int("games_played"), row.int("games_won"), row.int("total_score")), (played, won, score)))
}

pub fn recompute_stats(app: &App, ctx: &AdminContext, user_id: i64, reason: Option<&Value>) -> AdminResult<()> {
    action(&app.db, ctx, "user.recompute_stats", Some("user"), Some(user_id.to_string()), reason, json!({}), true, |act| {
        let row = get_row(act.tx, user_id)?;
        require_manageable(app, &row, false)?;
        let (before, after) = recompute_user_counters(act.tx, user_id)?;
        let named = |v: (i64, i64, i64)| json!({"games_played": v.0, "games_won": v.1, "total_score": v.2});
        act.details.insert("before".into(), named(before));
        act.details.insert("after".into(), named(after));
        Ok(())
    })
}

/// Kitüntetés adása / elvétele a `BADGES` listából.
pub fn set_badge(app: &App, ctx: &AdminContext, user_id: i64, badge: Option<&Value>, grant: Option<&Value>, reason: Option<&Value>) -> AdminResult<()> {
    let grant = bool_field(grant, "grant")?;
    let Some(badge) = str_of(badge).filter(|b| achievements::BADGES.contains(b)) else {
        return Err(AdminError::field("Ismeretlen kitüntetés.", 400, "badge"));
    };
    let name = if grant { "user.badge_grant" } else { "user.badge_revoke" };
    action(&app.db, ctx, name, Some("user"), Some(user_id.to_string()), reason, json!({}), true, |act| {
        let row = get_row(act.tx, user_id)?;
        require_manageable(app, &row, false)?;
        let have = fetch_one(act.tx, "SELECT 1 AS x FROM achievements WHERE user_id = ? AND badge = ?", rusqlite::params![user_id, badge])?.is_some();
        if grant && have {
            return Err(AdminError::field("A felhasználónak már megvan ez a kitüntetés.", 409, "badge"));
        }
        if !grant && !have {
            return Err(AdminError::field("A felhasználónak nincs ilyen kitüntetése.", 409, "badge"));
        }
        if grant {
            act.tx.execute("INSERT INTO achievements (user_id, badge) VALUES (?, ?)", rusqlite::params![user_id, badge])?;
        } else {
            act.tx.execute("DELETE FROM achievements WHERE user_id = ? AND badge = ?", rusqlite::params![user_id, badge])?;
        }
        act.details.insert("badge".into(), json!(badge));
        Ok(())
    })
}

// ===== Push, jegyzet =====

pub fn delete_push_device(app: &App, ctx: &AdminContext, user_id: i64, device_id: i64, reason: Option<&Value>) -> AdminResult<()> {
    action(&app.db, ctx, "user.push_delete", Some("user"), Some(user_id.to_string()), reason, json!({}), true, |act| {
        get_row(act.tx, user_id)?;
        let removed = act.tx.execute("DELETE FROM push_subscriptions WHERE id = ? AND user_id = ?", rusqlite::params![device_id, user_id])?;
        if removed == 0 {
            return Err(AdminError::new("Az eszköz nem található.", 404));
        }
        act.details.insert("device_id".into(), json!(device_id));
        Ok(())
    })
}

/// Belső jegyzet a felhasználóhoz (a jegyzet maga az indoklás: külön indoklás nem kell).
pub fn add_note(app: &App, ctx: &AdminContext, user_id: i64, note: Option<&Value>) -> AdminResult<i64> {
    let text = clean_text(note, "note", MAX_NOTE_LEN, true)?;
    txn(&app.db, |tx| {
        get_row(tx, user_id)?;
        tx.execute("INSERT INTO user_admin_notes (user_id, admin_user_id, note) VALUES (?, ?, ?)", rusqlite::params![user_id, ctx.admin_user_id, text])?;
        let id = tx.last_insert_rowid();
        record(tx, ctx, "user.note_add", Some("user"), Some(&user_id.to_string()), &json!({"note_id": id, "note": text}))?;
        Ok(id)
    })
}

pub fn delete_note(app: &App, ctx: &AdminContext, user_id: i64, note_id: i64) -> AdminResult<()> {
    txn(&app.db, |tx| {
        let row = fetch_one(tx, "SELECT note FROM user_admin_notes WHERE id = ? AND user_id = ?", rusqlite::params![note_id, user_id])?
            .ok_or_else(|| AdminError::new("A jegyzet nem található.", 404))?;
        tx.execute("DELETE FROM user_admin_notes WHERE id = ?", [note_id])?;
        record(tx, ctx, "user.note_delete", Some("user"), Some(&user_id.to_string()), &json!({"note_id": note_id, "note": row.text("note")}))?;
        Ok(())
    })
}

// ===== Export (GDPR) =====

/// A felhasználó összes adata JSON-ban: fiók, játékok, lépések, szavazatok, kitüntetések, barátok, napi eredmények.
pub fn export_user(app: &App, ctx: &AdminContext, user_id: i64) -> AdminResult<Value> {
    txn(&app.db, |tx| {
        let row = get_row(tx, user_id)?;
        let mut account = Map::new();
        for key in [
            "id", "email", "display_name", "created_at", "games_played", "games_won", "total_score", "rating", "rated_games",
            "last_login_at", "last_login_ip", "banned_until", "ban_reason", "chat_muted_until", "review_blocked", "deleted_at",
        ] {
            account.insert(key.to_string(), row.get(key).cloned().unwrap_or(Value::Null));
        }
        let mut games: Vec<Value> = Vec::new();
        for g in fetch_all(
            tx,
            "SELECT sg.id, sg.room_name, sg.status, sg.created_at, sg.updated_at, sg.is_async, sg.has_bots, \
             gp.player_name, gp.final_score, gp.is_winner, gp.rating_before, gp.rating_after \
             FROM game_players gp JOIN saved_games sg ON sg.id = gp.game_id WHERE gp.user_id = ? ORDER BY sg.id",
            [user_id],
        )? {
            let moves = fetch_all(
                tx,
                "SELECT move_number, player_name, action_type, details_json, created_at FROM game_moves \
                 WHERE game_id = ? AND player_name = ? ORDER BY move_number",
                rusqlite::params![g.int("id"), g.text("player_name")],
            )?;
            let mut entry = g.clone();
            entry.insert("moves".into(), rows_json(moves));
            games.push(Value::Object(entry));
        }
        let push: Vec<Value> = fetch_all(tx, "SELECT * FROM push_subscriptions WHERE user_id = ?", [user_id])?
            .iter()
            .map(|r| json!({"lang": r["lang"], "host": host_of(&r.text("endpoint")), "created_at": r["created_at"]}))
            .collect();
        let data = json!({
            "account": account, "games": games,
            "word_reviews": rows_json(fetch_all(tx, "SELECT word, verdict, created_at FROM word_reviews WHERE user_id = ? ORDER BY created_at", [user_id])?),
            "achievements": rows_json(fetch_all(tx, "SELECT badge, game_id, earned_at FROM achievements WHERE user_id = ?", [user_id])?),
            "friends": rows_json(fetch_all(
                tx,
                "SELECT u.id, u.display_name, f.status, f.created_at FROM friendships f JOIN users u \
                 ON (u.id = f.user_id OR u.id = f.friend_id) AND u.id != ? WHERE f.user_id = ? OR f.friend_id = ?",
                [user_id, user_id, user_id],
            )?),
            "daily_scores": rows_json(fetch_all(tx, "SELECT puzzle_date, best_score, attempts, revealed FROM daily_scores WHERE user_id = ?", [user_id])?),
            "rating_adjustments": rows_json(fetch_all(tx, "SELECT rating_before, rating_after, reason, created_at FROM rating_adjustments WHERE user_id = ?", [user_id])?),
            "login_events": rows_json(fetch_all(
                tx,
                "SELECT ip, user_agent, success, reason, created_at FROM login_events WHERE user_id = ? ORDER BY id DESC LIMIT 500",
                [user_id],
            )?),
            "push_devices": push,
        });
        record(tx, ctx, "view.user_export", Some("user"), Some(&user_id.to_string()), &Value::Null)?;
        Ok(data)
    })
}

// ===== Törlés / anonimizálás =====

/// Fiók törlése. `anonymize` (alapértelmezett): név → „Törölt felhasználó #id”, e-mail és jelszó törölve, a játékok és
/// az értékszám-előzmények megmaradnak. `delete`: a fiók sora is törlődik. A célpont nevének begépelése kötelező. A
/// név a játékokban is átíródik (névtelenítés). Visszatér: az érintett (régi) név.
pub fn delete_user(app: &App, ctx: &AdminContext, user_id: i64, mode: Option<&Value>, confirm_name: Option<&Value>, reason: Option<&Value>) -> AdminResult<String> {
    let mode = match mode {
        None | Some(Value::Null) => "anonymize",
        Some(Value::String(m)) if m == "anonymize" || m == "delete" => m.as_str(),
        _ => return Err(AdminError::field("Érvénytelen kérés.", 400, "mode")),
    };
    let name = if mode == "delete" { "user.delete" } else { "user.anonymize" };
    action(&app.db, ctx, name, Some("user"), Some(user_id.to_string()), reason, json!({}), true, |act| {
        let row = get_row(act.tx, user_id)?;
        require_manageable(app, &row, false)?;
        let display_name = row.text("display_name");
        if str_of(confirm_name).map(|c| c.trim()) != Some(display_name.as_str()) {
            return Err(AdminError::field("A megerősítő név nem egyezik.", 400, "confirm_name"));
        }
        let placeholder = deleted_name(user_id);
        rename_in_games(act.tx, user_id, &display_name, &placeholder, false)?;
        let tx = act.tx;
        for sql in [
            "DELETE FROM sessions WHERE user_id = ?",
            "DELETE FROM push_subscriptions WHERE user_id = ?",
            "DELETE FROM word_reviews WHERE user_id = ?",
            "DELETE FROM user_admin_notes WHERE user_id = ?",
            "DELETE FROM chat_log WHERE user_id = ?",
            "DELETE FROM login_events WHERE user_id = ?",
        ] {
            tx.execute(sql, [user_id])?;
        }
        tx.execute("DELETE FROM friendships WHERE user_id = ? OR friend_id = ?", [user_id, user_id])?;
        if mode == "delete" {
            tx.execute("DELETE FROM users WHERE id = ?", [user_id])?;
        } else {
            tx.execute(
                "UPDATE users SET display_name = ?, email = ?, email_lower = ?, password_hash = ?, reconnect_token = NULL, banned_until = NULL, \
                 ban_reason = NULL, chat_muted_until = NULL, last_login_ip = NULL, deleted_at = ? WHERE id = ?",
                rusqlite::params![placeholder, deleted_email(user_id), deleted_email(user_id), "", util::now_ts(), user_id],
            )?;
        }
        act.details.insert("mode".into(), json!(mode));
        act.details.insert("before".into(), json!({"display_name": display_name, "email": row.text("email")}));
        act.details.insert("after".into(), json!({"display_name": if mode == "anonymize" { json!(placeholder) } else { Value::Null }}));
        Ok(display_name)
    })
}
