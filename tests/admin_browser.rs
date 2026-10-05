//! Az admin felület valódi böngészőben: minden menüpont betöltődik, a legfontosabb műveletek és az élő események
//! működnek (a Python `test_admin_browser.py` megfelelője).
//!
//! Valódi szervert indít (ideiglenes adatbázissal, seedelt adatokkal) és Playwrightot (node) futtat rajta
//! (`tests/browser/admin_smoke.js`). Ha nincs node, Playwright vagy Chromium, a teszt kimarad. Útvonalak felülírása:
//! `PLAYWRIGHT_MODULE` (a playwright node-modul mappája), `PLAYWRIGHT_CHROMIUM` (a böngésző futtatható fájlja).
//! Az admin: boss@example.com / secret12.

mod common;
use common::admin::*;
use common::*;
use scrabble::db::StoredMove;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;

const BOSS: &str = "boss@example.com";

fn playwright_module() -> Option<PathBuf> {
    let candidates = [
        std::env::var("PLAYWRIGHT_MODULE").ok(),
        Some("/opt/node-tools/node_modules/playwright".into()),
        Some("/opt/node22/lib/node_modules/playwright".into()),
    ];
    candidates.into_iter().flatten().map(PathBuf::from).find(|p| p.is_dir())
}

fn chromium() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("PLAYWRIGHT_CHROMIUM").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(explicit);
    }
    let mut found: Vec<PathBuf> =
        std::fs::read_dir("/opt/pw-browsers").ok()?.filter_map(|e| e.ok()).map(|e| e.path().join("chrome-linux/chrome")).filter(|p| p.is_file()).collect();
    found.sort();
    found.pop()
}

fn board(tiles: &[(usize, usize, char)]) -> String {
    let mut cells: Vec<Vec<Value>> = vec![vec![Value::Null; 15]; 15];
    for (row, col, letter) in tiles {
        cells[*row][*col] = json!({"letter": letter.to_string(), "is_blank": false});
    }
    json!(cells).to_string()
}

/// Egy befejezett játék lépésnaplóval és tábla-pillanatképekkel (a visszajátszáshoz).
fn finished(server: &TestServer, room: &str, results: &[(i64, &str, i64)], moves: usize) {
    use scrabble::db::GamePlayerData;
    let best = results.iter().map(|r| r.2).max().unwrap();
    let players: Vec<GamePlayerData> = results
        .iter()
        .map(|(uid, name, score)| GamePlayerData {
            player_name: name.to_string(),
            user_id: Some(*uid),
            score: *score,
            is_winner: *score == best,
            resigned: false,
        })
        .collect();
    let state =
        json!({"players": results.iter().map(|(_, n, _)| json!({"name": n, "resigned": false, "is_bot": false})).collect::<Vec<_>>(), "finished": true});
    let id = server.app.db.finish_game(room, &state.to_string(), &players, &format!("Játék {room}"), false).unwrap();
    let word: Vec<char> = "ALMA".chars().collect();
    for n in 1..=moves {
        let placed: Vec<(usize, usize, char)> = word[..n.min(word.len())].iter().enumerate().map(|(i, c)| (7, 6 + i, *c)).collect();
        let tiles: Vec<Value> = placed.iter().map(|(r, c, l)| json!({"row": r, "col": c, "letter": l.to_string(), "is_blank": false})).collect();
        server
            .app
            .db
            .add_game_move(
                id,
                &StoredMove {
                    move_number: n as i64,
                    player_name: results[(n - 1) % results.len()].1.to_string(),
                    action_type: "place".into(),
                    details_json: Some(json!({"rack": ["A", "L", "M", "A", "K", "E", "T"], "score": 6 * n, "words": [word[..n.min(word.len())].iter().collect::<String>()], "tiles": tiles}).to_string()),
                    board_snapshot_json: Some(board(&placed)),
                },
            )
            .unwrap();
    }
}

struct Seeded {
    server: TestServer,
    _clients: Vec<Sio>,
}

async fn seeded() -> Seeded {
    let server = TestServer::start_with(|c| c.admin_emails = [BOSS.to_string()].into_iter().collect()).await;
    set_setting(&server, "bot_think_multiplier", json!(5.0));
    server.create_user(BOSS, "Főnök");
    let (anna, bela, cili, dani) = (
        server.create_user("anna@example.com", "Anna"),
        server.create_user("bela@example.com", "Béla"),
        server.create_user("cili@example.com", "Cili"),
        server.create_user("dani@example.com", "Dani"),
    );
    server
        .app
        .db
        .with(|tx| {
            tx.execute("UPDATE users SET banned_until = '9999-12-31 23:59:59', ban_reason = 'Sértegetés' WHERE id = ?", [dani])?;
            tx.execute("UPDATE users SET chat_muted_until = '2099-01-01 00:00:00' WHERE id = ?", [cili])?;
            Ok(())
        })
        .unwrap();
    for i in 0..12 {
        let ok = i % 2 == 0;
        server.app.db.record_login_event(
            "anna@example.com",
            ok,
            Some("203.0.113.9"),
            Some("Mozilla/5.0 Teszt"),
            if ok { Some(anna) } else { None },
            if ok { "" } else { "rossz jelszó" },
        );
    }
    finished(&server, "g1", &[(anna, "Anna", 310), (bela, "Béla", 250)], 4);
    finished(&server, "g2", &[(anna, "Anna", 280), (cili, "Cili", 300)], 4);
    let api = server.http_as(BOSS).await;
    let body = json!({"text_hu": "Szerdán karbantartás.", "text_en": "Maintenance on Wednesday.", "kind": "warning", "audience": "all", "reason": "smoke"});
    assert_eq!(api.admin_post("/announcements", body).await.status, 200);

    let (anna_io, bela_io, cili_io) = (sio_user(&server, anna, "Anna").await, sio_user(&server, bela, "Béla").await, sio_user(&server, cili, "Cili").await);
    live_room(&server, &anna_io, &[&bela_io], json!({"name": "Esti játék", "max_players": 4, "turn_time_limit": 90, "challenge_mode": true})).await;
    live_room(&server, &cili_io, &[], json!({"name": "Robotos játék", "max_players": 2, "ai_players": [6]})).await;
    anna_io.emit("send_chat", json!({"message": "Sziasztok!"}));
    cili_io.emit("send_chat", json!({"message": "Te hülye vagy"}));
    anna_io.settle().await;
    bela_io.emit("report_content", json!({"kind": "player", "name": "Anna", "reason": "Csal"}));
    anna_io.emit("report_content", json!({"kind": "chat", "name": "Béla", "message": "Te hülye vagy", "reason": "Sértő"}));
    anna_io.settle().await;
    Seeded { server, _clients: vec![anna_io, bela_io, cili_io] }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_admin_client_in_a_real_browser() {
    let (Some(module), Some(browser)) = (playwright_module(), chromium()) else {
        eprintln!("node, Playwright vagy Chromium nem elérhető: a teszt kimarad");
        return;
    };
    if !node_ok() {
        eprintln!("node nem elérhető: a teszt kimarad");
        return;
    }
    let seeded = seeded().await;
    let base = seeded.server.base();
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser/admin_smoke.js");
    let output = tokio::task::spawn_blocking(move || {
        Command::new("node")
            .arg(script)
            .env("ADMIN_SMOKE_BASE", base)
            .env("PLAYWRIGHT_MODULE", module)
            .env("PLAYWRIGHT_CHROMIUM", browser)
            .output()
            .expect("node")
    })
    .await
    .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let line = stdout.lines().find(|l| l.starts_with("RESULT ")).unwrap_or_else(|| panic!("nincs eredmény: {stdout}"));
    let data: Value = serde_json::from_str(&line["RESULT ".len()..]).unwrap();
    assert_eq!(data["problems"], json!([]), "{}", data["problems"]);
    let checks = &data["checks"];
    assert_eq!(checks["login"], 200);
    assert_eq!(checks["nav_items"], 15);
    assert_eq!((checks["muted_badge"].clone(), checks["banned_badge"].clone()), (json!(true), json!(true)));
    assert!(checks["search_hash"].as_str().unwrap().starts_with("#users/"));
    for key in ["subscribed", "live_overview", "watch_room", "live_room"] {
        assert_eq!(checks[key], true, "{key}");
    }
    assert_eq!(checks["report_toast"], true);
    assert_eq!(checks["mobile_nav"], true);
    assert!(checks["english_nav"].as_str().unwrap().starts_with("Overview"));
}

fn node_ok() -> bool {
    Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}
