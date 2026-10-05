//! Élő szobák, játékarchívum, levelezős játékok, ranglista / értékszám, napi feladvány.
//! `/api/admin/rooms`, `/games`, `/async`, `/ratings`, `/daily`.

use super::live_game_ids;
use crate::admin::routes::{AdminReq, AdminRouter, HResult, csv_response, flat, json_download, json_ok, json_ok_empty, sudo_error};
use crate::admin::{self, AdminError, games, live, stats};
use crate::app::AnalysisJob;
use crate::{analysis, db};
use serde_json::{Value, json};

/// A legsúlyosabb szoba-beavatkozásokhoz friss jelszó-megerősítés kell.
const SUDO_ROOM_ACTIONS: [&str; 3] = ["end", "void", "disband"];

pub fn register(r: &mut AdminRouter) {
    r.get("/rooms", rooms_list);
    r.get("/rooms/{key}", rooms_detail);
    r.get("/rooms/{key}/racks", rooms_racks);
    r.post_danger("/rooms/{key}/action", rooms_action);

    r.get("/games", games_list);
    r.get("/games/cleanup", games_cleanup_preview);
    r.post_sudo("/games/cleanup", games_cleanup);
    r.get("/games/{game_id}", games_detail);
    r.get("/games/{game_id}/moves", games_moves);
    r.get("/games/{game_id}/export", games_export);
    r.post_danger("/games/{game_id}/void", games_void);
    r.post_danger("/games/{game_id}/unvoid", games_unvoid);
    r.post("/games/{game_id}/unshare", games_unshare);
    r.post("/games/{game_id}/status", games_status);
    r.post_danger("/games/{game_id}/reanalyze", games_reanalyze);
    r.delete_sudo("/games/{game_id}", games_delete);

    r.get("/async", async_list);
    r.post_danger("/async/sweep", async_sweep);
    r.post("/async/{game_id}/extend", async_extend);
    r.post("/async/{game_id}/expire", async_expire);
    r.post_danger("/async/{game_id}/resign", async_resign);
    r.post("/async/{game_id}/remind", async_remind);
    r.post_danger("/async/{game_id}/void", async_void);
    r.post("/async/{game_id}/load", async_load);
    r.post("/async/{game_id}/unload", async_unload);

    r.get("/ratings", ratings_leaderboard);
    r.get("/ratings/suspicious", ratings_suspicious);
    r.post_danger("/ratings/recompute", ratings_recompute);

    r.get("/daily", daily_day);
    r.get("/daily/archive", daily_archive);
    r.get("/daily/preview", daily_preview);
    r.post_danger("/daily/{date}/regenerate", daily_regenerate);
    r.delete_danger("/daily/{date}/scores/{user_id}", daily_delete_score);
    r.post("/daily/{date}/scores/{user_id}/reset-revealed", daily_reset_revealed);
}

// ===== Szobák =====

fn rooms_list(req: &AdminReq) -> HResult {
    let data = {
        let st = req.app.state.lock();
        live::list_rooms(&req.app, &st, &req.query)
    };
    if req.wants_csv() {
        let fields = ["code", "id", "name", "kind", "status", "owner", "players", "spectators", "idle_seconds", "stuck"];
        let rows: Vec<Value> = data["items"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|r| {
                        let mut row = json!({});
                        for field in fields {
                            row[field] = r[field].clone();
                        }
                        row["players"] = flat(&json!(r["players"].as_array().map(|ps| ps.iter().map(|p| p["name"].clone()).collect::<Vec<_>>()).unwrap_or_default()));
                        row
                    })
                    .collect()
            })
            .unwrap_or_default();
        return Ok(csv_response("admin-rooms.csv", &fields, &rows));
    }
    json_ok(data)
}

fn rooms_detail(req: &AdminReq) -> HResult {
    let st = req.app.state.lock();
    let room_id = live::find_room_id(&st, req.param("key"))?;
    json_ok(json!({"room": live::room_detail(&req.app, &st, &room_id)}))
}

/// A játékosok keze: csak kifejezett kérésre, naplózva (`view.racks`).
fn rooms_racks(req: &AdminReq) -> HResult {
    let st = req.app.state.lock();
    json_ok(json!({"racks": live::room_racks(&req.app, &st, req.ctx(), req.param("key"))?}))
}

/// Beavatkozás: message, kick, transfer, skip, extend, pause, resume, resolve_vote, reschedule_bot, save, end, void,
/// disband. A legsúlyosabbakhoz (end, void, disband) sudo kell.
fn rooms_action(req: &AdminReq) -> HResult {
    let action = req.field("action").and_then(|a| a.as_str()).unwrap_or("");
    if SUDO_ROOM_ACTIONS.contains(&action) && !req.sudo_active() {
        return Err(sudo_error());
    }
    let mut st = req.app.state.lock();
    json_ok(live::room_action(&req.app, &mut st, req.ctx(), req.param("key"), &req.body)?)
}

// ===== Játékarchívum =====

fn games_list(req: &AdminReq) -> HResult {
    if req.wants_csv() {
        let data = games::list_games(&req.app, &req.query, Some((200, 0)))?;
        let rows: Vec<Value> = data["items"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|item| {
                        let mut row = item.clone();
                        let players: Vec<String> = item["players"]
                            .as_array()
                            .map(|ps| ps.iter().map(|p| format!("{} ({})", p["name"].as_str().unwrap_or(""), p["score"])).collect())
                            .unwrap_or_default();
                        row["players"] = flat(&json!(players));
                        row["winners"] = flat(&item["winners"]);
                        row
                    })
                    .collect()
            })
            .unwrap_or_default();
        return Ok(csv_response("admin-games.csv", &games::GAME_CSV_FIELDS, &rows));
    }
    json_ok(games::list_games(&req.app, &req.query, None)?)
}

fn games_detail(req: &AdminReq) -> HResult {
    json_ok(json!({"game": games::game_detail(&req.app, req.ctx(), req.id("game_id")?)?}))
}

/// A lépésnapló a tábla-pillanatképekkel (a visszajátszáshoz).
fn games_moves(req: &AdminReq) -> HResult {
    json_ok(json!({"moves": games::game_moves(&req.app, req.id("game_id")?)?}))
}

fn games_export(req: &AdminReq) -> HResult {
    let game_id = req.id("game_id")?;
    let data = games::export_game(&req.app, req.ctx(), game_id)?;
    Ok(json_download(&format!("game-{game_id}.json"), serde_json::to_string_pretty(&data).unwrap_or_default()))
}

fn games_void(req: &AdminReq) -> HResult {
    let summary = games::void_game(&req.app, req.ctx(), req.id("game_id")?, req.reason(), req.field("revoke_badges"))?;
    json_ok(json!({"rating_changes": summary["changed"]}))
}

fn games_unvoid(req: &AdminReq) -> HResult {
    let summary = games::unvoid_game(&req.app, req.ctx(), req.id("game_id")?, req.reason())?;
    json_ok(json!({"rating_changes": summary["changed"]}))
}

fn games_unshare(req: &AdminReq) -> HResult {
    games::unshare_game(&req.app, req.ctx(), req.id("game_id")?, req.reason())?;
    json_ok_empty()
}

fn games_status(req: &AdminReq) -> HResult {
    let ids = live_game_ids(&req.app.state.lock());
    games::set_game_status(&req.app, req.ctx(), req.id("game_id")?, req.field("status"), req.reason(), &ids)?;
    json_ok_empty()
}

/// Az elemzés újrafuttatása: a gyorsítótár törlése és az elemzés elindítása a háttérben.
fn games_reanalyze(req: &AdminReq) -> HResult {
    let game_id = req.id("game_id")?;
    let app = &req.app;
    let moves = admin::action(&app.db, req.ctx(), "game.reanalyze", Some("game"), Some(game_id.to_string()), req.reason(), json!({}), true, |act| {
        let status: Option<String> = act.tx.query_row("SELECT status FROM saved_games WHERE id = ?", [game_id], |r| r.get(0)).ok();
        match status.as_deref() {
            None => return Err(AdminError::new("A játék nem található.", 404)),
            Some("finished") => {}
            Some(_) => return Err(AdminError::new("Csak befejezett játék elemezhető.", 409)),
        }
        let moves: Vec<db::StoredMove> = app.db.get_game_moves_in(act.tx, game_id)?;
        let turns = analysis::analyzable_turns(&moves).len();
        if turns == 0 {
            return Err(AdminError::new("A játékhoz nincs rögzített kéz, nem elemezhető.", 409));
        }
        act.tx.execute("DELETE FROM game_analysis WHERE game_id = ?", [game_id])?;
        act.details.insert("turns".into(), json!(turns));
        Ok(moves)
    })?;
    let total = analysis::analyzable_turns(&moves).len();
    app.analysis_jobs.lock().insert(game_id, AnalysisJob::Running { done: 0, total });
    let app2 = app.clone();
    tokio::task::spawn_blocking(move || crate::server::http::run_analysis(&app2, game_id, moves));
    json_ok(json!({"total": total}))
}

fn games_delete(req: &AdminReq) -> HResult {
    let ids = live_game_ids(&req.app.state.lock());
    let summary = games::delete_game(&req.app, req.ctx(), req.id("game_id")?, req.reason(), req.field("confirm_finished"), &ids)?;
    json_ok(json!({"rating_changes": summary.map(|s| s["changed"].clone()).unwrap_or(json!(0))}))
}

fn games_cleanup_preview(req: &AdminReq) -> HResult {
    json_ok(games::old_abandoned_preview(&req.app, Some(&json!(req.int_arg("days", 30))))?)
}

fn games_cleanup(req: &AdminReq) -> HResult {
    let ids = live_game_ids(&req.app.state.lock());
    let days = req.field("days").cloned().unwrap_or(json!(30));
    let deleted = games::old_abandoned_delete(&req.app, req.ctx(), Some(&days), req.reason(), &ids)?;
    json_ok(json!({"deleted": deleted}))
}

// ===== Levelezős játékok =====

fn async_list(req: &AdminReq) -> HResult {
    let st = req.app.state.lock();
    json_ok(games::list_async(&req.app, &st)?)
}

fn async_sweep(req: &AdminReq) -> HResult {
    json_ok(json!({"expired": games::async_sweep(&req.app, req.ctx())?}))
}

fn async_extend(req: &AdminReq) -> HResult {
    let mut st = req.app.state.lock();
    let deadline = games::async_extend(&req.app, &mut st, req.ctx(), req.id("game_id")?, req.field("hours"), req.reason())?;
    json_ok(json!({"deadline": deadline}))
}

fn async_expire(req: &AdminReq) -> HResult {
    let mut st = req.app.state.lock();
    let player = games::async_expire(&req.app, &mut st, req.ctx(), req.id("game_id")?, req.reason())?;
    json_ok(json!({"player": player}))
}

fn async_resign(req: &AdminReq) -> HResult {
    let mut st = req.app.state.lock();
    let player = req.field("player").and_then(|p| p.as_str());
    games::async_resign(&req.app, &mut st, req.ctx(), req.id("game_id")?, player, req.reason())?;
    json_ok_empty()
}

fn async_remind(req: &AdminReq) -> HResult {
    let mut st = req.app.state.lock();
    json_ok(json!({"sent": games::async_remind(&req.app, &mut st, req.ctx(), req.id("game_id")?)?}))
}

fn async_void(req: &AdminReq) -> HResult {
    let mut st = req.app.state.lock();
    games::async_void(&req.app, &mut st, req.ctx(), req.id("game_id")?, req.reason())?;
    json_ok_empty()
}

fn async_load(req: &AdminReq) -> HResult {
    let mut st = req.app.state.lock();
    games::async_load(&req.app, &mut st, req.ctx(), req.id("game_id")?)?;
    json_ok_empty()
}

fn async_unload(req: &AdminReq) -> HResult {
    let mut st = req.app.state.lock();
    games::async_unload(&req.app, &mut st, req.ctx(), req.id("game_id")?, req.reason())?;
    json_ok_empty()
}

// ===== Ranglista és értékszám =====

fn ratings_leaderboard(req: &AdminReq) -> HResult {
    let metric = req.arg("metric").unwrap_or("rating");
    let limit = req.int_arg("limit", 100);
    let entries = games::leaderboard(&req.app, Some(metric), limit)?;
    if req.wants_csv() {
        let fields = ["rank", "user_id", "display_name", "games_played", "games_won", "win_rate", "avg_score", "best_score", "rating", "rated_games"];
        return Ok(csv_response("admin-leaderboard.csv", &fields, &entries));
    }
    json_ok(json!({"metric": metric, "entries": entries}))
}

fn ratings_suspicious(req: &AdminReq) -> HResult {
    json_ok(games::suspicious_patterns(&req.app)?)
}

/// Teljes ELO újraszámolás: `dry_run` (alapértelmezett) csak előnézet; az alkalmazáshoz sudo és indoklás kell.
fn ratings_recompute(req: &AdminReq) -> HResult {
    let dry_run = req.field("dry_run").map(|v| v != &json!(false)).unwrap_or(true);
    if dry_run {
        let mut data = games::ratings_dry_run(&req.app)?;
        data["dry_run"] = json!(true);
        return json_ok(data);
    }
    req.require_sudo()?;
    let mut data = games::ratings_apply(&req.app, req.ctx(), req.reason())?;
    data["dry_run"] = json!(false);
    json_ok(data)
}

// ===== Napi feladvány =====

fn daily_day(req: &AdminReq) -> HResult {
    json_ok(stats::daily_day(&req.app, req.arg("date"))?)
}

fn daily_archive(req: &AdminReq) -> HResult {
    json_ok(json!({"items": stats::daily_archive(&req.app, req.arg("days").or(Some("30")))?}))
}

fn daily_preview(req: &AdminReq) -> HResult {
    json_ok(stats::daily_preview(req.arg("date").unwrap_or(""))?)
}

fn daily_regenerate(req: &AdminReq) -> HResult {
    let result = stats::daily_regenerate(&req.app, req.ctx(), req.param("date"), req.reason(), req.field("reset_scores"), req.sudo_active())?;
    json_ok(result)
}

fn daily_delete_score(req: &AdminReq) -> HResult {
    stats::daily_delete_score(&req.app, req.ctx(), req.param("date"), req.id("user_id")?, req.reason())?;
    json_ok_empty()
}

fn daily_reset_revealed(req: &AdminReq) -> HResult {
    stats::daily_reset_revealed(&req.app, req.ctx(), req.param("date"), req.id("user_id")?, req.reason())?;
    json_ok_empty()
}
