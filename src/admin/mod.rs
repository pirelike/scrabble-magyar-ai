//! Admin panel logika: napló (audit), admin munkamenet, közös segédek és az admin almodulok.
//!
//! Alapelvek (lásd `docs/ADMIN_PANEL.md`):
//!  * minden módosító admin művelet naplózva van, kötelező indoklással, a művelettel EGY tranzakcióban: ha a napló
//!    nem írható, a művelet sem történik meg (`action`);
//!  * a napló csak hozzáfűzhető: a táblán adatbázis-trigger tiltja a módosítást és a törlést;
//!  * a személyes adat megtekintése is naplózódik (`view.*`), indoklás nélkül (`record`).

pub mod api;
pub mod audit;
pub mod comm;
pub mod dict;
pub mod games;
pub mod live;
pub mod mail;
pub mod moderation;
pub mod routes;
pub mod security;
pub mod stats;
pub mod session;
pub mod system;
pub mod update;
pub mod users;

use crate::config::IpNet;
use crate::db::Db;
use crate::util;
use rusqlite::Transaction;
use serde_json::{Map, Value, json};
use std::net::IpAddr;
use unicode_normalization::UnicodeNormalization;

pub const MIN_REASON_LEN: usize = 3;
pub const MAX_REASON_LEN: usize = 500;
pub const MAX_USER_AGENT_LEN: usize = 300;
/// A tétlenségi időzítő frissítése nem minden kérésnél ír az adatbázisba, csak ennyi másodpercenként.
pub const TOUCH_INTERVAL_SECONDS: i64 = 15;

pub const AUDIT_DEFAULT_LIMIT: i64 = 50;
pub const AUDIT_MAX_LIMIT: i64 = 200;
pub const AUDIT_EXPORT_MAX: i64 = 10_000;
pub const AUDIT_FILTER_MAX_LEN: usize = 100;
pub const AUDIT_CSV_FIELDS: [&str; 11] =
    ["id", "created_at", "admin_user_id", "admin_name", "action", "target_type", "target_id", "ip", "user_agent", "reason", "details_json"];

/// Az admin művelet nem hajtható végre; a HTTP réteg `status` kóddal és magyar üzenettel válaszol.
#[derive(Debug, Clone)]
pub struct AdminError {
    pub message: String,
    pub status: u16,
    pub extra: Map<String, Value>,
}

impl AdminError {
    pub fn new(message: &str, status: u16) -> AdminError {
        AdminError { message: message.to_string(), status, extra: Map::new() }
    }

    /// Hibás mező megjelölése (a kliens kiemeli).
    pub fn field(message: &str, status: u16, field: &str) -> AdminError {
        let mut e = AdminError::new(message, status);
        e.extra.insert("field".into(), json!(field));
        e
    }

    pub fn with(mut self, key: &str, value: Value) -> AdminError {
        self.extra.insert(key.to_string(), value);
        self
    }
}

impl std::fmt::Display for AdminError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<rusqlite::Error> for AdminError {
    fn from(error: rusqlite::Error) -> AdminError {
        println!("[admin] Adatbázis-hiba: {error}");
        AdminError::new("Adatbázis-hiba.", 500)
    }
}

pub type AdminResult<T> = Result<T, AdminError>;

/// Ki és honnan végzi a műveletet (a napló sorába kerül).
#[derive(Clone, Debug)]
pub struct AdminContext {
    pub admin_user_id: i64,
    pub ip: String,
    pub user_agent: String,
}

// ===== IP-engedélylista =====

/// Igaz, ha a kliens IP-je szerepel az engedélylistán. Üres lista → bármelyik IP engedélyezett.
pub fn ip_allowed(ip: &str, networks: &[IpNet]) -> bool {
    if networks.is_empty() {
        return true;
    }
    let Ok(address) = ip.trim().parse::<IpAddr>() else { return false };
    candidates(address).iter().any(|c| networks.iter().any(|n| n.contains(c)))
}

/// Az IPv4-be képezett IPv6 cím (::ffff:1.2.3.4) az IPv4 hálózatokra is illeszkedjen.
pub fn candidates(address: IpAddr) -> Vec<IpAddr> {
    let mut list = vec![address];
    if let IpAddr::V6(v6) = address {
        if let Some(v4) = v6.to_ipv4_mapped() {
            list.push(IpAddr::V4(v4));
        }
    }
    list
}

// ===== Napló =====

/// Az indoklás tisztítása és ellenőrzése. Módosító admin műveletnél kötelező.
pub fn normalize_reason(reason: Option<&Value>) -> AdminResult<String> {
    let text = reason.and_then(|r| r.as_str()).map(|s| s.trim()).unwrap_or("");
    let n = text.chars().count();
    if n < MIN_REASON_LEN {
        return Err(AdminError::field("Az indoklás megadása kötelező (legalább 3 karakter).", 400, "reason"));
    }
    if n > MAX_REASON_LEN {
        return Err(AdminError::field("Az indoklás legfeljebb 500 karakter lehet.", 400, "reason"));
    }
    Ok(text.to_string())
}

/// Egy naplósor beírása a megadott tranzakcióban. Visszaadja a sor id-ját.
pub fn record(tx: &Transaction, ctx: &AdminContext, action: &str, target_type: Option<&str>, target_id: Option<&str>, details: &Value) -> rusqlite::Result<i64> {
    let details_json = match details {
        Value::Null => None,
        Value::Object(o) if o.is_empty() => None,
        other => Some(other.to_string()),
    };
    let agent: String = ctx.user_agent.chars().take(MAX_USER_AGENT_LEN).collect();
    tx.execute(
        "INSERT INTO admin_audit (admin_user_id, action, target_type, target_id, details_json, ip, user_agent) VALUES (?, ?, ?, ?, ?, ?, ?)",
        rusqlite::params![
            ctx.admin_user_id,
            action,
            target_type,
            target_id,
            details_json,
            if ctx.ip.is_empty() { None } else { Some(ctx.ip.as_str()) },
            if agent.is_empty() { None } else { Some(agent.as_str()) },
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

/// Az `action` blokk kezelője: a nyitott tranzakció és a naplóba kerülő részletek (előtte / utána...).
pub struct Act<'a> {
    pub tx: &'a Transaction<'a>,
    pub details: Map<String, Value>,
}

/// Módosító admin művelet naplózva, EGY tranzakcióban.
///
/// Indoklás nélkül (túl rövid) a művelet el sem indul. A naplósor a blokk végén íródik be ugyanabba a
/// tranzakcióba, ezért ha a blokk vagy a naplózás hibázik, minden visszagördül. A memóriabeli mellékhatásokat
/// (socket bontás, szoba állapota) a blokk UTÁN kell elvégezni.
///
/// `require_reason` hamis: az olyan műveleteknél, ahol a tartalom maga az indoklás (közlemény, push, e-mail): az
/// indoklás elhagyható, de ha van, kötelezően ellenőrzött és naplózott.
pub fn action<T>(
    db: &Db,
    ctx: &AdminContext,
    name: &str,
    target_type: Option<&str>,
    target_id: Option<String>,
    reason: Option<&Value>,
    details: Value,
    require_reason: bool,
    body: impl FnOnce(&mut Act) -> AdminResult<T>,
) -> AdminResult<T> {
    let has_reason = reason.is_some_and(|r| !r.is_null() && r.as_str().map(|s| !s.is_empty()).unwrap_or(true));
    let clean_reason = if require_reason || has_reason { Some(normalize_reason(reason)?) } else { None };
    let mut act_details = Map::new();
    if let Some(r) = &clean_reason {
        act_details.insert("reason".into(), json!(r));
    }
    if let Value::Object(extra) = details {
        for (k, v) in extra {
            if k != "reason" {
                act_details.insert(k, v);
            }
        }
    }
    let mut conn = db.raw();
    let tx = conn.transaction()?;
    let result = {
        let mut act = Act { tx: &tx, details: act_details };
        let result = body(&mut act)?; // hiba esetén a tx eldobódik: visszagörgetés
        record(&tx, ctx, name, target_type, target_id.as_deref(), &Value::Object(act.details))?;
        result
    };
    tx.commit()?;
    Ok(result)
}

// ===== Közös segédek a listákhoz és az időtartamokhoz =====

/// Kis-nagybetű és ékezet nélküli alak a kereséshez (a magyar ő / ű is: o / u).
pub fn fold(text: &str) -> String {
    text.to_lowercase().nfd().filter(|c| !unicode_normalization::char::is_combining_mark(*c)).collect()
}

/// A `py_fold` SQL függvény a kapcsolaton: ékezet- és kisbetű-független LIKE keresésekhez.
pub fn register_sql_helpers(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    use rusqlite::functions::FunctionFlags;
    conn.create_scalar_function("py_fold", 1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |ctx| {
        let value: Option<String> = ctx.get(0)?;
        Ok(value.map(|v| fold(&v)))
    })
}

/// LIKE minta a (kisbetűs, ékezet nélküli) szöveg tartalmazására; ESCAPE '\'.
pub fn like_contains(text: &str) -> String {
    like(&fold(text))
}

/// LIKE minta a szöveg tartalmazására (a joker karakterek elmenekítve; ESCAPE '\').
pub fn like(text: &str) -> String {
    format!("%{}%", crate::db::escape_like(text))
}

pub fn prefix_like(text: &str) -> String {
    format!("{}%", crate::db::escape_like(text))
}

/// `limit` / `offset` a kérésből, korlátok között.
pub fn page_args(args: &std::collections::HashMap<String, String>, default: i64, maximum: i64) -> (i64, i64) {
    (int_arg(args.get("limit").map(|s| s.as_str()), default, 1, maximum), int_arg(args.get("offset").map(|s| s.as_str()), 0, 0, 1_000_000_000))
}

pub fn int_arg(value: Option<&str>, default: i64, low: i64, high: i64) -> i64 {
    match value.and_then(|v| v.trim().parse::<i64>().ok()) {
        Some(n) => n.clamp(low, high),
        None => default,
    }
}

/// ORDER BY rész egy fehérlistából (a rendezés oszlopa sosem jön közvetlenül a felhasználótól).
/// `allowed`: (kulcs, SQL-kifejezés); az `order` 'asc' / 'desc'. Visszatér: 'kifejezés ASC|DESC'.
pub fn order_by(sort: Option<&str>, order: Option<&str>, allowed: &[(&str, &str)], default: &str) -> String {
    let column = sort
        .and_then(|s| allowed.iter().find(|(k, _)| *k == s))
        .or_else(|| allowed.iter().find(|(k, _)| *k == default))
        .map(|(_, v)| *v)
        .unwrap_or("1");
    let direction = if order.map(|o| o.to_lowercase()) == Some("asc".to_string()) { "ASC" } else { "DESC" };
    format!("{column} {direction}")
}

/// Dátum (ÉÉÉÉ-HH-NN) vagy időpont (ÉÉÉÉ-HH-NN ÓÓ:PP[:MM]) → adatbázis-időbélyeg. `end`: a nap végéig.
/// Visszatér: (időbélyeg, kizáró-e a felső határ: a nap vége a következő nap kezdetéig).
pub fn parse_bound(value: &str, end: bool) -> AdminResult<(String, bool)> {
    use chrono::{NaiveDate, NaiveDateTime};
    let text = value.trim().replace('T', " ");
    if let Ok(date) = NaiveDate::parse_from_str(&text, "%Y-%m-%d") {
        let mut moment = date.and_hms_opt(0, 0, 0).unwrap();
        if end {
            moment += chrono::Duration::days(1); // a nap vége: a következő nap kezdetéig (kizárólag)
        }
        return Ok((util::format_ts(moment), end));
    }
    for fmt in ["%Y-%m-%d %H:%M", "%Y-%m-%d %H:%M:%S"] {
        if let Ok(moment) = NaiveDateTime::parse_from_str(&text, fmt) {
            return Ok((util::format_ts(moment), false));
        }
    }
    Err(AdminError::new("Érvénytelen dátum (ÉÉÉÉ-HH-NN formátum kell).", 400))
}

/// Lejárat megadása: relatív érték (1h / 1d / 7d / 30d), pontos időpont, vagy None / 'permanent' (végleges).
/// Visszatér: adatbázis-időbélyeg (a végleges a `PERMANENT_UNTIL`).
pub fn parse_until(value: Option<&Value>, permanent_ok: bool) -> AdminResult<String> {
    let now = util::utcnow();
    let permanent = match value {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) if s == "permanent" => true,
        _ => false,
    };
    if permanent {
        if !permanent_ok {
            return Err(AdminError::field("Érvénytelen időtartam.", 400, "until"));
        }
        return Ok(crate::db::PERMANENT_UNTIL.to_string());
    }
    let Some(text) = value.and_then(|v| v.as_str()) else { return Err(AdminError::field("Érvénytelen időtartam.", 400, "until")) };
    let relative = match text {
        "1h" => Some(chrono::Duration::hours(1)),
        "1d" => Some(chrono::Duration::days(1)),
        "7d" => Some(chrono::Duration::days(7)),
        "30d" => Some(chrono::Duration::days(30)),
        _ => None,
    };
    if let Some(delta) = relative {
        return Ok(util::format_ts(now + delta));
    }
    let (stamp, _) = parse_bound(text, false)?;
    if util::parse_ts(&stamp).is_some_and(|s| s <= now) {
        return Err(AdminError::field("A lejárat nem lehet a múltban.", 400, "until"));
    }
    Ok(stamp)
}

/// Szöveges mező tisztítása (szóközök levágva, hossz ellenőrizve).
pub fn clean_text(value: Option<&Value>, field: &str, maximum: usize, required: bool) -> AdminResult<String> {
    let text = value.and_then(|v| v.as_str()).map(|s| s.trim()).unwrap_or("");
    if required && text.is_empty() {
        return Err(AdminError::field("A mező kitöltése kötelező.", 400, field));
    }
    if text.chars().count() > maximum {
        return Err(AdminError::field("A szöveg túl hosszú.", 400, field));
    }
    Ok(text.to_string())
}

pub fn bool_field(value: Option<&Value>, field: &str) -> AdminResult<bool> {
    match value {
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err(AdminError::field("Érvénytelen kérés.", 400, field)),
    }
}

pub fn int_field(value: Option<&Value>, field: &str, low: i64, high: i64) -> AdminResult<i64> {
    match value.and_then(util::strict_int) {
        Some(n) if (low..=high).contains(&n) => Ok(n),
        _ => Err(AdminError::field("Érvénytelen szám.", 400, field)),
    }
}

/// Táblázatkezelőben megnyitva ne fusson le képlet: a `=`, `+`, `-`, `@` kezdetű cellát idézőjel előzi meg.
pub fn csv_cell(value: &Value) -> String {
    let text = match value {
        Value::Null => return String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if text.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{text}") } else { text }
}

/// CSV sor szövege (RFC 4180 szerinti idézés, CRLF sorvég).
pub fn csv_line(cells: &[String]) -> String {
    let quoted: Vec<String> = cells
        .iter()
        .map(|c| if c.contains([',', '"', '\n', '\r']) { format!("\"{}\"", c.replace('"', "\"\"")) } else { c.clone() })
        .collect();
    format!("{}\r\n", quoted.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_removes_accents_and_case() {
        assert_eq!(fold("ÁRVÍZTŰRŐ Tükörfúrógép"), "arvizturo tukorfurogep");
        assert_eq!(fold("Őz"), "oz");
    }

    #[test]
    fn reason_is_required() {
        assert!(normalize_reason(None).is_err());
        assert!(normalize_reason(Some(&json!("ab"))).is_err());
        assert!(normalize_reason(Some(&json!(5))).is_err());
        assert_eq!(normalize_reason(Some(&json!("  rendben  "))).unwrap(), "rendben");
        assert!(normalize_reason(Some(&json!("x".repeat(501)))).is_err());
    }

    #[test]
    fn csv_cells_are_safe() {
        assert_eq!(csv_cell(&json!("=SUM(A1)")), "'=SUM(A1)");
        assert_eq!(csv_cell(&json!("-1")), "'-1");
        assert_eq!(csv_cell(&Value::Null), "");
        assert_eq!(csv_cell(&json!(5)), "5");
        assert_eq!(csv_line(&["a,b".into(), "c\"d".into(), "e".into()]), "\"a,b\",\"c\"\"d\",e\r\n");
    }

    #[test]
    fn bounds_and_until() {
        assert_eq!(parse_bound("2026-10-05", false).unwrap().0, "2026-10-05 00:00:00");
        assert_eq!(parse_bound("2026-10-05", true).unwrap(), ("2026-10-06 00:00:00".to_string(), true));
        assert_eq!(parse_bound("2026-10-05T12:30", false).unwrap(), ("2026-10-05 12:30:00".to_string(), false));
        assert!(parse_bound("nem dátum", false).is_err());
        assert_eq!(parse_until(None, true).unwrap(), "9999-12-31 23:59:59");
        assert!(parse_until(None, false).is_err());
        assert!(parse_until(Some(&json!("2000-01-01")), true).is_err());
        assert!(parse_until(Some(&json!("1d")), true).is_ok());
        assert!(parse_until(Some(&json!(5)), true).is_err());
    }

    #[test]
    fn order_by_uses_whitelist() {
        let allowed = [("name", "u.display_name"), ("id", "u.id")];
        assert_eq!(order_by(Some("name"), Some("asc"), &allowed, "id"), "u.display_name ASC");
        assert_eq!(order_by(Some("DROP TABLE"), Some("x"), &allowed, "id"), "u.id DESC");
        assert_eq!(int_arg(Some("500"), 50, 1, 200), 200);
        assert_eq!(int_arg(Some("zz"), 50, 1, 200), 50);
    }

    #[test]
    fn action_rolls_back_on_error_and_logs_on_success() {
        let db = Db::open_temp().unwrap();
        let ctx = AdminContext { admin_user_id: 1, ip: "1.2.3.4".into(), user_agent: "UA".into() };
        let failed: AdminResult<()> = action(&db, &ctx, "x.fail", Some("t"), Some("1".into()), Some(&json!("ok reason")), json!({}), true, |act| {
            act.tx.execute("INSERT INTO app_settings (key, value) VALUES ('a', '1')", [])?;
            Err(AdminError::new("hiba", 409))
        });
        assert_eq!(failed.unwrap_err().status, 409);
        assert!(db.get_setting("a").is_none(), "a művelet visszagördült");
        let count: i64 = db.with(|tx| tx.query_row("SELECT COUNT(*) FROM admin_audit", [], |r| r.get(0))).unwrap();
        assert_eq!(count, 0);

        let value = action(&db, &ctx, "x.ok", Some("t"), Some("1".into()), Some(&json!("ok reason")), json!({"k": 1}), true, |act| {
            act.tx.execute("INSERT INTO app_settings (key, value) VALUES ('b', '2')", [])?;
            act.details.insert("after".into(), json!(2));
            Ok(7)
        })
        .unwrap();
        assert_eq!(value, 7);
        assert_eq!(db.get_setting("b").as_deref(), Some("2"));
        let details: String = db.with(|tx| tx.query_row("SELECT details_json FROM admin_audit", [], |r| r.get(0))).unwrap();
        let details: Value = serde_json::from_str(&details).unwrap();
        assert_eq!(details["reason"], "ok reason");
        assert_eq!(details["after"], 2);
        assert_eq!(details["k"], 1);
        // indoklás nélkül el sem indul
        let no_reason: AdminResult<()> = action(&db, &ctx, "x.no", None, None, None, json!({}), true, |_| Ok(()));
        assert_eq!(no_reason.unwrap_err().extra["field"], "reason");
        // indoklás nélkül is mehet, ha nem kötelező
        assert!(action(&db, &ctx, "x.free", None, None, None, json!({}), false, |_| Ok(())).is_ok());
    }

    #[test]
    fn ip_allowlist() {
        let nets = vec![IpNet::parse("10.0.0.0/8").unwrap()];
        assert!(ip_allowed("10.1.2.3", &nets));
        assert!(!ip_allowed("11.1.2.3", &nets));
        assert!(ip_allowed("::ffff:10.1.2.3", &nets));
        assert!(!ip_allowed("nem ip", &nets));
        assert!(ip_allowed("bármi", &[]));
    }
}

// ===== Tranzakció hibaátvitellel =====

/// Egy tranzakció, amelyben az admin hibák (`AdminError`) is visszagörgetést okoznak (a Python `auth.transaction()`).
pub fn txn<T>(db: &Db, body: impl FnOnce(&Transaction) -> AdminResult<T>) -> AdminResult<T> {
    let mut conn = db.raw();
    let tx = conn.transaction()?;
    let value = body(&tx)?;
    tx.commit()?;
    Ok(value)
}

/// Egy szöveges mező a JSON törzsből, vagy None.
pub fn str_of<'a>(value: Option<&'a Value>) -> Option<&'a str> {
    value.and_then(|v| v.as_str())
}

// ===== Beszúrási sorrendet őrző szótár =====

/// Beszúrási sorrendet őrző szótár (mint a Python `dict`): az egyenlő kulcsú rendezéseknél a kimenet determinisztikus.
pub struct Ordered<K: std::hash::Hash + Eq + Clone, V> {
    keys: Vec<K>,
    map: std::collections::HashMap<K, V>,
}

impl<K: std::hash::Hash + Eq + Clone, V> Default for Ordered<K, V> {
    fn default() -> Self {
        Ordered { keys: Vec::new(), map: std::collections::HashMap::new() }
    }
}

impl<K: std::hash::Hash + Eq + Clone, V> Ordered<K, V> {
    pub fn entry_or(&mut self, key: K, default: impl FnOnce() -> V) -> &mut V {
        if !self.map.contains_key(&key) {
            self.keys.push(key.clone());
            self.map.insert(key.clone(), default());
        }
        self.map.get_mut(&key).expect("beszúrva")
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.map.get(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.keys.iter().map(|k| (k, &self.map[k]))
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}
