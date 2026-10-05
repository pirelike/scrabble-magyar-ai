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
            value["details"] = serde_json::from_str(&row.text("details_json")).unwrap_or(Value::Null);
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
