//! Admin napló lekérdezése és exportja (a Python `admin.py` naplós része).

use super::*;
use crate::db::{RowExt, fetch_all, fetch_one};
use rusqlite::params_from_iter;
use rusqlite::types::Value as SqlValue;

fn text_filter(value: Option<&str>) -> AdminResult<Option<String>> {
    let Some(value) = value else { return Ok(None) };
    let value = value.trim();
    if value.chars().count() > AUDIT_FILTER_MAX_LEN {
        return Err(AdminError::new("A szűrő túl hosszú.", 400));
    }
    Ok(if value.is_empty() { None } else { Some(value.to_string()) })
}

fn audit_item(row: &crate::db::Row) -> Value {
    let details: Option<Value> = row.opt_text("details_json").and_then(|t| serde_json::from_str(&t).ok());
    let reason = details.as_ref().and_then(|d| d.get("reason")).and_then(|r| r.as_str()).map(|r| r.to_string());
    json!({
        "id": row.int("id"),
        "created_at": row.text("created_at"),
        "admin_user_id": row.get("admin_user_id").cloned().unwrap_or(Value::Null),
        "admin_name": row.get("admin_name").cloned().unwrap_or(Value::Null),
        "action": row.text("action"),
        "target_type": row.get("target_type").cloned().unwrap_or(Value::Null),
        "target_id": row.get("target_id").cloned().unwrap_or(Value::Null),
        "ip": row.get("ip").cloned().unwrap_or(Value::Null),
        "user_agent": row.get("user_agent").cloned().unwrap_or(Value::Null),
        "reason": reason,
        "details": details,
    })
}

/// A naplószűrők (mind opcionális).
#[derive(Default, Clone)]
pub struct AuditFilters {
    pub admin: Option<String>,
    pub action: Option<String>,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub q: Option<String>,
}

impl AuditFilters {
    pub fn from_args(args: &std::collections::HashMap<String, String>) -> AuditFilters {
        let get = |k: &str| args.get(k).cloned();
        AuditFilters {
            admin: get("admin"),
            action: get("action"),
            target_type: get("target_type"),
            target_id: get("target_id"),
            since: get("since"),
            until: get("until"),
            q: get("q"),
        }
    }

    pub fn active(&self) -> Value {
        let mut map = Map::new();
        for (k, v) in [
            ("admin", &self.admin),
            ("action", &self.action),
            ("target_type", &self.target_type),
            ("target_id", &self.target_id),
            ("since", &self.since),
            ("until", &self.until),
            ("q", &self.q),
        ] {
            if let Some(v) = v.as_ref().filter(|v| !v.is_empty()) {
                map.insert(k.to_string(), json!(v));
            }
        }
        Value::Object(map)
    }
}

/// Naplósorok szűrve, a legújabbal kezdve. Visszaad: {items, total, limit, offset}.
///
/// admin: id (szám) vagy név / e-mail részlete · action: előtag (pl. `user.` az összes felhasználói műveletre) ·
/// target_type / target_id: pontos egyezés · since / until: dátum vagy időpont (az `until` dátum a nap végéig) · q:
/// szabad szöveg az akcióban, a célpontban, az IP-ben és a részletekben · limit: legfeljebb `max_limit`.
pub fn query_audit(db: &Db, filters: &AuditFilters, limit: Option<&str>, offset: Option<&str>, max_limit: i64) -> AdminResult<Value> {
    let mut where_: Vec<String> = Vec::new();
    let mut params: Vec<SqlValue> = Vec::new();
    if let Some(admin) = text_filter(filters.admin.as_deref())? {
        if admin.chars().all(|c| c.is_ascii_digit()) {
            where_.push("a.admin_user_id = ?".into());
            params.push(SqlValue::Integer(admin.parse().unwrap_or(0)));
        } else {
            where_.push("(u.display_name LIKE ? ESCAPE '\\' OR u.email_lower LIKE ? ESCAPE '\\')".into());
            params.push(SqlValue::Text(like(&admin)));
            params.push(SqlValue::Text(like(&admin.to_lowercase())));
        }
    }
    if let Some(action) = text_filter(filters.action.as_deref())? {
        where_.push("a.action LIKE ? ESCAPE '\\'".into());
        params.push(SqlValue::Text(prefix_like(&action)));
    }
    if let Some(target_type) = text_filter(filters.target_type.as_deref())? {
        where_.push("a.target_type = ?".into());
        params.push(SqlValue::Text(target_type));
    }
    if let Some(target_id) = text_filter(filters.target_id.as_deref())? {
        where_.push("a.target_id = ?".into());
        params.push(SqlValue::Text(target_id));
    }
    if let Some(since) = text_filter(filters.since.as_deref())? {
        let (stamp, _) = parse_bound(&since, false)?;
        where_.push("a.created_at >= ?".into());
        params.push(SqlValue::Text(stamp));
    }
    if let Some(until) = text_filter(filters.until.as_deref())? {
        let (stamp, exclusive) = parse_bound(&until, true)?;
        where_.push(if exclusive { "a.created_at < ?" } else { "a.created_at <= ?" }.into());
        params.push(SqlValue::Text(stamp));
    }
    if let Some(q) = text_filter(filters.q.as_deref())? {
        let pattern = like(&q);
        where_.push("(a.action LIKE ? ESCAPE '\\' OR a.target_id LIKE ? ESCAPE '\\' OR a.ip LIKE ? ESCAPE '\\' OR a.details_json LIKE ? ESCAPE '\\')".into());
        for _ in 0..4 {
            params.push(SqlValue::Text(pattern.clone()));
        }
    }
    let limit = int_arg(limit, AUDIT_DEFAULT_LIMIT, 1, max_limit);
    let offset = int_arg(offset, 0, 0, 1_000_000_000);
    let clause = if where_.is_empty() { String::new() } else { format!("WHERE {}", where_.join(" AND ")) };
    let source = format!("FROM admin_audit a LEFT JOIN users u ON u.id = a.admin_user_id {clause}");
    let (total, rows) = db.with(|tx| {
        let total = fetch_one(tx, &format!("SELECT COUNT(*) AS n {source}"), params_from_iter(params.clone()))?.map(|r| r.int("n")).unwrap_or(0);
        let mut page_params = params.clone();
        page_params.push(SqlValue::Integer(limit));
        page_params.push(SqlValue::Integer(offset));
        let rows =
            fetch_all(tx, &format!("SELECT a.*, u.display_name AS admin_name {source} ORDER BY a.id DESC LIMIT ? OFFSET ?"), params_from_iter(page_params))?;
        Ok((total, rows))
    })?;
    Ok(json!({"items": rows.iter().map(audit_item).collect::<Vec<_>>(), "total": total, "limit": limit, "offset": offset}))
}

/// A naplósorok CSV-ben (UTF-8 BOM nélkül; a hívó adja a fejlécet).
pub fn audit_csv(items: &[Value]) -> String {
    let mut out = csv_line(&AUDIT_CSV_FIELDS.iter().map(|f| f.to_string()).collect::<Vec<_>>());
    for item in items {
        let cells: Vec<String> = AUDIT_CSV_FIELDS
            .iter()
            .map(|field| {
                if *field == "details_json" {
                    match item.get("details") {
                        Some(Value::Null) | None => String::new(),
                        Some(details) => csv_cell(&json!(details.to_string())),
                    }
                } else {
                    csv_cell(item.get(*field).unwrap_or(&Value::Null))
                }
            })
            .collect();
        out.push_str(&csv_line(&cells));
    }
    out
}
