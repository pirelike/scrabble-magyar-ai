//! Admin panel: játékarchívum, levelezős játékok, értékszám (újraszámolás, gyanús minták) és napi feladvány (a
//! Python `test_admin_games.py` megfelelője).

mod common;
use common::admin::*;
use common::push::PushSink;
use common::*;
use scrabble::board::Board;
use scrabble::daily;
use scrabble::db::{GamePlayerData, RowExt};
use scrabble::server::core;
use scrabble::util;
use serde_json::{Value, json};
use std::collections::HashSet;

const REASON: &str = "Teszt indoklás";

struct Env {
    server: TestServer,
    api: Http,
    a: i64,
    b: i64,
}

async fn env() -> Env {
    let server = TestServer::start().await;
    let api = server.admin_http().await;
    let a = server.create_user("anna@example.com", "Anna");
    let b = server.create_user("bela@example.com", "Béla");
    Env { server, api, a, b }
}

fn rating(e: &Env, uid: i64) -> (i64, i64) {
    let u = e.server.app.db.get_user_by_id(uid).unwrap().unwrap();
    (u.rating, u.rated_games)
}

fn game_status(e: &Env, game_id: i64) -> String {
    e.server.app.db.get_game_by_id(game_id).unwrap().unwrap().text("status")
}

fn exec(e: &Env, sql: &str) {
    e.server.app.db.with(|tx| tx.execute_batch(sql)).unwrap();
}

fn pair(e: &Env, room: &str, moves: i64) -> i64 {
    finished_game(&e.server, room, &[(Some(e.a), "Anna", 120), (Some(e.b), "Béla", 80)], false, &[], moves)
}

async fn game_ids(e: &Env, query: &str) -> Vec<i64> {
    let response = e.api.admin_get(&format!("/games{query}")).await;
    assert_eq!(response.status, 200, "{}", response.text());
    items_of(&response, "id").iter().map(|v| v.as_i64().unwrap()).collect()
}

// ===================================================================================================
// Archívum
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn lists_games_with_players_and_winners() {
    let e = env().await;
    let game_id = pair(&e, "r1", 4);
    let row = e.api.admin_get("/games").await.json()["items"][0].clone();
    assert_eq!((row["id"].clone(), row["status"].clone(), row["moves"].clone()), (json!(game_id), json!("finished"), json!(4)));
    assert_eq!(row["winners"], json!(["Anna"]));
    assert_eq!(row["players"].as_array().unwrap().iter().map(|p| p["name"].clone()).collect::<Vec<_>>(), vec![json!("Anna"), json!("Béla")]);
    assert_eq!((row["bots"].clone(), row["async"].clone(), row["shared"].clone()), (json!(false), json!(false), json!(false)));
}

#[tokio::test(flavor = "multi_thread")]
async fn archive_filters() {
    let e = env().await;
    let g1 = pair(&e, "r1", 0);
    let g2 = finished_game(&e.server, "r2", &[(Some(e.a), "Anna", 50), (None, "Robi", 80)], true, &[], 0);
    e.server.app.db.abandon_game("nincs");
    assert_eq!(game_ids(&e, "?bots=1").await, vec![g2]);
    assert_eq!(game_ids(&e, "?bots=0").await, vec![g1]);
    assert_eq!(game_ids(&e, &format!("?user={}", e.b)).await, vec![g1]);
    let mut by_name = game_ids(&e, "?user=Anna").await;
    by_name.sort();
    assert_eq!(by_name, vec![g1, g2]);
    assert!(game_ids(&e, "?status=abandoned").await.is_empty());
    assert_eq!(game_ids(&e, "?status=finished&q=r2").await, vec![g2]);
    assert_eq!(game_ids(&e, &format!("?q={g1}")).await, vec![g1]);
    assert!(game_ids(&e, "?since=2999-01-01").await.is_empty());
    assert_eq!(e.api.admin_get("/games?status=rossz").await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_detail_shows_hands_and_is_logged() {
    let e = env().await;
    let game_id = pair(&e, "r1", 2);
    let data = e.api.admin_get(&format!("/games/{game_id}")).await.json()["game"].clone();
    assert_eq!(data["players"].as_array().unwrap().iter().map(|p| p["rating_change"].clone()).collect::<Vec<_>>(), vec![json!(16), json!(-16)]);
    assert_eq!((data["moves"][0]["rack"].clone(), data["moves"][0]["type"].clone()), (json!(["A"]), json!("pass")));
    assert_eq!(audit_of(&e.server, "view.game")[0]["target_id"], json!(game_id.to_string()));
    assert_eq!(e.api.admin_get("/games/9999").await.status, 404);
}

#[tokio::test(flavor = "multi_thread")]
async fn moves_with_snapshots() {
    let e = env().await;
    let game_id = pair(&e, "r1", 2);
    let moves = e.api.admin_get(&format!("/games/{game_id}/moves")).await.json()["moves"].clone();
    assert_eq!(moves.as_array().unwrap().iter().map(|m| m["n"].clone()).collect::<Vec<_>>(), vec![json!(1), json!(2)]);
    assert_eq!(moves[0]["board"], json!({}));
}

#[tokio::test(flavor = "multi_thread")]
async fn games_csv() {
    let e = env().await;
    pair(&e, "r1", 0);
    assert!(e.api.admin_get("/games?format=csv").await.text().contains("Anna (120); Béla (80)"));
}

// ===================================================================================================
// Érvénytelenítés
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn voiding_recomputes_the_ratings_and_unvoiding_restores_them() {
    let e = env().await;
    pair(&e, "r1", 0);
    let after_one = (rating(&e, e.a), rating(&e, e.b));
    let g2 = pair(&e, "r2", 0);
    let after_two = (rating(&e, e.a), rating(&e, e.b));
    assert!(after_two.0.0 > after_one.0.0);
    let response = e.api.admin_post(&format!("/games/{g2}/void"), json!({"reason": REASON})).await;
    assert_eq!(response.status, 200);
    assert_eq!(response.json()["rating_changes"], 2);
    assert_eq!((rating(&e, e.a), rating(&e, e.b)), after_one);
    assert_eq!(game_status(&e, g2), "voided");
    assert_eq!(e.server.app.db.get_user_by_id(e.a).unwrap().unwrap().games_played, 1);
    let board = e.server.app.db.get_leaderboard("wins", 50, None, true).unwrap().0;
    assert_eq!(board.iter().filter(|r| r["user_id"] == e.a).map(|r| r["games_played"].clone()).collect::<Vec<_>>(), vec![json!(1)]);
    assert_eq!(e.api.admin_post(&format!("/games/{g2}/unvoid"), json!({"reason": REASON})).await.status, 200);
    assert_eq!((rating(&e, e.a), rating(&e, e.b)), after_two);
    assert_eq!(e.server.app.db.get_user_by_id(e.a).unwrap().unwrap().games_played, 2);
    let before: Vec<Value> = audit_of(&e.server, "game.void").iter().map(|r| r["details"]["before"].clone()).collect();
    assert_eq!(before, vec![json!({"status": "finished"})]);
}

#[tokio::test(flavor = "multi_thread")]
async fn badges_are_only_revoked_on_request() {
    let e = env().await;
    let game_id = pair(&e, "r1", 0);
    e.server.app.db.grant_achievements(e.a, &HashSet::from(["bingo".to_string()]), Some(game_id));
    let badges = |e: &Env| e.server.app.db.get_user_achievements(e.a).iter().map(|b| b["badge"].clone()).collect::<Vec<_>>();
    e.api.admin_post(&format!("/games/{game_id}/void"), json!({"reason": REASON})).await;
    assert_eq!(badges(&e), vec![json!("bingo")]);
    e.api.admin_post(&format!("/games/{game_id}/unvoid"), json!({"reason": REASON})).await;
    e.api.admin_post(&format!("/games/{game_id}/void"), json!({"reason": REASON, "revoke_badges": true})).await;
    assert!(badges(&e).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn only_finished_games_can_be_voided_and_a_reason_is_needed() {
    let e = env().await;
    let game_id = pair(&e, "r1", 0);
    assert_eq!(e.api.admin_post(&format!("/games/{game_id}/void"), json!({})).await.status, 400);
    assert_eq!(e.api.admin_post(&format!("/games/{game_id}/unvoid"), json!({"reason": REASON})).await.status, 409);
    exec(&e, &format!("UPDATE saved_games SET status = 'abandoned' WHERE id = {game_id}"));
    assert_eq!(e.api.admin_post(&format!("/games/{game_id}/void"), json!({"reason": REASON})).await.status, 409);
}

// ===================================================================================================
// Játék-karbantartás
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn unsharing() {
    let e = env().await;
    let game_id = pair(&e, "r1", 0);
    assert_eq!(e.api.admin_post(&format!("/games/{game_id}/unshare"), json!({"reason": REASON})).await.status, 409);
    let token = e.server.app.db.get_or_create_share_token(game_id).unwrap().unwrap();
    assert_eq!(e.api.admin_get(&format!("/games/{game_id}")).await.json()["game"]["share_token"], json!(token));
    assert_eq!(e.api.admin_post(&format!("/games/{game_id}/unshare"), json!({"reason": REASON})).await.status, 200);
    assert!(e.server.app.db.get_game_by_share_token(&token).unwrap().is_none());
}

fn saved(e: &Env, room: &str) -> i64 {
    let players = [GamePlayerData { player_name: "Anna".into(), user_id: Some(e.a), score: 0, is_winner: false, resigned: false }];
    e.server.app.db.save_game(room, "Mentés", "{}", false, &players, "", None, false, false).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_status_toggle() {
    let e = env().await;
    let game_id = saved(&e, "x1");
    let set = |status: &str| json!({"status": status, "reason": REASON});
    assert_eq!(e.api.admin_post(&format!("/games/{game_id}/status"), set("abandoned")).await.status, 200);
    assert_eq!(game_status(&e, game_id), "abandoned");
    assert_eq!(e.api.admin_post(&format!("/games/{game_id}/status"), set("abandoned")).await.status, 409);
    assert_eq!(e.api.admin_post(&format!("/games/{game_id}/status"), set("active")).await.status, 200);
    assert_eq!(e.api.admin_post(&format!("/games/{game_id}/status"), set("voided")).await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_running_game_cannot_be_changed() {
    let e = env().await;
    let (anna, bela) = (e.server.guest("Anna").await, e.server.guest("Béla").await);
    live_room(&e.server, &anna, &[&bela], json!({})).await;
    let id = with_room(&e.server, |room| room.db_game_id).expect("mentett játék");
    assert_eq!(e.api.admin_post(&format!("/games/{id}/status"), json!({"status": "abandoned", "reason": REASON})).await.status, 409);
    e.api.sudo().await;
    assert_eq!(e.api.admin_delete(&format!("/games/{id}"), json!({"reason": REASON})).await.status, 409);
}

#[tokio::test(flavor = "multi_thread")]
async fn game_export() {
    let e = env().await;
    let game_id = pair(&e, "r1", 2);
    let response = e.api.admin_get(&format!("/games/{game_id}/export")).await;
    let data: Value = serde_json::from_str(&response.text()).unwrap();
    assert_eq!(data["game"]["id"], game_id);
    assert_eq!((data["moves"].as_array().unwrap().len(), data["players"].as_array().unwrap().len()), (2, 2));
    assert!(!audit_of(&e.server, "view.game_export").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn reanalyzing_clears_the_cache_and_starts_a_job() {
    let e = env().await;
    let game_id = pair(&e, "r1", 3);
    e.server.app.db.save_game_analysis(game_id, 1, "{}").unwrap();
    let response = e.api.admin_post(&format!("/games/{game_id}/reanalyze"), json!({"reason": REASON})).await;
    assert_eq!(response.status, 200);
    assert_eq!(response.json()["total"], 3);
    // a gyorsítótár törölve (a háttérfeladat a hiányos pillanatképekből nem ment újat)
    tokio::time::sleep(std::time::Duration::from_millis(700)).await;
    assert!(e.server.app.db.get_game_analysis(game_id).is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn reanalyzing_needs_recorded_hands() {
    let e = env().await;
    let game_id = pair(&e, "r1", 0);
    assert_eq!(e.api.admin_post(&format!("/games/{game_id}/reanalyze"), json!({"reason": REASON})).await.status, 409);
}

// ===================================================================================================
// Törlés
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn a_finished_game_needs_the_extra_confirmation() {
    let e = env().await;
    let game_id = pair(&e, "r1", 0);
    assert_eq!(e.api.admin_delete(&format!("/games/{game_id}"), json!({"reason": REASON})).await.status, 401);
    e.api.sudo().await;
    let response = e.api.admin_delete(&format!("/games/{game_id}"), json!({"reason": REASON})).await;
    assert_eq!(response.status, 409);
    assert_eq!(response.json()["needs_confirm"], true);
    assert!(e.server.app.db.get_game_by_id(game_id).unwrap().is_some());
    let response = e.api.admin_delete(&format!("/games/{game_id}"), json!({"reason": REASON, "confirm_finished": true})).await;
    assert_eq!(response.status, 200);
    assert!(e.server.app.db.get_game_by_id(game_id).unwrap().is_none());
    assert_eq!(rating(&e, e.a), (1200, 0));
    assert_eq!(e.server.app.db.get_user_by_id(e.a).unwrap().unwrap().games_played, 0);
    assert_eq!(audit_of(&e.server, "game.delete")[0]["details"]["before"]["name"], "Játék r1");
}

#[tokio::test(flavor = "multi_thread")]
async fn other_games_are_deleted_without_the_confirmation() {
    let e = env().await;
    let game_id = saved(&e, "x1");
    e.server.app.db.abandon_game_by_id(game_id);
    e.api.sudo().await;
    assert_eq!(e.api.admin_delete(&format!("/games/{game_id}"), json!({"reason": REASON})).await.status, 200);
    assert!(e.server.app.db.get_game_by_id(game_id).unwrap().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn bulk_cleanup_of_old_abandoned_saves() {
    let e = env().await;
    let (old, new, keep) = (saved(&e, "x1"), saved(&e, "x2"), saved(&e, "x3"));
    e.server.app.db.abandon_game_by_id(old);
    e.server.app.db.abandon_game_by_id(new);
    exec(&e, &format!("UPDATE saved_games SET updated_at = '2020-01-01 00:00:00' WHERE id = {old}"));
    let preview = e.api.admin_get("/games/cleanup?days=30").await.json();
    assert_eq!(preview["total"], 1);
    assert_eq!(preview["items"].as_array().unwrap().iter().map(|g| g["id"].clone()).collect::<Vec<_>>(), vec![json!(old)]);
    assert_eq!(e.api.admin_post("/games/cleanup", json!({"days": 30, "reason": REASON})).await.status, 401);
    e.api.sudo().await;
    assert_eq!(e.api.admin_post("/games/cleanup", json!({"days": 30, "reason": REASON})).await.json()["deleted"], 1);
    assert!(e.server.app.db.get_game_by_id(old).unwrap().is_none());
    assert!(e.server.app.db.get_game_by_id(new).unwrap().is_some() && e.server.app.db.get_game_by_id(keep).unwrap().is_some());
}

// ===================================================================================================
// Értékszám újraszámolása
// ===================================================================================================

fn rating_games(e: &Env, c: i64) {
    let s = &e.server;
    let (a, b) = (e.a, e.b);
    finished_game(s, "r1", &[(Some(a), "Anna", 120), (Some(b), "Béla", 80), (Some(c), "Cili", 60)], false, &[], 0);
    finished_game(s, "r2", &[(Some(a), "Anna", 70), (Some(b), "Béla", 90)], false, &[], 0);
    finished_game(s, "r3", &[(Some(c), "Cili", 100), (Some(b), "Béla", 100)], false, &[], 0);
    finished_game(s, "r4", &[(Some(a), "Anna", 120), (Some(b), "Béla", 0)], false, &["Béla"], 0);
    finished_game(s, "r5", &[(Some(a), "Anna", 90), (None, "Vendég", 10)], false, &[], 0); // vendég: nincs értékelés
    finished_game(s, "r6", &[(Some(a), "Anna", 90), (Some(c), "Cili", 10)], true, &[], 0); // robotos: nincs értékelés
}

#[tokio::test(flavor = "multi_thread")]
async fn the_recomputation_matches_the_step_by_step_result() {
    let e = env().await;
    let admin_id = e.server.user_id(ADMIN_EMAIL);
    let c = e.server.create_user("cili@example.com", "Cili");
    rating_games(&e, c);
    let expected: Vec<(i64, i64)> = [e.a, e.b, c].iter().map(|u| rating(&e, *u)).collect();
    let rows = e.server.app.db.get_game_rating_changes(1).unwrap();
    exec(&e, "UPDATE users SET rating = 1500, rated_games = 99; UPDATE game_players SET rating_before = 1, rating_after = 2");
    let dry = e.api.admin_post("/ratings/recompute", json!({})).await.json();
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["changed"], 4); // az admin fiók is (1500 → 1200)
    assert_eq!(rating(&e, e.a), (1500, 99)); // az előnézet nem ír
    let changed: HashSet<i64> = dry["items"].as_array().unwrap().iter().map(|i| i["user_id"].as_i64().unwrap()).collect();
    assert_eq!(changed, HashSet::from([e.a, e.b, c, admin_id]));
    assert_eq!(e.api.admin_post("/ratings/recompute", json!({"dry_run": false, "reason": REASON})).await.status, 401);
    e.api.sudo().await;
    let applied = e.api.admin_post("/ratings/recompute", json!({"dry_run": false, "reason": REASON})).await.json();
    assert_eq!(applied["changed"], 4);
    assert_eq!([e.a, e.b, c].iter().map(|u| rating(&e, *u)).collect::<Vec<_>>(), expected);
    assert_eq!(e.server.app.db.get_game_rating_changes(1).unwrap(), rows); // a játékonkénti előzmény is
    assert_eq!(audit_of(&e.server, "ratings.recompute")[0]["details"]["changed"], 4);
    assert_eq!(e.api.admin_post("/ratings/recompute", json!({})).await.json()["changed"], 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn manual_adjustments_are_part_of_the_timeline() {
    let e = env().await;
    let g1 = pair(&e, "r1", 0);
    e.api.sudo().await;
    e.api.admin_post(&format!("/users/{}/rating", e.a), json!({"rating": 1500, "reason": REASON})).await;
    let g2 = pair(&e, "r2", 0);
    let expected = rating(&e, e.a);
    // az időrend a másodperc-pontos időbélyegekből áll össze
    exec(
        &e,
        &format!(
            "UPDATE saved_games SET updated_at = '2026-01-01 10:00:00' WHERE id = {g1}; \
             UPDATE rating_adjustments SET created_at = '2026-01-02 10:00:00'; \
             UPDATE saved_games SET updated_at = '2026-01-03 10:00:00' WHERE id = {g2}; \
             UPDATE users SET rating = 1"
        ),
    );
    e.api.admin_post("/ratings/recompute", json!({"dry_run": false, "reason": REASON})).await;
    assert_eq!(rating(&e, e.a), expected);
}

#[tokio::test(flavor = "multi_thread")]
async fn recomputing_without_games_resets_to_the_start() {
    let e = env().await;
    exec(&e, "UPDATE users SET rating = 1400, rated_games = 5");
    e.api.sudo().await;
    e.api.admin_post("/ratings/recompute", json!({"dry_run": false, "reason": REASON})).await;
    assert_eq!(rating(&e, e.a), (1200, 0));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_leaderboard_without_the_minimum() {
    let e = env().await;
    pair(&e, "r1", 0);
    assert!(e.server.app.db.get_leaderboard("rating", 50, None, false).unwrap().0.is_empty()); // a nyilvános: legalább 3 játék
    let data = e.api.admin_get("/ratings?metric=rating").await.json();
    assert_eq!(data["entries"].as_array().unwrap().iter().map(|x| x["display_name"].clone()).collect::<Vec<_>>(), vec![json!("Anna"), json!("Béla")]);
    assert_eq!(e.api.admin_get("/ratings?metric=rossz").await.status, 400);
    assert!(e.api.admin_get("/ratings?format=csv").await.text().contains("rank,user_id"));
}

// ===================================================================================================
// Gyanús minták
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn a_one_sided_pair() {
    let e = env().await;
    for i in 0..6 {
        finished_game(&e.server, &format!("r{i}"), &[(Some(e.a), "Anna", 100), (Some(e.b), "Béla", 50)], false, &[], 0);
    }
    let data = e.api.admin_get("/ratings/suspicious").await.json();
    let pair = &data["one_sided_pairs"][0];
    assert_eq!(pair["games"], 6);
    let wins: std::collections::HashMap<String, i64> = pair["users"].as_array().unwrap().iter().map(|u| (u["name"].as_str().unwrap().to_string(), u["wins"].as_i64().unwrap())).collect();
    assert_eq!(wins, std::collections::HashMap::from([("Anna".to_string(), 6), ("Béla".to_string(), 0)]));
    assert_eq!(game_ids(&e, "?suspicious=1").await.len(), 6);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_balanced_pair_is_not_flagged() {
    let e = env().await;
    for i in 0..6 {
        let (x, y) = if i % 2 == 1 { (100, 50) } else { (50, 100) };
        finished_game(&e.server, &format!("r{i}"), &[(Some(e.a), "Anna", x), (Some(e.b), "Béla", y)], false, &[], 0);
    }
    assert_eq!(e.api.admin_get("/ratings/suspicious").await.json()["one_sided_pairs"], json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_shared_ip() {
    let e = env().await;
    e.server.app.db.record_login_event("anna@example.com", true, Some("198.51.100.9"), Some("UA"), Some(e.a), "");
    e.server.app.db.record_login_event("bela@example.com", true, Some("198.51.100.9"), Some("UA"), Some(e.b), "");
    let game_id = finished_game(&e.server, "r1", &[(Some(e.a), "Anna", 100), (Some(e.b), "Béla", 50)], false, &[], 0);
    let entry = e.api.admin_get("/ratings/suspicious").await.json()["same_ip"][0].clone();
    assert_eq!((entry["ips"].clone(), entry["game_ids"].clone()), (json!(["198.51.100.9"]), json!([game_id])));
}

#[tokio::test(flavor = "multi_thread")]
async fn fast_passing_games() {
    let e = env().await;
    let game_id = finished_game(&e.server, "r1", &[(Some(e.a), "Anna", 0), (Some(e.b), "Béla", 0)], false, &[], 8);
    let long_game = finished_game(&e.server, "r2", &[(Some(e.a), "Anna", 5), (Some(e.b), "Béla", 3)], false, &[], 40);
    let flagged: Vec<Value> = e.api.admin_get("/ratings/suspicious").await.json()["fast_pass_games"].as_array().unwrap().iter().map(|g| g["game_id"].clone()).collect();
    assert_eq!(flagged, vec![json!(game_id)]);
    assert!(!flagged.contains(&json!(long_game)));
}

#[tokio::test(flavor = "multi_thread")]
async fn consistently_best_moves() {
    let e = env().await;
    for i in 0..3 {
        let game_id = finished_game(&e.server, &format!("r{i}"), &[(Some(e.a), "Anna", 100), (Some(e.b), "Béla", 50)], false, &[], 0);
        let analysis = json!({"players": [{"player": "Anna", "turns": 12, "efficiency": 99.0}, {"player": "Béla", "turns": 12, "efficiency": 70.0}]});
        e.server.app.db.save_game_analysis(game_id, 6, &analysis.to_string()).unwrap();
    }
    let entries = e.api.admin_get("/ratings/suspicious").await.json()["high_efficiency"].clone();
    let summary: Vec<(Value, Value, Value)> = entries.as_array().unwrap().iter().map(|x| (x["name"].clone(), x["games"].clone(), x["average"].clone())).collect();
    assert_eq!(summary, vec![(json!("Anna"), json!(3), json!(99.0))]);
}

// ===================================================================================================
// Levelezős játékok
// ===================================================================================================

struct AsyncEnv {
    server: TestServer,
    api: Http,
    anna: Sio,
    a_id: i64,
    bela: Sio,
    room_id: String,
    game_id: i64,
}

async fn async_env() -> AsyncEnv {
    let server = TestServer::start().await;
    *server.app.async_starter.lock() = Some(0);
    let api = server.admin_http().await;
    let a_id = server.create_user("anna@example.com", "Anna");
    let b_id = server.create_user("bela@example.com", "Béla");
    assert!(server.app.db.send_friend_request(a_id, b_id).0);
    assert!(server.app.db.accept_friend_request(b_id, a_id).0);
    let anna = sio_user(&server, a_id, "Anna").await;
    let bela = sio_user(&server, b_id, "Béla").await;
    anna.call("create_async_game", json!({"name": "Esti játék", "friend_ids": [b_id], "turn_hours": 24})).await;
    bela.take();
    let (room_id, game_id) = {
        let st = server.app.state.lock();
        let room = st.rooms.values().find(|r| r.is_async).expect("levelezős szoba");
        (room.id.clone(), room.db_game_id.unwrap())
    };
    AsyncEnv { server, api, anna, a_id, bela, room_id, game_id }
}

fn async_room<R>(a: &AsyncEnv, f: impl FnOnce(&mut scrabble::room::Room) -> R) -> R {
    let mut st = a.server.app.state.lock();
    f(st.rooms.get_mut(&a.room_id).expect("szoba"))
}

#[tokio::test(flavor = "multi_thread")]
async fn the_async_list() {
    let a = async_env().await;
    let data = a.api.admin_get("/async").await.json();
    let row = &data["items"][0];
    assert_eq!((row["id"].clone(), row["loaded"].clone(), row["current"].clone()), (json!(a.game_id), json!(true), json!("Anna")));
    assert!(row["remaining"].as_f64().unwrap() > 23.0 * 3600.0);
    assert_eq!(row["players"].as_array().unwrap().iter().map(|p| p["name"].clone()).collect::<Vec<_>>(), vec![json!("Anna"), json!("Béla")]);
    assert_eq!(data["sweeper"]["name"], "async_sweeper");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_list_includes_games_that_are_not_loaded() {
    let a = async_env().await;
    a.anna.close();
    a.bela.close();
    a.anna.settle_long().await;
    {
        let mut st = a.server.app.state.lock();
        core::cleanup_room(&a.server.app, &mut st, &a.room_id);
    }
    let row = a.api.admin_get("/async").await.json()["items"][0].clone();
    assert_eq!((row["id"].clone(), row["loaded"].clone(), row["current"].clone()), (json!(a.game_id), json!(false), json!("Anna")));
    assert_eq!(a.api.admin_post(&format!("/async/{}/load", a.game_id), json!({})).await.status, 200);
    assert!(a.server.app.state.lock().rooms.values().any(|r| r.is_async));
}

#[tokio::test(flavor = "multi_thread")]
async fn extending_the_deadline() {
    let a = async_env().await;
    let before = async_room(&a, |r| r.game.turn_deadline.unwrap());
    assert_eq!(a.api.admin_post(&format!("/async/{}/extend", a.game_id), json!({"hours": 24, "reason": REASON})).await.status, 200);
    let after = async_room(&a, |r| r.game.turn_deadline.unwrap());
    assert!((after - (before + 24.0 * 3600.0)).abs() < 5.0);
    assert_eq!(a.api.admin_post(&format!("/async/{}/extend", a.game_id), json!({"hours": 5, "reason": REASON})).await.status, 400);
    let row = audit_of(&a.server, "async.extend")[0]["details"].clone();
    assert_eq!(row["hours"], 24);
    assert!(row["after"]["deadline"].as_f64().unwrap() > row["before"]["deadline"].as_f64().unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_expired_deadline_is_extended_from_now() {
    let a = async_env().await;
    async_room(&a, |r| r.game.turn_deadline = Some(util::now() - 7200.0));
    a.api.admin_post(&format!("/async/{}/extend", a.game_id), json!({"hours": 6, "reason": REASON})).await;
    let deadline = async_room(&a, |r| r.game.turn_deadline.unwrap());
    assert!((deadline - (util::now() + 6.0 * 3600.0)).abs() < 10.0);
}

#[tokio::test(flavor = "multi_thread")]
async fn expiring_now() {
    let a = async_env().await;
    let response = a.api.admin_post(&format!("/async/{}/expire", a.game_id), json!({"reason": REASON})).await;
    assert_eq!(response.json()["player"], "Anna");
    let (current, timeouts) = async_room(&a, |r| (r.game.current_player().unwrap().name.clone(), r.game.players.iter().find(|p| p.name == "Anna").unwrap().timeouts));
    assert_eq!((current.as_str(), timeouts), ("Béla", 1));
}

#[tokio::test(flavor = "multi_thread")]
async fn resigning_on_behalf_of_a_player() {
    let a = async_env().await;
    assert_eq!(a.api.admin_post(&format!("/async/{}/resign", a.game_id), json!({"player": "Senki", "reason": REASON})).await.status, 404);
    assert_eq!(a.api.admin_post(&format!("/async/{}/resign", a.game_id), json!({"player": "Anna", "reason": REASON})).await.status, 200);
    assert_eq!(a.server.app.db.get_game_by_id(a.game_id).unwrap().unwrap().text("status"), "finished");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reminder_is_pushed_to_the_current_player() {
    let a = async_env().await;
    let sink = PushSink::start();
    let sub = sink.subscription("send/x");
    a.server.app.db.save_push_subscription(a.a_id, &sub.endpoint, &sub.p256dh, &sub.auth, "hu").unwrap();
    let data = a.api.admin_post(&format!("/async/{}/remind", a.game_id), json!({})).await.json();
    assert_eq!(data["sent"], true);
    let got = sink.wait_for(1, 5).await;
    assert!(got[0].payload["body"].as_str().unwrap().contains("Esti játék"), "{:?}", got[0].payload);
    assert!(!audit_of(&a.server, "async.remind").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn voiding_an_async_game_removes_the_room() {
    let a = async_env().await;
    assert_eq!(a.api.admin_post(&format!("/async/{}/void", a.game_id), json!({"reason": REASON})).await.status, 200);
    assert_eq!(a.server.app.db.get_game_by_id(a.game_id).unwrap().unwrap().text("status"), "voided");
    assert!(!a.server.app.state.lock().rooms.contains_key(&a.room_id));
}

#[tokio::test(flavor = "multi_thread")]
async fn unloading_only_when_nobody_is_inside() {
    let a = async_env().await;
    assert_eq!(a.api.admin_post(&format!("/async/{}/unload", a.game_id), json!({"reason": REASON})).await.status, 409);
    a.anna.call("leave_room", Value::Null).await;
    a.bela.call("leave_room", Value::Null).await;
    assert_eq!(a.api.admin_post(&format!("/async/{}/unload", a.game_id), json!({"reason": REASON})).await.status, 200);
    assert!(!a.server.app.state.lock().rooms.contains_key(&a.room_id));
    assert_eq!(a.server.app.db.get_game_by_id(a.game_id).unwrap().unwrap().text("status"), "active");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_manual_sweep() {
    let a = async_env().await;
    async_room(&a, |r| r.game.turn_deadline = Some(util::now() - 10.0));
    assert_eq!(a.api.admin_post("/async/sweep", json!({})).await.json()["expired"], 1);
    assert_eq!(async_room(&a, |r| r.game.current_player().unwrap().name.clone()), "Béla");
    assert!(!audit_of(&a.server, "async.sweep").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_async_game() {
    let e = env().await;
    assert_eq!(e.api.admin_post("/async/999/expire", json!({"reason": REASON})).await.status, 404);
}

// ===================================================================================================
// Napi feladvány
// ===================================================================================================

fn store_puzzle(e: &Env) -> String {
    let date = daily::today_str();
    e.server.app.db.save_daily_puzzle(&date, &Board::new().to_json(), &vec!["A".to_string(); 7], 60, &json!({"tiles": [], "words": ["ALMA"], "score": 60})).unwrap();
    date
}

#[tokio::test(flavor = "multi_thread")]
async fn the_day_overview() {
    let e = env().await;
    let date = store_puzzle(&e);
    let c = e.server.create_user("c@example.com", "Cili");
    e.server.app.db.record_daily_score(&date, e.a, 60).unwrap();
    e.server.app.db.record_daily_score(&date, e.b, 30).unwrap();
    e.server.app.db.record_daily_score(&date, e.b, 33).unwrap();
    e.server.app.db.mark_daily_revealed(&date, c);
    let data = e.api.admin_get("/daily").await.json();
    assert_eq!((data["participants"].clone(), data["revealed"].clone(), data["puzzle"]["best_score"].clone()), (json!(2), json!(1), json!(60)));
    assert_eq!((data["attempts"]["1"].clone(), data["attempts"]["2"].clone()), (json!(1), json!(1)));
    assert_eq!((data["scores"]["perfect"].clone(), data["scores"]["half"].clone()), (json!(1), json!(1)));
    let top: Vec<Value> = data["leaderboard"].as_array().unwrap().iter().take(2).map(|x| x["display_name"].clone()).collect();
    assert_eq!(top, vec![json!("Anna"), json!("Béla")]);
    assert_eq!(e.api.admin_get("/daily?date=hib%C3%A1s").await.status, 400);
    assert_eq!(e.api.admin_get("/daily?date=2999-01-01").await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_archive() {
    let e = env().await;
    let date = store_puzzle(&e);
    e.server.app.db.record_daily_score(&date, e.a, 45).unwrap();
    let row = e.api.admin_get("/daily/archive").await.json()["items"][0].clone();
    assert_eq!((row["date"].clone(), row["participants"].clone()), (json!(date), json!(1)));
    assert_eq!(row["avg_score"].as_f64(), Some(45.0));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_preview_does_not_save() {
    let e = env().await;
    let data = e.api.admin_get("/daily/preview?date=2999-01-01").await.json();
    assert_eq!(data["rack"].as_array().unwrap().len(), 7);
    let best = data["best_score"].as_i64().unwrap();
    assert!((daily::MIN_BEST_SCORE as i64..=daily::MAX_BEST_SCORE as i64).contains(&best));
    assert!(e.server.app.db.get_daily_puzzle("2999-01-01").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn regeneration_before_anyone_played() {
    let e = env().await;
    let date = store_puzzle(&e);
    let response = e.api.admin_post(&format!("/daily/{date}/regenerate"), json!({"reason": REASON})).await;
    assert_eq!(response.status, 200, "{}", response.text());
    let puzzle = e.server.app.db.get_daily_puzzle(&date).unwrap();
    assert_ne!(puzzle.rack, vec!["A".to_string(); 7]);
    let row = audit_of(&e.server, "daily.regenerate")[0]["details"].clone();
    assert_eq!(row["before"], json!({"best_score": 60}));
    assert_eq!(row["after"], json!({"best_score": puzzle.best_score}));
}

#[tokio::test(flavor = "multi_thread")]
async fn regeneration_after_attempts_needs_sudo() {
    let e = env().await;
    let date = store_puzzle(&e);
    e.server.app.db.record_daily_score(&date, e.a, 45).unwrap();
    let response = e.api.admin_post(&format!("/daily/{date}/regenerate"), json!({"reason": REASON})).await;
    assert_eq!(response.status, 401);
    assert_eq!(response.json()["attempts"], 1);
    assert_eq!(e.server.app.db.get_daily_puzzle(&date).unwrap().best_score, 60);
    e.api.sudo().await;
    assert_eq!(e.api.admin_post(&format!("/daily/{date}/regenerate"), json!({"reason": REASON, "reset_scores": true})).await.status, 200);
    assert!(e.server.app.db.get_daily_leaderboard(&date, 10, None).0.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn deleting_a_score_and_resetting_the_revealed_flag() {
    let e = env().await;
    let date = store_puzzle(&e);
    e.server.app.db.record_daily_score(&date, e.a, 45).unwrap();
    e.server.app.db.mark_daily_revealed(&date, e.b);
    let reset = |uid: i64| format!("/daily/{date}/scores/{uid}/reset-revealed");
    assert_eq!(e.api.admin_post(&reset(e.a), json!({"reason": REASON})).await.status, 404);
    assert_eq!(e.api.admin_post(&reset(e.b), json!({"reason": REASON})).await.status, 200);
    assert!(!e.server.app.db.get_daily_entry(&date, e.b).unwrap().revealed);
    let score = format!("/daily/{date}/scores/{}", e.a);
    assert_eq!(e.api.admin_delete(&score, json!({"reason": REASON})).await.status, 200);
    assert!(e.server.app.db.get_daily_entry(&date, e.a).is_none());
    assert_eq!(e.api.admin_delete(&score, json!({"reason": REASON})).await.status, 404);
}
