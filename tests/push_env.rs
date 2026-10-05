//! A VAPID kulcs és tárgy környezeti változói. Külön fájlban (külön folyamat), mert a környezeti változók az egész
//! folyamatra hatnak: a tesztek egymás után futnak.

mod common;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::temp_dir;
use parking_lot::Mutex;
use scrabble::db::Db;
use scrabble::push::{Push, parse_private_key};
use std::sync::Arc;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn set_env(name: &str, value: Option<&str>) {
    // SAFETY: a teszteket az `ENV_LOCK` sorba állítja, más szál ebben a folyamatban nem olvassa ezeket
    unsafe {
        match value {
            Some(v) => std::env::set_var(name, v),
            None => std::env::remove_var(name),
        }
    }
}

fn clear_env() {
    for var in ["VAPID_PRIVATE_KEY", "VAPID_PUBLIC_KEY", "VAPID_SUBJECT"] {
        set_env(var, None);
    }
}

fn fresh_db() -> (Arc<Db>, std::path::PathBuf) {
    let dir = temp_dir("pushenv");
    let db = Arc::new(Db::open(dir.join("p.db").to_str().unwrap()).unwrap());
    db.init().unwrap();
    (db, dir)
}

fn new_push(db: &Arc<Db>) -> Push {
    Push::new(db.clone(), Box::new(|| "scrabble@example.com".to_string()))
}

#[test]
fn environment_key_wins_over_a_generated_one() {
    let _guard = ENV_LOCK.lock();
    clear_env();
    let (db1, dir1) = fresh_db();
    let expected = new_push(&db1).public_key();
    let pem = db1.get_setting("vapid_private_pem").unwrap();
    // másik adatbázis: a környezeti kulcs (a sortörések \n-nel) adja ugyanazt a nyilvános kulcsot
    let (db2, dir2) = fresh_db();
    set_env("VAPID_PRIVATE_KEY", Some(&pem.replace('\n', "\\n")));
    assert_eq!(new_push(&db2).public_key(), expected);
    assert!(db2.get_setting("vapid_private_pem").is_none(), "nem generált újat");
    clear_env();
    let _ = (std::fs::remove_dir_all(dir1), std::fs::remove_dir_all(dir2));
}

#[test]
fn environment_key_in_base64url_form() {
    // a `web-push generate-vapid-keys` formája: a nyers 32 bájtos kulcs base64url-ben
    let _guard = ENV_LOCK.lock();
    clear_env();
    let (db1, dir1) = fresh_db();
    let expected = new_push(&db1).public_key();
    let secret = parse_private_key(&db1.get_setting("vapid_private_pem").unwrap()).unwrap();
    let raw = URL_SAFE_NO_PAD.encode(secret.to_bytes());
    let (db2, dir2) = fresh_db();
    set_env("VAPID_PRIVATE_KEY", Some(&raw));
    assert_eq!(new_push(&db2).public_key(), expected);
    assert!(db2.get_setting("vapid_private_pem").is_none(), "nem cserélte le egy generáltra");
    clear_env();
    let _ = (std::fs::remove_dir_all(dir1), std::fs::remove_dir_all(dir2));
}

#[test]
fn an_unreadable_environment_key_falls_back_to_the_stored_one() {
    let _guard = ENV_LOCK.lock();
    clear_env();
    let (db, dir) = fresh_db();
    let stored = new_push(&db).public_key();
    set_env("VAPID_PRIVATE_KEY", Some("nem-kulcs"));
    assert_eq!(new_push(&db).public_key(), stored);
    clear_env();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_subject_can_be_configured() {
    let _guard = ENV_LOCK.lock();
    clear_env();
    let (db, dir) = fresh_db();
    assert!(new_push(&db).subject().starts_with("mailto:"));
    set_env("VAPID_SUBJECT", Some("https://scrabble.example.com"));
    assert_eq!(new_push(&db).subject(), "https://scrabble.example.com");
    clear_env();
    let _ = std::fs::remove_dir_all(dir);
}
