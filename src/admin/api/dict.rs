//! Szótár és moderáció (`/api/admin/dictionary...`, `/reports`, `/moderation...`).

use crate::admin::routes::{AdminReq, AdminRouter, HResult, csv_response, json_ok, json_ok_empty};
use crate::admin::{dict, live, moderation};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::IntoResponse;
use serde_json::json;

pub fn register(r: &mut AdminRouter) {
    r.get("/dictionary/word", dictionary_word);
    r.get("/dictionary/rejected", dictionary_rejected_list);
    r.post("/dictionary/rejected/preview", dictionary_rejected_preview);
    r.post_danger("/dictionary/rejected", dictionary_rejected_add);
    r.delete_danger("/dictionary/rejected", dictionary_rejected_remove);
    r.get("/dictionary/rejected/download", dictionary_rejected_download);
    r.post_sudo("/dictionary/rejected/export-votes", dictionary_export_votes);
    r.get("/dictionary/overrides", dictionary_overrides_list);
    r.post_danger("/dictionary/overrides", dictionary_overrides_set);
    r.delete_danger("/dictionary/overrides", dictionary_overrides_remove);
    r.get("/dictionary/additions", dictionary_additions_list);
    r.post_danger("/dictionary/additions", dictionary_additions_add);
    r.delete_danger("/dictionary/additions", dictionary_additions_remove);
    r.delete_sudo("/dictionary/votes", dictionary_votes_delete);
    r.get("/dictionary/second-opinion", dictionary_second_opinion);
    r.get("/dictionary/reviewers", dictionary_reviewers);
    r.get("/dictionary/review-summary", dictionary_review_summary);
    r.get("/dictionary/threshold", dictionary_threshold);
    r.post_danger("/dictionary/caches/clear", dictionary_caches_clear);

    r.get("/reports", reports_list);
    r.get("/reports/{report_id}", reports_detail);
    r.patch("/reports/{report_id}", reports_update);
    r.get("/moderation/chat", moderation_chat);
    r.get("/moderation/words", moderation_words_list);
    r.post_danger("/moderation/words", moderation_words_add);
    r.delete_danger("/moderation/words", moderation_words_remove);
    r.get("/moderation/names", moderation_names);
}

// ===== Szótár =====

/// Szó-vizsgáló: a teljes döntési lánc egy szóra.
fn dictionary_word(req: &AdminReq) -> HResult {
    let raw = req.arg("w").map(|w| json!(w));
    json_ok(dict::inspect_word(&req.app, raw.as_ref())?)
}

fn dictionary_rejected_list(req: &AdminReq) -> HResult {
    let data = dict::list_rejected(&req.app, &req.query)?;
    if req.wants_csv() {
        let items = data["items"].as_array().cloned().unwrap_or_default();
        return Ok(csv_response("admin-rejected.csv", &["word", "listed", "voted", "allowed", "good", "bad"], &items));
    }
    json_ok(data)
}

/// Beillesztett lista előnézete: melyik szó vehető fel, melyik már a listán van, melyik nem is érvényes most.
fn dictionary_rejected_preview(req: &AdminReq) -> HResult {
    json_ok(dict::preview_add(req.field("words"))?)
}

fn dictionary_rejected_add(req: &AdminReq) -> HResult {
    json_ok(dict::add_rejected(&req.app, req.ctx(), req.field("words"), req.reason(), false)?)
}

fn dictionary_rejected_remove(req: &AdminReq) -> HResult {
    json_ok(json!({"removed": dict::remove_rejected(&req.app, req.ctx(), req.field("words"), req.reason())?}))
}

/// A tartós lista fájlja (hu_rejected.txt), vagy a változások diffje (`diff=1`) a repóba való átvezetéshez.
fn dictionary_rejected_download(req: &AdminReq) -> HResult {
    let (text, name) = if req.arg("diff") == Some("1") {
        (dict::list_diff(), "hu_rejected.diff")
    } else {
        (dict::read_list_text(), "hu_rejected.txt")
    };
    let mut response = (StatusCode::OK, text).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"));
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    Ok(response)
}

/// A szavazatokból kizárt szavak átvezetése a tartós listába.
fn dictionary_export_votes(req: &AdminReq) -> HResult {
    json_ok(dict::export_votes(&req.app, req.ctx(), req.reason())?)
}

fn dictionary_overrides_list(req: &AdminReq) -> HResult {
    json_ok(json!({"items": dict::list_overrides(&req.app)?}))
}

fn dictionary_overrides_set(req: &AdminReq) -> HResult {
    json_ok(json!({"word": dict::set_override(&req.app, req.ctx(), req.field("word"), req.field("verdict"), req.reason())?}))
}

fn dictionary_overrides_remove(req: &AdminReq) -> HResult {
    dict::remove_override(&req.app, req.ctx(), req.field("word"), req.reason())?;
    json_ok_empty()
}

fn dictionary_additions_list(req: &AdminReq) -> HResult {
    json_ok(json!({"items": dict::list_additions(&req.app)?}))
}

fn dictionary_additions_add(req: &AdminReq) -> HResult {
    json_ok(json!({"word": dict::add_addition(&req.app, req.ctx(), req.field("word"), req.reason())?}))
}

fn dictionary_additions_remove(req: &AdminReq) -> HResult {
    dict::remove_addition(&req.app, req.ctx(), req.field("word"), req.reason())?;
    json_ok_empty()
}

fn dictionary_votes_delete(req: &AdminReq) -> HResult {
    json_ok(json!({"deleted": dict::delete_votes(&req.app, req.ctx(), req.field("word"), req.reason())?}))
}

fn dictionary_second_opinion(req: &AdminReq) -> HResult {
    json_ok(json!({"items": dict::second_opinion(req.arg("n"))?}))
}

fn dictionary_reviewers(req: &AdminReq) -> HResult {
    json_ok(dict::reviewers(&req.app)?)
}

fn dictionary_review_summary(req: &AdminReq) -> HResult {
    json_ok(dict::review_summary(&req.app, req.arg("days"))?)
}

fn dictionary_threshold(req: &AdminReq) -> HResult {
    json_ok(dict::threshold_info(&req.app))
}

fn dictionary_caches_clear(req: &AdminReq) -> HResult {
    json_ok(dict::clear_cache(&req.app, req.ctx(), req.field("which"), req.reason())?)
}

// ===== Moderáció =====

fn reports_list(req: &AdminReq) -> HResult {
    json_ok(moderation::list_reports(&req.app, &req.query)?)
}

fn reports_detail(req: &AdminReq) -> HResult {
    json_ok(json!({"report": moderation::get_report(&req.app, req.ctx(), req.id("report_id")?)?}))
}

fn reports_update(req: &AdminReq) -> HResult {
    moderation::update_report(&req.app, req.ctx(), req.id("report_id")?, req.field("status"), req.reason())?;
    json_ok_empty()
}

/// A chat napló: a futó szobák üzenetei élőben (`source=live`, alapértelmezett), vagy a tartósan naplózottak
/// (`source=log`). Naplózva: `view.chat`.
fn moderation_chat(req: &AdminReq) -> HResult {
    if req.arg("source") == Some("log") {
        let data = moderation::query_chat_log(&req.app, req.ctx(), &req.query)?;
        if req.wants_csv() {
            let items = data["items"].as_array().cloned().unwrap_or_default();
            return Ok(csv_response("admin-chat.csv", &["id", "created_at", "room_name", "name", "user_id", "message"], &items));
        }
        return json_ok(data);
    }
    let st = req.app.state.lock();
    json_ok(live::live_chat(&req.app, &st, req.ctx(), &req.query)?)
}

fn moderation_words_list(req: &AdminReq) -> HResult {
    json_ok(json!({"items": moderation::list_banned_words(&req.app)?}))
}

fn moderation_words_add(req: &AdminReq) -> HResult {
    json_ok(json!({"word": moderation::add_banned_word(&req.app, req.ctx(), req.field("word"), req.reason())?}))
}

fn moderation_words_remove(req: &AdminReq) -> HResult {
    moderation::remove_banned_word(&req.app, req.ctx(), req.field("word"), req.reason())?;
    json_ok_empty()
}

fn moderation_names(req: &AdminReq) -> HResult {
    json_ok(moderation::recent_names(&req.app, &req.query)?)
}
