//! HTTP: regisztráció, belépés, munkamenet, profil, mentett játékok (a Python `test_server_auth.py` megfelelője).

mod common;
use common::*;
use scrabble::db::{GamePlayerData, StoredMove};
use serde_json::{Value, json};

fn player(name: &str, user_id: Option<i64>, score: i64, winner: bool) -> GamePlayerData {
    GamePlayerData { player_name: name.into(), user_id, score, is_winner: winner, resigned: false }
}

/// Az e-mail cím megerősítése (kód kérése és beváltása), mint a regisztráció első két lépése.
fn verify_email(server: &TestServer, email: &str) {
    let code = server.app.db.create_verification_code(email).unwrap();
    let (ok, message) = server.app.db.verify_code(email, &code);
    assert!(ok, "{message}");
}

// ---------------------------------------------------------------------------------------------------
// kód kérése / ellenőrzése
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn request_code_validates_the_email() {
    let server = TestServer::start().await;
    let http = server.http();
    let ok = http.post("/api/auth/request-code", json!({"email": "test@example.com"})).await;
    assert_eq!(ok.status, 200);
    assert_eq!(ok.json()["success"], true);
    // SMTP nélkül a kód a válaszban (fejlesztői mód)
    assert_eq!(ok.json()["dev_code"].as_str().unwrap().len(), 6);
    for body in [json!({"email": "not-an-email"}), json!({"email": ""}), json!({})] {
        let resp = http.post("/api/auth/request-code", body).await;
        assert_eq!(resp.status, 400);
        assert_eq!(resp.json()["success"], false);
    }
    // nincs törzs
    let resp = http.request("POST", "/api/auth/request-code", None, &[("Content-Type", "application/json")]).await;
    assert_eq!(resp.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn request_code_for_a_registered_email_conflicts() {
    let server = TestServer::start().await;
    server.create_user("test@example.com", "TestUser");
    let resp = server.http().post("/api/auth/request-code", json!({"email": "test@example.com"})).await;
    assert_eq!(resp.status, 409);
    assert!(resp.json()["message"].as_str().unwrap().contains("már regisztrálva"));
}

#[tokio::test(flavor = "multi_thread")]
async fn verify_code_rules() {
    let server = TestServer::start().await;
    let http = server.http();
    let code = server.app.db.create_verification_code("test@example.com").unwrap();
    let ok = http.post("/api/auth/verify-code", json!({"email": "test@example.com", "code": code})).await;
    assert_eq!(ok.status, 200);
    server.app.db.create_verification_code("other@example.com").unwrap();
    let wrong = http.post("/api/auth/verify-code", json!({"email": "other@example.com", "code": "000000"})).await;
    assert_eq!(wrong.status, 400);
    assert_eq!(wrong.json()["success"], false);
    assert_eq!(http.post("/api/auth/verify-code", json!({"email": "test@example.com", "code": "abc"})).await.status, 400);
    assert_eq!(http.post("/api/auth/verify-code", json!({"email": "", "code": ""})).await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn verify_code_attempts_are_limited() {
    let server = TestServer::start().await;
    let http = server.http();
    let code = server.app.db.create_verification_code("brute@example.com").unwrap();
    let wrong = if code == "111111" { "222222" } else { "111111" };
    for _ in 0..5 {
        assert_eq!(http.post("/api/auth/verify-code", json!({"email": "brute@example.com", "code": wrong})).await.status, 400);
    }
    // az 5 próba után a helyes kód sem jó
    let resp = http.post("/api/auth/verify-code", json!({"email": "brute@example.com", "code": code})).await;
    assert_eq!(resp.status, 400);
}

// ---------------------------------------------------------------------------------------------------
// regisztráció
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn register_requires_a_verified_email() {
    let server = TestServer::start().await;
    let resp = server.http().post("/api/auth/register", json!({"email": "unverified@example.com", "password": "password123", "display_name": "Sneaky"})).await;
    assert_eq!(resp.status, 403);
    assert_eq!(resp.json()["success"], false);
    assert!(server.app.db.get_user_by_email("unverified@example.com").unwrap().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_verification_is_single_use_and_per_email() {
    let server = TestServer::start().await;
    let http = server.http();
    verify_email(&server, "once@example.com");
    let resp = http.post("/api/auth/register", json!({"email": "once@example.com", "password": "password123", "display_name": "Once"})).await;
    assert_eq!(resp.status, 200);
    // törölt fiók után sem regisztrálható újra kód nélkül
    server.app.db.with(|tx| tx.execute("DELETE FROM users WHERE email_lower = ?", ["once@example.com"]).map(|_| ())).unwrap();
    let resp = http.post("/api/auth/register", json!({"email": "once@example.com", "password": "password123", "display_name": "Again"})).await;
    assert_eq!(resp.status, 403);
    // másik e-mail címre nem jó a megerősítés
    verify_email(&server, "verified@example.com");
    let resp = http.post("/api/auth/register", json!({"email": "other@example.com", "password": "password123", "display_name": "Other"})).await;
    assert_eq!(resp.status, 403);
}

#[tokio::test(flavor = "multi_thread")]
async fn register_success_sets_the_session_cookie() {
    let server = TestServer::start().await;
    verify_email(&server, "test@example.com");
    let http = server.http();
    let resp = http.post("/api/auth/register", json!({"email": "test@example.com", "password": "password123", "display_name": "TestUser"})).await;
    assert_eq!(resp.status, 200);
    assert_eq!(resp.json()["user"]["display_name"], "TestUser");
    let cookie = resp.headers_all("set-cookie").into_iter().find(|c| c.starts_with("session_token=")).expect("session süti");
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Lax") && cookie.contains("Path=/"));
    assert!(!cookie.contains("Secure"), "helyi, titkosítatlan kapcsolaton nincs Secure");
    // belépve marad
    assert_eq!(http.get("/api/auth/me").await.status, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn register_validation() {
    let server = TestServer::start().await;
    let http = server.http();
    for (body, expect) in [
        (json!({"email": "test@example.com"}), None),
        (json!({"email": "test@example.com", "password": "12345", "display_name": "TestUser"}), Some("legalább 6")),
        (json!({"email": "test2@example.com", "password": "x".repeat(129), "display_name": "TestUser"}), None),
        (json!({"email": "bad-email", "password": "password123", "display_name": "TestUser"}), None),
        (json!({"email": "test3@example.com", "password": "password123", "display_name": "<script>alert(1)</script>"}), None),
        (json!({"email": 5, "password": ["x"], "display_name": {"a": 1}}), None),
    ] {
        let resp = http.post("/api/auth/register", body.clone()).await;
        assert_eq!(resp.status, 400, "{body}");
        if let Some(text) = expect {
            assert!(resp.json()["message"].as_str().unwrap().contains(text));
        }
    }
    assert_eq!(http.post("/api/auth/login", json!({"email": 5, "password": 7})).await.status, 400);
    assert_eq!(http.post("/api/auth/verify-code", json!({"email": 1, "code": 2})).await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn register_duplicate_email_conflicts() {
    let server = TestServer::start().await;
    server.create_user("test@example.com", "User1");
    verify_email(&server, "test@example.com");
    let resp = server.http().post("/api/auth/register", json!({"email": "test@example.com", "password": "password456", "display_name": "User2"})).await;
    assert_eq!(resp.status, 409);
}

// ---------------------------------------------------------------------------------------------------
// belépés / kilépés / munkamenet
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn login_rules() {
    let server = TestServer::start().await;
    server.create_user("test@example.com", "TestUser");
    let http = server.http();
    let ok = http.post("/api/auth/login", json!({"email": "test@example.com", "password": PASSWORD})).await;
    assert_eq!(ok.status, 200);
    assert_eq!(ok.json()["success"], true);
    assert!(ok.headers_all("set-cookie").iter().any(|c| c.starts_with("session_token=")));
    assert_eq!(server.http().post("/api/auth/login", json!({"email": "test@example.com", "password": "wrong"})).await.status, 401);
    assert_eq!(server.http().post("/api/auth/login", json!({"email": "nobody@example.com", "password": "pass"})).await.status, 401);
    assert_eq!(server.http().post("/api/auth/login", json!({"email": "", "password": ""})).await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn login_is_case_insensitive_on_the_email() {
    let server = TestServer::start().await;
    server.create_user("Mixed@Example.com", "Mixed");
    let resp = server.http().post("/api/auth/login", json!({"email": "  MIXED@example.COM ", "password": PASSWORD})).await;
    assert_eq!(resp.status, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn me_and_logout() {
    let server = TestServer::start().await;
    let uid = server.create_user("flow@example.com", "FlowUser");
    let http = server.http();
    assert_eq!(http.get("/api/auth/me").await.status, 401);
    http.set_cookie(Some("invalid_token"));
    assert_eq!(http.get("/api/auth/me").await.status, 401);
    let token = server.app.db.create_session(uid, None, None).unwrap();
    http.set_cookie(Some(&token));
    let me = http.get("/api/auth/me").await;
    assert_eq!(me.status, 200);
    assert_eq!(me.json()["user"]["display_name"], "FlowUser");
    assert!(me.json()["user"].get("is_admin").is_none(), "a nem admin válaszában nincs is_admin");
    assert_eq!(http.post("/api/auth/logout", json!({})).await.json()["success"], true);
    // a munkamenet az adatbázisból is törlődött
    http.set_cookie(Some(&token));
    assert_eq!(http.get("/api/auth/me").await.status, 401);
    // munkamenet nélküli kilépés is sikeres
    assert_eq!(server.http().post("/api/auth/logout", json!({})).await.json()["success"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_admin_sees_the_admin_flag() {
    let server = TestServer::start().await;
    let admin = server.admin_http().await;
    assert_eq!(admin.get("/api/auth/me").await.json()["user"]["is_admin"], true);
    server.create_user("plain@example.com", "Plain");
    let plain = server.http_as("plain@example.com").await;
    assert!(plain.get("/api/auth/me").await.json()["user"].get("is_admin").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn socket_token_endpoint() {
    let server = TestServer::start().await;
    let http = server.http();
    assert_eq!(http.get("/api/auth/socket-token").await.status, 401);
    server.create_user("tok@example.com", "Tok");
    let http = server.http_as("tok@example.com").await;
    let resp = http.get("/api/auth/socket-token").await;
    assert_eq!(resp.status, 200);
    let token = resp.json()["token"].as_str().unwrap().to_string();
    let uid = server.user_id("tok@example.com");
    assert_eq!(scrabble::socket_auth::verify_socket_token(&server.app.config.secret_key, Some(&token), scrabble::socket_auth::TOKEN_MAX_AGE), Some(uid));
}

// ---------------------------------------------------------------------------------------------------
// profil
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn profile_is_empty_for_a_new_user() {
    let server = TestServer::start().await;
    assert_eq!(server.http().get("/api/auth/profile").await.status, 401);
    server.create_user("test@example.com", "TestUser");
    let http = server.http_as("test@example.com").await;
    let data = http.get("/api/auth/profile").await.json();
    assert_eq!(data["success"], true);
    assert_eq!(data["stats"]["games_played"], 0);
    assert_eq!(data["stats"]["win_rate"].as_f64(), Some(0.0));
    assert_eq!(data["stats"]["avg_score"].as_f64(), Some(0.0));
    assert!(data["history"].as_array().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_counts_finished_games() {
    let server = TestServer::start().await;
    let uid = server.create_user("test@example.com", "TestUser");
    server.app.db.finish_game("room-1", "{}", &[player("TestUser", Some(uid), 100, true), player("Bot", None, 50, false)], "", false).unwrap();
    let http = server.http_as("test@example.com").await;
    let data = http.get("/api/auth/profile").await.json();
    assert_eq!(data["stats"]["games_played"], 1);
    assert_eq!(data["stats"]["games_won"], 1);
    assert_eq!(data["stats"]["win_rate"].as_f64(), Some(100.0));
    assert_eq!(data["history"].as_array().unwrap().len(), 1);
    assert_eq!(data["history"][0]["final_score"], 100);
    assert_eq!(data["history"][0]["is_winner"], true);
}

// ---------------------------------------------------------------------------------------------------
// játék API: lépések, mentett játékok, feladás
// ---------------------------------------------------------------------------------------------------

fn saved(server: &TestServer, room: &str, players: &[GamePlayerData], owner: &str) -> i64 {
    server.app.db.save_game(room, "Room", "{}", false, players, owner, None, false, false).unwrap()
}

fn mv(n: i64, who: &str, kind: &str, details: &str) -> StoredMove {
    StoredMove { move_number: n, player_name: who.into(), action_type: kind.into(), details_json: Some(details.into()), board_snapshot_json: Some("{}".into()) }
}

#[tokio::test(flavor = "multi_thread")]
async fn game_moves_api() {
    let server = TestServer::start().await;
    assert_eq!(server.http().get("/api/game/1/moves").await.status, 401);
    let uid = server.create_user("moves@test.com", "MovesUser");
    let other = server.create_user("other@test.com", "Other");
    let http = server.http_as("moves@test.com").await;
    let game_id = saved(&server, "room-1", &[player("Alice", Some(uid), 0, false)], "Alice");
    server.app.db.add_game_move(game_id, &mv(1, "Alice", "place", "{\"tiles\":[]}")).unwrap();
    server.app.db.add_game_move(game_id, &mv(2, "Bob", "pass", "{}")).unwrap();
    let resp = http.get(&format!("/api/game/{game_id}/moves")).await;
    assert_eq!(resp.status, 200);
    let data = resp.json();
    assert_eq!(data["moves"].as_array().unwrap().len(), 2);
    assert_eq!(data["moves"][0]["player_name"], "Alice");
    assert_eq!(http.get("/api/game/99999/moves").await.status, 404);
    // nem résztvevő
    let foreign = saved(&server, "room-2", &[player("Other", Some(other), 0, false)], "Other");
    assert_eq!(http.get(&format!("/api/game/{foreign}/moves")).await.status, 403);
    // nem szám az azonosító → a szokásos 404
    assert_eq!(http.get("/api/game/abc/moves").await.status, 404);
}

#[tokio::test(flavor = "multi_thread")]
async fn saved_games_api() {
    let server = TestServer::start().await;
    assert_eq!(server.http().get("/api/auth/saved-games").await.status, 401);
    let uid = server.create_user("test@example.com", "TestUser");
    let http = server.http_as("test@example.com").await;
    assert_eq!(http.get("/api/auth/saved-games").await.json()["games"], json!([]));
    saved(&server, "room-1", &[player("TestUser", Some(uid), 50, false)], "TestUser");
    let data = http.get("/api/auth/saved-games").await.json();
    assert_eq!(data["success"], true);
    assert_eq!(data["games"].as_array().unwrap().len(), 1);
    assert_eq!(data["games"][0]["room_name"], "Room");
}

#[tokio::test(flavor = "multi_thread")]
async fn abandon_game_api() {
    let server = TestServer::start().await;
    assert_eq!(server.http().post("/api/game/1/abandon", json!({})).await.status, 401);
    let uid = server.create_user("test@example.com", "TestUser");
    let http = server.http_as("test@example.com").await;
    let game_id = saved(&server, "room-1", &[player("TestUser", Some(uid), 50, false)], "TestUser");
    let resp = http.post(&format!("/api/game/{game_id}/abandon"), json!({})).await;
    assert_eq!(resp.json()["success"], true);
    let game = server.app.db.get_game_by_id(game_id).unwrap().unwrap();
    assert_eq!(game.get("status").and_then(|v| v.as_str()), Some("abandoned"));
    assert_eq!(http.post("/api/game/99999/abandon", json!({})).await.status, 404);
    // befejezett játék nem adható fel
    let finished = server.app.db.finish_game("room-2", "{}", &[player("TestUser", Some(uid), 50, false)], "", false).unwrap();
    assert_eq!(http.post(&format!("/api/game/{finished}/abandon"), json!({})).await.status, 400);
    // nem résztvevő
    let foreign = saved(&server, "room-3", &[], "X");
    assert_eq!(http.post(&format!("/api/game/{foreign}/abandon"), json!({})).await.status, 403);
}

// ---------------------------------------------------------------------------------------------------
// forgalomkorlát
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn ip_rate_limits_apply() {
    let server = TestServer::start().await;
    restore_default_limits(&server.app);
    let http = server.http();
    for _ in 0..3 {
        assert_eq!(http.post("/api/auth/request-code", json!({"email": "a@example.com"})).await.status, 200);
    }
    let resp = http.post("/api/auth/request-code", json!({"email": "a@example.com"})).await;
    assert_eq!(resp.status, 429);
    assert_eq!(resp.json()["success"], false);
    for _ in 0..10 {
        assert_eq!(http.post("/api/auth/login", json!({"email": "x@example.com", "password": "rossz"})).await.status, 401);
    }
    assert_eq!(http.post("/api/auth/login", json!({"email": "x@example.com", "password": "rossz"})).await.status, 429);
    // a regisztráció 3 / óra
    for _ in 0..3 {
        assert_eq!(http.post("/api/auth/register", json!({"email": "new@example.com", "password": "password123", "display_name": "N"})).await.status, 403);
    }
    assert_eq!(http.post("/api/auth/register", json!({"email": "new@example.com", "password": "password123", "display_name": "N"})).await.status, 429);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_client_ip_comes_from_the_proxy_header_only_for_loopback_peers() {
    let server = TestServer::start().await;
    restore_default_limits(&server.app);
    let http = server.http();
    // a tesztkliens loopbackről jön, ezért a CF-Connecting-IP érvényes: két különböző „kliens” külön korlátot kap
    for ip in ["198.51.100.1", "198.51.100.2"] {
        for _ in 0..3 {
            let resp = http.request("POST", "/api/auth/request-code", Some(json!({"email": "a@example.com"})), &[("CF-Connecting-IP", ip)]).await;
            assert_eq!(resp.status, 200);
        }
        let resp = http.request("POST", "/api/auth/request-code", Some(json!({"email": "a@example.com"})), &[("CF-Connecting-IP", ip)]).await;
        assert_eq!(resp.status, 429);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn error_pages_are_the_standard_ones() {
    let server = TestServer::start().await;
    let http = server.http();
    let missing = http.get("/nincs-ilyen").await;
    assert_eq!(missing.status, 404);
    assert!(missing.text().contains("<title>404 Not Found</title>"));
    assert!(missing.header("content-type").unwrap().starts_with("text/html"));
    let wrong_method = http.request("POST", "/", None, &[]).await;
    assert_eq!(wrong_method.status, 405);
    assert!(wrong_method.text().contains("405 Method Not Allowed"));
    assert_eq!(http.get("/static/nincs.js").await.status, 404);
}

#[tokio::test(flavor = "multi_thread")]
async fn body_must_be_json_objects() {
    let server = TestServer::start().await;
    let http = server.http();
    // nem JSON tartalomtípus → mintha üres lenne a törzs
    let resp = http.request("POST", "/api/auth/login", None, &[("Content-Type", "text/plain")]).await;
    assert_eq!(resp.status, 400);
    let _: Value = resp.json();
}
