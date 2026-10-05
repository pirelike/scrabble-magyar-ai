//! Kommunikáció (közlemény, karbantartási mód, push, e-mail), beállítások és statisztika.

use crate::admin::routes::{AdminReq, AdminRouter, HResult, csv_response, json_ok, json_ok_empty};
use crate::admin::{self, AdminError, comm, live, stats};
use crate::db::{RowExt, fetch_all};
use crate::server::core;
use serde_json::{Map, Value, json};

pub fn register(r: &mut AdminRouter) {
    r.get("/announcements", announcements_list);
    r.post("/announcements", announcements_create);
    r.patch("/announcements/{announcement_id}", announcements_update);
    r.delete("/announcements/{announcement_id}", announcements_revoke);

    r.get("/maintenance", maintenance_get);
    r.post_danger("/maintenance", maintenance_set);

    r.post("/push/preview", push_preview);
    r.post_danger("/push", push_send);
    r.post_danger("/email", email_send);
    r.get("/email/preview", email_preview);
    r.post_sudo("/email/bulk", email_bulk);

    r.get("/settings", settings_get);
    r.patch_danger("/settings", settings_patch);

    r.get("/stats", stats_get);
}

/// A közlemények élő frissítése minden kliensnek (a szerver `announcement` eseményével).
fn broadcast(req: &AdminReq) {
    core::broadcast_announcements(&req.app);
}

fn online(req: &AdminReq) -> std::collections::HashSet<i64> {
    live::online_user_ids(&req.app.state.lock())
}

// ===== Közlemények =====

fn announcements_list(req: &AdminReq) -> HResult {
    json_ok(json!({"items": comm::list_announcements(&req.app)?, "maintenance": comm::maintenance_state(&req.app)}))
}

fn announcements_create(req: &AdminReq) -> HResult {
    let id = comm::create_announcement(&req.app, req.ctx(), &req.body)?;
    broadcast(req);
    json_ok(json!({"id": id}))
}

fn announcements_update(req: &AdminReq) -> HResult {
    comm::update_announcement(&req.app, req.ctx(), req.id("announcement_id")?, &req.body)?;
    broadcast(req);
    json_ok_empty()
}

fn announcements_revoke(req: &AdminReq) -> HResult {
    comm::revoke_announcement(&req.app, req.ctx(), req.id("announcement_id")?)?;
    broadcast(req);
    json_ok_empty()
}

// ===== Karbantartási mód =====

fn maintenance_get(req: &AdminReq) -> HResult {
    json_ok(comm::maintenance_state(&req.app))
}

fn maintenance_set(req: &AdminReq) -> HResult {
    let state = comm::set_maintenance(&req.app, req.ctx(), &req.body)?;
    broadcast(req);
    json_ok(state)
}

// ===== Push és e-mail =====

fn push_preview(req: &AdminReq) -> HResult {
    json_ok(comm::push_preview(&req.app, &req.body, &online(req))?)
}

fn push_send(req: &AdminReq) -> HResult {
    json_ok(comm::push_send(&req.app, req.ctx(), &req.body, &online(req))?)
}

fn email_send(req: &AdminReq) -> HResult {
    // A felhasználó azonosítója egész szám kell legyen (a logikai érték nem az)
    let Some(user_id) = req.field("user_id").and_then(|v| v.as_i64()) else {
        return Err(AdminError::field("Érvénytelen kérés.", 400, "user_id"));
    };
    json_ok(comm::email_user(&req.app, req.ctx(), user_id, &req.body)?)
}

fn email_preview(req: &AdminReq) -> HResult {
    json_ok(comm::email_preview(&req.app, req.arg("group"))?)
}

fn email_bulk(req: &AdminReq) -> HResult {
    json_ok(comm::email_bulk(&req.app, req.ctx(), &req.body)?)
}

// ===== Beállítások (funkciókapcsolók) =====

fn settings_get(req: &AdminReq) -> HResult {
    let mut items = req.app.settings.describe(None);
    let mut ids: Vec<i64> = items.iter().filter_map(|i| i["changed_by"].as_i64()).collect();
    ids.sort_unstable();
    ids.dedup();
    let mut names = std::collections::HashMap::new();
    if !ids.is_empty() {
        let rows = admin::txn(&req.app.db, |tx| {
            let sql = format!("SELECT id, display_name FROM users WHERE id IN ({})", crate::db::placeholders(ids.len()));
            Ok(fetch_all(tx, &sql, rusqlite::params_from_iter(ids.iter()))?)
        })?;
        for row in rows {
            names.insert(row.int("id"), row.text("display_name"));
        }
    }
    for item in &mut items {
        item["changed_by_name"] = item["changed_by"].as_i64().and_then(|id| names.get(&id)).map(|n| json!(n)).unwrap_or(Value::Null);
    }
    json_ok(json!({"items": items, "groups": crate::settings::GROUPS}))
}

/// Egy vagy több beállítás módosítása: `{changes: {kulcs: érték | null}, reason}`. A `null` visszaállít az alapra.
/// Minden változás a naplóba kerül (előtte / utána).
fn settings_patch(req: &AdminReq) -> HResult {
    let changes = match req.field("changes") {
        Some(Value::Object(map)) if !map.is_empty() => map.clone(),
        _ => return Err(AdminError::new("Érvénytelen kérés.", 400)),
    };
    let settings = &req.app.settings;
    // Az értékek olvasása (és a gyorsítótár betöltése) a naplózott tranzakció előtt: abban nem szabad újra az
    // adatbázishoz nyúlni
    let mut keys: Vec<&String> = changes.keys().collect();
    keys.sort();
    let mut before = Map::new();
    for key in &keys {
        match settings.definition(key) {
            Some(spec) if !spec.hidden => {}
            _ => return Err(AdminError::field("Ismeretlen beállítás.", 400, key)),
        }
        before.insert((*key).clone(), settings.get(key));
    }
    let target = keys.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(",");
    let admin_id = req.admin.user.id;
    admin::action(&req.app.db, req.ctx(), "settings.update", Some("setting"), Some(target), req.reason(), json!({}), true, |act| {
        let mut after = Map::new();
        for key in &keys {
            let value = &changes[*key];
            settings.store(act.tx, key, value, Some(admin_id)).map_err(|e| AdminError::field(&e.0, 400, key))?;
            let stored =
                if value.is_null() { settings.default_of(key) } else { settings.validate(key, value).map_err(|e| AdminError::field(&e.0, 400, key))? };
            after.insert((*key).clone(), stored);
        }
        act.details.insert("before".into(), Value::Object(before.clone()));
        act.details.insert("after".into(), Value::Object(after));
        Ok(())
    })?;
    req.app.apply_runtime_settings();
    json_ok(json!({"changed": keys}))
}

// ===== Statisztika =====

fn stats_get(req: &AdminReq) -> HResult {
    let data = stats::stats(&req.app, req.arg("metric"), req.arg("range").or(Some("30")))?;
    if req.wants_csv() {
        let columns: Vec<String> =
            data["table"]["columns"].as_array().map(|c| c.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
        let rows: Vec<Value> = data["table"]["rows"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .map(|row| {
                        let mut object = Map::new();
                        for (column, cell) in columns.iter().zip(row.as_array().cloned().unwrap_or_default()) {
                            object.insert(column.clone(), cell);
                        }
                        Value::Object(object)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let fields: Vec<&str> = columns.iter().map(|c| c.as_str()).collect();
        let name = format!("admin-stats-{}.csv", data["metric"].as_str().unwrap_or("stats"));
        return Ok(csv_response(&name, &fields, &rows));
    }
    json_ok(data)
}
