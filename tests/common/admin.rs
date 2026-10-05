//! Segédek az admin panel tesztjeihez: napló, munkamenet-mezők, felhasználók.

#![allow(dead_code)]

use super::*;
use scrabble::db::{RowExt, fetch_all};

/// Egy regisztrált felhasználó és a vele bejelentkezett HTTP kliens.
pub async fn make_user(server: &TestServer, email: &str, name: &str) -> (i64, Http) {
    let id = server.create_user(email, name);
    (id, server.http_as(email).await)
}

/// A naplóbejegyzések (legfrissebb elöl): a `details_json` feldolgozva a `details` mezőbe.
pub fn audit_rows(server: &TestServer) -> Vec<Value> {
    let rows = server.app.db.with(|tx| Ok(fetch_all(tx, "SELECT * FROM admin_audit ORDER BY id DESC", []).unwrap())).unwrap();
    rows.into_iter()
        .map(|row| {
            let mut value = Value::Object(row.clone());
            let details: Value = serde_json::from_str(&row.text("details_json")).unwrap_or(Value::Null);
            value["reason"] = details.get("reason").cloned().unwrap_or(Value::Null);
            value["details"] = details;
            value
        })
        .collect()
}

pub fn audit_actions(server: &TestServer) -> Vec<String> {
    audit_rows(server).iter().map(|r| r["action"].as_str().unwrap_or("").to_string()).collect()
}

/// A munkamenet egy oszlopának felülírása (pl. `admin_seen_at`, `sudo_until`, `expires_at`).
pub fn set_session(server: &TestServer, token: &str, column: &str, value: Option<&str>) {
    let sql = format!("UPDATE sessions SET {column} = ? WHERE token = ?");
    server.app.db.with(|tx| tx.execute(&sql, rusqlite::params![value, token]).map(|_| ())).unwrap();
}

/// Az időbélyeg `secs` másodperccel ezelőttről (az adatbázis szövegalakjában).
pub fn ts_ago(secs: i64) -> String {
    scrabble::util::format_ts(chrono::Utc::now().naive_utc() - chrono::Duration::seconds(secs))
}

pub fn admin_seen(server: &TestServer, token: &str) -> Option<String> {
    server.app.db.get_admin_session(Some(token)).and_then(|(seen, _)| seen)
}

pub fn sudo_active(server: &TestServer, token: &str) -> bool {
    scrabble::admin::session::sudo_active(&server.app, token)
}

/// Socket.IO kapcsolat egy tetszőleges regisztrált felhasználóként.
pub async fn sio_user(server: &TestServer, user_id: i64, name: &str) -> Sio {
    let sio = Sio::connect(server).await;
    let before = server.app.state.lock().player_names.len();
    sio.emit("set_name", json!({"name": name, "is_guest": false, "auth_token": server.socket_token(user_id)}));
    let end = tokio::time::Instant::now() + Duration::from_secs(10);
    while server.app.state.lock().player_names.len() <= before && tokio::time::Instant::now() < end {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    sio.settle().await;
    sio.take();
    sio
}

/// A mentett levelező beállítás a hamis SMTP kiszolgálóra (titkosítás nélkül): a szerver ezen át küld.
pub fn use_smtp(server: &TestServer, smtp: &smtp::FakeSmtp) {
    let config = json!({
        "host": "127.0.0.1", "port": smtp.port(), "security": "none", "username": "", "password": "",
        "from_address": "scrabble@example.com", "from_name": "Teszt Scrabble", "verify_tls": false,
    });
    server.app.db.with(|tx| server.app.mailer.save(tx, &config, 0)).unwrap();
}

/// Befejezett játék az adatbázisban (az értékszámot és a statisztikát is rögzíti).
///
/// `results`: (user_id vagy None, név, pont); a legtöbb pontot elérő nyer. Visszatér: a játék azonosítója.
pub fn finished_game(server: &TestServer, room_id: &str, results: &[(Option<i64>, &str, i64)], has_bots: bool, resigned: &[&str], moves: i64) -> i64 {
    use scrabble::db::{GamePlayerData, StoredMove};
    let best = results.iter().map(|r| r.2).max().unwrap_or(0);
    let players: Vec<GamePlayerData> = results
        .iter()
        .map(|(uid, name, score)| GamePlayerData {
            player_name: name.to_string(),
            user_id: *uid,
            score: *score,
            is_winner: *score == best,
            resigned: resigned.contains(name),
        })
        .collect();
    let state = json!({
        "players": results.iter().map(|(_, name, _)| json!({"name": name, "resigned": resigned.contains(name), "is_bot": false})).collect::<Vec<_>>(),
        "finished": true,
    });
    let id = server.app.db.finish_game(room_id, &state.to_string(), &players, &format!("Játék {room_id}"), has_bots).unwrap();
    for number in 1..=moves {
        let mover = results[((number - 1) as usize) % results.len()].1;
        server
            .app
            .db
            .add_game_move(
                id,
                &StoredMove {
                    move_number: number,
                    player_name: mover.to_string(),
                    action_type: "pass".into(),
                    details_json: Some(json!({"rack": ["A"], "score": 0}).to_string()),
                    board_snapshot_json: Some("{}".into()),
                },
            )
            .unwrap();
    }
    id
}

/// Egy befejezett játék két játékossal, mozgásnapló nélkül.
pub fn simple_game(server: &TestServer, room_id: &str, a: (i64, &str, i64), b: (i64, &str, i64)) -> i64 {
    finished_game(server, room_id, &[(Some(a.0), a.1, a.2), (Some(b.0), b.1, b.2)], false, &[], 0)
}

/// Szoba létrehozása és indítása: a tulajdonos és a csatlakozók. Visszatér: a szoba azonosítója és a kódja.
pub async fn live_room(server: &TestServer, owner: &Sio, others: &[&Sio], options: Value) -> (String, String) {
    let mut options = options;
    if options.get("name").is_none() {
        options["name"] = json!("Teszt szoba");
    }
    if options.get("max_players").is_none() {
        options["max_players"] = json!(4);
    }
    owner.emit("create_room", options);
    let code = owner.wait_code().await;
    for other in others {
        other.emit("join_room", json!({"code": code}));
        other.settle().await;
    }
    owner.emit0("start_game");
    owner.settle_long().await;
    owner.take();
    for other in others {
        other.take();
    }
    let id = server.app.state.lock().rooms.values().find(|r| r.join_code == code).map(|r| r.id.clone()).expect("szoba");
    (id, code)
}

/// Lekérdezési sztring (az értékek százalékkódolva): `?a=1&b=%C3%A1`.
pub fn qs(pairs: &[(&str, &str)]) -> String {
    format!("?{}", serde_urlencoded::to_string(pairs).unwrap())
}

/// A válaszban lévő `items` mezők egy kulcsa.
pub fn items_of(response: &Resp, key: &str) -> Vec<Value> {
    response.json()["items"].as_array().unwrap_or(&Vec::new()).iter().map(|i| i[key].clone()).collect()
}

/// A naplóbejegyzések egy művelet-előtaggal (legfrissebb elöl).
pub fn audit_of(server: &TestServer, prefix: &str) -> Vec<Value> {
    audit_rows(server).into_iter().filter(|r| r["action"].as_str().unwrap_or("").starts_with(prefix)).collect()
}

/// Futásidejű beállítás közvetlenül az adatbázisban (az `app.settings` gyorsítótára is frissül).
pub fn set_setting(server: &TestServer, key: &str, value: Value) {
    server.app.db.with(|tx| server.app.settings.store(tx, key, &value, None).map(|_| ()).map_err(|e| rusqlite::Error::InvalidParameterName(e.0))).unwrap();
}
