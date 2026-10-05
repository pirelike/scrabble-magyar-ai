//! Admin panel: statisztika és a napi feladvány kezelése.
//!
//! A statisztikák időszak-választóval (7 / 30 / 90 / 365 nap) készülnek, mindegyik táblázatos alakban (`table`) is
//! elérhető a CSV exporthoz. A számítások az adatbázisból dolgoznak (a megmaradt játékokból és naplókból), a szerver
//! állapotát nem érintik.

use super::live::{ACTIVITY_SQL_PUB as ACTIVITY_SQL, charts, day_range};
use super::*;
use crate::app::App;
use crate::db::{Row, RowExt, fetch_all, fetch_one};
use chrono::{Duration, NaiveDate, Timelike};
use std::collections::{HashMap, HashSet};

pub const RANGES: [i64; 4] = [7, 30, 90, 365];
pub const METRICS: [&str; 10] = ["registrations", "active", "retention", "games", "bots", "words", "challenges", "practice", "review", "heatmap"];
const MAX_MOVE_ROWS: i64 = 60000;
const TOP_WORDS: usize = 30;
const TOP_MOVES: usize = 20;
const RETENTION_DAYS: [i64; 3] = [1, 7, 30];
pub const PRACTICE_KEYS: [&str; 7] = ["quiz", "answer", "short_words", "rack", "rack_word", "word_review", "word_review_vote"];

fn range_days(value: Option<&str>) -> AdminResult<i64> {
    let days = match value.map(|v| v.trim().parse::<i64>()) {
        None => return Ok(30),
        Some(Err(_)) => return Ok(30),
        Some(Ok(d)) => d,
    };
    if !RANGES.contains(&days) {
        return Err(AdminError::field("Érvénytelen időszak.", 400, "range"));
    }
    Ok(days)
}

fn table(columns: Vec<Value>, rows: Vec<Value>) -> Value {
    json!({"columns": columns, "rows": rows})
}

fn str_columns(columns: &[&str]) -> Vec<Value> {
    columns.iter().map(|c| json!(c)).collect()
}

fn series_table(days: &[Value], series: &[(&str, &Value)]) -> Value {
    let mut columns = vec![json!("day")];
    columns.extend(series.iter().map(|(n, _)| json!(n)));
    let rows: Vec<Value> = days
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let mut row = vec![d.clone()];
            row.extend(series.iter().map(|(_, s)| s[i]["value"].clone()));
            Value::Array(row)
        })
        .collect();
    table(columns, rows)
}

fn since_date(days: i64) -> String {
    (util::utcnow().date() - Duration::days(days - 1)).format("%Y-%m-%d").to_string()
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// Egy statisztika: {metric, range, ..., table}. Érvénytelen metrika / időszak: AdminError.
pub fn stats(app: &App, metric: Option<&str>, range_value: Option<&str>) -> AdminResult<Value> {
    let Some(metric) = metric.filter(|m| METRICS.contains(m)) else {
        return Err(AdminError::field("Ismeretlen statisztika.", 400, "metric"));
    };
    let days = range_days(range_value)?;
    let mut result = json!({"metric": metric, "range": days});
    let extra = match metric {
        "registrations" => stat_registrations(app, days)?,
        "active" => stat_active(app, days)?,
        "retention" => stat_retention(app, days)?,
        "games" => stat_games(app, days)?,
        "bots" => stat_bots(app, days)?,
        "words" => stat_words(app, days)?,
        "challenges" => stat_challenges(app, days)?,
        "practice" => stat_practice(app, days)?,
        "review" => stat_review(app, days)?,
        _ => stat_heatmap(app, days)?,
    };
    if let (Some(base), Some(more)) = (result.as_object_mut(), extra.as_object()) {
        for (k, v) in more {
            base.insert(k.clone(), v.clone());
        }
    }
    Ok(result)
}

// ===== Regisztráció, aktivitás, megtartás =====

fn stat_registrations(app: &App, days: i64) -> AdminResult<Value> {
    let charts = charts(app, days)?;
    let total = txn(&app.db, |tx| Ok(fetch_one(tx, "SELECT COUNT(*) AS n FROM users WHERE deleted_at IS NULL", [])?.map(|r| r.int("n")).unwrap_or(0)))?;
    let day_list = charts["days"].as_array().cloned().unwrap_or_default();
    Ok(json!({
        "days": charts["days"], "series": charts["registrations"], "total_users": total,
        "table": series_table(&day_list, &[("registrations", &charts["registrations"])]),
    }))
}

fn activity_count(tx: &Transaction, since_days: i64) -> AdminResult<i64> {
    let since = (util::utcnow().date() - Duration::days(since_days - 1)).format("%Y-%m-%d").to_string();
    Ok(fetch_one(tx, &format!("SELECT COUNT(DISTINCT user_id) AS n FROM ({ACTIVITY_SQL}) WHERE day >= ?"), [since])?.map(|r| r.int("n")).unwrap_or(0))
}

fn stat_active(app: &App, days: i64) -> AdminResult<Value> {
    let charts = charts(app, days)?;
    let (dau, wau, mau) = txn(&app.db, |tx| Ok((activity_count(tx, 1)?, activity_count(tx, 7)?, activity_count(tx, 30)?)))?;
    let day_list = charts["days"].as_array().cloned().unwrap_or_default();
    Ok(json!({
        "days": charts["days"], "series": charts["active_users"], "dau": dau, "wau": wau, "mau": mau,
        "table": series_table(&day_list, &[("active_users", &charts["active_users"])]),
    }))
}

fn parse_day(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()
}

/// Az X. napon visszatérők aránya: a legalább X napja regisztrált (az időszakban regisztrált) felhasználók közül hányan
/// voltak aktívak pontosan a regisztráció utáni X. napon.
fn stat_retention(app: &App, days: i64) -> AdminResult<Value> {
    let today = util::utcnow().date();
    let since = (today - Duration::days(days)).format("%Y-%m-%d").to_string();
    let (users, active) = txn(&app.db, |tx| {
        let users: Vec<(i64, String)> =
            fetch_all(tx, "SELECT id, date(created_at) AS day FROM users WHERE date(created_at) >= ? AND deleted_at IS NULL", [&since])?
                .iter()
                .map(|r| (r.int("id"), r.text("day")))
                .collect();
        let active: HashSet<(i64, String)> = fetch_all(tx, &format!("SELECT user_id, day FROM ({ACTIVITY_SQL}) WHERE day >= ?"), [&since])?
            .iter()
            .map(|r| (r.int("user_id"), r.text("day")))
            .collect();
        Ok((users, active))
    })?;
    let mut rows: Vec<Value> = Vec::new();
    for n in RETENTION_DAYS {
        let eligible: Vec<&(i64, String)> = users.iter().filter(|(_, day)| parse_day(day).is_some_and(|d| (today - d).num_days() >= n)).collect();
        let returned = eligible
            .iter()
            .filter(|(uid, day)| parse_day(day).is_some_and(|d| active.contains(&(*uid, (d + Duration::days(n)).format("%Y-%m-%d").to_string()))))
            .count();
        let share = if eligible.is_empty() { Value::Null } else { json!(round1(returned as f64 / eligible.len() as f64 * 100.0)) };
        rows.push(json!({"day": n, "eligible": eligible.len(), "returned": returned, "share": share}));
    }
    let table_rows: Vec<Value> = rows.iter().map(|r| json!([r["day"], r["eligible"], r["returned"], r["share"]])).collect();
    Ok(json!({"retention": rows, "table": table(str_columns(&["day", "eligible", "returned", "share"]), table_rows)}))
}

// ===== Játékok =====

fn stat_games(app: &App, days: i64) -> AdminResult<Value> {
    let charts = charts(app, days)?;
    let day_list = charts["days"].as_array().cloned().unwrap_or_default();
    let since = day_list[0].as_str().unwrap_or("").to_string();
    let (types, daily_attempts, status, length, duration) = txn(&app.db, |tx| {
        let types = fetch_one(
            tx,
            "SELECT SUM(CASE WHEN is_async = 1 THEN 1 ELSE 0 END) AS async_games, \
             SUM(CASE WHEN is_async = 0 AND has_bots = 1 THEN 1 ELSE 0 END) AS bot_games, \
             SUM(CASE WHEN is_async = 0 AND has_bots = 0 THEN 1 ELSE 0 END) AS human_games \
             FROM saved_games WHERE status = 'finished' AND date(updated_at) >= ?",
            [&since],
        )?
        .unwrap_or_default();
        let daily_attempts =
            fetch_one(tx, "SELECT COALESCE(SUM(attempts), 0) AS n FROM daily_scores WHERE puzzle_date >= ?", [&since])?.map(|r| r.int("n")).unwrap_or(0);
        let status: HashMap<String, i64> =
            fetch_all(tx, "SELECT status, COUNT(*) AS n FROM saved_games WHERE date(created_at) >= ? GROUP BY status", [&since])?
                .iter()
                .map(|r| (r.text("status"), r.int("n")))
                .collect();
        let length = fetch_one(
            tx,
            "SELECT AVG(m.n) AS avg_moves FROM (SELECT COUNT(*) AS n FROM game_moves gm JOIN saved_games sg ON sg.id = gm.game_id \
             WHERE sg.status = 'finished' AND date(sg.updated_at) >= ? GROUP BY gm.game_id) m",
            [&since],
        )?
        .unwrap_or_default();
        let duration = fetch_one(
            tx,
            "SELECT AVG((julianday(updated_at) - julianday(created_at)) * 1440.0) AS minutes FROM saved_games \
             WHERE status = 'finished' AND is_async = 0 AND date(updated_at) >= ?",
            [&since],
        )?
        .unwrap_or_default();
        Ok((types, daily_attempts, status, length, duration))
    })?;
    let finished = status.get("finished").copied().unwrap_or(0);
    let abandoned = status.get("abandoned").copied().unwrap_or(0);
    let avg = |row: &Row, key: &str| row.get(key).and_then(|v| v.as_f64()).filter(|v| *v != 0.0).map(|v| json!(round1(v))).unwrap_or(Value::Null);
    let rows: Vec<Value> = day_list.iter().enumerate().map(|(i, d)| json!([d, charts["games_human"][i]["value"], charts["games_bots"][i]["value"]])).collect();
    Ok(json!({
        "days": charts["days"], "human": charts["games_human"], "bots": charts["games_bots"],
        "types": {"human": types.int("human_games"), "bots": types.int("bot_games"), "async": types.int("async_games"), "daily": daily_attempts},
        "avg_moves": avg(&length, "avg_moves"), "avg_minutes": avg(&duration, "minutes"),
        "finished": finished, "abandoned": abandoned,
        "completion_rate": if finished + abandoned > 0 { json!(round1(finished as f64 / (finished + abandoned) as f64 * 100.0)) } else { Value::Null },
        "table": table(str_columns(&["day", "human", "bots"]), rows),
    }))
}

/// A robot fokozatok népszerűsége és az emberek nyerési aránya fokozatonként (a kalibráláshoz: összevethető a
/// `LEVEL_STRENGTH`-szel).
fn stat_bots(app: &App, days: i64) -> AdminResult<Value> {
    let since = since_date(days);
    let (rows, winners) = txn(&app.db, |tx| {
        let rows = fetch_all(
            tx,
            "SELECT id, state_json FROM saved_games WHERE status = 'finished' AND has_bots = 1 AND date(updated_at) >= ? ORDER BY id DESC LIMIT 5000",
            [&since],
        )?;
        let mut winners: HashMap<i64, HashSet<String>> = HashMap::new();
        for r in fetch_all(
            tx,
            "SELECT game_id, player_name FROM game_players WHERE is_winner = 1 AND game_id IN \
             (SELECT id FROM saved_games WHERE status = 'finished' AND has_bots = 1 AND date(updated_at) >= ?)",
            [&since],
        )? {
            winners.entry(r.int("game_id")).or_default().insert(r.text("player_name"));
        }
        Ok((rows, winners))
    })?;
    let mut played: HashMap<String, i64> = HashMap::new();
    let mut human_wins: HashMap<String, i64> = HashMap::new();
    for row in &rows {
        let Ok(state) = serde_json::from_str::<Value>(&row.text("state_json")) else { continue };
        let players = state.get("players").and_then(|p| p.as_array()).cloned().unwrap_or_default();
        let humans: Vec<String> =
            players.iter().filter(|p| !p.get("is_bot").is_some_and(util::truthy)).filter_map(|p| p["name"].as_str().map(|s| s.to_string())).collect();
        let levels: HashSet<String> = players
            .iter()
            .filter(|p| p.get("is_bot").is_some_and(util::truthy))
            .map(|p| match p.get("difficulty") {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Null) | None => "None".to_string(),
                Some(other) => other.to_string(),
            })
            .collect();
        let empty = HashSet::new();
        let winning = winners.get(&row.int("id")).unwrap_or(&empty);
        let human_won = humans.iter().any(|n| winning.contains(n));
        for level in levels {
            *played.entry(level.clone()).or_insert(0) += 1;
            if human_won {
                *human_wins.entry(level).or_insert(0) += 1;
            }
        }
    }
    let mut order: Vec<String> = (crate::ai::MIN_LEVEL..=crate::ai::MAX_LEVEL).map(|n| n.to_string()).collect();
    order.push("auto".to_string());
    let mut items: Vec<Value> = Vec::new();
    for level in order {
        let games = played.get(&level).copied().unwrap_or(0);
        if games > 0 || level != "auto" {
            let wins = human_wins.get(&level).copied().unwrap_or(0);
            let strength = level.parse::<usize>().ok().and_then(|n| crate::ai::LEVEL_STRENGTH.get(n - 1)).map(|s| json!(s)).unwrap_or(Value::Null);
            items.push(json!({
                "level": level, "games": games, "human_wins": wins,
                "human_win_rate": if games > 0 { json!(round1(wins as f64 / games as f64 * 100.0)) } else { Value::Null },
                "strength": strength,
            }));
        }
    }
    let rows: Vec<Value> = items.iter().map(|i| json!([i["level"], i["games"], i["human_wins"], i["human_win_rate"], i["strength"]])).collect();
    Ok(json!({"levels": items, "table": table(str_columns(&["level", "games", "human_wins", "human_win_rate", "strength"]), rows)}))
}

// ===== Szavak, lépések, megtámadások =====

fn moves(tx: &Transaction, since: &str, kinds: &[&str]) -> AdminResult<Vec<Row>> {
    let marks = crate::db::placeholders(kinds.len());
    let mut params: Vec<rusqlite::types::Value> = vec![since.to_string().into()];
    params.extend(kinds.iter().map(|k| k.to_string().into()));
    params.push(MAX_MOVE_ROWS.into());
    Ok(fetch_all(
        tx,
        &format!(
            "SELECT m.game_id, m.player_name, m.action_type, m.details_json, m.created_at FROM game_moves m \
             WHERE date(m.created_at) >= ? AND m.action_type IN ({marks}) ORDER BY m.id DESC LIMIT ?"
        ),
        rusqlite::params_from_iter(params),
    )?)
}

fn details_of(raw: Option<String>) -> Value {
    raw.and_then(|r| serde_json::from_str::<Value>(&r).ok()).filter(|v| v.is_object()).unwrap_or_else(|| json!({}))
}

fn stat_words(app: &App, days: i64) -> AdminResult<Value> {
    let since = since_date(days);
    let mut words: Vec<(String, i64)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut best: Vec<(i64, i64, String, Value)> = Vec::new();
    let mut bingos = 0;
    txn(&app.db, |tx| {
        for row in moves(tx, &since, &["place", "challenge_accept"])? {
            let details = details_of(row.opt_text("details_json"));
            for word in details.get("words").and_then(|w| w.as_array()).into_iter().flatten() {
                let Some(word) = word.as_str() else { continue };
                match index.get(word) {
                    Some(i) => words[*i].1 += 1,
                    None => {
                        index.insert(word.to_string(), words.len());
                        words.push((word.to_string(), 1));
                    }
                }
            }
            if details.get("tiles").and_then(|t| t.as_array()).is_some_and(|t| t.len() >= 7) {
                bingos += 1;
            }
            if let Some(score) = details.get("score").and_then(|s| s.as_i64()).filter(|s| *s != 0) {
                best.push((score, row.int("game_id"), row.text("player_name"), details.get("words").cloned().unwrap_or(json!([]))));
            }
        }
        Ok(())
    })?;
    best.sort_by_key(|b| std::cmp::Reverse(b.0)); // stabil: egyenlő pontnál az eredeti sorrend marad
    words.sort_by_key(|w| std::cmp::Reverse(w.1));
    let top_words: Vec<Value> = words.iter().take(TOP_WORDS).map(|(w, n)| json!({"word": w, "count": n})).collect();
    let top_moves: Vec<Value> = best.iter().take(TOP_MOVES).map(|(s, g, p, w)| json!({"score": s, "game_id": g, "player": p, "words": w})).collect();
    let rows: Vec<Value> = top_words.iter().map(|w| json!([w["word"], w["count"]])).collect();
    Ok(json!({"top_words": top_words, "top_moves": top_moves, "bingos": bingos, "table": table(str_columns(&["word", "count"]), rows)}))
}

/// Megtámadásos játékok: hány lerakás ment át, hány lett elutasítva.
fn stat_challenges(app: &App, days: i64) -> AdminResult<Value> {
    let since = since_date(days);
    let (counts, games) = txn(&app.db, |tx| {
        let counts: HashMap<String, i64> = fetch_all(
            tx,
            "SELECT m.action_type, COUNT(*) AS n FROM game_moves m JOIN saved_games sg ON sg.id = m.game_id \
             WHERE sg.challenge_mode = 1 AND date(m.created_at) >= ? AND m.action_type IN ('challenge_accept', 'challenge_reject') GROUP BY m.action_type",
            [&since],
        )?
        .iter()
        .map(|r| (r.text("action_type"), r.int("n")))
        .collect();
        let games = fetch_one(tx, "SELECT COUNT(*) AS n FROM saved_games WHERE challenge_mode = 1 AND date(created_at) >= ?", [&since])?
            .map(|r| r.int("n"))
            .unwrap_or(0);
        Ok((counts, games))
    })?;
    let accepted = counts.get("challenge_accept").copied().unwrap_or(0);
    let rejected = counts.get("challenge_reject").copied().unwrap_or(0);
    let total = accepted + rejected;
    let rate = if total > 0 { json!(round1(rejected as f64 / total as f64 * 100.0)) } else { Value::Null };
    Ok(json!({
        "games": games, "accepted": accepted, "rejected": rejected, "rejection_rate": rate,
        "table": table(str_columns(&["accepted", "rejected", "rejection_rate"]), vec![json!([accepted, rejected, rate])]),
    }))
}

/// A gyakorló módok használata: a szerveroldali API hívások száma naponta (a számlálók az első használattól élnek).
fn stat_practice(app: &App, days: i64) -> AdminResult<Value> {
    let span = day_range(days);
    let marks = crate::db::placeholders(PRACTICE_KEYS.len());
    let mut params: Vec<rusqlite::types::Value> = vec![span[0].clone().into()];
    params.extend(PRACTICE_KEYS.iter().map(|k| k.to_string().into()));
    let rows = txn(&app.db, |tx| {
        Ok(fetch_all(tx, &format!("SELECT day, key, count FROM usage_counters WHERE day >= ? AND key IN ({marks})"), rusqlite::params_from_iter(params))?)
    })?;
    let mut data: HashMap<&str, HashMap<String, i64>> = PRACTICE_KEYS.iter().map(|k| (*k, span.iter().map(|d| (d.clone(), 0)).collect())).collect();
    for r in &rows {
        if let Some(per_day) = data.get_mut(r.text("key").as_str()) {
            per_day.insert(r.text("day"), r.int("count"));
        }
    }
    let series: Map<String, Value> =
        PRACTICE_KEYS.iter().map(|k| (k.to_string(), json!(span.iter().map(|d| json!({"day": d, "value": data[k][d]})).collect::<Vec<_>>()))).collect();
    let totals: Map<String, Value> = PRACTICE_KEYS.iter().map(|k| (k.to_string(), json!(data[k].values().sum::<i64>()))).collect();
    let mut columns = vec![json!("day")];
    columns.extend(PRACTICE_KEYS.iter().map(|k| json!(k)));
    let table_rows: Vec<Value> = span
        .iter()
        .map(|d| {
            let mut row = vec![json!(d)];
            row.extend(PRACTICE_KEYS.iter().map(|k| json!(data[k][d])));
            Value::Array(row)
        })
        .collect();
    Ok(json!({"days": span, "series": series, "totals": totals, "table": table(columns, table_rows)}))
}

fn stat_review(app: &App, days: i64) -> AdminResult<Value> {
    let span = day_range(days);
    let rows = txn(&app.db, |tx| {
        Ok(fetch_all(
            tx,
            "SELECT date(created_at) AS day, COUNT(*) AS decisions, COALESCE(SUM(1 - verdict), 0) AS rejections FROM word_reviews WHERE date(created_at) >= ? GROUP BY day",
            [&span[0]],
        )?)
    })?;
    let by_day: HashMap<String, &Row> = rows.iter().map(|r| (r.text("day"), r)).collect();
    let pick =
        |key: &str| -> Value { json!(span.iter().map(|d| json!({"day": d, "value": by_day.get(d).map(|r| r.int(key)).unwrap_or(0)})).collect::<Vec<_>>()) };
    let (decisions, rejections) = (pick("decisions"), pick("rejections"));
    let day_values: Vec<Value> = span.iter().map(|d| json!(d)).collect();
    Ok(json!({
        "days": span, "decisions": decisions, "rejections": rejections,
        "table": series_table(&day_values, &[("decisions", &decisions), ("rejections", &rejections)]),
    }))
}

/// Napszakos / heti aktivitás: a lépések ideje hét napja × óra szerint (magyar idő).
fn stat_heatmap(app: &App, days: i64) -> AdminResult<Value> {
    let since = since_date(days);
    let stamps = txn(&app.db, |tx| {
        let mut stamps: Vec<String> =
            fetch_all(tx, "SELECT created_at FROM game_moves WHERE date(created_at) >= ? ORDER BY id DESC LIMIT ?", rusqlite::params![&since, MAX_MOVE_ROWS])?
                .iter()
                .map(|r| r.text("created_at"))
                .collect();
        stamps.extend(
            fetch_all(tx, "SELECT created_at FROM login_events WHERE success = 1 AND date(created_at) >= ? LIMIT ?", rusqlite::params![&since, MAX_MOVE_ROWS])?
                .iter()
                .map(|r| r.text("created_at")),
        );
        Ok(stamps)
    })?;
    let mut grid = vec![vec![0i64; 24]; 7];
    for stamp in stamps {
        let Some(moment) = util::parse_ts(&stamp) else { continue };
        let local = moment + Duration::hours(crate::daily::budapest_offset_hours(moment));
        let weekday = chrono::Datelike::weekday(&local.date()).num_days_from_monday() as usize;
        grid[weekday][local.hour() as usize] += 1;
    }
    let max = grid.iter().map(|row| row.iter().copied().max().unwrap_or(0)).max().unwrap_or(0);
    let mut columns = vec![json!("weekday")];
    columns.extend((0..24).map(|h| json!(h.to_string())));
    let rows: Vec<Value> = grid
        .iter()
        .enumerate()
        .map(|(d, row)| {
            let mut cells = vec![json!(d)];
            cells.extend(row.iter().map(|c| json!(c)));
            Value::Array(cells)
        })
        .collect();
    Ok(json!({"grid": grid, "max": max, "timezone": "Europe/Budapest", "table": table(columns, rows)}))
}

// ===== Napi feladvány =====

const SCORE_BUCKETS: [(&str, f64); 5] = [("perfect", 1.0), ("high", 0.9), ("good", 0.75), ("half", 0.5), ("low", 0.0)];
const ATTEMPT_BUCKETS: [&str; 5] = ["1", "2", "3", "4-5", "6+"];

fn attempt_bucket(attempts: i64) -> &'static str {
    match attempts {
        i64::MIN..=1 => "1",
        2 => "2",
        3 => "3",
        4 | 5 => "4-5",
        _ => "6+",
    }
}

fn check_date(date_str: &str, allow_future: bool) -> AdminResult<String> {
    if !crate::daily::is_valid_date(date_str) || (!allow_future && date_str > crate::daily::today_str().as_str()) {
        return Err(AdminError::field("Érvénytelen dátum.", 400, "date"));
    }
    Ok(date_str.to_string())
}

fn puzzle_json(p: &crate::db::DailyPuzzle) -> Value {
    json!({"date": p.date, "board": p.board, "rack": p.rack, "best_score": p.best_score, "best": p.best})
}

/// Egy nap feladványa: tábla, kéz, legjobb lépés, résztvevők, próbálkozások és pontszámok eloszlása, ranglista.
pub fn daily_day(app: &App, date_str: Option<&str>) -> AdminResult<Value> {
    let date = check_date(&date_str.map(|d| d.to_string()).unwrap_or_else(crate::daily::today_str), false)?;
    let puzzle = app.db.get_daily_puzzle(&date);
    let rows = txn(&app.db, |tx| {
        Ok(fetch_all(
            tx,
            "SELECT ds.user_id, u.display_name, ds.best_score, ds.attempts, ds.revealed, ds.first_best_at \
             FROM daily_scores ds LEFT JOIN users u ON u.id = ds.user_id WHERE ds.puzzle_date = ? \
             ORDER BY ds.best_score DESC, ds.attempts ASC, ds.first_best_at ASC",
            [&date],
        )?)
    })?;
    let best = puzzle.as_ref().map(|p| p.best_score);
    let mut attempts: HashMap<&str, i64> = HashMap::new();
    let mut buckets: HashMap<&str, i64> = HashMap::new();
    for r in &rows {
        if r.int("attempts") > 0 {
            *attempts.entry(attempt_bucket(r.int("attempts"))).or_insert(0) += 1;
            if let Some(best) = best.filter(|b| *b != 0) {
                let ratio = r.int("best_score") as f64 / best as f64;
                let name = SCORE_BUCKETS.iter().find(|(_, floor)| ratio >= *floor).map(|(n, _)| *n).unwrap_or("low");
                *buckets.entry(name).or_insert(0) += 1;
            }
        }
    }
    let attempts_json: Map<String, Value> = ATTEMPT_BUCKETS.iter().map(|b| (b.to_string(), json!(attempts.get(b).copied().unwrap_or(0)))).collect();
    let scores_json: Map<String, Value> = SCORE_BUCKETS.iter().map(|(n, _)| (n.to_string(), json!(buckets.get(n).copied().unwrap_or(0)))).collect();
    Ok(json!({
        "date": date, "puzzle": puzzle.as_ref().map(puzzle_json),
        "participants": rows.iter().filter(|r| r.int("attempts") > 0).count(),
        "revealed": rows.iter().filter(|r| r.flag("revealed")).count(),
        "attempts": attempts_json, "scores": scores_json,
        "leaderboard": rows.iter().take(100).enumerate().map(|(i, r)| json!({
            "rank": i + 1, "user_id": r.get("user_id"), "display_name": r.get("display_name"),
            "best_score": r.int("best_score"), "attempts": r.int("attempts"), "revealed": r.flag("revealed"), "at": r.get("first_best_at"),
        })).collect::<Vec<_>>(),
    }))
}

/// A korábbi napok statisztikái: résztvevők, legjobb pont, átlag, próbálkozások.
pub fn daily_archive(app: &App, days: Option<&str>) -> AdminResult<Value> {
    let days = match days {
        None => 30,
        Some(d) => int_field(Some(&json!(d.trim().parse::<i64>().unwrap_or(-1))), "days", 1, 365)?,
    };
    let since = (util::utcnow().date() - Duration::days(days)).format("%Y-%m-%d").to_string();
    let rows = txn(&app.db, |tx| {
        Ok(fetch_all(
            tx,
            "SELECT p.puzzle_date AS date, p.best_score AS best_score, \
             COALESCE(SUM(CASE WHEN ds.attempts > 0 THEN 1 ELSE 0 END), 0) AS participants, \
             COALESCE(SUM(ds.attempts), 0) AS attempts, AVG(CASE WHEN ds.attempts > 0 THEN ds.best_score END) AS avg_score \
             FROM daily_puzzles p LEFT JOIN daily_scores ds ON ds.puzzle_date = p.puzzle_date \
             WHERE p.puzzle_date >= ? GROUP BY p.puzzle_date ORDER BY p.puzzle_date DESC",
            [&since],
        )?)
    })?;
    Ok(json!(
        rows.iter()
            .map(|r| json!({
                "date": r.text("date"), "best_score": r.int("best_score"), "participants": r.int("participants"), "attempts": r.int("attempts"),
                "avg_score": r.get("avg_score").and_then(|v| v.as_f64()).map(|v| json!(round1(v))).unwrap_or(Value::Null),
            }))
            .collect::<Vec<_>>()
    ))
}

/// Egy (akár jövőbeli) nap feladványa mentés nélkül, a nehézség becslésével (a legjobb pontszám). Másodpercekig
/// tarthat, ezért a hívó naponként egyet kér.
pub fn daily_preview(date_str: &str) -> AdminResult<Value> {
    let date = check_date(date_str, true)?;
    let puzzle = crate::daily::generate_puzzle(&date, &crate::ai::get_vocabulary()).map_err(|_| AdminError::new("A feladvány most nem állítható elő.", 503))?;
    Ok(json!({"date": date, "board": puzzle.board, "rack": puzzle.rack, "best_score": puzzle.best_score, "best": puzzle.best}))
}

/// Egy nap feladványának újragenerálása (pl. a szótár módosítása után). Ha már voltak próbálkozások, sudo kell, és a
/// ranglista torzulhat (`reset_scores`: a nap eredményei törlődnek).
pub fn daily_regenerate(
    app: &App,
    ctx: &AdminContext,
    date_str: &str,
    reason: Option<&Value>,
    reset_scores: Option<&Value>,
    sudo_ok: bool,
) -> AdminResult<Value> {
    let reset_scores = bool_field(Some(reset_scores.unwrap_or(&json!(false))), "reset_scores")?;
    let date = check_date(date_str, true)?;
    let attempts = txn(&app.db, |tx| {
        Ok(fetch_one(tx, "SELECT COUNT(*) AS n FROM daily_scores WHERE puzzle_date = ? AND attempts > 0", [&date])?.map(|r| r.int("n")).unwrap_or(0))
    })?;
    if attempts > 0 && !sudo_ok {
        return Err(super::routes::sudo_error().with("attempts", json!(attempts)));
    }
    let puzzle = crate::daily::generate_puzzle(&date, &crate::ai::get_vocabulary()).map_err(|_| AdminError::new("A feladvány most nem állítható elő.", 503))?;
    action(
        &app.db,
        ctx,
        "daily.regenerate",
        Some("daily"),
        Some(date.clone()),
        reason,
        json!({"attempts": attempts, "reset_scores": reset_scores}),
        true,
        |act| {
            let old = fetch_one(act.tx, "SELECT best_score FROM daily_puzzles WHERE puzzle_date = ?", [&date])?;
            act.tx.execute("DELETE FROM daily_puzzles WHERE puzzle_date = ?", [&date])?;
            act.tx.execute(
                "INSERT INTO daily_puzzles (puzzle_date, board_json, rack_json, best_score, best_json) VALUES (?, ?, ?, ?, ?)",
                rusqlite::params![
                    date,
                    puzzle.board.to_string(),
                    serde_json::to_string(&puzzle.rack).unwrap_or_default(),
                    puzzle.best_score,
                    puzzle.best.to_string()
                ],
            )?;
            if reset_scores {
                let deleted = act.tx.execute("DELETE FROM daily_scores WHERE puzzle_date = ?", [&date])?;
                act.details.insert("scores_deleted".into(), json!(deleted));
            }
            act.details.insert("before".into(), json!({"best_score": old.map(|o| o.int("best_score"))}));
            act.details.insert("after".into(), json!({"best_score": puzzle.best_score}));
            Ok(())
        },
    )?;
    Ok(json!({"date": date, "best_score": puzzle.best_score, "attempts": attempts}))
}

/// A ranglista-bejegyzés törlése (csalás esetén).
pub fn daily_delete_score(app: &App, ctx: &AdminContext, date_str: &str, user_id: i64, reason: Option<&Value>) -> AdminResult<()> {
    let date = check_date(date_str, false)?;
    action(&app.db, ctx, "daily.score_delete", Some("daily"), Some(date.clone()), reason, json!({}), true, |act| {
        let row = fetch_one(act.tx, "SELECT best_score, attempts FROM daily_scores WHERE puzzle_date = ? AND user_id = ?", rusqlite::params![date, user_id])?
            .ok_or_else(|| AdminError::new("A bejegyzés nem található.", 404))?;
        act.tx.execute("DELETE FROM daily_scores WHERE puzzle_date = ? AND user_id = ?", rusqlite::params![date, user_id])?;
        act.details.insert("user_id".into(), json!(user_id));
        act.details.insert("before".into(), json!({"best_score": row.int("best_score"), "attempts": row.int("attempts")}));
        Ok(())
    })
}

/// A `revealed` jelző visszaállítása (a felhasználó újra rangsorolt próbálkozhat).
pub fn daily_reset_revealed(app: &App, ctx: &AdminContext, date_str: &str, user_id: i64, reason: Option<&Value>) -> AdminResult<()> {
    let date = check_date(date_str, false)?;
    action(&app.db, ctx, "daily.revealed_reset", Some("daily"), Some(date.clone()), reason, json!({}), true, |act| {
        let row = fetch_one(act.tx, "SELECT revealed FROM daily_scores WHERE puzzle_date = ? AND user_id = ?", rusqlite::params![date, user_id])?;
        if !row.is_some_and(|r| r.flag("revealed")) {
            return Err(AdminError::new("A bejegyzésnél nincs megnézett megoldás.", 404));
        }
        act.tx.execute("UPDATE daily_scores SET revealed = 0 WHERE puzzle_date = ? AND user_id = ?", rusqlite::params![date, user_id])?;
        act.details.insert("user_id".into(), json!(user_id));
        Ok(())
    })
}
