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
