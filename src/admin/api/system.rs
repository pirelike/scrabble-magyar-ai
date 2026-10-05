//! Biztonság (belépési napló, IP tiltás, forgalomkorlát, kódok, munkamenetek) és rendszer (állapot, konfiguráció,
//! adatbázis, mentés, naplók, folyamatok, tunnel, push / SMTP teszt, levelező szerver, frissítés).

use crate::admin::routes::{AdminReq, AdminRouter, HResult, csv_response, json_ok, json_ok_empty};
use crate::admin::{self, AdminError, comm, live, mail, record, security, system, update};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::IntoResponse;
use serde_json::{Value, json};

pub fn register(r: &mut AdminRouter) {
    // Biztonság
    r.get("/security/logins", security_logins);
    r.get("/security/rate-limits", security_rate_limits);
    r.post_danger("/security/rate-limits/unblock", security_rate_unblock);
    r.get("/security/ip-bans", security_ip_bans);
    r.post_sudo("/security/ip-bans", security_ip_ban_add);
    r.delete_danger("/security/ip-bans/{ban_id}", security_ip_ban_remove);
    r.get("/security/codes", security_codes);
    r.delete_danger("/security/codes/{code_id}", security_code_invalidate);
    r.get("/security/sessions", security_sessions);
    r.delete_danger("/security/sessions/{session_id}", security_session_revoke);
    r.post_sudo("/security/sessions/revoke-all", security_sessions_revoke_all);
    r.post_danger("/security/sessions/close-admins", security_sessions_close_admins);

    // Rendszer
    r.get("/system", system_overview);
    r.get("/system/logs", system_logs);
    r.get_sudo("/system/backup", system_backup_download);
    r.post_danger("/system/backup", system_backup_create);
    r.post_danger("/system/vacuum", system_vacuum);
    r.get("/system/cleanup", system_cleanup_preview);
    r.post_sudo("/system/cleanup", system_cleanup);
    r.post_sudo("/system/tunnel/restart", system_tunnel_restart);
    r.post_danger("/system/smtp-test", system_smtp_test);

    // Levelező szerver (SMTP)
    r.get("/system/mail", system_mail_get);
    r.patch_sudo("/system/mail", system_mail_update);
    r.delete_sudo("/system/mail", system_mail_reset);
    r.post_sudo("/system/mail/check", system_mail_check);
    r.post_danger("/system/push-test", system_push_test);

    // Frissítés GitHubról és újraindítás
    r.get("/system/update", system_update_status);
    r.post_danger("/system/update/check", system_update_check);
    r.post_sudo("/system/update/apply", system_update_apply);
    r.post_sudo("/system/restart", system_restart);
}

// ===== Biztonság =====

fn security_logins(req: &AdminReq) -> HResult {
    if req.wants_csv() {
        let data = security::list_logins(&req.app, &req.query, Some((200, 0)))?;
        let items = data["items"].as_array().cloned().unwrap_or_default();
        req.app.db.with(|tx| record(tx, req.ctx(), "view.logins_export", Some("security"), None, &json!({"rows": items.len()})).map(|_| ()))?;
        return Ok(csv_response("admin-logins.csv", &security::LOGIN_CSV_FIELDS, &items));
    }
    json_ok(security::list_logins(&req.app, &req.query, None)?)
}

fn security_rate_limits(req: &AdminReq) -> HResult {
    json_ok(security::rate_limit_state(&req.app.limiter))
}

fn security_rate_unblock(req: &AdminReq) -> HResult {
    security::unblock_ip(&req.app, req.ctx(), req.field("ip"), req.reason())?;
    json_ok_empty()
}

fn security_ip_bans(req: &AdminReq) -> HResult {
    json_ok(json!({"items": security::list_ip_bans(&req.app)?, "own_ip": req.ctx().ip}))
}

fn security_ip_ban_add(req: &AdminReq) -> HResult {
    let id = security::add_ip_ban(&req.app, req.ctx(), req.field("ip"), req.field("until"), req.reason(), &req.ctx().ip)?;
    json_ok(json!({"id": id}))
}

fn security_ip_ban_remove(req: &AdminReq) -> HResult {
    security::remove_ip_ban(&req.app, req.ctx(), req.id("ban_id")?, req.reason())?;
    json_ok_empty()
}

/// A függő regisztrációs kódok (SMTP nélkül itt látszik a kód). Naplózva: `view.codes`.
fn security_codes(req: &AdminReq) -> HResult {
    json_ok(json!({"items": security::list_codes(&req.app, req.ctx())?, "smtp": req.app.mailer.is_configured()}))
}

fn security_code_invalidate(req: &AdminReq) -> HResult {
    security::invalidate_code(&req.app, req.ctx(), req.id("code_id")?, req.reason())?;
    json_ok_empty()
}

fn security_sessions(req: &AdminReq) -> HResult {
    json_ok(json!({"items": security::list_sessions(&req.app, &req.query, Some(&req.admin.token))?}))
}

fn security_session_revoke(req: &AdminReq) -> HResult {
    security::revoke_session(&req.app, req.ctx(), req.id("session_id")?, req.reason(), Some(&req.admin.token))?;
    json_ok_empty()
}

/// Mindenki kijelentkeztetése (pl. a SECRET_KEY cseréje után) — a saját munkameneted megmarad.
fn security_sessions_revoke_all(req: &AdminReq) -> HResult {
    let removed = security::revoke_all_sessions(&req.app, req.ctx(), Some(&req.admin.token), req.reason())?;
    {
        let mut st = req.app.state.lock();
        live::kick_all(&req.app, &mut st, &std::collections::HashSet::from([req.admin.user.id]));
    }
    json_ok(json!({"removed": removed}))
}

/// Az adminok minden más munkamenetének bezárása (a sajátodon kívül).
fn security_sessions_close_admins(req: &AdminReq) -> HResult {
    json_ok(json!({"removed": security::close_other_admin_sessions(&req.app, req.ctx(), Some(&req.admin.token), req.reason())?}))
}

// ===== Rendszer =====

/// A rendszer oldal teljes tartalma: szerver, szolgáltatások, konfiguráció (titkok maszkolva), verziók, adatbázis,
/// mentések, háttérfolyamatok, elemzések sora.
fn system_overview(req: &AdminReq) -> HResult {
    let app = &req.app;
    let subscriptions: i64 = admin::txn(&app.db, |tx| Ok(tx.query_row("SELECT COUNT(*) FROM push_subscriptions", [], |r| r.get(0))?))?;
    json_ok(json!({
        "server": system::process_info(app), "services": system::services_info(app), "config": system::config_view(app),
        "versions": system::versions(app),
        "database": {
            "size": system::db_file_size(app), "tables": system::table_counts(app)?, "last_backup": system::last_backup_time(app),
            "backups": system::list_backups(app),
        },
        "jobs": system::jobs_status(app), "analysis": system::analysis_queue(app),
        "push": {
            "available": app.push.is_available(),
            "public_key": if app.push.is_available() { json!(app.push.public_key()) } else { Value::Null },
            "subscriptions": subscriptions,
        },
        "tunnel": app.tunnel.status(), "mail": mail::view(app)?,
    }))
}

/// A szerver naplójának utolsó sorai (gyűrűpuffer): szint szerint (az adott szinttől felfelé), szöveg szerint,
/// `since_id` után (élő követéshez).
fn system_logs(req: &AdminReq) -> HResult {
    let level = req.arg("level").filter(|l| !l.is_empty());
    if level.is_some_and(|l| !system::LOG_LEVELS.contains(&l)) {
        return Err(AdminError::new("Érvénytelen szűrő.", 400));
    }
    let limit = req.int_arg("limit", 200).clamp(1, 500) as usize;
    let since_id = req.int_arg("since_id", 0).max(0) as u64;
    let items: Vec<Value> = system::LOG_BUFFER.query(level, req.arg("q"), since_id, limit).iter().map(|r| r.to_json()).collect();
    json_ok(json!({"items": items, "groups": system::LOG_BUFFER.error_groups()}))
}

/// Az adatbázis konzisztens pillanatképe letöltésként (SQLite backup API; naplózva).
fn system_backup_download(req: &AdminReq) -> HResult {
    let folder = std::env::temp_dir().join(format!("scrabble-backup-{}", crate::util::token_urlsafe(8)));
    let result = (|| -> admin::AdminResult<(String, Vec<u8>)> {
        let path = system::make_backup(&req.app, Some(&folder))?;
        let bytes = std::fs::read(&path).map_err(|e| AdminError::new(&format!("A mentés nem olvasható: {e}"), 500))?;
        req.app.db.with(|tx| record(tx, req.ctx(), "system.backup_download", Some("system"), None, &json!({"size": bytes.len()})).map(|_| ()))?;
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "scrabble.db".into());
        Ok((name, bytes))
    })();
    let _ = std::fs::remove_dir_all(&folder);
    let (name, bytes) = result?;
    let mut response = (StatusCode::OK, bytes).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    Ok(response)
}

/// Mentés a szerver mentési mappájába (a beállított számú legutóbbi megmarad).
fn system_backup_create(req: &AdminReq) -> HResult {
    let path = system::make_backup(&req.app, None)?;
    let removed = system::prune_backups(&req.app, req.app.settings.get_i64("backup_keep").max(0) as usize);
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    req.app.db.with(|tx| record(tx, req.ctx(), "system.backup", Some("system"), Some(&name), &json!({"size": size, "pruned": removed})).map(|_| ()))?;
    json_ok(json!({"name": name, "size": size, "pruned": removed}))
}

fn system_vacuum(req: &AdminReq) -> HResult {
    admin::action(&req.app.db, req.ctx(), "system.vacuum", Some("system"), None, req.reason(), json!({}), true, |_| Ok(()))?;
    let (before, after) = system::maintenance_vacuum(&req.app)?;
    json_ok(json!({"before": before, "after": after}))
}

fn system_cleanup_preview(req: &AdminReq) -> HResult {
    let days = req.int_arg("days", system::ABANDONED_DEFAULT_DAYS);
    json_ok(json!({"counts": system::cleanup_preview(&req.app, Some(&json!(days)))?}))
}

fn system_cleanup(req: &AdminReq) -> HResult {
    let days = req.field("days").cloned().unwrap_or(json!(system::ABANDONED_DEFAULT_DAYS));
    json_ok(json!({"removed": system::cleanup_run(&req.app, req.ctx(), req.field("items"), Some(&days), req.reason())?}))
}

fn system_tunnel_restart(req: &AdminReq) -> HResult {
    admin::action(&req.app.db, req.ctx(), "system.tunnel_restart", Some("system"), None, req.reason(), json!({}), true, |_| Ok(()))?;
    if !req.app.tunnel.restart() {
        return Err(AdminError::new("A tunnel nem indítható újra.", 409));
    }
    json_ok(req.app.tunnel.status())
}

/// Teszt e-mail magamnak (a küldés eredményével).
fn system_smtp_test(req: &AdminReq) -> HResult {
    req.app.db.with(|tx| record(tx, req.ctx(), "system.smtp_test", Some("system"), None, &json!({})).map(|_| ()))?;
    let (ok, error) = req.app.mailer.send_plain_email(
        &req.admin.user.email,
        "Magyar Scrabble — teszt e-mail",
        "Ez egy teszt e-mail az admin panelről. Az SMTP működik.",
        true,
    );
    if error.as_deref() == Some("smtp_not_configured") {
        return json_ok(json!({"sent": false, "console": true}));
    }
    if !ok {
        return Err(AdminError::new("Az e-mail küldése nem sikerült.", 502).with("code", json!(error)));
    }
    json_ok(json!({"sent": true, "console": false}))
}

// ===== Levelező szerver (SMTP) =====

/// A levelező szerver beállítása (a jelszó nélkül) és a környezeti alapérték.
fn system_mail_get(req: &AdminReq) -> HResult {
    json_ok(json!({"mail": mail::view(&req.app)?}))
}

/// A levelező szerver beállítása: kiszolgáló, port, titkosítás, bejelentkezés, feladó. Újraindítás nélkül hat.
fn system_mail_update(req: &AdminReq) -> HResult {
    json_ok(json!({"mail": mail::update(&req.app, req.ctx(), &req.body)?}))
}

/// A mentett beállítás törlése: a környezeti változók (`SMTP_*`) értéke lép érvénybe.
fn system_mail_reset(req: &AdminReq) -> HResult {
    json_ok(json!({"mail": mail::reset(&req.app, req.ctx(), req.reason())?}))
}

/// Kapcsolat-próba a megadott (még nem mentett) beállítással; `send: true` esetén teszt levéllel a saját címedre.
fn system_mail_check(req: &AdminReq) -> HResult {
    json_ok(mail::check(&req.app, req.ctx(), &req.body, Some(&req.admin.user.email))?)
}

/// Teszt push üzenet magamnak.
fn system_push_test(req: &AdminReq) -> HResult {
    json_ok(comm::push_test(&req.app, req.ctx(), req.admin.user.id)?)
}

// ===== Frissítés GitHubról és újraindítás =====

/// A tár állapota hálózat nélkül: ág, commit, távoli cím, helyi módosítások, újraindítás szükséges-e.
fn system_update_status(req: &AdminReq) -> HResult {
    json_ok(json!({"update": update::status(&req.app)}))
}

/// Lekéri az ágakat a GitHubról (`git fetch`), és megmondja, mi változna: `{branch}` (elhagyható).
fn system_update_check(req: &AdminReq) -> HResult {
    let branch = req.field("branch").and_then(|b| b.as_str());
    let fetch = req.field("fetch") != Some(&json!(false));
    json_ok(json!({"update": update::check(&req.app, branch, fetch)?}))
}

/// Frissítés a GitHub egy ágára (előretekerés): `{branch, reason, install?, restart?}`.
fn system_update_apply(req: &AdminReq) -> HResult {
    let install = req.field("install") == Some(&json!(true));
    let restart = req.field("restart") == Some(&json!(true));
    json_ok(json!({"update": update::apply(&req.app, req.ctx(), req.field("branch"), req.reason(), install, restart)?}))
}

/// A szerver újraindítása (a folyamat önmagára cserélése); a kapcsolatok megszakadnak, a kliensek újracsatlakoznak.
fn system_restart(req: &AdminReq) -> HResult {
    json_ok(update::restart(&req.app, req.ctx(), req.reason())?)
}
