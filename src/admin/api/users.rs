//! Áttekintés, globális kereső és felhasználók (`/api/admin/overview`, `/search`, `/users...`).

use crate::admin::routes::{AdminReq, AdminRouter, HResult, csv_response, json_download, json_ok, json_ok_empty, sudo_error};
use crate::admin::{self, AdminError, comm, live, users};
use crate::db::{RowExt, fetch_all};
use serde_json::{Value, json};

pub fn register(r: &mut AdminRouter) {
    r.get("/overview", overview);
    r.get("/charts", charts);
    r.get("/search", global_search);

    r.get("/users", users_list);
    r.get("/users/{user_id}", users_detail);
    r.get("/users/{user_id}/profile", users_profile);
    r.patch("/users/{user_id}", users_patch);
    r.post_sudo("/users/{user_id}/ban", users_ban);
    r.post("/users/{user_id}/unban", users_unban);
    r.post("/users/{user_id}/mute", users_mute);
    r.post("/users/{user_id}/unmute", users_unmute);
    r.post_danger("/users/{user_id}/logout-all", users_logout_all);
    r.post_sudo("/users/{user_id}/reset-password", users_reset_password);
    r.post_sudo("/users/{user_id}/rating", users_rating);
    r.post("/users/{user_id}/badges", users_badges);
    r.post("/users/{user_id}/recompute-stats", users_recompute_stats);
    r.post_sudo("/users/{user_id}/reviews/revert", users_reviews_revert);
    r.post("/users/{user_id}/reviews/block", users_reviews_block);
    r.post("/users/{user_id}/push-test", users_push_test);
    r.delete("/users/{user_id}/push/{device_id}", users_push_delete);
    r.post("/users/{user_id}/notes", users_note_add);
    r.delete("/users/{user_id}/notes/{note_id}", users_note_delete);
    r.get("/users/{user_id}/export", users_export);
    r.delete_sudo("/users/{user_id}", users_delete);
}

// ===== Áttekintés, grafikonok, kereső =====

/// Élő számlálók, mai adatok, szerver és szolgáltatások állapota, figyelmeztetések.
fn overview(req: &AdminReq) -> HResult {
    let st = req.app.state.lock();
    json_ok(live::overview(&req.app, &st)?)
}

/// Napi bontású grafikonok (alapértelmezés: 30 nap).
fn charts(req: &AdminReq) -> HResult {
    json_ok(live::charts(&req.app, req.int_arg("days", 30))?)
}

/// Globális kereső: felhasználó (név / e-mail / id), szoba (kód / név / id), játék (id / név), szó.
fn global_search(req: &AdminReq) -> HResult {
    let q = req.arg("q").unwrap_or("").trim().to_string();
    if q.chars().count() > 60 {
        return Err(AdminError::new("A szűrő túl hosszú.", 400));
    }
    let mut result = json!({"users": [], "rooms": [], "games": [], "words": []});
    if q.is_empty() {
        return json_ok(result);
    }
    result["users"] = json!(users::search_brief(&req.app, &q, 8)?);
    let lowered = q.to_lowercase();
    let mut rooms = Vec::new();
    {
        let st = req.app.state.lock();
        for room in st.rooms.values() {
            if room.join_code.contains(&lowered) || lowered == room.id.to_lowercase() || room.name.to_lowercase().contains(&lowered) {
                rooms.push(json!({"id": room.id, "code": room.join_code, "name": room.name}));
                if rooms.len() >= 8 {
                    break;
                }
            }
        }
    }
    result["rooms"] = json!(rooms);
    let games = admin::txn(&req.app.db, |tx| {
        let mut clauses = vec!["room_name LIKE ? ESCAPE '\\'".to_string()];
        let mut params: Vec<rusqlite::types::Value> = vec![rusqlite::types::Value::Text(admin::like(&q))];
        if q.chars().all(|c| c.is_ascii_digit()) {
            clauses.push("id = ?".into());
            params.push(rusqlite::types::Value::Integer(q.parse().unwrap_or(0)));
        }
        let sql = format!("SELECT id, room_name, status FROM saved_games WHERE {} ORDER BY id DESC LIMIT 8", clauses.join(" OR "));
        Ok(fetch_all(tx, &sql, rusqlite::params_from_iter(params))?)
    })?;
    result["games"] = json!(games.iter().map(|r| json!({"id": r.int("id"), "name": r.text("room_name"), "status": r.text("status")})).collect::<Vec<_>>());
    let word = q.to_uppercase();
    let length = word.chars().count();
    if (2..=15).contains(&length) && crate::tiles::tokenize_word(&word).is_some() {
        result["words"] = json!([{"word": word}]);
    }
    json_ok(result)
}

// ===== Felhasználók =====

fn users_list(req: &AdminReq) -> HResult {
    let online = live::online_user_ids(&req.app.state.lock());
    if req.wants_csv() {
        let data = users::list_users(&req.app, &req.query, &online, Some((10000, 0)))?;
        let items = data["items"].as_array().cloned().unwrap_or_default();
        return Ok(csv_response("admin-users.csv", &users::USER_CSV_FIELDS, &items));
    }
    json_ok(users::list_users(&req.app, &req.query, &online, None)?)
}

fn users_detail(req: &AdminReq) -> HResult {
    let user_id = req.id("user_id")?;
    let live_info = live::user_live(&req.app.state.lock(), user_id);
    let online = live_info["online"].as_bool().unwrap_or(false);
    let mut data = users::user_detail(&req.app, req.ctx(), user_id, online)?;
    data["live"] = live_info;
    data["is_self"] = json!(user_id == req.admin.user.id);
    json_ok(data)
}

/// A profil oldal tartalma, ahogy a felhasználó látja (csak olvasható nézet; megszemélyesítés nincs).
fn users_profile(req: &AdminReq) -> HResult {
    json_ok(json!({"profile": users::profile_view(&req.app, req.ctx(), req.id("user_id")?)?}))
}

/// Név és / vagy e-mail módosítása. Az e-mail módosításához sudo kell; a régi címre értesítés megy.
fn users_patch(req: &AdminReq) -> HResult {
    let user_id = req.id("user_id")?;
    let wants_name = req.body.get("display_name").is_some();
    let wants_email = req.body.get("email").is_some();
    if !(wants_name || wants_email) {
        return Err(AdminError::new("Érvénytelen kérés.", 400));
    }
    if wants_email && !req.sudo_active() {
        return Err(sudo_error());
    }
    let mut result = json!({});
    if wants_name {
        let new_name = users::sanitize_name(req.field("display_name"));
        if let Some(name) = new_name {
            if !live::rename_conflicts(&req.app.state.lock(), user_id, &name).is_empty() {
                return Err(AdminError::new("Már van ilyen nevű játékos abban a szobában, ahol a felhasználó játszik.", 409));
            }
        }
        let (old, new, _games) = users::rename(&req.app, req.ctx(), user_id, req.field("display_name"), req.reason())?;
        {
            let mut st = req.app.state.lock();
            live::rename_live(&req.app, &mut st, user_id, &old, &new);
        }
        result["display_name"] = json!(new);
    }
    if wants_email {
        let old_email = users::change_email(&req.app, req.ctx(), user_id, req.field("email"), req.reason())?;
        let (ok, _error) = req.app.mailer.send_plain_email(
            &old_email,
            "Magyar Scrabble — megváltozott az e-mail címed",
            "Az e-mailcímedet egy moderátor megváltoztatta. Ha ez nem várt, vedd fel velünk a kapcsolatot.",
            false,
        );
        result["email"] = json!(req.field("email").and_then(|v| v.as_str()).unwrap_or("").trim());
        result["notified"] = json!(ok);
    }
    json_ok(result)
}

fn users_ban(req: &AdminReq) -> HResult {
    let user_id = req.id("user_id")?;
    let until = users::ban(&req.app, req.ctx(), user_id, req.field("until"), req.reason())?;
    let reason = admin::normalize_reason(req.reason())?;
    let permanent = until == crate::db::PERMANENT_UNTIL;
    let payload = json!({"reason": reason, "until": if permanent { Value::Null } else { json!(until) }});
    let kicked = {
        let mut st = req.app.state.lock();
        live::kick_user(&req.app, &mut st, user_id, "account_banned", Some(&payload))
    };
    json_ok(json!({"until": if permanent { Value::Null } else { json!(until) }, "kicked": kicked}))
}

fn users_unban(req: &AdminReq) -> HResult {
    users::unban(&req.app, req.ctx(), req.id("user_id")?, req.reason())?;
    json_ok_empty()
}

fn users_mute(req: &AdminReq) -> HResult {
    let until = users::mute(&req.app, req.ctx(), req.id("user_id")?, req.field("until"), req.reason())?;
    json_ok(json!({"until": until}))
}

fn users_unmute(req: &AdminReq) -> HResult {
    users::unmute(&req.app, req.ctx(), req.id("user_id")?, req.reason())?;
    json_ok_empty()
}

fn kick(req: &AdminReq, user_id: i64) -> usize {
    let mut st = req.app.state.lock();
    live::kick_user(&req.app, &mut st, user_id, "force_logout", None)
}

fn users_logout_all(req: &AdminReq) -> HResult {
    let user_id = req.id("user_id")?;
    let closed = users::logout_all(&req.app, req.ctx(), user_id, req.reason())?;
    json_ok(json!({"sessions_closed": closed, "disconnected": kick(req, user_id)}))
}

/// Ideiglenes jelszó: e-mailben megy (SMTP-vel); SMTP nélkül az admin kapja meg egyszer a válaszban.
fn users_reset_password(req: &AdminReq) -> HResult {
    let user_id = req.id("user_id")?;
    let (password, email, closed) = users::reset_password(&req.app, req.ctx(), user_id, req.reason())?;
    let (emailed, _error) = req.app.mailer.send_plain_email(
        &email,
        "Magyar Scrabble — ideiglenes jelszó",
        &format!("Az ideiglenes jelszavad: {password}\n\nBelépés után változtasd meg. A korábbi munkameneteid kijelentkeztek."),
        false,
    );
    kick(req, user_id);
    let mut data = json!({"emailed": emailed, "sessions_closed": closed});
    if !req.app.mailer.is_configured() {
        data["temp_password"] = json!(password);
    }
    json_ok(data)
}

fn users_rating(req: &AdminReq) -> HResult {
    users::set_rating(&req.app, req.ctx(), req.id("user_id")?, req.field("rating"), req.reason())?;
    json_ok_empty()
}

fn users_badges(req: &AdminReq) -> HResult {
    users::set_badge(&req.app, req.ctx(), req.id("user_id")?, req.field("badge"), req.field("grant"), req.reason())?;
    json_ok_empty()
}

fn users_recompute_stats(req: &AdminReq) -> HResult {
    users::recompute_stats(&req.app, req.ctx(), req.id("user_id")?, req.reason())?;
    json_ok_empty()
}

fn users_reviews_revert(req: &AdminReq) -> HResult {
    let words = users::revert_reviews(&req.app, req.ctx(), req.id("user_id")?, req.field("since"), req.reason())?;
    crate::word_review::refresh(&req.app.db, &req.app.settings);
    json_ok(json!({"count": words.len()}))
}

fn users_reviews_block(req: &AdminReq) -> HResult {
    users::set_review_blocked(&req.app, req.ctx(), req.id("user_id")?, req.field("blocked"), req.reason())?;
    crate::word_review::refresh(&req.app.db, &req.app.settings);
    json_ok_empty()
}

fn users_push_test(req: &AdminReq) -> HResult {
    json_ok(comm::push_test(&req.app, req.ctx(), req.id("user_id")?)?)
}

fn users_push_delete(req: &AdminReq) -> HResult {
    users::delete_push_device(&req.app, req.ctx(), req.id("user_id")?, req.id("device_id")?, req.reason())?;
    json_ok_empty()
}

fn users_note_add(req: &AdminReq) -> HResult {
    json_ok(json!({"id": users::add_note(&req.app, req.ctx(), req.id("user_id")?, req.field("note"))?}))
}

fn users_note_delete(req: &AdminReq) -> HResult {
    users::delete_note(&req.app, req.ctx(), req.id("user_id")?, req.id("note_id")?)?;
    json_ok_empty()
}

/// GDPR export: a felhasználó összes adata JSON-ban (letöltésként; naplózva).
fn users_export(req: &AdminReq) -> HResult {
    let user_id = req.id("user_id")?;
    let data = users::export_user(&req.app, req.ctx(), user_id)?;
    let text = serde_json::to_string_pretty(&data).unwrap_or_default();
    Ok(json_download(&format!("user-{user_id}.json"), text))
}

/// Fiók törlése / anonimizálása (a célpont nevének begépelése kötelező).
fn users_delete(req: &AdminReq) -> HResult {
    let user_id = req.id("user_id")?;
    let name = users::delete_user(&req.app, req.ctx(), user_id, req.field("mode").or(Some(&json!("anonymize"))), req.field("confirm_name"), req.reason())?;
    kick(req, user_id);
    crate::word_review::refresh(&req.app.db, &req.app.settings);
    json_ok(json!({"name": name}))
}
