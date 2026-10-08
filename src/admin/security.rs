//! Admin panel: biztonság — IP tiltás (a játékszerver őre is használja).

use crate::admin::candidates;
use crate::config::IpNet;
use crate::db::Db;
use crate::util;
use parking_lot::Mutex;
use std::collections::HashSet;
use std::net::IpAddr;

const CACHE_SECONDS: f64 = 30.0;

/// Az érvényes IP-tiltások gyorsítótára (a változás azonnal érvényesül: `invalidate`).
#[derive(Default)]
pub struct IpBans {
    cache: Mutex<Option<(f64, Vec<(IpNet, i64, String)>)>>,
}

impl IpBans {
    pub fn invalidate(&self) {
        *self.cache.lock() = None;
    }

    fn load(&self, db: &Db) -> Vec<(IpNet, i64, String)> {
        let now = util::now();
        let mut cache = self.cache.lock();
        if let Some((loaded, bans)) = cache.as_ref()
            && now - loaded < CACHE_SECONDS
        {
            return bans.clone();
        }
        let stamp = util::now_ts();
        let rows = db
            .with(|tx| crate::db::fetch_all(tx, "SELECT id, ip, reason, expires_at FROM ip_bans WHERE expires_at IS NULL OR expires_at > ?", [&stamp]))
            .unwrap_or_default(); // séma nélküli adatbázis: nincs tiltás
        let bans: Vec<(IpNet, i64, String)> = rows
            .iter()
            .filter_map(|r| {
                let net = IpNet::parse(r.get("ip")?.as_str()?)?;
                Some((net, r.get("id")?.as_i64()?, r.get("reason").and_then(|v| v.as_str()).unwrap_or("").to_string()))
            })
            .collect();
        *cache = Some((now, bans.clone()));
        bans
    }

    /// Igaz, ha a kliens IP-je egy érvényes tiltás alá esik.
    pub fn is_banned(&self, db: &Db, ip: &str) -> bool {
        let bans = self.load(db);
        if bans.is_empty() {
            return false;
        }
        let Ok(address) = ip.trim().parse::<IpAddr>() else { return false };
        let options = candidates(address);
        bans.iter().any(|(net, _, _)| options.iter().any(|c| net.contains(c)))
    }
}

// ===================================================================================================
// Biztonság (admin): IP tiltás, belépési napló, kódok, munkamenetek, forgalomkorlát
// ===================================================================================================

use crate::admin::{AdminContext, AdminError, AdminResult, action, like, page_args, parse_bound, parse_until, record, txn};
use crate::app::App;
use crate::db::{RowExt, fetch_all, fetch_one};
use crate::ratelimit::RateLimiter;
use rusqlite::Transaction;
use rusqlite::params_from_iter;
use rusqlite::types::Value as SqlValue;
use serde_json::{Value, json};
use std::collections::HashMap;

pub const FAILED_IP_THRESHOLD: i64 = 5; // ennyi sikertelen belépés / IP / 24 óra → gyanús
pub const FAILED_ACCOUNT_THRESHOLD: i64 = 3; // ennyi sikertelen belépés / fiók / 24 óra → gyanús
pub const MANY_ACCOUNTS_THRESHOLD: i64 = 3; // ennyi különböző fiók ugyanarról az IP-ről / 24 óra → gyanús
pub const BAN_REASON_MAX: usize = 300;
pub const LOGIN_CSV_FIELDS: [&str; 8] = ["id", "created_at", "success", "email", "user_id", "ip", "user_agent", "reason"];

/// IP-cím vagy CIDR → hálózat (AdminError, ha érvénytelen).
pub fn parse_network(text: Option<&Value>) -> AdminResult<IpNet> {
    text.and_then(|t| t.as_str()).and_then(|t| IpNet::parse(t.trim())).ok_or_else(|| AdminError::field("Érvénytelen IP-cím vagy hálózat.", 400, "ip"))
}

pub fn list_ip_bans(app: &App) -> AdminResult<Vec<Value>> {
    let stamp = util::now_ts();
    let rows = txn(&app.db, |tx| {
        Ok(fetch_all(tx, "SELECT b.*, u.display_name AS admin_name FROM ip_bans b LEFT JOIN users u ON u.id = b.created_by ORDER BY b.id DESC", [])?)
    })?;
    Ok(rows
        .iter()
        .map(|r| {
            let active = r.opt_text("expires_at").is_none_or(|e| e > stamp);
            json!({
                "id": r.int("id"), "ip": r.text("ip"), "reason": r.get("reason"), "expires_at": r.get("expires_at"), "created_at": r.text("created_at"),
                "created_by": r.get("created_by"), "admin_name": r.get("admin_name"), "active": active,
            })
        })
        .collect())
}

/// IP vagy hálózat tiltása (lejárattal vagy véglegesen). A saját IP-d nem tiltható ki.
pub fn add_ip_ban(app: &App, ctx: &AdminContext, ip: Option<&Value>, until: Option<&Value>, reason: Option<&Value>, own_ip: &str) -> AdminResult<i64> {
    let network = parse_network(ip)?;
    if let Ok(own) = own_ip.trim().parse::<IpAddr>()
        && own.is_ipv4() == network.addr.is_ipv4()
        && network.contains(&own)
    {
        return Err(AdminError::field("Saját IP-címedet nem tilthatod ki.", 409, "ip"));
    }
    let expires: Option<String> = match until {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s == "permanent" => None,
        other => Some(parse_until(other, false)?),
    };
    let id = action(&app.db, ctx, "security.ip_ban", Some("ip"), Some(network.to_string()), reason, json!({"until": expires}), true, |act| {
        let reason_text: String = crate::admin::normalize_reason(reason)?.chars().take(BAN_REASON_MAX).collect();
        act.tx.execute(
            "INSERT INTO ip_bans (ip, reason, expires_at, created_by) VALUES (?, ?, ?, ?)",
            rusqlite::params![network.to_string(), reason_text, expires, ctx.admin_user_id],
        )?;
        let id = act.tx.last_insert_rowid();
        act.details.insert("ban_id".into(), json!(id));
        Ok(id)
    })?;
    app.ip_bans.invalidate();
    Ok(id)
}

pub fn remove_ip_ban(app: &App, ctx: &AdminContext, ban_id: i64, reason: Option<&Value>) -> AdminResult<()> {
    action(&app.db, ctx, "security.ip_unban", Some("ip"), None, reason, json!({}), true, |act| {
        let row =
            fetch_one(act.tx, "SELECT ip, expires_at FROM ip_bans WHERE id = ?", [ban_id])?.ok_or_else(|| AdminError::new("A tiltás nem található.", 404))?;
        act.tx.execute("DELETE FROM ip_bans WHERE id = ?", [ban_id])?;
        act.details.insert("ip".into(), json!(row.text("ip")));
        act.details.insert("ban_id".into(), json!(ban_id));
        Ok(())
    })?;
    app.ip_bans.invalidate();
    Ok(())
}

// ===== Belépési napló =====

fn suspicious_sets(tx: &Transaction, since: &str) -> AdminResult<(HashSet<String>, HashSet<String>, HashSet<String>)> {
    let ips = fetch_all(
        tx,
        "SELECT ip FROM login_events WHERE success = 0 AND created_at >= ? AND ip IS NOT NULL GROUP BY ip HAVING COUNT(*) >= ?",
        rusqlite::params![since, FAILED_IP_THRESHOLD],
    )?
    .iter()
    .map(|r| r.text("ip"))
    .collect();
    let accounts = fetch_all(
        tx,
        "SELECT email FROM login_events WHERE success = 0 AND created_at >= ? AND email != '' GROUP BY email HAVING COUNT(*) >= ?",
        rusqlite::params![since, FAILED_ACCOUNT_THRESHOLD],
    )?
    .iter()
    .map(|r| r.text("email"))
    .collect();
    let spread = fetch_all(
        tx,
        "SELECT ip FROM login_events WHERE created_at >= ? AND ip IS NOT NULL AND email != '' GROUP BY ip HAVING COUNT(DISTINCT email) >= ?",
        rusqlite::params![since, MANY_ACCOUNTS_THRESHOLD],
    )?
    .iter()
    .map(|r| r.text("ip"))
    .collect();
    Ok((ips, accounts, spread))
}

fn sorted(set: &HashSet<String>) -> Vec<&String> {
    let mut list: Vec<&String> = set.iter().collect();
    list.sort();
    list
}

/// Belépési kísérletek szűrve (user / ip / success / since / until / q) és lapozva; a gyanús mintákat (sok sikertelen
/// próbálkozás egy IP-ről / fiókra, egy IP sok fiókkal) a sorok jelölik.
pub fn list_logins(app: &App, args: &HashMap<String, String>, paging: Option<(i64, i64)>) -> AdminResult<Value> {
    let mut where_: Vec<String> = Vec::new();
    let mut params: Vec<SqlValue> = Vec::new();
    let arg = |k: &str| args.get(k).map(|s| s.as_str()).filter(|s| !s.is_empty());
    let user = args.get("user").map(|u| u.trim()).unwrap_or("");
    if !user.is_empty() {
        if user.chars().all(|c| c.is_ascii_digit()) {
            where_.push("e.user_id = ?".into());
            params.push(SqlValue::Integer(user.parse().unwrap_or(0)));
        } else {
            where_.push("e.email LIKE ? ESCAPE '\\'".into());
            params.push(SqlValue::Text(like(&user.to_lowercase())));
        }
    }
    let ip = args.get("ip").map(|i| i.trim()).unwrap_or("");
    if !ip.is_empty() {
        where_.push("e.ip LIKE ? ESCAPE '\\'".into());
        params.push(SqlValue::Text(like(ip)));
    }
    if let Some(v @ ("0" | "1")) = arg("success") {
        where_.push("e.success = ?".into());
        params.push(SqlValue::Integer(v.parse().unwrap_or(0)));
    }
    if let Some(since) = arg("since") {
        where_.push("e.created_at >= ?".into());
        params.push(SqlValue::Text(parse_bound(since, false)?.0));
    }
    if let Some(until) = arg("until") {
        let (stamp, exclusive) = parse_bound(until, true)?;
        where_.push(if exclusive { "e.created_at < ?" } else { "e.created_at <= ?" }.into());
        params.push(SqlValue::Text(stamp));
    }
    let q = args.get("q").map(|q| q.trim()).unwrap_or("");
    if !q.is_empty() {
        where_.push("(e.email LIKE ? ESCAPE '\\' OR e.ip LIKE ? ESCAPE '\\' OR e.user_agent LIKE ? ESCAPE '\\' OR e.reason LIKE ? ESCAPE '\\')".into());
        for _ in 0..4 {
            params.push(SqlValue::Text(like(q)));
        }
    }
    let (limit, offset) = paging.unwrap_or_else(|| page_args(args, 50, 200));
    let clause = if where_.is_empty() { String::new() } else { format!("WHERE {}", where_.join(" AND ")) };
    let since = util::format_ts(util::utcnow() - chrono::Duration::hours(24));
    let ((ips, accounts, spread), total, rows) = txn(&app.db, |tx| {
        let sets = suspicious_sets(tx, &since)?;
        let total =
            fetch_one(tx, &format!("SELECT COUNT(*) AS n FROM login_events e {clause}"), params_from_iter(params.clone()))?.map(|r| r.int("n")).unwrap_or(0);
        let mut page = params.clone();
        page.push(SqlValue::Integer(limit));
        page.push(SqlValue::Integer(offset));
        let rows = fetch_all(
            tx,
            &format!(
                "SELECT e.*, u.display_name AS display_name FROM login_events e LEFT JOIN users u ON u.id = e.user_id {clause} ORDER BY e.id DESC LIMIT ? OFFSET ?"
            ),
            params_from_iter(page),
        )?;
        Ok((sets, total, rows))
    })?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            let success = r.flag("success");
            let mut flags: Vec<&str> = Vec::new();
            if !success && r.opt_text("ip").is_some_and(|i| ips.contains(&i)) {
                flags.push("ip_failures");
            }
            if !success && accounts.contains(&r.text("email")) {
                flags.push("account_failures");
            }
            if r.opt_text("ip").is_some_and(|i| spread.contains(&i)) {
                flags.push("many_accounts");
            }
            json!({
                "id": r.int("id"), "user_id": r.get("user_id"), "display_name": r.get("display_name"), "email": r.text("email"), "ip": r.get("ip"),
                "user_agent": r.text("user_agent").chars().take(160).collect::<String>(), "success": success, "reason": r.get("reason"),
                "created_at": r.text("created_at"), "flags": flags,
            })
        })
        .collect();
    Ok(json!({
        "items": items, "total": total, "limit": limit, "offset": offset,
        "suspicious": {"ips": sorted(&ips), "accounts": sorted(&accounts), "spread_ips": sorted(&spread)},
    }))
}

// ===== Ellenőrző kódok =====

/// A függő regisztrációs kódok (SMTP nélkül itt látszik a kód). A megtekintés naplózva: `view.codes`.
pub fn list_codes(app: &App, ctx: &AdminContext) -> AdminResult<Vec<Value>> {
    let stamp = util::now_ts();
    let rows = txn(&app.db, |tx| {
        let rows = fetch_all(
            tx,
            "SELECT id, email, code, created_at, expires_at, attempts FROM verification_codes WHERE used = 0 AND expires_at > ? ORDER BY id DESC",
            [&stamp],
        )?;
        record(tx, ctx, "view.codes", Some("verification"), None, &json!({"count": rows.len()}))?;
        Ok(rows)
    })?;
    Ok(rows.into_iter().map(Value::Object).collect())
}

pub fn invalidate_code(app: &App, ctx: &AdminContext, code_id: i64, reason: Option<&Value>) -> AdminResult<()> {
    action(&app.db, ctx, "security.code_invalidate", Some("verification"), Some(code_id.to_string()), reason, json!({}), true, |act| {
        let row = fetch_one(act.tx, "SELECT email FROM verification_codes WHERE id = ? AND used = 0", [code_id])?
            .ok_or_else(|| AdminError::new("A kód nem található.", 404))?;
        act.tx.execute("UPDATE verification_codes SET used = 1 WHERE id = ?", [code_id])?;
        act.details.insert("email".into(), json!(row.text("email")));
        Ok(())
    })
}

// ===== Munkamenetek =====

fn session_rows(app: &App, tx: &Transaction, where_: &str, params: Vec<SqlValue>, token: Option<&str>) -> AdminResult<Vec<Value>> {
    let mut all: Vec<SqlValue> = vec![SqlValue::Text(token.unwrap_or("").to_string())];
    all.extend(params);
    all.push(SqlValue::Integer(500));
    let rows = fetch_all(
        tx,
        &format!(
            "SELECT s.id, s.user_id, s.created_at, s.expires_at, s.ip, s.user_agent, s.last_seen, s.admin_seen_at, (s.token = ?) AS current, u.display_name, u.email_lower \
             FROM sessions s JOIN users u ON u.id = s.user_id {where_} ORDER BY COALESCE(s.last_seen, s.created_at) DESC LIMIT ?"
        ),
        params_from_iter(all),
    )?;
    Ok(rows
        .iter()
        .map(|r| {
            json!({
                "id": r.int("id"), "user_id": r.int("user_id"), "display_name": r.text("display_name"), "ip": r.get("ip"),
                "user_agent": r.text("user_agent").chars().take(160).collect::<String>(), "created_at": r.text("created_at"),
                "last_seen": r.get("last_seen"), "expires_at": r.text("expires_at"), "current": r.flag("current"),
                "is_admin": app.config.admin_emails.contains(&r.text("email_lower")),
            })
        })
        .collect())
}

/// Az aktív munkamenetek (a tokenek nélkül): q (név / e-mail / IP), `admin=1`: csak az adminoké.
pub fn list_sessions(app: &App, args: &HashMap<String, String>, token: Option<&str>) -> AdminResult<Vec<Value>> {
    let mut where_ = vec!["s.expires_at > ?".to_string()];
    let mut params: Vec<SqlValue> = vec![SqlValue::Text(util::now_ts())];
    let q = args.get("q").map(|q| q.trim()).unwrap_or("");
    if !q.is_empty() {
        where_.push("(u.display_name LIKE ? ESCAPE '\\' OR u.email_lower LIKE ? ESCAPE '\\' OR s.ip LIKE ? ESCAPE '\\')".into());
        for _ in 0..3 {
            params.push(SqlValue::Text(like(q)));
        }
    }
    if matches!(args.get("admin").map(|s| s.as_str()), Some("1") | Some("true")) {
        let mut emails: Vec<String> = app.config.admin_emails.iter().cloned().collect();
        emails.sort();
        where_.push(if emails.is_empty() { "0".to_string() } else { format!("u.email_lower IN ({})", crate::db::placeholders(emails.len())) });
        params.extend(emails.into_iter().map(SqlValue::Text));
    }
    txn(&app.db, |tx| session_rows(app, tx, &format!("WHERE {}", where_.join(" AND ")), params, token))
}

/// Egy munkamenet érvénytelenítése. A saját (aktuális) munkameneted nem.
pub fn revoke_session(app: &App, ctx: &AdminContext, session_id: i64, reason: Option<&Value>, token: Option<&str>) -> AdminResult<i64> {
    action(&app.db, ctx, "security.session_revoke", Some("session"), Some(session_id.to_string()), reason, json!({}), true, |act| {
        let row = fetch_one(act.tx, "SELECT user_id, token FROM sessions WHERE id = ?", [session_id])?
            .ok_or_else(|| AdminError::new("A munkamenet nem található.", 404))?;
        if token.is_some_and(|t| !t.is_empty() && t == row.text("token")) {
            return Err(AdminError::new("A saját munkameneted nem zárható be.", 409));
        }
        act.tx.execute("DELETE FROM sessions WHERE id = ?", [session_id])?;
        act.details.insert("user_id".into(), json!(row.int("user_id")));
        Ok(row.int("user_id"))
    })
}

/// Mindenki kijelentkeztetése (pl. a SECRET_KEY cseréje után): minden munkamenet törlése a sajátodon kívül. Visszatér:
/// a törölt munkamenetek száma.
pub fn revoke_all_sessions(app: &App, ctx: &AdminContext, keep_token: Option<&str>, reason: Option<&Value>) -> AdminResult<usize> {
    action(&app.db, ctx, "security.sessions_revoke_all", Some("session"), None, reason, json!({}), true, |act| {
        let removed = act.tx.execute("DELETE FROM sessions WHERE token != ?", [keep_token.unwrap_or("")])?;
        act.details.insert("removed".into(), json!(removed));
        Ok(removed)
    })
}

/// Az adminok minden más munkamenetének bezárása (a sajátodon kívül).
pub fn close_other_admin_sessions(app: &App, ctx: &AdminContext, keep_token: Option<&str>, reason: Option<&Value>) -> AdminResult<usize> {
    let mut emails: Vec<String> = app.config.admin_emails.iter().cloned().collect();
    emails.sort();
    if emails.is_empty() {
        return Ok(0);
    }
    action(&app.db, ctx, "security.admin_sessions_close", Some("session"), None, reason, json!({}), true, |act| {
        let mut params: Vec<SqlValue> = vec![SqlValue::Text(keep_token.unwrap_or("").to_string())];
        params.extend(emails.iter().cloned().map(SqlValue::Text));
        let removed = act.tx.execute(
            &format!(
                "DELETE FROM sessions WHERE token != ? AND user_id IN (SELECT id FROM users WHERE email_lower IN ({}))",
                crate::db::placeholders(emails.len())
            ),
            params_from_iter(params),
        )?;
        act.details.insert("removed".into(), json!(removed));
        Ok(removed)
    })
}

// ===== Forgalomkorlát =====

pub fn rate_limit_state(limiter: &RateLimiter) -> Value {
    json!({"ips": limiter.limited_ips(), "sids": limiter.limited_sids(), "limits": limiter.effective_limits()})
}

pub fn unblock_ip(app: &App, ctx: &AdminContext, ip: Option<&Value>, reason: Option<&Value>) -> AdminResult<()> {
    let address = ip.and_then(|i| i.as_str()).map(|i| i.trim()).unwrap_or("");
    if address.is_empty() {
        return Err(AdminError::field("Érvénytelen IP-cím vagy hálózat.", 400, "ip"));
    }
    if !app.limiter.has_ip(address) {
        return Err(AdminError::new("Ehhez az IP-hez nincs forgalomkorlát-előzmény.", 404));
    }
    action(&app.db, ctx, "security.rate_unblock", Some("ip"), Some(address.to_string()), reason, json!({}), true, |_| Ok(()))?;
    app.limiter.clear_ip(address);
    Ok(())
}
