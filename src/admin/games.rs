//! Admin panel: játékok — archívum, levelezős játékok, ranglista és értékszám (ELO újraszámolás, gyanús minták).
//!
//! Az archívum a `saved_games` táblából dolgozik. Az érvénytelenítés (`status = 'voided'`) csak jelölés: a játék
//! kikerül a ranglistából és az értékszámításból, de megmarad, és visszavonható. Az értékszám-újraszámolás a
//! befejezett, nem érvénytelenített, robot nélküli játékokból épül fel időrendben (`elo::rating_changes`), a kézi
//! módosításokkal együtt.

use super::*;
use crate::app::App;
use crate::db::{Row, RowExt, fetch_all, fetch_one, placeholders};
use crate::elo;
use crate::server::core;
use crate::state::ServerState;
use rusqlite::params_from_iter;
use rusqlite::types::Value as SqlValue;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

const GAME_STATUSES: [&str; 4] = ["active", "finished", "abandoned", "voided"];
const GAME_SORTS: [(&str, &str); 3] = [("id", "sg.id"), ("created_at", "sg.created_at"), ("updated_at", "sg.updated_at")];
const MAX_CHANGES_SHOWN: usize = 200;
pub const SUSPICIOUS_PAIR_GAMES: usize = 5; // ennyi közös játék után jelzünk egyoldalú párosítást
pub const SUSPICIOUS_PAIR_SHARE: f64 = 0.8; // a nagyobbik fél győzelmi aránya
const FAST_PASS_MAX_MOVES: i64 = 16;
const FAST_PASS_SHARE: f64 = 0.8;
const FAST_PASS_MIN_MOVES: i64 = 6;
const EFFICIENCY_MIN_GAMES: usize = 3;
const EFFICIENCY_THRESHOLD: f64 = 95.0;
pub const ASYNC_EXTEND_HOURS: [i64; 6] = [6, 12, 24, 48, 72, 168];
pub const GAME_CSV_FIELDS: [&str; 10] = ["id", "name", "status", "bots", "async", "moves", "created_at", "updated_at", "players", "winners"];

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

// ===== Archívum =====

fn players_of(rows: &[Row]) -> HashMap<i64, Vec<Value>> {
    let mut players: HashMap<i64, Vec<Value>> = HashMap::new();
    for r in rows {
        players.entry(r.int("game_id")).or_default().push(json!({
            "name": r.text("player_name"), "user_id": r.get("user_id"), "score": r.int("final_score"),
            "winner": r.flag("is_winner"), "rating_before": r.get("rating_before"), "rating_after": r.get("rating_after"),
        }));
    }
    players
}

fn arg<'a>(args: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    args.get(key).map(|s| s.as_str()).filter(|s| !s.is_empty())
}

/// Az archívum szűrve (status, since / until, user, bots, async, suspicious, q) és lapozva.
pub fn list_games(app: &App, args: &HashMap<String, String>, paging: Option<(i64, i64)>) -> AdminResult<Value> {
    let mut where_: Vec<String> = Vec::new();
    let mut params: Vec<SqlValue> = Vec::new();
    if let Some(status) = arg(args, "status") {
        if !GAME_STATUSES.contains(&status) {
            return Err(AdminError::new("Érvénytelen szűrő.", 400));
        }
        where_.push("sg.status = ?".into());
        params.push(SqlValue::Text(status.to_string()));
    }
    if let Some(since) = arg(args, "since") {
        where_.push("sg.created_at >= ?".into());
        params.push(SqlValue::Text(parse_bound(since, false)?.0));
    }
    if let Some(until) = arg(args, "until") {
        let (stamp, exclusive) = parse_bound(until, true)?;
        where_.push(if exclusive { "sg.created_at < ?" } else { "sg.created_at <= ?" }.into());
        params.push(SqlValue::Text(stamp));
    }
    let user = args.get("user").map(|u| u.trim()).unwrap_or("");
    if !user.is_empty() {
        if user.chars().all(|c| c.is_ascii_digit()) {
            where_.push("EXISTS (SELECT 1 FROM game_players p WHERE p.game_id = sg.id AND p.user_id = ?)".into());
            params.push(SqlValue::Integer(user.parse().unwrap_or(0)));
        } else {
            where_.push("EXISTS (SELECT 1 FROM game_players p WHERE p.game_id = sg.id AND p.player_name LIKE ? ESCAPE '\\')".into());
            params.push(SqlValue::Text(like(user)));
        }
    }
    for (key, column) in [("bots", "sg.has_bots"), ("async", "sg.is_async")] {
        if let Some(v @ ("0" | "1")) = args.get(key).map(|s| s.as_str()) {
            where_.push(format!("{column} = ?"));
            params.push(SqlValue::Integer(v.parse().unwrap_or(0)));
        }
    }
    if matches!(arg(args, "suspicious"), Some("1") | Some("true")) {
        let mut ids: Vec<i64> = suspicious_game_ids(app)?.into_iter().collect();
        ids.sort();
        where_.push(if ids.is_empty() { "0".to_string() } else { format!("sg.id IN ({})", placeholders(ids.len())) });
        params.extend(ids.into_iter().map(SqlValue::Integer));
    }
    let q = args.get("q").map(|q| q.trim()).unwrap_or("");
    if !q.is_empty() {
        let mut clause = "sg.room_name LIKE ? ESCAPE '\\'".to_string();
        params.push(SqlValue::Text(like(q)));
        if q.chars().all(|c| c.is_ascii_digit()) {
            clause.push_str(" OR sg.id = ?");
            params.push(SqlValue::Integer(q.parse().unwrap_or(0)));
        }
        where_.push(format!("({clause})"));
    }
    let (limit, offset) = paging.unwrap_or_else(|| page_args(args, 50, 200));
    let clause = if where_.is_empty() { String::new() } else { format!("WHERE {}", where_.join(" AND ")) };
    let order = order_by(args.get("sort").map(|s| s.as_str()), Some(args.get("order").map(|s| s.as_str()).unwrap_or("desc")), &GAME_SORTS, "id");
    let (total, rows, players) = txn(&app.db, |tx| {
        let total = fetch_one(tx, &format!("SELECT COUNT(*) AS n FROM saved_games sg {clause}"), params_from_iter(params.clone()))?.map(|r| r.int("n")).unwrap_or(0);
        let mut page = params.clone();
        page.push(SqlValue::Integer(limit));
        page.push(SqlValue::Integer(offset));
        let rows = fetch_all(
            tx,
            &format!(
                "SELECT sg.id, sg.room_name, sg.status, sg.has_bots, sg.is_async, sg.challenge_mode, sg.created_at, sg.updated_at, \
                 sg.share_token IS NOT NULL AS shared, (SELECT COUNT(*) FROM game_moves m WHERE m.game_id = sg.id) AS moves, \
                 EXISTS (SELECT 1 FROM game_analysis a WHERE a.game_id = sg.id) AS analysed \
                 FROM saved_games sg {clause} ORDER BY {order}, sg.id DESC LIMIT ? OFFSET ?"
            ),
            params_from_iter(page),
        )?;
        let ids: Vec<i64> = rows.iter().map(|r| r.int("id")).collect();
        let players = if ids.is_empty() {
            HashMap::new()
        } else {
            players_of(&fetch_all(
                tx,
                &format!(
                    "SELECT game_id, player_name, user_id, final_score, is_winner, rating_before, rating_after FROM game_players WHERE game_id IN ({}) ORDER BY id",
                    placeholders(ids.len())
                ),
                params_from_iter(ids),
            )?)
        };
        Ok((total, rows, players))
    })?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            let game_players = players.get(&r.int("id")).cloned().unwrap_or_default();
            let winners: Vec<Value> = game_players.iter().filter(|p| p["winner"] == json!(true)).map(|p| p["name"].clone()).collect();
            json!({
                "id": r.int("id"), "name": r.text("room_name"), "status": r.text("status"), "bots": r.flag("has_bots"),
                "async": r.flag("is_async"), "challenge": r.flag("challenge_mode"), "moves": r.int("moves"),
                "created_at": r.text("created_at"), "updated_at": r.text("updated_at"), "shared": r.flag("shared"),
                "analysed": r.flag("analysed"), "players": game_players, "winners": winners,
            })
        })
        .collect();
    Ok(json!({"items": items, "total": total, "limit": limit, "offset": offset}))
}

fn details_of(raw: Option<String>) -> Value {
    raw.and_then(|r| serde_json::from_str::<Value>(&r).ok()).filter(|v| v.is_object()).unwrap_or_else(|| json!({}))
}

/// Egy játék részletei: adatok, játékosok (értékszám-változással), lépésnapló (kezekkel), elemzés összegzése. A kezek
/// miatt a megtekintés naplózva: `view.game`.
pub fn game_detail(app: &App, ctx: &AdminContext, game_id: i64) -> AdminResult<Value> {
    let history = audit::query_audit(
        &app.db,
        &audit::AuditFilters { target_type: Some("game".into()), target_id: Some(game_id.to_string()), ..Default::default() },
        Some("30"),
        None,
        AUDIT_MAX_LIMIT,
    )?["items"]
        .clone();
    let (row, players, moves, analysis_row) = txn(&app.db, |tx| {
        let row = fetch_one(tx, "SELECT * FROM saved_games WHERE id = ?", [game_id])?.ok_or_else(|| AdminError::new("A játék nem található.", 404))?;
        let players: Vec<Value> = fetch_all(
            tx,
            "SELECT player_name AS name, user_id, final_score AS score, is_winner AS winner, rating_before, rating_after FROM game_players WHERE game_id = ? ORDER BY id",
            [game_id],
        )?
        .into_iter()
        .map(|mut p| {
            let change = match (p.opt_int("rating_before"), p.opt_int("rating_after")) {
                (Some(b), Some(a)) => json!(a - b),
                _ => Value::Null,
            };
            let winner = p.flag("winner");
            p.insert("winner".into(), json!(winner));
            p.insert("rating_change".into(), change);
            Value::Object(p)
        })
        .collect();
        let moves: Vec<Value> = fetch_all(
            tx,
            "SELECT move_number, player_name, action_type, details_json, created_at FROM game_moves WHERE game_id = ? ORDER BY move_number",
            [game_id],
        )?
        .iter()
        .map(|m| {
            let details = details_of(m.opt_text("details_json"));
            json!({
                "n": m.int("move_number"), "player": m.text("player_name"), "type": m.text("action_type"),
                "score": details.get("score").cloned().unwrap_or(Value::Null), "words": details.get("words").cloned().unwrap_or(json!([])),
                "rack": details.get("rack").cloned().unwrap_or(Value::Null), "admin": details.get("admin").is_some_and(util::truthy),
                "at": m.text("created_at"),
            })
        })
        .collect();
        let analysis_row = fetch_one(tx, "SELECT version, result_json, created_at FROM game_analysis WHERE game_id = ?", [game_id])?;
        record(tx, ctx, "view.game", Some("game"), Some(&game_id.to_string()), &Value::Null)?;
        Ok((row, players, moves, analysis_row))
    })?;
    let analysis = analysis_row.and_then(|a| {
        let result: Value = serde_json::from_str(&a.text("result_json")).ok()?;
        Some(json!({"version": a.int("version"), "created_at": a.text("created_at"), "players": result.get("players").cloned().unwrap_or(json!([]))}))
    });
    Ok(json!({
        "id": row.int("id"), "name": row.text("room_name"), "status": row.text("status"), "room_id": row.text("room_id"),
        "bots": row.flag("has_bots"), "async": row.flag("is_async"), "challenge": row.flag("challenge_mode"),
        "owner": row.get("owner_name"), "created_at": row.text("created_at"), "updated_at": row.text("updated_at"),
        "share_token": row.get("share_token"), "players": players, "moves": moves, "analysis": analysis, "admin_history": history,
    }))
}

/// A lépésnapló a tábla-pillanatképekkel (a visszajátszáshoz).
pub fn game_moves(app: &App, game_id: i64) -> AdminResult<Value> {
    let rows = txn(&app.db, |tx| {
        if fetch_one(tx, "SELECT 1 AS x FROM saved_games WHERE id = ?", [game_id])?.is_none() {
            return Err(AdminError::new("A játék nem található.", 404));
        }
        Ok(fetch_all(
            tx,
            "SELECT move_number, player_name, action_type, details_json, board_snapshot_json FROM game_moves WHERE game_id = ? ORDER BY move_number",
            [game_id],
        )?)
    })?;
    Ok(json!(rows
        .iter()
        .map(|r| {
            let details = details_of(r.opt_text("details_json"));
            let board = r.opt_text("board_snapshot_json").and_then(|b| serde_json::from_str::<Value>(&b).ok()).unwrap_or(Value::Null);
            json!({
                "n": r.int("move_number"), "player": r.text("player_name"), "type": r.text("action_type"),
                "score": details.get("score").cloned().unwrap_or(Value::Null), "words": details.get("words").cloned().unwrap_or(json!([])),
                "tiles": details.get("tiles").cloned().unwrap_or(json!([])), "board": board,
            })
        })
        .collect::<Vec<_>>()))
}

fn game_row(tx: &Transaction, game_id: i64) -> AdminResult<Row> {
    fetch_one(tx, "SELECT * FROM saved_games WHERE id = ?", [game_id])?.ok_or_else(|| AdminError::new("A játék nem található.", 404))
}

fn participants(tx: &Transaction, game_id: i64) -> AdminResult<Vec<i64>> {
    Ok(fetch_all(tx, "SELECT DISTINCT user_id FROM game_players WHERE game_id = ? AND user_id IS NOT NULL", [game_id])?.iter().map(|r| r.int("user_id")).collect())
}

/// Státuszváltozás után: a résztvevők számlálói és az (összes érintett) értékszám újraszámolása.
fn refresh_after_status_change(tx: &Transaction, user_ids: &[i64]) -> AdminResult<Value> {
    for uid in user_ids {
        users::recompute_user_counters(tx, *uid)?;
    }
    recompute_ratings(tx, true)
}

/// Érvénytelenítés: kikerül a ranglistából, a résztvevők értékszáma és számlálói újraszámolódnak; a kitüntetések csak
/// kérésre vesznek el. Visszatér: az értékszám-újraszámolás összegzése.
pub fn void_game(app: &App, ctx: &AdminContext, game_id: i64, reason: Option<&Value>, revoke_badges: Option<&Value>) -> AdminResult<Value> {
    let revoke = bool_field(Some(revoke_badges.unwrap_or(&json!(false))), "revoke_badges")?;
    action(&app.db, ctx, "game.void", Some("game"), Some(game_id.to_string()), reason, json!({}), true, |act| {
        let row = game_row(act.tx, game_id)?;
        if row.text("status") != "finished" {
            return Err(AdminError::new("Csak befejezett játék érvényteleníthető.", 409));
        }
        act.tx.execute("UPDATE saved_games SET status = 'voided' WHERE id = ?", [game_id])?;
        let users = participants(act.tx, game_id)?;
        if revoke {
            let removed = act.tx.execute("DELETE FROM achievements WHERE game_id = ?", [game_id])?;
            act.details.insert("badges_revoked".into(), json!(removed));
        }
        let summary = refresh_after_status_change(act.tx, &users)?;
        act.details.insert("before".into(), json!({"status": "finished"}));
        act.details.insert("after".into(), json!({"status": "voided"}));
        act.details.insert("rating_changes".into(), summary["changed"].clone());
        Ok(summary)
    })
}

pub fn unvoid_game(app: &App, ctx: &AdminContext, game_id: i64, reason: Option<&Value>) -> AdminResult<Value> {
    action(&app.db, ctx, "game.unvoid", Some("game"), Some(game_id.to_string()), reason, json!({}), true, |act| {
        let row = game_row(act.tx, game_id)?;
        if row.text("status") != "voided" {
            return Err(AdminError::new("A játék nincs érvénytelenítve.", 409));
        }
        act.tx.execute("UPDATE saved_games SET status = 'finished' WHERE id = ?", [game_id])?;
        let users = participants(act.tx, game_id)?;
        let summary = refresh_after_status_change(act.tx, &users)?;
        act.details.insert("before".into(), json!({"status": "voided"}));
        act.details.insert("after".into(), json!({"status": "finished"}));
        act.details.insert("rating_changes".into(), summary["changed"].clone());
        Ok(summary)
    })
}

pub fn unshare_game(app: &App, ctx: &AdminContext, game_id: i64, reason: Option<&Value>) -> AdminResult<()> {
    action(&app.db, ctx, "game.unshare", Some("game"), Some(game_id.to_string()), reason, json!({}), true, |act| {
        let row = game_row(act.tx, game_id)?;
        if row.opt_text("share_token").is_none() {
            return Err(AdminError::new("A játéknak nincs megosztási linkje.", 409));
        }
        act.tx.execute("UPDATE saved_games SET share_token = NULL WHERE id = ?", [game_id])?;
        Ok(())
    })
}

/// Mentett játék állapotának módosítása: `abandoned` ↔ `active`.
pub fn set_game_status(app: &App, ctx: &AdminContext, game_id: i64, status: Option<&Value>, reason: Option<&Value>, live_game_ids: &HashSet<i64>) -> AdminResult<()> {
    let Some(status) = str_of(status).filter(|s| *s == "abandoned" || *s == "active") else {
        return Err(AdminError::field("Érvénytelen állapot.", 400, "status"));
    };
    action(&app.db, ctx, "game.status", Some("game"), Some(game_id.to_string()), reason, json!({}), true, |act| {
        let row = game_row(act.tx, game_id)?;
        let current = row.text("status");
        if current != "abandoned" && current != "active" {
            return Err(AdminError::new("Ez a játék állapota nem módosítható.", 409));
        }
        if current == status {
            return Err(AdminError::new("A játék már ebben az állapotban van.", 409));
        }
        if live_game_ids.contains(&game_id) {
            return Err(AdminError::new("A játék éppen fut, előbb oszlasd fel a szobát.", 409));
        }
        act.tx.execute("UPDATE saved_games SET status = ? WHERE id = ?", rusqlite::params![status, game_id])?;
        act.details.insert("before".into(), json!({"status": current}));
        act.details.insert("after".into(), json!({"status": status}));
        Ok(())
    })
}

fn rows_json(rows: Vec<Row>) -> Value {
    Value::Array(rows.into_iter().map(Value::Object).collect())
}

/// A játék állapota + lépései JSON-ban (naplózva).
pub fn export_game(app: &App, ctx: &AdminContext, game_id: i64) -> AdminResult<Value> {
    txn(&app.db, |tx| {
        let row = game_row(tx, game_id)?;
        let mut game = Map::new();
        for key in ["id", "room_id", "room_name", "status", "challenge_mode", "owner_name", "has_bots", "is_async", "created_at", "updated_at"] {
            game.insert(key.to_string(), row.get(key).cloned().unwrap_or(Value::Null));
        }
        let data = json!({
            "game": game,
            "state": row.opt_text("state_json").filter(|s| !s.is_empty()).and_then(|s| serde_json::from_str::<Value>(&s).ok()),
            "players": rows_json(fetch_all(tx, "SELECT player_name, user_id, final_score, is_winner, rating_before, rating_after FROM game_players WHERE game_id = ?", [game_id])?),
            "moves": rows_json(fetch_all(
                tx,
                "SELECT move_number, player_name, action_type, details_json, board_snapshot_json, created_at FROM game_moves WHERE game_id = ? ORDER BY move_number",
                [game_id],
            )?),
        });
        record(tx, ctx, "view.game_export", Some("game"), Some(&game_id.to_string()), &Value::Null)?;
        Ok(data)
    })
}

/// Végleges törlés. Befejezett (ranglistás) játékot csak külön megerősítéssel (`confirm_finished`) lehet törölni; a
/// törlés után a résztvevők számlálói és az értékszámok újraszámolódnak.
pub fn delete_game(app: &App, ctx: &AdminContext, game_id: i64, reason: Option<&Value>, confirm_finished: Option<&Value>, live_game_ids: &HashSet<i64>) -> AdminResult<Option<Value>> {
    let confirm = bool_field(Some(confirm_finished.unwrap_or(&json!(false))), "confirm_finished")?;
    action(&app.db, ctx, "game.delete", Some("game"), Some(game_id.to_string()), reason, json!({}), true, |act| {
        let row = game_row(act.tx, game_id)?;
        if live_game_ids.contains(&game_id) {
            return Err(AdminError::new("A játék éppen fut, előbb oszlasd fel a szobát.", 409));
        }
        let status = row.text("status");
        if status == "finished" && !confirm {
            return Err(AdminError::new("Befejezett játék törléséhez külön megerősítés kell.", 409).with("needs_confirm", json!(true)));
        }
        let users = participants(act.tx, game_id)?;
        let names: Vec<String> = fetch_all(act.tx, "SELECT player_name FROM game_players WHERE game_id = ?", [game_id])?.iter().map(|r| r.text("player_name")).collect();
        act.tx.execute("DELETE FROM achievements WHERE game_id = ?", [game_id])?;
        act.tx.execute("DELETE FROM saved_games WHERE id = ?", [game_id])?;
        let summary = if status == "finished" || status == "voided" { Some(refresh_after_status_change(act.tx, &users)?) } else { None };
        act.details.insert("before".into(), json!({"name": row.text("room_name"), "status": status, "players": names}));
        act.details.insert("rating_changes".into(), summary.as_ref().map(|s| s["changed"].clone()).unwrap_or(json!(0)));
        Ok(summary)
    })
}

/// A régi (`abandoned`, `days` napnál régebbi) mentések, a törlés előnézete.
pub fn old_abandoned_preview(app: &App, days: Option<&Value>) -> AdminResult<Value> {
    let days = int_field(days, "days", 1, 3650)?;
    let cutoff = util::format_ts(util::utcnow() - chrono::Duration::days(days));
    let (rows, total) = txn(&app.db, |tx| {
        let rows = fetch_all(tx, "SELECT id, room_name, updated_at FROM saved_games WHERE status = 'abandoned' AND updated_at < ? ORDER BY updated_at LIMIT 500", [&cutoff])?;
        let total = fetch_one(tx, "SELECT COUNT(*) AS n FROM saved_games WHERE status = 'abandoned' AND updated_at < ?", [&cutoff])?.map(|r| r.int("n")).unwrap_or(0);
        Ok((rows, total))
    })?;
    Ok(json!({"total": total, "items": rows_json(rows), "days": days}))
}

pub fn old_abandoned_delete(app: &App, ctx: &AdminContext, days: Option<&Value>, reason: Option<&Value>, live_game_ids: &HashSet<i64>) -> AdminResult<usize> {
    let days = int_field(days, "days", 1, 3650)?;
    let cutoff = util::format_ts(util::utcnow() - chrono::Duration::days(days));
    action(&app.db, ctx, "game.bulk_delete", Some("game"), None, reason, json!({"days": days}), true, |act| {
        let ids: Vec<i64> = fetch_all(act.tx, "SELECT id FROM saved_games WHERE status = 'abandoned' AND updated_at < ?", [&cutoff])?
            .iter()
            .map(|r| r.int("id"))
            .filter(|i| !live_game_ids.contains(i))
            .collect();
        for id in &ids {
            act.tx.execute("DELETE FROM saved_games WHERE id = ?", [id])?;
        }
        act.details.insert("deleted".into(), json!(ids.len()));
        act.details.insert("ids".into(), json!(ids.iter().take(500).collect::<Vec<_>>()));
        Ok(ids.len())
    })
}

// ===== Értékszám újraszámolása =====

/// {játékosnév: feladta-e} a mentett állapotból.
fn state_flags(state_json: &str) -> HashMap<String, bool> {
    let Ok(data) = serde_json::from_str::<Value>(state_json) else { return HashMap::new() };
    data.get("players")
        .and_then(|p| p.as_array())
        .map(|players| players.iter().filter_map(|p| Some((p.get("name")?.as_str()?.to_string(), p.get("resigned").is_some_and(util::truthy)))).collect())
        .unwrap_or_default()
}

enum TimelineEvent {
    Game(Row),
    Adjustment(Row),
}

/// A befejezett, nem érvénytelenített, robot nélküli játékok és a kézi módosítások időrendben.
fn timeline(tx: &Transaction) -> AdminResult<Vec<TimelineEvent>> {
    let mut events: Vec<(String, u8, i64, TimelineEvent)> = Vec::new();
    for g in fetch_all(tx, "SELECT id, updated_at, state_json FROM saved_games WHERE status = 'finished' AND has_bots = 0 ORDER BY updated_at, id", [])? {
        events.push((g.opt_text("updated_at").unwrap_or_default(), 0, g.int("id"), TimelineEvent::Game(g)));
    }
    for a in fetch_all(tx, "SELECT id, user_id, rating_before, rating_after, created_at FROM rating_adjustments ORDER BY created_at, id", [])? {
        events.push((a.opt_text("created_at").unwrap_or_default(), 1, a.int("id"), TimelineEvent::Adjustment(a)));
    }
    events.sort_by(|a, b| (&a.0, a.1, a.2).cmp(&(&b.0, b.1, b.2)));
    Ok(events.into_iter().map(|e| e.3).collect())
}

pub type RatingState = HashMap<i64, (i64, i64)>;
pub type PerGame = HashMap<(i64, i64), (i64, i64)>;

/// A teljes értékszám-számítás a nulláról. Visszatér: ({user_id: (értékszám, értékelt játékok)}, {(game_id, user_id):
/// (előtte, utána)}).
pub fn compute_ratings(tx: &Transaction) -> AdminResult<(RatingState, PerGame)> {
    let mut state: RatingState = fetch_all(tx, "SELECT id FROM users", [])?.iter().map(|r| (r.int("id"), (elo::INITIAL_RATING, 0))).collect();
    let mut per_game: PerGame = HashMap::new();
    for event in timeline(tx)? {
        match event {
            TimelineEvent::Adjustment(row) => {
                if let Some((rating, games)) = state.get(&row.int("user_id")).copied() {
                    state.insert(row.int("user_id"), (rating + (row.int("rating_after") - row.int("rating_before")), games));
                }
            }
            TimelineEvent::Game(row) => {
                let flags = state_flags(&row.text("state_json"));
                let mut ranked: Vec<(i64, i64, i64, i64)> = Vec::new(); // (uid, értékszám, játékok, pont)
                for p in fetch_all(tx, "SELECT player_name, user_id, final_score FROM game_players WHERE game_id = ? AND user_id IS NOT NULL ORDER BY id", [row.int("id")])? {
                    let uid = p.int("user_id");
                    if state.contains_key(&uid) && !ranked.iter().any(|r| r.0 == uid) {
                        let score = if flags.get(&p.text("player_name")).copied().unwrap_or(false) { -1_000_000_000 } else { p.int("final_score") };
                        let (rating, games) = state[&uid];
                        ranked.push((uid, rating, games, score));
                    }
                }
                if ranked.len() < 2 {
                    continue;
                }
                let changes = elo::rating_changes(&ranked);
                for (uid, before, games, _) in &ranked {
                    let after = before + changes[uid];
                    state.insert(*uid, (after, games + 1));
                    per_game.insert((row.int("id"), *uid), (*before, after));
                }
            }
        }
    }
    Ok((state, per_game))
}

/// A teljes ELO újraszámolás. `apply` hamis: csak előnézet (ki mennyit változna). Igaz: a felhasználók és a
/// `game_players` értékei felülíródnak. Visszatér: {'changed', 'unchanged', 'items': [...]}.
pub fn recompute_ratings(tx: &Transaction, apply: bool) -> AdminResult<Value> {
    let (state, per_game) = compute_ratings(tx)?;
    let names: HashMap<i64, String> = fetch_all(tx, "SELECT id, display_name FROM users", [])?.iter().map(|r| (r.int("id"), r.text("display_name"))).collect();
    let current: HashMap<i64, (i64, i64)> =
        fetch_all(tx, "SELECT id, rating, rated_games FROM users", [])?.iter().map(|r| (r.int("id"), (r.opt_int("rating").unwrap_or(elo::INITIAL_RATING), r.int("rated_games")))).collect();
    let mut items: Vec<Value> = Vec::new();
    let mut unchanged = 0;
    let mut uids: Vec<i64> = state.keys().copied().collect();
    uids.sort();
    for uid in &uids {
        let (rating, games) = state[uid];
        let (cur_rating, cur_games) = current[uid];
        if (rating, games) == (cur_rating, cur_games) {
            unchanged += 1;
            continue;
        }
        items.push(json!({
            "user_id": uid, "display_name": names.get(uid), "current": cur_rating, "new": rating, "delta": rating - cur_rating,
            "rated_games": cur_games, "new_rated_games": games,
        }));
    }
    items.sort_by_key(|i| std::cmp::Reverse(i["delta"].as_i64().unwrap_or(0).abs()));
    if apply {
        for uid in &uids {
            let (rating, games) = state[uid];
            if (rating, games) != current[uid] {
                tx.execute("UPDATE users SET rating = ?, rated_games = ? WHERE id = ?", rusqlite::params![rating, games, uid])?;
            }
        }
        tx.execute("UPDATE game_players SET rating_before = NULL, rating_after = NULL WHERE rating_before IS NOT NULL OR rating_after IS NOT NULL", [])?;
        for ((game_id, uid), (before, after)) in &per_game {
            tx.execute(
                "UPDATE game_players SET rating_before = ?, rating_after = ? WHERE game_id = ? AND user_id = ?",
                rusqlite::params![before, after, game_id, uid],
            )?;
        }
    }
    let changed = items.len();
    items.truncate(MAX_CHANGES_SHOWN);
    Ok(json!({"changed": changed, "unchanged": unchanged, "items": items}))
}

pub fn ratings_dry_run(app: &App) -> AdminResult<Value> {
    txn(&app.db, |tx| recompute_ratings(tx, false))
}

/// Az újraszámolt értékszámok alkalmazása (naplózva).
pub fn ratings_apply(app: &App, ctx: &AdminContext, reason: Option<&Value>) -> AdminResult<Value> {
    action(&app.db, ctx, "ratings.recompute", Some("setting"), None, reason, json!({}), true, |act| {
        let summary = recompute_ratings(act.tx, true)?;
        act.details.insert("changed".into(), summary["changed"].clone());
        act.details.insert("top_changes".into(), json!(summary["items"].as_array().map(|i| i.iter().take(20).collect::<Vec<_>>()).unwrap_or_default()));
        Ok(summary)
    })
}

// ===== Ranglista és gyanús minták =====

/// A nyilvánossal azonos ranglista, a minimum játékszám nélkül.
pub fn leaderboard(app: &App, metric: Option<&str>, limit: i64) -> AdminResult<Vec<Value>> {
    let Some(metric) = metric.filter(|m| crate::db::LEADERBOARD_METRICS.contains(m)) else {
        return Err(AdminError::new("Ismeretlen rangsor.", 400));
    };
    let (entries, _) = app.db.get_leaderboard(metric, limit, None, true)?;
    Ok(entries)
}

type HumanGame = (i64, String, i64, bool);

/// {game_id: [(user_id, név, pont, nyert)]} a befejezett, robot nélküli játékokból (csak regisztráltakkal).
fn human_games(tx: &Transaction) -> AdminResult<Ordered<i64, Vec<HumanGame>>> {
    let mut games: Ordered<i64, Vec<HumanGame>> = Ordered::default();
    for r in fetch_all(
        tx,
        "SELECT gp.game_id, gp.user_id, gp.player_name, gp.final_score, gp.is_winner FROM game_players gp \
         JOIN saved_games sg ON sg.id = gp.game_id AND sg.status = 'finished' AND sg.has_bots = 0 \
         WHERE gp.user_id IS NOT NULL ORDER BY gp.game_id, gp.id",
        [],
    )? {
        games.entry_or(r.int("game_id"), Vec::new).push((r.int("user_id"), r.text("player_name"), r.int("final_score"), r.flag("is_winner")));
    }
    Ok(games)
}

fn user_ips(tx: &Transaction) -> AdminResult<HashMap<i64, HashSet<String>>> {
    let mut ips: HashMap<i64, HashSet<String>> = HashMap::new();
    for r in fetch_all(tx, "SELECT user_id, ip FROM login_events WHERE success = 1 AND user_id IS NOT NULL AND ip IS NOT NULL GROUP BY user_id, ip", [])? {
        ips.entry(r.int("user_id")).or_default().insert(r.text("ip"));
    }
    for r in fetch_all(tx, "SELECT id, last_login_ip FROM users WHERE last_login_ip IS NOT NULL", [])? {
        ips.entry(r.int("id")).or_default().insert(r.text("last_login_ip"));
    }
    Ok(ips)
}

/// Gyanús minták (csak jelzés, automatikus büntetés nincs): egyoldalú párosítás, azonos IP-ről érkező ellenfelek, gyors
/// passzolós játékok, kiugróan jó (a legjobb lépést rendszeresen rakó) játékosok.
pub fn suspicious_patterns(app: &App) -> AdminResult<Value> {
    txn(&app.db, |tx| {
        let names: HashMap<i64, String> = fetch_all(tx, "SELECT id, display_name FROM users", [])?.iter().map(|r| (r.int("id"), r.text("display_name"))).collect();
        let games = human_games(tx)?;
        let ips = user_ips(tx)?;
        let mut pair_games: Ordered<(i64, i64), Vec<i64>> = Ordered::default();
        let mut pair_wins: HashMap<(i64, i64), HashMap<i64, usize>> = HashMap::new();
        let mut shared_ip: Ordered<(i64, i64), (Vec<i64>, std::collections::BTreeSet<String>)> = Ordered::default();
        for (game_id, players) in games.iter() {
            if players.len() == 2 {
                let (a, _, _, wa) = &players[0];
                let (b, _, _, wb) = &players[1];
                if a != b {
                    let key = (*a.min(b), *a.max(b));
                    pair_games.entry_or(key, Vec::new).push(*game_id);
                    let wins = pair_wins.entry(key).or_insert_with(|| HashMap::from([(key.0, 0), (key.1, 0)]));
                    if *wa && !*wb {
                        *wins.get_mut(a).expect("kulcs") += 1;
                    } else if *wb && !*wa {
                        *wins.get_mut(b).expect("kulcs") += 1;
                    }
                }
            }
            for i in 0..players.len() {
                for j in (i + 1)..players.len() {
                    let (ua, ub) = (players[i].0, players[j].0);
                    if ua != ub {
                        let empty = HashSet::new();
                        let common: Vec<&String> = ips.get(&ua).unwrap_or(&empty).intersection(ips.get(&ub).unwrap_or(&empty)).collect();
                        if !common.is_empty() {
                            let entry = shared_ip.entry_or((ua.min(ub), ua.max(ub)), || (Vec::new(), Default::default()));
                            entry.0.push(*game_id);
                            entry.1.extend(common.into_iter().cloned());
                        }
                    }
                }
            }
        }
        let name_of = |u: &i64| names.get(u).cloned();
        let mut one_sided: Vec<Value> = Vec::new();
        for (key, ids) in pair_games.iter() {
            let wins = &pair_wins[key];
            let top = wins.values().copied().max().unwrap_or(0);
            if ids.len() >= SUSPICIOUS_PAIR_GAMES && top as f64 / ids.len() as f64 >= SUSPICIOUS_PAIR_SHARE {
                let pair_users: Vec<Value> = [key.0, key.1].iter().map(|u| json!({"id": u, "name": name_of(u), "wins": wins[u]})).collect();
                one_sided.push(json!({
                    "users": pair_users,
                    "games": ids.len(), "game_ids": ids.iter().rev().take(20).rev().collect::<Vec<_>>(),
                }));
            }
        }
        one_sided.sort_by_key(|e| std::cmp::Reverse(e["games"].as_u64().unwrap_or(0)));
        let mut same_ip: Vec<Value> = shared_ip
            .iter()
            .map(|(key, (game_ids, ip_set))| {
                let pair_users: Vec<Value> = [key.0, key.1].iter().map(|u| json!({"id": u, "name": name_of(u)})).collect();
                json!({
                    "users": pair_users,
                    "games": game_ids.len(), "game_ids": game_ids.iter().rev().take(20).rev().collect::<Vec<_>>(),
                    "ips": ip_set.iter().take(5).collect::<Vec<_>>(),
                })
            })
            .collect();
        same_ip.sort_by_key(|e| std::cmp::Reverse(e["games"].as_u64().unwrap_or(0)));
        let mut fast: Vec<Value> = Vec::new();
        for r in fetch_all(
            tx,
            "SELECT m.game_id AS game_id, COUNT(*) AS total, SUM(CASE WHEN m.action_type = 'pass' THEN 1 ELSE 0 END) AS passes \
             FROM game_moves m JOIN saved_games sg ON sg.id = m.game_id WHERE sg.status = 'finished' AND sg.has_bots = 0 GROUP BY m.game_id \
             HAVING total >= ? AND total <= ? AND passes * 1.0 / total >= ?",
            rusqlite::params![FAST_PASS_MIN_MOVES, FAST_PASS_MAX_MOVES, FAST_PASS_SHARE],
        )? {
            let players: Vec<String> = games.get(&r.int("game_id")).map(|g| g.iter().map(|p| p.1.clone()).collect()).unwrap_or_default();
            fast.push(json!({"game_id": r.int("game_id"), "moves": r.int("total"), "passes": r.int("passes"), "players": players}));
        }
        let efficiency = efficiency_outliers(tx, &games, &names)?;
        Ok(json!({
            "one_sided_pairs": one_sided, "same_ip": same_ip, "fast_pass_games": fast, "high_efficiency": efficiency,
            "thresholds": {"pair_games": SUSPICIOUS_PAIR_GAMES, "pair_share": SUSPICIOUS_PAIR_SHARE, "efficiency": EFFICIENCY_THRESHOLD, "efficiency_games": EFFICIENCY_MIN_GAMES},
        }))
    })
}

/// A játékelemzésekből: akinek az átlagos hatékonysága (a legjobb lépéshez mérve) rendszeresen kiugróan magas.
fn efficiency_outliers(tx: &Transaction, games: &Ordered<i64, Vec<HumanGame>>, names: &HashMap<i64, String>) -> AdminResult<Vec<Value>> {
    let mut per_user: Ordered<i64, Vec<f64>> = Ordered::default();
    for r in fetch_all(tx, "SELECT game_id, result_json FROM game_analysis", [])? {
        let Ok(result) = serde_json::from_str::<Value>(&r.text("result_json")) else { continue };
        let players = result.get("players").and_then(|p| p.as_array()).cloned().unwrap_or_default();
        let by_name: HashMap<&str, &Value> = players.iter().filter_map(|p| Some((p.get("player")?.as_str()?, p))).collect();
        for (user_id, name, _, _) in games.get(&r.int("game_id")).map(|g| g.as_slice()).unwrap_or(&[]) {
            if let Some(entry) = by_name.get(name.as_str()) {
                if entry.get("turns").and_then(|t| t.as_i64()).unwrap_or(0) >= 8 {
                    if let Some(eff) = entry.get("efficiency").and_then(|e| e.as_f64()) {
                        per_user.entry_or(*user_id, Vec::new).push(eff);
                    }
                }
            }
        }
    }
    let mut result: Vec<Value> = Vec::new();
    for (user_id, values) in per_user.iter() {
        let average = values.iter().sum::<f64>() / values.len() as f64;
        if values.len() >= EFFICIENCY_MIN_GAMES && average >= EFFICIENCY_THRESHOLD {
            result.push(json!({"user_id": user_id, "name": names.get(user_id), "games": values.len(), "average": round1(average)}));
        }
    }
    result.sort_by(|a, b| b["average"].as_f64().partial_cmp(&a["average"].as_f64()).unwrap_or(std::cmp::Ordering::Equal));
    Ok(result)
}

/// A gyanús mintákban szereplő játékok azonosítói (az archívum „gyanús” szűrőjéhez).
pub fn suspicious_game_ids(app: &App) -> AdminResult<HashSet<i64>> {
    let patterns = suspicious_patterns(app)?;
    let mut ids = HashSet::new();
    for key in ["one_sided_pairs", "same_ip"] {
        for entry in patterns[key].as_array().into_iter().flatten() {
            ids.extend(entry["game_ids"].as_array().into_iter().flatten().filter_map(|i| i.as_i64()));
        }
    }
    ids.extend(patterns["fast_pass_games"].as_array().into_iter().flatten().filter_map(|e| e["game_id"].as_i64()));
    Ok(ids)
}

// ===== Levelezős játékok =====

/// Egy levelezős játék sora: a memóriában lévő szoba (ha betöltött), különben a mentett állapot.
fn async_row(row: &Row, room: Option<&crate::room::Room>, now: f64) -> Value {
    let state: Value = serde_json::from_str(&row.text("state_json")).unwrap_or(json!({}));
    let (players, current, deadline, turn_hours, finished): (Vec<Value>, Option<String>, Option<f64>, i64, bool) = match room {
        Some(room) => {
            let game = &room.game;
            (
                game.players.iter().map(|p| json!({"name": p.name, "score": p.score, "timeouts": p.timeouts, "resigned": p.resigned})).collect(),
                game.current_player().map(|p| p.name.clone()),
                game.turn_deadline,
                game.turn_hours,
                game.finished,
            )
        }
        None => {
            let players: Vec<Value> = state
                .get("players")
                .and_then(|p| p.as_array())
                .map(|list| {
                    list.iter()
                        .map(|p| {
                            json!({
                                "name": p.get("name"), "score": p.get("score").cloned().unwrap_or(json!(0)),
                                "timeouts": p.get("timeouts").cloned().unwrap_or(json!(0)), "resigned": p.get("resigned").is_some_and(util::truthy),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let index = state.get("current_player_idx").and_then(|i| i.as_i64()).unwrap_or(0);
            let current = if index >= 0 { players.get(index as usize).and_then(|p| p["name"].as_str()).map(|s| s.to_string()) } else { None };
            (
                players,
                current,
                state.get("turn_deadline").and_then(|d| d.as_f64()),
                state.get("turn_hours").and_then(|h| h.as_i64()).unwrap_or(0),
                state.get("finished").is_some_and(util::truthy),
            )
        }
    };
    let max_timeouts = players.iter().map(|p| p["timeouts"].as_i64().unwrap_or(0)).max().unwrap_or(0);
    json!({
        "id": row.int("id"), "name": row.text("room_name"), "players": players, "current": current, "deadline": deadline,
        "remaining": deadline.filter(|d| *d != 0.0).map(|d| d - now), "turn_hours": turn_hours, "finished": finished,
        "loaded": room.is_some(), "room_id": row.text("room_id"), "updated_at": row.text("updated_at"),
        "created_at": row.text("created_at"), "moves": row.int("moves"), "last_move_at": row.get("last_move_at"),
        "max_timeouts": max_timeouts,
    })
}

/// A folyamatban lévő levelezős játékok és a háttérfeladat (`async_sweeper`) állapota.
pub fn list_async(app: &App, st: &ServerState) -> AdminResult<Value> {
    let loaded: HashMap<i64, &crate::room::Room> = st.rooms.values().filter(|r| r.is_async).filter_map(|r| Some((r.db_game_id?, r))).collect();
    let now = util::now();
    let rows = txn(&app.db, |tx| {
        Ok(fetch_all(
            tx,
            "SELECT sg.id, sg.room_id, sg.room_name, sg.state_json, sg.updated_at, sg.created_at, \
             (SELECT COUNT(*) FROM game_moves m WHERE m.game_id = sg.id) AS moves, \
             (SELECT MAX(m.created_at) FROM game_moves m WHERE m.game_id = sg.id) AS last_move_at \
             FROM saved_games sg WHERE sg.is_async = 1 AND sg.status = 'active' ORDER BY sg.updated_at DESC",
            [],
        )?)
    })?;
    let items: Vec<Value> = rows.iter().map(|r| async_row(r, loaded.get(&r.int("id")).copied(), now)).collect();
    let sweeper = system::jobs_status(app).into_iter().find(|j| j["name"] == "async_sweeper");
    Ok(json!({"items": items, "sweeper": sweeper, "loaded_count": loaded.len()}))
}

/// (sor, szoba azonosító): a levelezős játék sora, a szoba betöltve a memóriába (ha még nem volt).
fn async_game(app: &Arc<App>, st: &mut ServerState, game_id: i64) -> AdminResult<String> {
    let row = txn(&app.db, |tx| Ok(fetch_one(tx, "SELECT * FROM saved_games WHERE id = ? AND is_async = 1 AND status = 'active'", [game_id])?))?
        .ok_or_else(|| AdminError::new("Ez a levelezős játék nem található.", 404))?;
    core::async_room_for_db_game(app, st, &row).ok_or_else(|| AdminError::new("Ez a levelezős játék nem tölthető be.", 500))
}

/// A soron lévő határidejének meghosszabbítása `hours` órával (a lejárt határidő is a mostantól számolódik).
pub fn async_extend(app: &Arc<App>, st: &mut ServerState, ctx: &AdminContext, game_id: i64, hours: Option<&Value>, reason: Option<&Value>) -> AdminResult<f64> {
    let Some(hours) = hours.and_then(|h| h.as_i64()).filter(|h| ASYNC_EXTEND_HOURS.contains(h)) else {
        return Err(AdminError::field("Érvénytelen időtartam.", 400, "hours"));
    };
    let room_id = async_game(app, st, game_id)?;
    let (finished, deadline) = {
        let game = &st.rooms[&room_id].game;
        (game.finished, game.turn_deadline)
    };
    let Some(deadline) = deadline.filter(|d| !finished && *d != 0.0) else {
        return Err(AdminError::new("Ennek a játéknak nincs határideje.", 409));
    };
    let new_deadline = deadline.max(util::now()) + hours as f64 * 3600.0;
    action(
        &app.db,
        ctx,
        "async.extend",
        Some("game"),
        Some(game_id.to_string()),
        reason,
        json!({"hours": hours, "before": {"deadline": deadline}, "after": {"deadline": new_deadline}}),
        true,
        |_| Ok(()),
    )?;
    st.rooms.get_mut(&room_id).expect("létező szoba").game.turn_deadline = Some(new_deadline);
    core::emit_all_states(app, st, &room_id);
    Ok(new_deadline)
}

/// A kör azonnali lejáratása: automatikus passz (három egymás utáni után a játékos feladja).
pub fn async_expire(app: &Arc<App>, st: &mut ServerState, ctx: &AdminContext, game_id: i64, reason: Option<&Value>) -> AdminResult<String> {
    let room_id = async_game(app, st, game_id)?;
    let current = st.rooms[&room_id].game.current_player().map(|p| p.name.clone()).unwrap_or_default();
    action(&app.db, ctx, "async.expire", Some("game"), Some(game_id.to_string()), reason, json!({"player": current}), true, |_| Ok(()))?;
    if !st.rooms.get_mut(&room_id).expect("létező szoba").game.expire_turn() {
        return Err(AdminError::new("A kör most nem járatható le.", 409));
    }
    core::emit_all_states(app, st, &room_id);
    core::cleanup_finished_async(app, st, &room_id);
    Ok(current)
}

/// Feladás valaki nevében (a játék véget ér, a feladó nem lehet győztes).
pub fn async_resign(app: &Arc<App>, st: &mut ServerState, ctx: &AdminContext, game_id: i64, player_name: Option<&str>, reason: Option<&Value>) -> AdminResult<()> {
    let room_id = async_game(app, st, game_id)?;
    let player = st.rooms[&room_id].game.players.iter().find(|p| Some(p.name.as_str()) == player_name && !p.is_bot).map(|p| (p.id.clone(), p.name.clone()));
    let Some((player_id, name)) = player else {
        return Err(AdminError::field("A játékos nem található.", 404, "player"));
    };
    action(&app.db, ctx, "async.resign", Some("game"), Some(game_id.to_string()), reason, json!({"player": name}), true, |_| Ok(()))?;
    if st.rooms.get_mut(&room_id).expect("létező szoba").game.resign(&player_id).is_err() {
        return Err(AdminError::new("A játék nem adható fel.", 409));
    }
    core::emit_all_states(app, st, &room_id);
    core::cleanup_finished_async(app, st, &room_id);
    Ok(())
}

/// Push emlékeztető a soron lévőnek.
pub fn async_remind(app: &Arc<App>, st: &mut ServerState, ctx: &AdminContext, game_id: i64) -> AdminResult<bool> {
    let room_id = async_game(app, st, game_id)?;
    let room = &st.rooms[&room_id];
    let current = room.game.current_player().map(|p| p.name.clone());
    let user_id = current.as_ref().and_then(|name| room.known_user_ids.get(name).copied().flatten()).filter(|u| *u != 0);
    let Some(user_id) = user_id else {
        return Err(AdminError::new("A soron lévő játékos nem regisztrált.", 409));
    };
    let sent = app.push.notify_turn(user_id, &room.name, &room.id, "reminder");
    app.db.with(|tx| record(tx, ctx, "async.remind", Some("game"), Some(&game_id.to_string()), &json!({"player": current, "sent": sent})).map(|_| ()))?;
    Ok(sent)
}

/// A folyamatban lévő levelezős játék érvénytelenítése: jelölés, a szoba megszűnik.
pub fn async_void(app: &Arc<App>, st: &mut ServerState, ctx: &AdminContext, game_id: i64, reason: Option<&Value>) -> AdminResult<()> {
    let room_id = async_game(app, st, game_id)?;
    action(&app.db, ctx, "async.void", Some("game"), Some(game_id.to_string()), reason, json!({}), true, |act| {
        act.tx.execute("UPDATE saved_games SET status = 'voided' WHERE id = ?", [game_id])?;
        Ok(())
    })?;
    st.rooms.get_mut(&room_id).expect("létező szoba").manually_saved = true;
    core::disband_active_room(app, st, &room_id, "A játék érvénytelenítve lett.", None);
    app.emit_all("rooms_list", &st.get_rooms_list());
    Ok(())
}

/// A játék kiürítése a memóriából (az adatbázisban megmarad; csak ha senki sincs a szobában).
pub fn async_unload(app: &Arc<App>, st: &mut ServerState, ctx: &AdminContext, game_id: i64, reason: Option<&Value>) -> AdminResult<()> {
    let room = st.rooms.values().find(|r| r.is_async && r.db_game_id == Some(game_id));
    let Some(room) = room else { return Err(AdminError::new("A játék nincs betöltve.", 404)) };
    if room.game.has_connected_human() || !room.spectators.is_empty() {
        return Err(AdminError::new("A szobában éppen vannak, nem üríthető ki.", 409));
    }
    let room_id = room.id.clone();
    action(&app.db, ctx, "async.unload", Some("game"), Some(game_id.to_string()), reason, json!({}), true, |_| Ok(()))?;
    core::cleanup_room(app, st, &room_id);
    Ok(())
}

/// A játék betöltése a memóriába.
pub fn async_load(app: &Arc<App>, st: &mut ServerState, ctx: &AdminContext, game_id: i64) -> AdminResult<()> {
    async_game(app, st, game_id)?;
    app.db.with(|tx| record(tx, ctx, "async.load", Some("game"), Some(&game_id.to_string()), &Value::Null).map(|_| ()))?;
    Ok(())
}

/// A határidő-figyelő kézi indítása. Visszatér: a léptetett játékok száma. (A `core::expire_async_turns` maga zárol,
/// ezért a hívó nem tarthatja a szerver állapotát.)
pub fn async_sweep(app: &Arc<App>, ctx: &AdminContext) -> AdminResult<usize> {
    app.db.with(|tx| record(tx, ctx, "async.sweep", Some("system"), None, &Value::Null).map(|_| ()))?;
    Ok(core::expire_async_turns(app, None))
}
