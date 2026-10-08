//! HTTP: ranglista, szótár-ellenőrző, PWA (a Python `test_public_api.py` megfelelője).

mod common;
use common::*;
use scrabble::db::GamePlayerData;
use serde_json::{Value, json};

fn player(name: &str, user_id: Option<i64>, score: i64, winner: bool) -> GamePlayerData {
    GamePlayerData { player_name: name.into(), user_id, score, is_winner: winner, resigned: false }
}

fn finished(server: &TestServer, room: &str, results: &[(Option<i64>, &str, i64, bool)]) {
    let players: Vec<GamePlayerData> = results.iter().map(|(uid, n, s, w)| player(n, *uid, *s, *w)).collect();
    server.app.db.finish_game(room, "{}", &players, room, false).unwrap();
}

// ---------------------------------------------------------------------------------------------------
// ranglista
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn leaderboard_route_basics() {
    let server = TestServer::start().await;
    let http = server.http();
    let empty = http.get("/api/leaderboard").await.json();
    assert_eq!(empty["entries"], json!([]));
    let a = server.create_user("aladar@example.com", "Aladár");
    finished(&server, "r1", &[(Some(a), "Aladár", 100, true)]);
    let resp = http.get("/api/leaderboard").await;
    assert_eq!(resp.status, 200);
    let data = resp.json();
    assert_eq!(data["success"], true);
    assert_eq!(data["metric"], "wins");
    assert_eq!(data["entries"][0]["display_name"], "Aladár");
    assert_eq!(data["me"], Value::Null);
    assert!(!resp.text().contains("email"));
    assert_eq!(http.get("/api/leaderboard?metric=avg_score").await.json()["min_games"], 3);
    assert_eq!(http.get("/api/leaderboard?metric=x").await.status, 400);
    assert_eq!(http.get("/api/leaderboard?limit=abc").await.status, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn leaderboard_marks_my_row() {
    let server = TestServer::start().await;
    let a = server.create_user("aladar@example.com", "Aladár");
    let b = server.create_user("bela@example.com", "Béla");
    finished(&server, "r1", &[(Some(a), "Aladár", 100, true), (Some(b), "Béla", 50, false)]);
    let http = server.http_as("aladar@example.com").await;
    let data = http.get("/api/leaderboard").await.json();
    assert_eq!(data["entries"][0]["is_me"], true);
    assert_eq!(data["me"]["user_id"], a);
    // a saját helyezés a top listán kívül is látszik
    let data = server.http_as("bela@example.com").await.get("/api/leaderboard?limit=1").await.json();
    assert_eq!(data["entries"].as_array().unwrap().len(), 1);
    assert_eq!(data["me"]["user_id"], b);
    assert!(data["me"]["rank"].as_i64().unwrap() > 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn leaderboard_is_rate_limited() {
    let server = TestServer::start().await;
    restore_default_limits(&server.app);
    let http = server.http();
    let mut codes = Vec::new();
    for _ in 0..35 {
        codes.push(http.get("/api/leaderboard").await.status);
    }
    assert_eq!(codes[0], 200);
    assert!(codes.contains(&429));
}

#[tokio::test(flavor = "multi_thread")]
async fn bot_games_are_excluded_but_counted_in_the_profile() {
    let server = TestServer::start().await;
    let a = server.create_user("aladar@example.com", "Aladár");
    let players = [player("Aladár", Some(a), 300, true), player("Rezső", None, 100, false)];
    server.app.db.finish_game("botgame", "{}", &players, "Bot", true).unwrap();
    let http = server.http();
    assert_eq!(http.get("/api/leaderboard").await.json()["entries"], json!([]));
    assert_eq!(server.app.db.get_user_by_id(a).unwrap().unwrap().games_played, 1);
    server.app.db.finish_game("humangame", "{}", &[player("Aladár", Some(a), 300, true)], "Ember", false).unwrap();
    let entries = http.get("/api/leaderboard").await.json()["entries"].clone();
    assert_eq!(entries.as_array().unwrap().len(), 1);
    assert_eq!(entries[0]["games_played"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_finished_server_game_with_bots_is_marked_and_not_ranked() {
    let server = TestServer::start().await;
    let uid = server.ensure_user(1);
    let p = server.player(1).await;
    p.call("create_room", json!({"name": "R", "ai_players": ["easy"]})).await;
    p.call("start_game", Value::Null).await;
    {
        let mut st = server.app.state.lock();
        let room = st.rooms.values_mut().next().unwrap();
        room.game.finished = true;
        room.game.winners = vec![room.game.players[0].clone()];
    }
    let room_id = p.room();
    {
        let mut st = server.app.state.lock();
        scrabble::server::core::save_game_to_db(&server.app, &mut st, &room_id);
    }
    assert_eq!(server.app.db.get_user_by_id(uid).unwrap().unwrap().games_played, 1);
    assert_eq!(server.http().get("/api/leaderboard").await.json()["entries"], json!([]));
    let flag: i64 = server.app.db.raw().query_row("SELECT has_bots FROM saved_games", [], |r| r.get(0)).unwrap();
    assert_eq!(flag, 1);
}

// ---------------------------------------------------------------------------------------------------
// szótár-ellenőrző
// ---------------------------------------------------------------------------------------------------

async fn check(http: &Http, q: &str) -> Value {
    http.get(&format!("/api/dictionary/check?q={q}")).await.json()["results"][0].clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn dictionary_check_words() {
    let server = TestServer::start().await;
    let http = server.http();
    let alma = check(&http, "alma").await;
    assert_eq!(alma["word"], "ALMA");
    assert_eq!(alma["valid"], true);
    assert_eq!(alma["tiles"], json!(["A", "L", "M", "A"]));
    assert_eq!(alma["score"], 4);
    let wrong = check(&http, "almak").await;
    assert_eq!(wrong["valid"], false);
    assert_eq!(wrong["reason"], "not_in_dictionary");
    assert!(wrong["suggestions"].is_array());
    // regresszió: a SALYT nem magyar szó
    let salyt = check(&http, "salyt").await;
    assert_eq!(salyt["valid"], false);
    assert_eq!(salyt["reason"], "not_in_dictionary");
    assert!(!salyt["suggestions"].as_array().unwrap().is_empty());
    let szek = check(&http, "szék").await;
    assert_eq!(szek["tiles"], json!(["SZ", "É", "K"]));
    assert_eq!(szek["score"], 7);
    assert_eq!(check(&http, "budapest").await["valid"], false, "tulajdonnév");
}

#[tokio::test(flavor = "multi_thread")]
async fn dictionary_check_input_handling() {
    let server = TestServer::start().await;
    let http = server.http();
    let results = http.get("/api/dictionary/check?q=alma,k%C3%B6rte%20xyzzy").await.json()["results"].clone();
    let words: Vec<&str> = results.as_array().unwrap().iter().map(|r| r["word"].as_str().unwrap()).collect();
    assert_eq!(words, vec!["ALMA", "KÖRTE", "XYZZY"]);
    let valid: Vec<bool> = results.as_array().unwrap().iter().map(|r| r["valid"].as_bool().unwrap()).collect();
    assert_eq!(valid, vec![true, true, false]);
    assert_eq!(http.get("/api/dictionary/check?q=alma%20ALMA%20Alma").await.json()["results"].as_array().unwrap().len(), 1);
    let many: Vec<String> = (0..20).map(|i| format!("sz%C3%B3{i}")).collect();
    assert_eq!(http.get(&format!("/api/dictionary/check?q={}", many.join("%20"))).await.json()["results"].as_array().unwrap().len(), 8);
    assert_eq!(check(&http, "al1ma").await["reason"], "invalid_chars");
    assert_eq!(check(&http, &"a".repeat(20)).await["reason"], "too_long");
    assert_eq!(check(&http, "a").await["reason"], "too_short");
    let empty = http.get("/api/dictionary/check?q=").await;
    assert_eq!(empty.status, 400);
    assert_eq!(empty.json()["success"], false);
    let post = http.post("/api/dictionary/check", json!({"words": ["alma", "körte"]})).await.json();
    assert_eq!(post["results"].as_array().unwrap().iter().map(|r| r["valid"].as_bool().unwrap()).collect::<Vec<_>>(), vec![true, true]);
    assert_eq!(http.request("POST", "/api/dictionary/check", None, &[("Content-Type", "text/plain")]).await.status, 400);
    let huge = http.get(&format!("/api/dictionary/check?q={}", "sz%C3%B3%20".repeat(5000))).await;
    assert_eq!(huge.status, 200);
    assert!(huge.json()["results"].as_array().unwrap().len() <= 8);
}

#[tokio::test(flavor = "multi_thread")]
async fn dictionary_check_is_rate_limited() {
    let server = TestServer::start().await;
    restore_default_limits(&server.app);
    let http = server.http();
    let mut codes = Vec::new();
    for _ in 0..65 {
        codes.push(http.get("/api/dictionary/check?q=alma").await.status);
    }
    assert!(codes.contains(&429));
}

// ---------------------------------------------------------------------------------------------------
// PWA
// ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn manifest_and_icons() {
    let server = TestServer::start().await;
    let http = server.http();
    let resp = http.get("/manifest.webmanifest").await;
    assert_eq!(resp.status, 200);
    assert!(resp.header("content-type").unwrap().starts_with("application/manifest+json"));
    let manifest = resp.json();
    assert_eq!(manifest["display"], "standalone");
    assert!(manifest["start_url"].as_str().unwrap().starts_with('/'));
    assert_eq!(manifest["scope"], "/");
    assert!(!manifest["name"].as_str().unwrap().is_empty() && !manifest["short_name"].as_str().unwrap().is_empty());
    let sizes: Vec<(String, String)> = manifest["icons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| (i["sizes"].as_str().unwrap().to_string(), i["purpose"].as_str().unwrap_or("").to_string()))
        .collect();
    assert!(sizes.contains(&("192x192".into(), "any".into())) && sizes.contains(&("512x512".into(), "maskable".into())));
    for icon in manifest["icons"].as_array().unwrap() {
        let r = http.get(icon["src"].as_str().unwrap()).await;
        assert_eq!(r.status, 200, "{}", icon["src"]);
        assert_eq!(r.header("content-type").unwrap(), icon["type"].as_str().unwrap());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn png_icons_have_the_right_dimensions() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("web/static/icons");
    for (name, size) in [("icon-192.png", 192u32), ("icon-512.png", 512), ("icon-maskable-512.png", 512), ("apple-touch-icon.png", 180)] {
        let bytes = std::fs::read(base.join(name)).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        assert_eq!((width, height), (size, size), "{name}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn service_worker() {
    let server = TestServer::start().await;
    let http = server.http();
    let resp = http.get("/sw.js").await;
    assert_eq!(resp.status, 200);
    assert!(resp.header("content-type").unwrap().contains("javascript"));
    assert_eq!(resp.header("service-worker-allowed").unwrap(), "/");
    assert_eq!(resp.header("cache-control").unwrap(), "no-cache");
    let body = resp.text();
    assert!(body.contains("const VERSION = ") && !body.contains("{{"));
    assert!(body.contains("addEventListener('fetch'"));
    assert!(body.contains("/socket.io/") && body.contains("/api/"));
    // az előtöltött váz minden eleme kiszolgálható
    let start = body.find("const SHELL_URLS").unwrap();
    let block = &body[start..body[start..].find("];").unwrap() + start];
    let re = regex::Regex::new(r"'(/[^']*)'").unwrap();
    let paths: Vec<String> = re.captures_iter(block).map(|c| c[1].to_string()).collect();
    assert!(!paths.is_empty());
    for path in paths {
        assert_eq!(http.get(&path).await.status, 200, "{path}");
    }
    let offline = http.get("/static/offline.html").await;
    assert_eq!(offline.status, 200);
    assert!(offline.text().contains("<!DOCTYPE html>"));
}

#[tokio::test(flavor = "multi_thread")]
async fn index_page_links_and_versions() {
    let server = TestServer::start().await;
    let http = server.http();
    let html = http.get("/").await.text();
    assert!(html.contains(r#"rel="manifest" href="/manifest.webmanifest""#));
    assert!(html.contains("apple-touch-icon"));
    assert!(html.contains("mobile-web-app-capable"));
    let re = regex::Regex::new(r"/static/app\.js\?v=(\d+)").unwrap();
    let version = re.captures(&html).expect("verziózott app.js")[1].to_string();
    assert!(regex::Regex::new(r"/static/style\.css\?v=\d+").unwrap().is_match(&html));
    assert!(!html.contains("{{"));
    // a service worker ugyanazt a verziót használja
    let sw = http.get("/sw.js").await.text().replace('"', "");
    assert!(sw.contains(&format!("const VERSION = {version};")));
}

#[tokio::test(flavor = "multi_thread")]
async fn static_files_are_served_with_charset() {
    let server = TestServer::start().await;
    let http = server.http();
    for path in ["/static/app.js", "/static/style.css", "/static/offline.html", "/static/i18n.js"] {
        let resp = http.get(path).await;
        assert_eq!(resp.status, 200, "{path}");
        assert!(resp.header("content-type").unwrap().contains("charset=utf-8"), "{path}: {:?}", resp.header("content-type"));
        assert_eq!(resp.header("cache-control").unwrap(), "no-cache");
    }
}
