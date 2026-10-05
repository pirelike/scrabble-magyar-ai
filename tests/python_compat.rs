//! A Python változattal létrehozott adatbázis és mentett játékállapotok használhatósága: bejelentkezés a
//! werkzeug-hash-sel, munkamenet, mentett játékok visszatöltése és újra-szerializálása.

use scrabble::db::Db;
use scrabble::game::{Game, MoveLog};
use serde_json::Value;
use std::path::PathBuf;

fn fixture_db() -> Db {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let target = std::env::temp_dir().join(format!("scrabble-compat-{}-{:?}.sqlite", std::process::id(), std::thread::current().id()));
    std::fs::copy(dir.join("python_fixture.sqlite"), &target).unwrap();
    let db = Db::open(target.to_str().unwrap()).unwrap();
    db.init().unwrap();
    db
}

fn meta() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/python_fixture.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn python_users_can_log_in_and_sessions_validate() {
    let db = fixture_db();
    let alice = db.verify_password("alice@example.com", "titok123").expect("a Python-hash érvényes");
    assert_eq!(alice.display_name, "Alice");
    assert!(db.verify_password("alice@example.com", "rossz").is_err());
    let bob = db.verify_password("bob@example.com", "Jelszó-ékezet ő").expect("ékezetes jelszó");
    assert_eq!(bob.display_name, "Bób");
    let token = meta()["session"].as_str().unwrap().to_string();
    let session = db.validate_session(Some(&token)).expect("a Python session érvényes");
    assert_eq!(session.id, alice.id);
}

#[test]
fn python_saved_games_roundtrip_through_game_model() {
    let db = fixture_db();
    let meta = meta();
    let mut checked = 0;
    for (key, original) in meta["states"].as_object().unwrap() {
        let row = db.load_active_games().unwrap().into_iter().chain(db.get_game_by_id(1).unwrap()).find(|r| r["room_name"] == format!("Szoba {key}"));
        // a befejezett játék már nem aktív: azonosító szerint keressük
        let row = row.or_else(|| (1..=10).filter_map(|id| db.get_game_by_id(id).unwrap()).find(|r| r["room_name"] == format!("Szoba {key}")));
        let row = row.unwrap_or_else(|| panic!("hiányzó játék: {key}"));
        let state: Value = serde_json::from_str(row["state_json"].as_str().unwrap()).unwrap();
        let game = Game::from_save_dict(&state).unwrap_or_else(|e| panic!("{key}: {e}"));
        let again = game.to_save_dict();
        assert_eq!(&again, original, "{key}: az újra-szerializált állapot eltér");
        checked += 1;
    }
    assert_eq!(checked, 4);
}

#[test]
fn python_move_logs_load() {
    let db = fixture_db();
    let id = (1..=10).filter_map(|id| db.get_game_by_id(id).unwrap()).find(|r| r["room_name"] == "Szoba robot").map(|r| r["id"].as_i64().unwrap()).unwrap();
    let stored = db.get_game_moves(id).unwrap();
    assert!(stored.len() >= 5);
    let log: Vec<MoveLog> = stored
        .iter()
        .map(|m| MoveLog::from_stored(m.move_number, &m.player_name, &m.action_type, m.details_json.as_deref(), m.board_snapshot_json.as_deref()))
        .collect();
    assert_eq!(log[0].move_number, 1);
    assert!(log.iter().any(|m| m.action_type == "place" && m.details["tiles"].is_array()));
    let last = log.last().unwrap();
    assert!(!last.board.is_empty || last.action_type != "place");
    // a tárolt pillanatkép ugyanazt adja vissza
    assert_eq!(serde_json::from_str::<Value>(&last.board_snapshot_json()).unwrap().as_array().unwrap().len(), 15);
}

#[test]
fn finished_python_game_has_results_and_stats() {
    let db = fixture_db();
    let id = (1..=10).filter_map(|id| db.get_game_by_id(id).unwrap()).find(|r| r["room_name"] == "Szoba finished").map(|r| r["id"].as_i64().unwrap()).unwrap();
    assert_eq!(db.get_game_by_id(id).unwrap().unwrap()["status"], "finished");
    let results = db.get_game_results(id).unwrap();
    assert_eq!(results.len(), 2);
    let history = db.get_user_game_history(meta()["alice"].as_i64().unwrap(), 10).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0]["opponents"].as_array().unwrap().len(), 1);
}
