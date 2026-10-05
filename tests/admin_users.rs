//! Admin panel: felhasználók — lista, részletek és a műveletek: kitiltás, némítás, anonimizálás... (a Python
//! `test_admin_users.py` megfelelője).

mod common;
use common::admin::*;
use common::push::PushSink;
use common::smtp::FakeSmtp;
use common::*;
use scrabble::dictionary;
use scrabble::server::core;
use scrabble::word_review;
use serde_json::{Value, json};

const REASON: &str = "Teszt indoklás";

struct Env {
    server: TestServer,
    api: Http,
    admin_id: i64,
    anna: i64,
    anna_http: Http,
}

async fn env() -> Env {
    let server = TestServer::start().await;
    let api = server.admin_http().await;
    let admin_id = server.user_id(ADMIN_EMAIL);
    let (anna, anna_http) = make_user(&server, "anna@example.com", "Anna").await;
    Env { server, api, admin_id, anna, anna_http }
}

fn user(e: &Env, id: i64) -> Option<scrabble::db::User> {
    e.server.app.db.get_user_by_id(id).unwrap()
}

fn name_of(e: &Env, id: i64) -> String {
    user(e, id).unwrap().display_name
}

fn users_url(id: i64, tail: &str) -> String {
    format!("/users/{id}{tail}")
}

async fn ids_of(e: &Env, query: &str) -> Vec<i64> {
    let response = e.api.admin_get(&format!("/users{query}")).await;
    assert_eq!(response.status, 200, "{}", response.text());
    items_of(&response, "id").iter().map(|v| v.as_i64().unwrap()).collect()
}

/// Egy szó, amelyet a szótár elfogad, és amelyet más teszt nem használ (a kizárt szavak a folyamat egészére hatnak).
fn valid_word(index: usize) -> String {
    dictionary::warm_up();
    let candidates = ["ALMA", "KÖRTE", "ASZTAL", "LÁMPA", "HAJÓ", "ERDŐ", "TENGER", "VIRÁG"];
    let word = candidates.iter().filter(|w| dictionary::is_word_valid(w)).nth(index).expect("érvényes szó").to_string();
    word
}

// ===================================================================================================
// Lista
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn lists_users_with_their_state() {
    let e = env().await;
    let data = e.api.admin_get("/users").await.json();
    assert_eq!(data["total"], 2);
    let items = data["items"].as_array().unwrap();
    let row = items.iter().find(|u| u["id"] == e.anna).unwrap();
    assert_eq!((row["display_name"].clone(), row["status"].clone(), row["online"].clone()), (json!("Anna"), json!("active"), json!(false)));
    assert_eq!((row["push_devices"].clone(), row["is_admin"].clone()), (json!(0), json!(false)));
    assert_eq!(items.iter().find(|u| u["id"] == e.admin_id).unwrap()["is_admin"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn search_is_case_and_accent_insensitive() {
    let e = env().await;
    e.server.create_user("o@example.com", "Őrült Ödön");
    for query in ["orult", "ŐRÜLT", "odon"] {
        let names = items_of(&e.api.admin_get(&format!("/users{}", qs(&[("q", query)]))).await, "display_name");
        assert_eq!(names, vec![json!("Őrült Ödön")], "{query}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn search_by_email_and_id() {
    let e = env().await;
    assert_eq!(ids_of(&e, "?q=anna@exa").await, vec![e.anna]);
    assert_eq!(ids_of(&e, &format!("?q={}", e.anna)).await, vec![e.anna]);
}

#[tokio::test(flavor = "multi_thread")]
async fn status_filters() {
    let e = env().await;
    let (other, _) = make_user(&e.server, "b@example.com", "Béla").await;
    e.api.sudo().await;
    e.api.admin_post(&users_url(e.anna, "/ban"), json!({"until": "1d", "reason": REASON})).await;
    e.api.admin_post(&users_url(other, "/mute"), json!({"until": "1h", "reason": REASON})).await;
    assert_eq!(ids_of(&e, "?status=banned").await, vec![e.anna]);
    assert_eq!(ids_of(&e, "?status=muted").await, vec![other]);
    let active = ids_of(&e, "?status=active").await;
    assert!(active.contains(&e.admin_id) && !active.contains(&e.anna));
    assert_eq!(ids_of(&e, "?admin=1").await, vec![e.admin_id]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_online_filter() {
    let e = env().await;
    let client = sio_user(&e.server, e.anna, "Anna").await;
    assert_eq!(ids_of(&e, "?online=1").await, vec![e.anna]);
    assert_eq!(e.api.admin_get(&users_url(e.anna, "")).await.json()["live"]["online"], true);
    client.close();
    client.settle_long().await;
    assert!(ids_of(&e, "?online=1").await.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn sorting_and_paging() {
    let e = env().await;
    for i in 0..5 {
        e.server.create_user(&format!("u{i}@example.com"), &format!("Név{i}"));
    }
    let data = e.api.admin_get("/users?sort=name&order=asc&limit=2&offset=1").await.json();
    assert_eq!(data["total"], 7);
    assert_eq!(data["items"].as_array().unwrap().len(), 2);
    let names: Vec<String> = items_of(&e.api.admin_get("/users?sort=name&order=asc").await, "display_name").iter().map(|n| n.as_str().unwrap().to_string()).collect();
    let mut sorted = names.clone();
    sorted.sort_by_key(|n| scrabble::admin::fold(n));
    assert_eq!(names, sorted);
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_filters() {
    let e = env().await;
    assert_eq!(e.api.admin_get("/users?status=nincs").await.status, 400);
    assert_eq!(e.api.admin_get(&format!("/users{}", qs(&[("from", "hibás")]))).await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn csv_export() {
    let e = env().await;
    let response = e.api.admin_get("/users?format=csv").await;
    assert_eq!(response.status, 200);
    assert!(response.header("Content-Type").unwrap().contains("text/csv"));
    let text = response.text();
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("id,display_name,email"), "{}", lines[0]);
    assert_eq!(lines.len(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn csv_cells_cannot_inject_formulas() {
    let e = env().await;
    e.server.create_user("x@example.com", "=SUM(A1)");
    assert!(e.api.admin_get("/users?format=csv").await.text().contains("'=SUM(A1)"));
}

#[tokio::test(flavor = "multi_thread")]
async fn global_search() {
    let e = env().await;
    let data = e.api.admin_get("/search?q=anna").await.json();
    assert_eq!(data["users"].as_array().unwrap().iter().map(|u| u["display_name"].clone()).collect::<Vec<_>>(), vec![json!("Anna")]);
    assert_eq!(data["words"], json!([{"word": "ANNA"}]));
}

// ===================================================================================================
// Részletek
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_detail_is_logged_as_a_view() {
    let e = env().await;
    let data = e.api.admin_get(&users_url(e.anna, "")).await.json();
    assert_eq!(data["user"]["email"], "anna@example.com");
    assert_eq!(data["is_self"], false);
    let rows = audit_of(&e.server, "view.user");
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0]["target_id"].clone(), rows[0]["admin_user_id"].clone()), (json!(e.anna.to_string()), json!(e.admin_id)));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_detail_contains_all_sections() {
    let e = env().await;
    let (other, _) = make_user(&e.server, "b@example.com", "Béla").await;
    assert!(e.server.app.db.send_friend_request(e.anna, other).0);
    assert!(e.server.app.db.accept_friend_request(other, e.anna).0);
    e.server.app.db.save_word_review(e.anna, "almaa", false).unwrap();
    e.server.app.db.save_push_subscription(e.anna, "https://push.example.org/abcdefghij", &"p".repeat(30), &"a".repeat(12), "en").unwrap();
    simple_game(&e.server, "r1", (e.anna, "Anna", 120), (other, "Béla", 80));
    let data = e.api.admin_get(&users_url(e.anna, "")).await.json();
    assert_eq!(data["friends"].as_array().unwrap().iter().map(|f| f["display_name"].clone()).collect::<Vec<_>>(), vec![json!("Béla")]);
    assert_eq!((data["reviews"]["total"].clone(), data["reviews"]["invalid"].clone()), (json!(1), json!(1)));
    let push = &data["push"][0];
    assert_eq!((push["lang"].clone(), push["host"].clone()), (json!("en"), json!("push.example.org")));
    assert!(push.get("id").is_some() && push.get("created_at").is_some());
    assert_eq!(data["games"].as_array().unwrap().iter().map(|g| g["score"].clone()).collect::<Vec<_>>(), vec![json!(120)]);
    assert_eq!(data["rating_history"][0]["kind"], "game");
    let text = data.to_string();
    assert!(!text.contains("password_hash"));
    if let Some(session) = data["sessions"].as_array().and_then(|s| s.first()) {
        assert!(session.get("token").is_none());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_user() {
    let e = env().await;
    assert_eq!(e.api.admin_get("/users/9999").await.status, 404);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_profile_view_is_logged() {
    let e = env().await;
    let data = e.api.admin_get(&users_url(e.anna, "/profile")).await.json()["profile"].clone();
    assert_eq!((data["display_name"].clone(), data["stats"]["rating"].clone()), (json!("Anna"), json!(1200)));
    assert_eq!(audit_of(&e.server, "view.user_profile").len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_online_user_shows_where_they_are() {
    let e = env().await;
    let anna = sio_user(&e.server, e.anna, "Anna").await;
    let bela = e.server.guest("Béla").await;
    let (_, code) = live_room(&e.server, &anna, &[&bela], json!({})).await;
    let live = e.api.admin_get(&users_url(e.anna, "")).await.json()["live"].clone();
    assert_eq!(live["online"], true);
    assert_eq!((live["rooms"][0]["code"].clone(), live["rooms"][0]["role"].clone()), (json!(code), json!("player")));
}

// ===================================================================================================
// Minden módosításhoz indoklás kell
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn every_change_needs_a_reason() {
    let e = env().await;
    e.api.sudo().await;
    let cases: Vec<(&str, &str, Value)> = vec![
        ("POST", "/unban", json!({})),
        ("POST", "/mute", json!({"until": "1h"})),
        ("POST", "/unmute", json!({})),
        ("POST", "/logout-all", json!({})),
        ("POST", "/rating", json!({"rating": 1300})),
        ("POST", "/badges", json!({"badge": "bingo", "grant": true})),
        ("POST", "/recompute-stats", json!({})),
        ("POST", "/reviews/block", json!({"blocked": true})),
        ("PATCH", "", json!({"display_name": "Új név"})),
        ("POST", "/ban", json!({"until": "1d"})),
        ("POST", "/reset-password", json!({})),
        ("POST", "/reviews/revert", json!({})),
    ];
    for (method, path, body) in cases {
        let before = audit_rows(&e.server).len();
        let response = e.api.admin(method, &users_url(e.anna, path), Some(body)).await;
        assert_eq!(response.status, 400, "{method} {path}: {}", response.text());
        assert_eq!(response.json()["field"], "reason", "{method} {path}");
        assert_eq!(audit_rows(&e.server).len(), before, "{method} {path}");
        assert_eq!(name_of(&e, e.anna), "Anna");
        assert!(user(&e, e.anna).unwrap().banned_until.is_none());
    }
}

// ===================================================================================================
// Átnevezés
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn renames_and_logs_before_and_after() {
    let e = env().await;
    let response = e.api.admin_patch(&users_url(e.anna, ""), json!({"display_name": "Annácska", "reason": REASON})).await;
    assert_eq!(response.status, 200);
    assert_eq!(name_of(&e, e.anna), "Annácska");
    let row = audit_of(&e.server, "user.rename").remove(0);
    assert_eq!(row["details"]["before"], json!({"display_name": "Anna"}));
    assert_eq!(row["details"]["after"], json!({"display_name": "Annácska"}));
    assert_eq!(row["reason"], REASON);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_registration_rules_apply_to_renames() {
    let e = env().await;
    for name in [json!(""), json!("   "), json!("x".repeat(21)), json!("Rossz<név>"), json!(12)] {
        let response = e.api.admin_patch(&users_url(e.anna, ""), json!({"display_name": name, "reason": REASON})).await;
        assert_eq!(response.status, 400, "{name}");
        assert_eq!(name_of(&e, e.anna), "Anna");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_same_name_is_rejected() {
    let e = env().await;
    assert_eq!(e.api.admin_patch(&users_url(e.anna, ""), json!({"display_name": "Anna", "reason": REASON})).await.status, 409);
}

#[tokio::test(flavor = "multi_thread")]
async fn renames_the_player_in_a_running_game() {
    let e = env().await;
    let anna = sio_user(&e.server, e.anna, "Anna").await;
    let bela = e.server.guest("Béla").await;
    let (room_id, _) = live_room(&e.server, &anna, &[&bela], json!({})).await;
    let (ok, _) = {
        let mut st = e.server.app.state.lock();
        core::save_game_to_db(&e.server.app, &mut st, &room_id)
    };
    assert!(ok);
    let game_id = with_room(&e.server, |room| room.db_game_id).unwrap();
    let anna_sid = with_room(&e.server, |room| room.game.players.iter().find(|p| p.name == "Anna").unwrap().id.clone());
    e.api.admin_patch(&users_url(e.anna, ""), json!({"display_name": "Annácska", "reason": REASON})).await;
    {
        let st = e.server.app.state.lock();
        let room = &st.rooms[&room_id];
        assert!(room.game.players.iter().any(|p| p.id == anna_sid && p.name == "Annácska"));
        assert_eq!(room.owner_name, "Annácska");
        assert_eq!(st.player_names[&anna_sid], "Annácska");
        assert!(!room.known_user_ids.contains_key("Anna"));
    }
    {
        let mut st = e.server.app.state.lock();
        core::save_game_to_db(&e.server.app, &mut st, &room_id);
    }
    let mut names: Vec<String> = e.server.app.db.get_game_players(game_id).unwrap().into_iter().map(|(n, _, _)| n).collect();
    names.sort();
    assert_eq!(names, vec!["Annácska", "Béla"]); // nincs duplikált sor
    assert_eq!(with_room(&e.server, |room| room.db_game_id), Some(game_id));
    anna.settle().await;
    let state = anna.state();
    assert!(state["players"].as_array().unwrap().iter().any(|p| p["name"] == "Annácska"), "{state}");
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_a_name_taken_in_the_same_room() {
    let e = env().await;
    let anna = sio_user(&e.server, e.anna, "Anna").await;
    let bela = e.server.guest("Béla").await;
    live_room(&e.server, &anna, &[&bela], json!({})).await;
    let response = e.api.admin_patch(&users_url(e.anna, ""), json!({"display_name": "Béla", "reason": REASON})).await;
    assert_eq!(response.status, 409);
    assert_eq!(name_of(&e, e.anna), "Anna");
}

#[tokio::test(flavor = "multi_thread")]
async fn finished_games_keep_the_historic_name() {
    let e = env().await;
    let (other, _) = make_user(&e.server, "b@example.com", "Béla").await;
    let game_id = simple_game(&e.server, "r1", (e.anna, "Anna", 90), (other, "Béla", 80));
    e.api.admin_patch(&users_url(e.anna, ""), json!({"display_name": "Annácska", "reason": REASON})).await;
    let names: Vec<String> = e.server.app.db.get_game_players(game_id).unwrap().into_iter().map(|(n, _, _)| n).collect();
    assert!(names.contains(&"Anna".to_string()));
}

// ===================================================================================================
// E-mail cím
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn changing_the_email_needs_sudo() {
    let e = env().await;
    let response = e.api.admin_patch(&users_url(e.anna, ""), json!({"email": "uj@example.com", "reason": REASON})).await;
    assert_eq!(response.status, 401);
    assert_eq!(response.json()["sudo_required"], true);
    assert_eq!(user(&e, e.anna).unwrap().email, "anna@example.com");
}

#[tokio::test(flavor = "multi_thread")]
async fn changes_the_email_and_notifies_the_old_address() {
    let e = env().await;
    let smtp = FakeSmtp::start();
    use_smtp(&e.server, &smtp);
    e.api.sudo().await;
    let response = e.api.admin_patch(&users_url(e.anna, ""), json!({"email": "Uj@Example.com", "reason": REASON})).await;
    assert_eq!(response.status, 200, "{}", response.text());
    assert_eq!(response.json()["notified"], true);
    let changed = user(&e, e.anna).unwrap();
    assert_eq!((changed.email.as_str(), changed.email_lower.as_str()), ("Uj@Example.com", "uj@example.com"));
    let mails = smtp.wait_for(1, 5).await;
    assert_eq!(mails[0].to, vec!["anna@example.com".to_string()]);
    assert_eq!(audit_of(&e.server, "user.email")[0]["details"]["after"], json!({"email": "Uj@Example.com"}));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_email_must_be_unique_valid_and_not_an_admin_address() {
    let e = env().await;
    make_user(&e.server, "b@example.com", "Béla").await;
    e.api.sudo().await;
    for (email, status) in [("b@example.com", 409), ("nem-email", 400), (ADMIN_EMAIL, 403)] {
        let response = e.api.admin_patch(&users_url(e.anna, ""), json!({"email": email, "reason": REASON})).await;
        assert_eq!(response.status, status, "{email}");
    }
    assert_eq!(user(&e, e.anna).unwrap().email, "anna@example.com");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_admin_account_cannot_be_changed() {
    let e = env().await;
    e.api.sudo().await;
    assert_eq!(e.api.admin_patch(&users_url(e.admin_id, ""), json!({"email": "x@example.com", "reason": REASON})).await.status, 403);
    assert_eq!(e.api.admin_patch(&users_url(e.admin_id, ""), json!({"display_name": "Más", "reason": REASON})).await.status, 403);
}

// ===================================================================================================
// Kitiltás
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn banning_needs_sudo() {
    let e = env().await;
    let response = e.api.admin_post(&users_url(e.anna, "/ban"), json!({"until": "1d", "reason": REASON})).await;
    assert_eq!(response.status, 401);
    assert_eq!(response.json()["sudo_required"], true);
    assert!(e.server.app.db.get_ban(e.anna).is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_ban_closes_sessions_and_blocks_login_with_the_reason() {
    let e = env().await;
    let token = e.anna_http.session_token().unwrap();
    e.api.sudo().await;
    let response = e.api.admin_post(&users_url(e.anna, "/ban"), json!({"until": "7d", "reason": "Sértegetés"})).await;
    assert_eq!(response.status, 200);
    assert!(response.json()["until"].is_string());
    assert!(e.server.app.db.validate_session(Some(&token)).is_none());
    let ban = e.server.app.db.get_ban(e.anna).unwrap();
    assert_eq!(ban.reason, "Sértegetés");
    assert!(!ban.permanent);
    let login = e.server.http().post("/api/auth/login", json!({"email": "anna@example.com", "password": PASSWORD})).await;
    assert_eq!(login.status, 403);
    let body = login.json();
    assert_eq!(body["message"], "A fiókod ki van tiltva.");
    assert_eq!(body["banned"]["reason"], "Sértegetés");
    assert_eq!(body["banned"]["until"], json!(ban.until));
    assert_eq!(audit_of(&e.server, "user.ban")[0]["details"]["sessions_closed"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_permanent_ban() {
    let e = env().await;
    e.api.sudo().await;
    let response = e.api.admin_post(&users_url(e.anna, "/ban"), json!({"until": null, "reason": REASON})).await;
    assert!(response.json()["until"].is_null());
    let ban = e.server.app.db.get_ban(e.anna).unwrap();
    assert!(ban.permanent && ban.until.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_expired_ban_no_longer_applies() {
    let e = env().await;
    e.api.sudo().await;
    e.api.admin_post(&users_url(e.anna, "/ban"), json!({"until": "1h", "reason": REASON})).await;
    e.server.app.db.with(|tx| tx.execute("UPDATE users SET banned_until = '2000-01-01 00:00:00' WHERE id = ?", [e.anna]).map(|_| ())).unwrap();
    assert!(e.server.app.db.get_ban(e.anna).is_none());
    let login = e.server.http().post("/api/auth/login", json!({"email": "anna@example.com", "password": PASSWORD})).await;
    assert_eq!(login.status, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn unbanning() {
    let e = env().await;
    e.api.sudo().await;
    e.api.admin_post(&users_url(e.anna, "/ban"), json!({"until": "1d", "reason": REASON})).await;
    assert_eq!(e.api.admin_post(&users_url(e.anna, "/unban"), json!({"reason": REASON})).await.status, 200);
    assert!(e.server.app.db.get_ban(e.anna).is_none());
    assert_eq!(e.api.admin_post(&users_url(e.anna, "/unban"), json!({"reason": REASON})).await.status, 409);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_invalid_ban_duration() {
    let e = env().await;
    e.api.sudo().await;
    for until in [json!("2h"), json!("holnap"), json!(5), json!("2000-01-01")] {
        assert_eq!(e.api.admin_post(&users_url(e.anna, "/ban"), json!({"until": until, "reason": REASON})).await.status, 400, "{until}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_admin_cannot_be_banned() {
    let e = env().await;
    e.api.sudo().await;
    assert_eq!(e.api.admin_post(&users_url(e.admin_id, "/ban"), json!({"until": "1d", "reason": REASON})).await.status, 403);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_live_connection_is_dropped_and_the_player_leaves_the_game() {
    let e = env().await;
    let anna = sio_user(&e.server, e.anna, "Anna").await;
    let bela = e.server.guest("Béla").await;
    live_room(&e.server, &anna, &[&bela], json!({})).await;
    e.api.sudo().await;
    e.api.admin_post(&users_url(e.anna, "/ban"), json!({"until": "1d", "reason": REASON})).await;
    anna.settle_long().await;
    let events: Vec<String> = anna.take().into_iter().map(|(n, _)| n).collect();
    assert!(events.contains(&"account_banned".to_string()), "{events:?}");
    assert!(anna.is_closed());
    assert!(with_room(&e.server, |room| room.game.players.iter().find(|p| p.name == "Anna").unwrap().disconnected));
    assert!(!e.server.app.state.lock().is_user_online(e.anna));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_banned_user_cannot_rejoin_with_the_old_token() {
    let e = env().await;
    let anna = sio_user(&e.server, e.anna, "Anna").await;
    let bela = e.server.guest("Béla").await;
    live_room(&e.server, &anna, &[&bela], json!({})).await;
    let token = anna.token();
    anna.close(); // türelmi idő: a token még érvényes
    anna.settle_long().await;
    e.api.sudo().await;
    e.api.admin_post(&users_url(e.anna, "/ban"), json!({"until": "1d", "reason": REASON})).await;
    let again = e.server.guest("Anna").await;
    let got = again.call("rejoin_room", json!({"token": token})).await;
    assert!(got.iter().any(|(n, _)| n == "rejoin_failed"), "{got:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_banned_user_cannot_identify_over_the_socket() {
    let e = env().await;
    e.api.sudo().await;
    e.api.admin_post(&users_url(e.anna, "/ban"), json!({"until": "1d", "reason": "Sértegetés"})).await;
    let client = Sio::connect(&e.server).await;
    client.emit("set_name", json!({"name": "Anna", "is_guest": false, "auth_token": e.server.socket_token(e.anna)}));
    client.settle_long().await;
    let banned: Vec<Value> = client.take().into_iter().filter(|(n, _)| n == "account_banned").map(|(_, d)| d).collect();
    assert!(!banned.is_empty() && banned[0]["reason"] == "Sértegetés", "{banned:?}");
    assert!(!e.server.app.state.lock().is_user_online(e.anna));
}

// ===================================================================================================
// Némítás
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn muted_messages_are_silently_dropped() {
    let e = env().await;
    let anna = sio_user(&e.server, e.anna, "Anna").await;
    let bela = e.server.guest("Béla").await;
    live_room(&e.server, &anna, &[&bela], json!({})).await;
    e.api.admin_post(&users_url(e.anna, "/mute"), json!({"until": "1h", "reason": REASON})).await;
    anna.emit("send_chat", json!({"message": "Halló"}));
    anna.settle().await;
    assert!(!bela.take().iter().any(|(n, _)| n == "chat_message"));
    assert!(!anna.take().iter().any(|(n, _)| n == "chat_message" || n == "error"));
    e.api.admin_post(&users_url(e.anna, "/unmute"), json!({"reason": REASON})).await;
    anna.emit("send_chat", json!({"message": "Újra"}));
    anna.settle().await;
    let messages: Vec<Value> = bela.take().into_iter().filter(|(n, _)| n == "chat_message").map(|(_, d)| d["message"].clone()).collect();
    assert_eq!(messages, vec![json!("Újra")]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_mute_expires() {
    let e = env().await;
    e.api.admin_post(&users_url(e.anna, "/mute"), json!({"until": "1h", "reason": REASON})).await;
    assert!(e.server.app.db.is_chat_muted(e.anna));
    e.server.app.db.with(|tx| tx.execute("UPDATE users SET chat_muted_until = '2000-01-01 00:00:00' WHERE id = ?", [e.anna]).map(|_| ())).unwrap();
    assert!(!e.server.app.db.is_chat_muted(e.anna));
}

// ===================================================================================================
// A szótár-építő feletti ellenőrzés
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn blocked_reviewers_cannot_vote_and_their_votes_do_not_count() {
    let e = env().await;
    let word = valid_word(0);
    let app = &e.server.app;
    assert_eq!(word_review::record_vote(&app.db, &app.settings, e.anna, &word, false), Some(true));
    assert!(dictionary::is_rejected(&word));
    e.api.admin_post(&users_url(e.anna, "/reviews/block"), json!({"blocked": true, "reason": REASON})).await;
    assert!(!dictionary::is_rejected(&word)); // a letiltott bíráló szavazata nem számít
    let response = e.anna_http.post("/api/practice/word-review", json!({"word": word, "valid": false})).await;
    assert_eq!(response.status, 403);
    e.api.admin_post(&users_url(e.anna, "/reviews/block"), json!({"blocked": false, "reason": REASON})).await;
    assert!(dictionary::is_rejected(&word));
    dictionary::mark_voted_rejected(&word, false);
}

#[tokio::test(flavor = "multi_thread")]
async fn reverting_deletes_the_votes_and_recomputes_the_exclusions() {
    let e = env().await;
    let word = valid_word(1);
    let app = &e.server.app;
    word_review::record_vote(&app.db, &app.settings, e.anna, &word, false);
    e.api.sudo().await;
    let response = e.api.admin_post(&users_url(e.anna, "/reviews/revert"), json!({"reason": REASON})).await;
    assert_eq!(response.json()["count"], 1);
    assert_eq!(app.db.get_word_review_votes(&word.to_lowercase()), (0, 0));
    assert!(!dictionary::is_rejected(&word));
    assert_eq!(audit_of(&e.server, "user.reviews_revert")[0]["details"]["words"], json!([word.to_lowercase()]));
}

// ===================================================================================================
// Értékszám, statisztika, kitüntetések
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn a_manual_rating_is_a_separate_history_row() {
    let e = env().await;
    e.api.sudo().await;
    let response = e.api.admin_post(&users_url(e.anna, "/rating"), json!({"rating": 1350, "reason": REASON})).await;
    assert_eq!(response.status, 200);
    assert_eq!(user(&e, e.anna).unwrap().rating, 1350);
    let history = e.api.admin_get(&users_url(e.anna, "")).await.json()["rating_history"].clone();
    let last = history.as_array().unwrap().last().unwrap().clone();
    assert_eq!((last["kind"].clone(), last["before"].clone(), last["after"].clone()), (json!("adjustment"), json!(1200), json!(1350)));
    assert_eq!(audit_of(&e.server, "user.rating")[0]["details"]["after"], json!({"rating": 1350}));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_rating_range() {
    let e = env().await;
    e.api.sudo().await;
    for rating in [json!(50), json!(99999), json!("1300"), json!(null), json!(true)] {
        assert_eq!(e.api.admin_post(&users_url(e.anna, "/rating"), json!({"rating": rating, "reason": REASON})).await.status, 400, "{rating}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_rating_needs_sudo() {
    let e = env().await;
    assert_eq!(e.api.admin_post(&users_url(e.anna, "/rating"), json!({"rating": 1300, "reason": REASON})).await.status, 401);
}

#[tokio::test(flavor = "multi_thread")]
async fn recomputing_the_stats_from_the_games() {
    let e = env().await;
    let (other, _) = make_user(&e.server, "b@example.com", "Béla").await;
    simple_game(&e.server, "r1", (e.anna, "Anna", 120), (other, "Béla", 80));
    e.server.app.db.with(|tx| tx.execute("UPDATE users SET games_played = 99, games_won = 50, total_score = 5 WHERE id = ?", [e.anna]).map(|_| ())).unwrap();
    e.api.admin_post(&users_url(e.anna, "/recompute-stats"), json!({"reason": REASON})).await;
    let u = user(&e, e.anna).unwrap();
    assert_eq!((u.games_played, u.games_won, u.total_score), (1, 1, 120));
    assert_eq!(audit_of(&e.server, "user.recompute_stats")[0]["details"]["before"]["games_played"], 99);
}

#[tokio::test(flavor = "multi_thread")]
async fn badges_can_be_granted_and_revoked() {
    let e = env().await;
    let badge = |grant: bool, name: &str| json!({"badge": name, "grant": grant, "reason": REASON});
    assert_eq!(e.api.admin_post(&users_url(e.anna, "/badges"), badge(true, "bingo")).await.status, 200);
    let granted: Vec<Value> = e.server.app.db.get_user_achievements(e.anna).iter().map(|b| b["badge"].clone()).collect();
    assert_eq!(granted, vec![json!("bingo")]);
    assert_eq!(e.api.admin_post(&users_url(e.anna, "/badges"), badge(true, "bingo")).await.status, 409);
    assert_eq!(e.api.admin_post(&users_url(e.anna, "/badges"), badge(false, "bingo")).await.status, 200);
    assert!(e.server.app.db.get_user_achievements(e.anna).is_empty());
    assert_eq!(e.api.admin_post(&users_url(e.anna, "/badges"), badge(true, "nincs")).await.status, 400);
}

// ===================================================================================================
// Munkamenetek és jelszavak
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn logout_everywhere() {
    let e = env().await;
    let token = e.anna_http.session_token().unwrap();
    let second = e.server.app.db.create_session(e.anna, None, None).unwrap();
    let anna = sio_user(&e.server, e.anna, "Anna").await;
    let response = e.api.admin_post(&users_url(e.anna, "/logout-all"), json!({"reason": REASON})).await;
    assert_eq!(response.json()["sessions_closed"], 2);
    assert!(e.server.app.db.validate_session(Some(&token)).is_none());
    assert!(e.server.app.db.validate_session(Some(&second)).is_none());
    anna.settle_long().await;
    assert!(anna.is_closed());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_password_reset_returns_the_password_only_without_smtp() {
    let e = env().await;
    let token = e.anna_http.session_token().unwrap();
    e.api.sudo().await;
    let data = e.api.admin_post(&users_url(e.anna, "/reset-password"), json!({"reason": REASON})).await.json();
    let temp = data["temp_password"].as_str().unwrap().to_string();
    assert_eq!(temp.len(), 12);
    assert_eq!(data["sessions_closed"], 1);
    assert!(e.server.app.db.validate_session(Some(&token)).is_none());
    assert!(e.server.app.db.verify_password("anna@example.com", &temp).is_ok());
    assert!(e.server.app.db.verify_password("anna@example.com", PASSWORD).is_err());
    assert!(!json!(audit_of(&e.server, "user.reset_password")).to_string().contains(&temp));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_password_reset_with_smtp_is_only_mailed() {
    let e = env().await;
    let smtp = FakeSmtp::start();
    use_smtp(&e.server, &smtp);
    e.api.sudo().await;
    let data = e.api.admin_post(&users_url(e.anna, "/reset-password"), json!({"reason": REASON})).await.json();
    assert!(data.get("temp_password").is_none(), "{data}");
    assert_eq!(data["emailed"], true);
    let mails = smtp.wait_for(1, 5).await;
    assert!(mails[0].body().contains("jelszavad:"), "{}", mails[0].body());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_password_reset_needs_sudo() {
    let e = env().await;
    assert_eq!(e.api.admin_post(&users_url(e.anna, "/reset-password"), json!({"reason": REASON})).await.status, 401);
}

// ===================================================================================================
// Push és jegyzetek
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_push_test() {
    let e = env().await;
    let sink = PushSink::start();
    assert_eq!(e.api.admin_post(&users_url(e.anna, "/push-test"), json!({})).await.status, 409);
    let sub = sink.subscription("send/x");
    e.server.app.db.save_push_subscription(e.anna, &sub.endpoint, &sub.p256dh, &sub.auth, "hu").unwrap();
    let data = e.api.admin_post(&users_url(e.anna, "/push-test"), json!({})).await.json();
    assert_eq!(data["sent"], 1, "{data}");
    let got = sink.wait_for(1, 5).await;
    assert!(got[0].payload["body"].as_str().unwrap().starts_with("Teszt"));
    assert!(!audit_of(&e.server, "user.push_test").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn deleting_a_device() {
    let e = env().await;
    e.server.app.db.save_push_subscription(e.anna, "https://push.example.org/abcdefghij", &"p".repeat(30), &"a".repeat(12), "hu").unwrap();
    let device = e.api.admin_get(&users_url(e.anna, "")).await.json()["push"][0]["id"].clone();
    let path = users_url(e.anna, &format!("/push/{device}"));
    assert_eq!(e.api.admin_delete(&path, json!({"reason": REASON})).await.status, 200);
    assert_eq!(e.server.app.db.count_push_subscriptions(e.anna), 0);
    assert_eq!(e.api.admin_delete(&path, json!({"reason": REASON})).await.status, 404);
}

#[tokio::test(flavor = "multi_thread")]
async fn notes() {
    let e = env().await;
    let note_id = e.api.admin_post(&users_url(e.anna, "/notes"), json!({"note": "Figyelni kell."})).await.json()["id"].clone();
    let notes = e.api.admin_get(&users_url(e.anna, "")).await.json()["notes"].clone();
    assert_eq!(notes.as_array().unwrap().iter().map(|n| n["note"].clone()).collect::<Vec<_>>(), vec![json!("Figyelni kell.")]);
    assert_eq!(notes[0]["admin_name"], "Admin");
    assert_eq!(e.api.admin_post(&users_url(e.anna, "/notes"), json!({"note": "  "})).await.status, 400);
    assert_eq!(e.api.admin_delete(&users_url(e.anna, &format!("/notes/{note_id}")), json!({})).await.status, 200);
    assert_eq!(e.api.admin_get(&users_url(e.anna, "")).await.json()["notes"], json!([]));
    let actions: Vec<String> = audit_of(&e.server, "user.note").iter().map(|r| r["action"].as_str().unwrap().to_string()).collect();
    assert_eq!(actions, vec!["user.note_delete", "user.note_add"]);
}

// ===================================================================================================
// Export és törlés
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_export_contains_the_data_but_no_secrets() {
    let e = env().await;
    let (other, _) = make_user(&e.server, "b@example.com", "Béla").await;
    finished_game(&e.server, "r1", &[(Some(e.anna), "Anna", 120), (Some(other), "Béla", 80)], false, &[], 2);
    e.server.app.db.save_word_review(e.anna, "almaa", false).unwrap();
    e.server.app.db.grant_achievements(e.anna, &std::collections::HashSet::from(["bingo".to_string()]), None);
    let response = e.api.admin_get(&users_url(e.anna, "/export")).await;
    assert!(response.header("Content-Disposition").unwrap().contains("attachment"));
    let data: Value = serde_json::from_str(&response.text()).unwrap();
    assert_eq!(data["account"]["email"], "anna@example.com");
    assert_eq!(data["games"].as_array().unwrap().len(), 1);
    assert_eq!(data["games"][0]["moves"].as_array().unwrap().len(), 1);
    assert_eq!((data["word_reviews"][0]["word"].clone(), data["achievements"][0]["badge"].clone()), (json!("almaa"), json!("bingo")));
    let text = data.to_string().to_lowercase();
    assert!(!text.contains("password") && !text.contains("reconnect_token"));
    assert!(!audit_of(&e.server, "view.user_export").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn anonymizing_keeps_the_games_and_hides_the_name() {
    let e = env().await;
    let token = e.anna_http.session_token().unwrap();
    let (other, _) = make_user(&e.server, "b@example.com", "Béla").await;
    let game_id = finished_game(&e.server, "r1", &[(Some(e.anna), "Anna", 120), (Some(other), "Béla", 80)], false, &[], 2);
    for i in 0..3 {
        simple_game(&e.server, &format!("x{i}"), (e.anna, "Anna", 100), (other, "Béla", 50));
    }
    e.api.sudo().await;
    let response = e.api.admin_delete(&users_url(e.anna, ""), json!({"mode": "anonymize", "confirm_name": "Anna", "reason": REASON})).await;
    assert_eq!(response.status, 200, "{}", response.text());
    let u = user(&e, e.anna).unwrap();
    let anonymous = format!("Törölt felhasználó #{}", e.anna);
    assert_eq!(u.display_name, anonymous);
    assert!(u.deleted_at.is_some());
    assert_eq!((u.email.clone(), u.password_hash.clone()), (format!("deleted-{}@deleted.invalid", e.anna), String::new()));
    assert!(e.server.app.db.validate_session(Some(&token)).is_none());
    let players: std::collections::HashSet<String> = e.server.app.db.get_game_players(game_id).unwrap().into_iter().map(|(n, _, _)| n).collect();
    assert_eq!(players, std::collections::HashSet::from([anonymous.clone(), "Béla".to_string()])); // a játék megmarad
    let movers: std::collections::HashSet<String> = e.server.app.db.get_game_moves(game_id).unwrap().into_iter().map(|m| m.player_name).collect();
    assert!(!movers.contains("Anna"));
    let board = e.server.app.db.get_leaderboard("wins", 50, None, false).unwrap().0;
    let names: Vec<String> = board.iter().map(|r| r["display_name"].as_str().unwrap().to_string()).collect();
    assert!(!names.contains(&anonymous) && !names.contains(&"Anna".to_string()));
    assert!(e.server.app.db.verify_password("anna@example.com", PASSWORD).is_err());
    assert!(!audit_of(&e.server, "user.anonymize")[0]["details"]["after"].to_string().contains("Anna"));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_target_name_must_be_typed() {
    let e = env().await;
    e.api.sudo().await;
    for confirm in [json!(null), json!(""), json!("anna"), json!("Béla")] {
        let response = e.api.admin_delete(&users_url(e.anna, ""), json!({"mode": "anonymize", "confirm_name": confirm, "reason": REASON})).await;
        assert_eq!(response.status, 400, "{confirm}");
    }
    assert_eq!(name_of(&e, e.anna), "Anna");
}

#[tokio::test(flavor = "multi_thread")]
async fn full_deletion_removes_the_row_but_not_the_games() {
    let e = env().await;
    let (other, _) = make_user(&e.server, "b@example.com", "Béla").await;
    let game_id = simple_game(&e.server, "r1", (e.anna, "Anna", 120), (other, "Béla", 80));
    e.server.app.db.save_word_review(e.anna, "almaa", false).unwrap();
    e.api.sudo().await;
    let response = e.api.admin_delete(&users_url(e.anna, ""), json!({"mode": "delete", "confirm_name": "Anna", "reason": REASON})).await;
    assert_eq!(response.status, 200, "{}", response.text());
    assert!(user(&e, e.anna).is_none());
    assert!(e.server.app.db.get_game_by_id(game_id).unwrap().is_some());
    assert!(e.server.app.db.get_game_players(game_id).unwrap().iter().all(|(_, uid, _)| uid.is_none() || *uid == Some(other)));
    assert!(e.server.app.db.get_voted_rejected_words(1).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn deletion_needs_sudo_and_never_touches_admins() {
    let e = env().await;
    let body = |name: &str| json!({"mode": "delete", "confirm_name": name, "reason": REASON});
    assert_eq!(e.api.admin_delete(&users_url(e.anna, ""), body("Anna")).await.status, 401);
    e.api.sudo().await;
    assert_eq!(e.api.admin_delete(&users_url(e.admin_id, ""), body("Admin")).await.status, 403);
    assert!(user(&e, e.admin_id).is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_audit_write_rolls_the_deletion_back() {
    let e = env().await;
    e.server.app.db.with(|tx| tx.execute_batch("ALTER TABLE admin_audit RENAME TO admin_audit_elmozgatva")).unwrap();
    let ctx = scrabble::admin::AdminContext { admin_user_id: e.admin_id, ip: "1.1.1.1".into(), user_agent: "t".into() };
    let result = scrabble::admin::users::delete_user(&e.server.app, &ctx, e.anna, Some(&json!("delete")), Some(&json!("Anna")), Some(&json!(REASON)));
    assert!(result.is_err());
    assert!(user(&e, e.anna).is_some());
}
