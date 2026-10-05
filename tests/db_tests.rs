//! Az adatbázis-réteg tesztjei (a Python `test_auth.py`, `test_friends.py`, `test_elo.py` és a ranglista
//! tesztek megfelelői).

use scrabble::db::{Db, GamePlayerData, StoredMove};
use scrabble::util;
use rusqlite::params;
use std::collections::HashSet;

fn db() -> Db {
    Db::open_temp().unwrap()
}

fn player(name: &str, user_id: Option<i64>, score: i64, winner: bool) -> GamePlayerData {
    GamePlayerData { player_name: name.into(), user_id, score, is_winner: winner, resigned: false }
}

fn user(db: &Db, email: &str, name: &str) -> i64 {
    db.create_user(email, name, "pass123").unwrap()
}

// --- séma ---

#[test]
fn init_creates_tables_and_is_idempotent() {
    let db = db();
    db.init().unwrap();
    db.init().unwrap();
    let tables: Vec<String> = db
        .with(|tx| {
            let mut stmt = tx.prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
            let names = stmt.query_map([], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?;
            Ok(names)
        })
        .unwrap();
    for table in ["users", "sessions", "verification_codes", "saved_games", "game_players", "game_moves", "friendships", "admin_audit", "app_settings"] {
        assert!(tables.contains(&table.to_string()), "hiányzik: {table}");
    }
}

#[test]
fn admin_audit_is_append_only() {
    let db = db();
    db.with(|tx| tx.execute("INSERT INTO admin_audit (admin_user_id, action) VALUES (1, 'x')", []).map(|_| ())).unwrap();
    assert!(db.with(|tx| tx.execute("UPDATE admin_audit SET action = 'y'", []).map(|_| ())).is_err());
    assert!(db.with(|tx| tx.execute("DELETE FROM admin_audit", []).map(|_| ())).is_err());
}

// --- felhasználók ---

#[test]
fn create_and_find_users() {
    let db = db();
    let id = user(&db, "Test@Example.com", "Teszt");
    assert!(id > 0);
    assert!(db.create_user("test@example.com", "Más", "x").is_err());
    let by_mail = db.get_user_by_email("  TEST@example.com ").unwrap().unwrap();
    assert_eq!(by_mail.id, id);
    assert_eq!(by_mail.display_name, "Teszt");
    assert_eq!(by_mail.email, "Test@Example.com");
    assert_eq!(by_mail.games_played, 0);
    assert_eq!(by_mail.rating, 1200);
    assert!(db.get_user_by_id(id).unwrap().is_some());
    assert!(db.get_user_by_id(999).unwrap().is_none());
    assert!(db.get_user_by_email("nobody@example.com").unwrap().is_none());
}

#[test]
fn password_verification() {
    let db = db();
    user(&db, "a@example.com", "A");
    assert!(db.verify_password("a@example.com", "pass123").is_ok());
    assert!(db.verify_password("A@Example.com", "pass123").is_ok());
    assert!(db.verify_password("a@example.com", "rossz").is_err());
    assert!(db.verify_password("nincs@example.com", "pass123").is_err());
    let stored = db.get_user_by_email("a@example.com").unwrap().unwrap().password_hash;
    assert!(!stored.contains("pass123"));
    assert!(stored.starts_with("pbkdf2:sha256:260000$"));
}

#[test]
fn deleted_users_cannot_log_in() {
    let db = db();
    let id = user(&db, "a@example.com", "A");
    db.with(|tx| tx.execute("UPDATE users SET deleted_at = datetime('now') WHERE id = ?", [id]).map(|_| ())).unwrap();
    assert!(db.verify_password("a@example.com", "pass123").is_err());
}

// --- verifikációs kódok ---

#[test]
fn verification_codes() {
    let db = db();
    let code = db.create_verification_code("test@example.com").unwrap();
    assert_eq!(code.len(), 6);
    assert!(code.chars().all(|c| c.is_ascii_digit()));
    let (ok, _) = db.verify_code("test@example.com", &code);
    assert!(ok);
    assert!(db.is_email_verified("test@example.com"));
    // a kód csak egyszer használható
    assert!(!db.verify_code("test@example.com", &code).0);
    db.clear_email_verification("test@example.com");
    assert!(!db.is_email_verified("test@example.com"));
}

#[test]
fn wrong_code_and_attempt_limit() {
    let db = db();
    let code = db.create_verification_code("t@example.com").unwrap();
    let (ok, message) = db.verify_code("t@example.com", "000000");
    assert!(!ok);
    assert!(message.contains("Hibás") || message.contains("próbálkozás"), "{message}");
    for _ in 0..5 {
        db.verify_code("t@example.com", "000000");
    }
    assert!(!db.verify_code("t@example.com", &code).0, "a próbálkozások után a helyes kód sem jó");
    assert!(!db.verify_code("nobody@example.com", "123456").0);
}

#[test]
fn new_code_invalidates_old_and_email_is_case_insensitive() {
    let db = db();
    let code1 = db.create_verification_code("t@example.com").unwrap();
    let code2 = db.create_verification_code("T@Example.com").unwrap();
    if code1 != code2 {
        assert!(!db.verify_code("t@example.com", &code1).0);
    }
    let code3 = db.create_verification_code("t@example.com").unwrap();
    assert!(db.verify_code("T@EXAMPLE.com", &code3).0);
}

#[test]
fn expired_verification_code_is_rejected() {
    let db = db();
    let code = db.create_verification_code("t@example.com").unwrap();
    db.with(|tx| tx.execute("UPDATE verification_codes SET expires_at = '2000-01-01 00:00:00'", []).map(|_| ())).unwrap();
    let (ok, message) = db.verify_code("t@example.com", &code);
    assert!(!ok);
    assert!(message.contains("lejárt"));
}

// --- munkamenetek ---

#[test]
fn sessions_lifecycle() {
    let db = db();
    let id = user(&db, "a@example.com", "A");
    let token = db.create_session(id, Some("1.2.3.4"), Some("UA")).unwrap();
    assert!(token.len() >= 60);
    let session = db.validate_session(Some(&token)).unwrap();
    assert_eq!(session.id, id);
    assert_eq!(session.display_name, "A");
    assert_eq!(session.rating, 1200);
    assert!(db.validate_session(Some("rossz")).is_none());
    assert!(db.validate_session(None).is_none());
    assert!(db.validate_session(Some("")).is_none());
    let second = db.create_session(id, None, None).unwrap();
    assert!(db.validate_session(Some(&second)).is_some());
    db.delete_session(Some(&token));
    assert!(db.validate_session(Some(&token)).is_none());
    assert!(db.validate_session(Some(&second)).is_some());
    db.delete_session(None);
}

#[test]
fn expired_sessions_are_removed() {
    let db = db();
    let id = user(&db, "a@example.com", "A");
    let token = db.create_session(id, None, None).unwrap();
    db.with(|tx| tx.execute("UPDATE sessions SET expires_at = '2000-01-01 00:00:00'", []).map(|_| ())).unwrap();
    assert!(db.validate_session(Some(&token)).is_none());
    let token2 = db.create_session(id, None, None).unwrap();
    db.with(|tx| tx.execute("UPDATE sessions SET expires_at = '2000-01-01 00:00:00'", []).map(|_| ())).unwrap();
    db.cleanup_expired();
    assert!(db.validate_session(Some(&token2)).is_none());
    let count: i64 = db.with(|tx| tx.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn banned_and_deleted_users_lose_their_session() {
    let db = db();
    let id = user(&db, "a@example.com", "A");
    let token = db.create_session(id, None, None).unwrap();
    db.with(|tx| tx.execute("UPDATE users SET banned_until = '9999-12-31 23:59:59', ban_reason = 'spam' WHERE id = ?", [id]).map(|_| ())).unwrap();
    assert!(db.validate_session(Some(&token)).is_none());
    let ban = db.get_ban(id).unwrap();
    assert!(ban.permanent && ban.until.is_none());
    assert_eq!(ban.reason, "spam");
    // lejárt tiltás nem aktív
    db.with(|tx| tx.execute("UPDATE users SET banned_until = '2000-01-01 00:00:00' WHERE id = ?", [id]).map(|_| ())).unwrap();
    assert!(db.get_ban(id).is_none());
    assert!(db.validate_session(Some(&token)).is_some());
    db.with(|tx| tx.execute("UPDATE users SET deleted_at = datetime('now') WHERE id = ?", [id]).map(|_| ())).unwrap();
    assert!(db.validate_session(Some(&token)).is_none());
}

#[test]
fn timed_ban_reports_its_expiry() {
    let db = db();
    let id = user(&db, "a@example.com", "A");
    let until = util::format_ts(util::utcnow() + chrono::Duration::hours(2));
    db.with(|tx| tx.execute("UPDATE users SET banned_until = ?, ban_reason = 'x' WHERE id = ?", params![until, id]).map(|_| ())).unwrap();
    let ban = db.get_ban(id).unwrap();
    assert!(!ban.permanent);
    assert_eq!(ban.until.as_deref(), Some(until.as_str()));
}

#[test]
fn chat_mute_and_review_block() {
    let db = db();
    let id = user(&db, "a@example.com", "A");
    assert!(!db.is_chat_muted(id));
    let until = util::format_ts(util::utcnow() + chrono::Duration::hours(1));
    db.with(|tx| tx.execute("UPDATE users SET chat_muted_until = ?, review_blocked = 1 WHERE id = ?", params![until, id]).map(|_| ())).unwrap();
    assert!(db.is_chat_muted(id));
    assert!(db.is_review_blocked(id));
    assert!(!db.is_chat_muted(0));
}

#[test]
fn login_events_and_last_login() {
    let db = db();
    let id = user(&db, "a@example.com", "A");
    db.record_login_event("a@example.com", false, Some("1.1.1.1"), Some("UA"), None, "bad_credentials");
    db.record_login_event("a@example.com", true, Some("2.2.2.2"), Some("UA"), Some(id), "");
    let (count, ip): (i64, Option<String>) = db
        .with(|tx| {
            let count = tx.query_row("SELECT COUNT(*) FROM login_events", [], |r| r.get(0))?;
            let ip = tx.query_row("SELECT last_login_ip FROM users WHERE id = ?", [id], |r| r.get(0))?;
            Ok((count, ip))
        })
        .unwrap();
    assert_eq!(count, 2);
    assert_eq!(ip.as_deref(), Some("2.2.2.2"));
}

#[test]
fn reconnect_token_is_stable() {
    let db = db();
    let id = user(&db, "a@example.com", "A");
    let t1 = db.get_or_create_user_reconnect_token(id).unwrap();
    let t2 = db.get_or_create_user_reconnect_token(id).unwrap();
    assert_eq!(t1, t2);
    assert!(t1.len() >= 16);
}

#[test]
fn admin_session_state() {
    let db = db();
    let id = user(&db, "a@example.com", "A");
    let token = db.create_session(id, None, None).unwrap();
    let (seen, sudo) = db.get_admin_session(Some(&token)).unwrap();
    assert!(seen.is_none() && sudo.is_none());
    db.touch_admin_session(&token);
    let (seen, _) = db.get_admin_session(Some(&token)).unwrap();
    assert!(seen.is_some());
    db.set_admin_sudo(&token, Some(util::utcnow() + chrono::Duration::minutes(5)));
    assert!(db.get_admin_session(Some(&token)).unwrap().1.is_some());
    db.set_admin_sudo(&token, None);
    assert!(db.get_admin_session(Some(&token)).unwrap().1.is_none());
    assert!(db.get_admin_session(Some("nincs")).is_none());
}

// --- mentett játékok ---

#[test]
fn save_game_new_and_upsert() {
    let db = db();
    let id1 = db.save_game("room-1", "Szoba", "{\"state\": 1}", false, &[], "Alice", None, false, false).unwrap();
    let row = db.get_game_by_id(id1).unwrap().unwrap();
    assert_eq!(row["status"], "active");
    assert_eq!(row["owner_name"], "Alice");
    assert_eq!(row["challenge_mode"], 0);
    let id2 = db.save_game("room-1", "Szoba", "{\"state\": 2}", true, &[], "Alice", Some("tok"), true, false).unwrap();
    assert_eq!(id1, id2);
    let row = db.get_game_by_id(id1).unwrap().unwrap();
    assert_eq!(row["state_json"], "{\"state\": 2}");
    assert_eq!(row["has_bots"], 1);
    assert_eq!(row["owner_token"], "tok");
    assert!(db.get_game_by_id(999).unwrap().is_none());
}

#[test]
fn save_game_upserts_players() {
    let db = db();
    let id = db.save_game("room-1", "Szoba", "{}", false, &[player("Alice", None, 50, false)], "", None, false, false).unwrap();
    db.save_game("room-1", "Szoba", "{}", false, &[player("Alice", None, 80, false), player("Bob", None, 30, false)], "", None, false, false).unwrap();
    let mut players = db.get_game_players(id).unwrap();
    players.sort();
    assert_eq!(players.len(), 2);
    assert_eq!(players[0], ("Alice".to_string(), None, 80));
}

#[test]
fn finish_game_updates_stats_and_players() {
    let db = db();
    let alice = user(&db, "alice@example.com", "Alice");
    let bob = user(&db, "bob@example.com", "Bob");
    let id = db.save_game("room-1", "Szoba", "{}", false, &[player("Alice", Some(alice), 50, false)], "Alice", None, false, false).unwrap();
    let finished = db
        .finish_game("room-1", "{\"f\":1}", &[player("Alice", Some(alice), 150, true), player("Bob", Some(bob), 80, false)], "Szoba", false)
        .unwrap();
    assert_eq!(finished, id);
    assert_eq!(db.get_game_by_id(id).unwrap().unwrap()["status"], "finished");
    let a = db.get_user_by_id(alice).unwrap().unwrap();
    assert_eq!((a.games_played, a.games_won, a.total_score), (1, 1, 150));
    let b = db.get_user_by_id(bob).unwrap().unwrap();
    assert_eq!((b.games_played, b.games_won, b.total_score), (1, 0, 80));
    let players = db.get_game_players(id).unwrap();
    assert_eq!(players.len(), 2, "nincs duplikált játékossor");
    // értékszám: két regisztrált, robot nélkül
    assert!(a.rating > 1200 && b.rating < 1200);
    assert_eq!(a.rating + b.rating, 2400);
    let changes = db.get_game_rating_changes(id).unwrap();
    assert_eq!(changes[&alice], (1200, a.rating));
}

#[test]
fn finish_game_without_active_row_creates_one() {
    let db = db();
    let id = db.finish_game("room-x", "{}", &[player("Ghost", None, 10, true)], "Szoba", false).unwrap();
    assert_eq!(db.get_game_by_id(id).unwrap().unwrap()["status"], "finished");
}

#[test]
fn bot_games_and_guests_do_not_change_ratings() {
    let db = db();
    let alice = user(&db, "alice@example.com", "Alice");
    let bob = user(&db, "bob@example.com", "Bob");
    db.finish_game("r1", "{}", &[player("Alice", Some(alice), 150, true), player("Bob", Some(bob), 80, false)], "x", true).unwrap();
    assert_eq!(db.get_user_by_id(alice).unwrap().unwrap().rating, 1200);
    db.finish_game("r2", "{}", &[player("Alice", Some(alice), 150, true), player("Vendég", None, 80, false)], "x", false).unwrap();
    assert_eq!(db.get_user_by_id(alice).unwrap().unwrap().rating, 1200);
    assert_eq!(db.get_user_by_id(alice).unwrap().unwrap().rated_games, 0);
}

#[test]
fn resigned_player_ranks_last_regardless_of_score() {
    let db = db();
    let alice = user(&db, "alice@example.com", "Alice");
    let bob = user(&db, "bob@example.com", "Bob");
    let mut quitter = player("Alice", Some(alice), 500, false);
    quitter.resigned = true;
    db.finish_game("r1", "{}", &[quitter, player("Bob", Some(bob), 10, true)], "x", false).unwrap();
    assert!(db.get_user_by_id(alice).unwrap().unwrap().rating < 1200);
    assert!(db.get_user_by_id(bob).unwrap().unwrap().rating > 1200);
}

#[test]
fn game_moves_roundtrip_in_order() {
    let db = db();
    let id = db.save_game("room-1", "Szoba", "{}", false, &[], "", None, false, false).unwrap();
    assert!(db.get_game_moves(id).unwrap().is_empty());
    for n in [3, 1, 2] {
        db.add_game_move(
            id,
            &StoredMove { move_number: n, player_name: "A".into(), action_type: "place".into(), details_json: Some("{}".into()), board_snapshot_json: Some("[]".into()) },
        )
        .unwrap();
    }
    let moves = db.get_game_moves(id).unwrap();
    assert_eq!(moves.iter().map(|m| m.move_number).collect::<Vec<_>>(), vec![1, 2, 3]);
}

#[test]
fn active_games_and_abandon() {
    let db = db();
    let a = db.save_game("r1", "A", "{}", false, &[], "", None, false, false).unwrap();
    db.save_game("r2", "B", "{}", false, &[], "", None, false, false).unwrap();
    db.finish_game("r3", "{}", &[], "C", false).unwrap();
    assert_eq!(db.load_active_games().unwrap().len(), 2);
    db.abandon_game("r1");
    assert_eq!(db.get_game_by_id(a).unwrap().unwrap()["status"], "abandoned");
    assert_eq!(db.load_active_games().unwrap().len(), 1);
    let b = db.load_active_games().unwrap()[0]["id"].as_i64().unwrap();
    db.abandon_game_by_id(b);
    assert!(db.load_active_games().unwrap().is_empty());
    // csak az aktív játék hagyható el
    db.abandon_game_by_id(a);
    db.abandon_game("nincs");
}

#[test]
fn user_history_lists_opponents_and_respects_limit() {
    let db = db();
    let alice = user(&db, "alice@example.com", "Alice");
    for i in 0..3 {
        db.finish_game(
            &format!("room-{i}"),
            "{}",
            &[player("Alice", Some(alice), 100 + i, true), player("Bob", None, 50, false)],
            &format!("Szoba {i}"),
            false,
        )
        .unwrap();
    }
    let history = db.get_user_game_history(alice, 20).unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!(history[0]["opponents"][0]["player_name"], "Bob");
    assert_eq!(db.get_user_game_history(alice, 2).unwrap().len(), 2);
    assert!(db.get_user_game_history(999, 20).unwrap().is_empty());
}

#[test]
fn user_active_games_only_for_owner() {
    let db = db();
    let alice = user(&db, "alice@example.com", "Alice");
    let bob = user(&db, "bob@example.com", "Bob");
    let players = [player("Alice", Some(alice), 10, false), player("Bob", Some(bob), 5, false)];
    db.save_game("r1", "Szoba", "{}", true, &players, "Alice", Some("tokA"), false, false).unwrap();
    let mine = db.get_user_active_games(alice, Some("tokA")).unwrap();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0]["opponents"][0]["name"], "Bob");
    assert!(db.get_user_active_games(bob, Some("tokB")).unwrap().is_empty());
    // token nélkül a tulajdonos neve + fiókja alapján
    assert_eq!(db.get_user_active_games(alice, None).unwrap().len(), 1);
    assert!(db.get_user_active_games(bob, None).unwrap().is_empty());
    db.finish_game("r1", "{}", &players, "Szoba", false).unwrap();
    assert!(db.get_user_active_games(alice, Some("tokA")).unwrap().is_empty());
}

#[test]
fn async_games_are_listed_separately() {
    let db = db();
    let alice = user(&db, "alice@example.com", "Alice");
    db.save_game("r1", "Levelezős", "{}", false, &[player("Alice", Some(alice), 0, false)], "", None, false, true).unwrap();
    assert_eq!(db.get_user_async_games(alice).unwrap().len(), 1);
    assert_eq!(db.get_active_async_games().unwrap().len(), 1);
    assert!(db.get_user_active_games(alice, None).unwrap().is_empty());
}

#[test]
fn share_tokens_only_for_finished_games() {
    let db = db();
    let id = db.save_game("r1", "Szoba", "{}", false, &[], "", None, false, false).unwrap();
    assert!(db.get_or_create_share_token(id).unwrap().is_none());
    db.finish_game("r1", "{}", &[player("A", None, 1, true)], "Szoba", false).unwrap();
    let token = db.get_or_create_share_token(id).unwrap().unwrap();
    assert_eq!(db.get_or_create_share_token(id).unwrap().unwrap(), token);
    assert_eq!(db.get_game_by_share_token(&token).unwrap().unwrap()["id"], id);
    assert!(db.get_game_by_share_token("nincs").unwrap().is_none());
    assert!(db.get_game_by_share_token("").unwrap().is_none());
    assert!(db.get_or_create_share_token(999).unwrap().is_none());
    let results = db.get_game_results(id).unwrap();
    assert_eq!(results[0]["player_name"], "A");
    assert_eq!(results[0]["is_winner"], true);
}

#[test]
fn user_membership_check() {
    let db = db();
    let alice = user(&db, "alice@example.com", "Alice");
    let id = db.save_game("r1", "Szoba", "{}", false, &[player("Alice", Some(alice), 0, false)], "", None, false, false).unwrap();
    assert!(db.is_user_in_game(id, alice));
    assert!(!db.is_user_in_game(id, 999));
}

// --- ranglista ---

fn play_game(db: &Db, room: &str, results: &[(&str, i64, i64, bool)], bots: bool) {
    let players: Vec<GamePlayerData> = results.iter().map(|(n, uid, score, w)| player(n, Some(*uid), *score, *w)).collect();
    db.finish_game(room, "{}", &players, room, bots).unwrap();
}

#[test]
fn leaderboard_metrics_and_minimums() {
    let db = db();
    let a = user(&db, "a@example.com", "Anna");
    let b = user(&db, "b@example.com", "Béla");
    for i in 0..3 {
        play_game(&db, &format!("g{i}"), &[("Anna", a, 200, true), ("Béla", b, 100, false)], false);
    }
    let (entries, me) = db.get_leaderboard("wins", 50, Some(b), false).unwrap();
    assert_eq!(entries[0]["display_name"], "Anna");
    assert_eq!(entries[0]["games_won"], 3);
    assert_eq!(entries[0]["rank"], 1);
    assert_eq!(me.unwrap()["rank"], 2);
    let (avg, _) = db.get_leaderboard("avg_score", 50, None, false).unwrap();
    assert_eq!(avg[0]["avg_score"], 200.0);
    let (rating, _) = db.get_leaderboard("rating", 50, None, false).unwrap();
    assert_eq!(rating.len(), 2, "3 értékelt játék után felkerülnek");
    assert!(rating[0]["rating"].as_i64().unwrap() > rating[1]["rating"].as_i64().unwrap());
    let (best, _) = db.get_leaderboard("best_game", 1, None, false).unwrap();
    assert_eq!(best.len(), 1);
    assert_eq!(best[0]["best_score"], 200);
    // ismeretlen metrika: wins
    let (fallback, _) = db.get_leaderboard("zzz", 50, None, false).unwrap();
    assert_eq!(fallback.len(), 2);
}

#[test]
fn leaderboard_excludes_bot_games_and_new_players() {
    let db = db();
    let a = user(&db, "a@example.com", "Anna");
    let b = user(&db, "b@example.com", "Béla");
    play_game(&db, "g1", &[("Anna", a, 200, true), ("Béla", b, 100, false)], true);
    assert!(db.get_leaderboard("wins", 50, None, false).unwrap().0.is_empty());
    play_game(&db, "g2", &[("Anna", a, 200, true), ("Béla", b, 100, false)], false);
    assert_eq!(db.get_leaderboard("wins", 50, None, false).unwrap().0.len(), 2);
    // a nyerési arányhoz legalább 3 játék kell
    assert!(db.get_leaderboard("win_rate", 50, None, false).unwrap().0.is_empty());
    assert_eq!(db.get_leaderboard("win_rate", 50, None, true).unwrap().0.len(), 2);
}

// --- barátok ---

#[test]
fn friend_request_flow() {
    let db = db();
    let a = user(&db, "a@example.com", "Anna");
    let b = user(&db, "b@example.com", "Béla");
    assert!(!db.send_friend_request(a, a).0);
    assert!(!db.send_friend_request(a, 999).0);
    assert!(db.send_friend_request(a, b).0);
    assert!(db.send_friend_request(a, b).1.contains("Már küldtél"));
    assert!(db.send_friend_request(b, a).1.contains("már küldött neked"));
    assert_eq!(db.get_pending_requests(b).len(), 1);
    assert_eq!(db.get_sent_requests(a).len(), 1);
    assert!(db.get_friends(a).is_empty());
    assert!(!db.accept_friend_request(a, b).0, "a küldő nem fogadhatja el");
    assert!(db.accept_friend_request(b, a).0);
    assert_eq!(db.get_friends(a)[0]["display_name"], "Béla");
    assert_eq!(db.get_friends(b)[0]["display_name"], "Anna");
    assert!(db.send_friend_request(a, b).1.contains("Már barátok"));
    assert!(db.remove_friend(b, a).0);
    assert!(!db.remove_friend(b, a).0);
    assert!(db.get_friends(a).is_empty());
}

#[test]
fn friend_request_decline() {
    let db = db();
    let a = user(&db, "a@example.com", "Anna");
    let b = user(&db, "b@example.com", "Béla");
    db.send_friend_request(a, b);
    assert!(db.decline_friend_request(b, a).0);
    assert!(!db.decline_friend_request(b, a).0);
    assert!(db.get_pending_requests(b).is_empty());
}

#[test]
fn user_search_is_accent_and_case_insensitive_and_hides_emails() {
    let db = db();
    let me = user(&db, "me@example.com", "Saját");
    user(&db, "a@example.com", "Árpád");
    user(&db, "b@example.com", "Őzike");
    user(&db, "c@example.com", "100%_user");
    assert_eq!(db.search_users("árp", me, 10).len(), 1);
    assert_eq!(db.search_users("ÁRP", me, 10).len(), 1);
    assert_eq!(db.search_users("őz", me, 10).len(), 1);
    assert!(db.search_users("a", me, 10).is_empty(), "két karakternél rövidebb");
    assert_eq!(db.search_users("a@example.com", me, 10).len(), 1, "pontos e-mail egyezés");
    assert!(db.search_users("example", me, 10).is_empty(), "az e-mail nem részletre illeszkedik");
    assert_eq!(db.search_users("0%_", me, 10).len(), 1, "a LIKE joker karakterei menekítve");
    assert!(db.search_users("saj", me, 10).is_empty(), "a saját fiók kimarad");
    db.with(|tx| tx.execute("UPDATE users SET deleted_at = datetime('now') WHERE display_name = 'Árpád'", []).map(|_| ())).unwrap();
    assert!(db.search_users("árp", me, 10).is_empty(), "a törölt fiók kimarad");
}

// --- napi feladvány, szavazatok, egyebek ---

#[test]
fn daily_scores_and_ranking() {
    let db = db();
    let a = user(&db, "a@example.com", "Anna");
    let b = user(&db, "b@example.com", "Béla");
    db.save_daily_puzzle("2026-10-05", &serde_json::json!([]), &["A".to_string()], 40, &serde_json::json!({"tiles": [], "words": ["X"], "score": 40})).unwrap();
    assert_eq!(db.get_daily_puzzle("2026-10-05").unwrap().best_score, 40);
    // a második mentés nem írja felül
    db.save_daily_puzzle("2026-10-05", &serde_json::json!([]), &[], 99, &serde_json::json!({})).unwrap();
    assert_eq!(db.get_daily_puzzle("2026-10-05").unwrap().best_score, 40);
    let r1 = db.record_daily_score("2026-10-05", a, 30).unwrap();
    assert!(r1.recorded && r1.improved && r1.attempts == 1);
    let r2 = db.record_daily_score("2026-10-05", a, 20).unwrap();
    assert!(!r2.improved && r2.best == 30 && r2.attempts == 2);
    let r3 = db.record_daily_score("2026-10-05", a, 45).unwrap();
    assert!(r3.improved && r3.best == 45);
    db.record_daily_score("2026-10-05", b, 45).unwrap();
    let (entries, me) = db.get_daily_leaderboard("2026-10-05", 10, Some(b));
    assert_eq!(entries[0]["display_name"], "Béla", "kevesebb próbálkozás előz");
    assert_eq!(me.unwrap()["rank"], 1);
    db.mark_daily_revealed("2026-10-05", a);
    assert!(!db.record_daily_score("2026-10-05", a, 100).unwrap().recorded);
    assert!(db.get_daily_entry("2026-10-05", a).unwrap().revealed);
    db.raise_daily_best("2026-10-05", 60, &serde_json::json!({"score": 60}));
    assert_eq!(db.get_daily_puzzle("2026-10-05").unwrap().best_score, 60);
    db.raise_daily_best("2026-10-05", 10, &serde_json::json!({"score": 10}));
    assert_eq!(db.get_daily_puzzle("2026-10-05").unwrap().best_score, 60);
}

#[test]
fn word_review_votes() {
    let db = db();
    let a = user(&db, "a@example.com", "Anna");
    let b = user(&db, "b@example.com", "Béla");
    db.save_word_review(a, "gyűlöletország", false).unwrap();
    db.save_word_review(b, "gyűlöletország", true).unwrap();
    assert_eq!(db.get_word_review_votes("gyűlöletország"), (1, 1));
    assert!(db.get_voted_rejected_words(1).is_empty());
    db.save_word_review(b, "gyűlöletország", false).unwrap();
    assert_eq!(db.get_voted_rejected_words(1), HashSet::from(["gyűlöletország".to_string()]));
    assert!(db.get_voted_rejected_words(3).is_empty());
    assert_eq!(db.get_reviewed_words().len(), 1);
    let stats = db.get_word_review_stats(a);
    assert_eq!(stats["total"], 1);
    assert_eq!(stats["invalid"], 1);
    assert_eq!(stats["today"], 1);
    assert_eq!(db.delete_word_review(a, "gyűlöletország"), 1);
    assert_eq!(db.delete_word_review(a, "gyűlöletország"), 0);
    // a letiltott felhasználó szavazata nem számít
    db.with(|tx| tx.execute("UPDATE users SET review_blocked = 1 WHERE id = ?", [b]).map(|_| ())).unwrap();
    assert!(db.get_voted_rejected_words(1).is_empty());
}

#[test]
fn achievements_are_granted_once() {
    let db = db();
    let a = user(&db, "a@example.com", "Anna");
    let badges: HashSet<String> = ["bingo".to_string(), "first_game".to_string()].into();
    let new = db.grant_achievements(a, &badges, Some(1));
    assert_eq!(new, vec!["bingo".to_string(), "first_game".to_string()]);
    assert!(db.grant_achievements(a, &badges, Some(2)).is_empty());
    assert_eq!(db.get_user_achievements(a).len(), 2);
}

#[test]
fn settings_push_and_analysis_cache() {
    let db = db();
    assert_eq!(db.get_setting("x"), None);
    db.set_setting("x", "1").unwrap();
    db.set_setting("x", "2").unwrap();
    assert_eq!(db.get_setting("x").as_deref(), Some("2"));

    let a = user(&db, "a@example.com", "Anna");
    let b = user(&db, "b@example.com", "Béla");
    db.save_push_subscription(a, "https://push.example/1", "key", "auth", "hu").unwrap();
    db.save_push_subscription(b, "https://push.example/1", "key2", "auth2", "en").unwrap();
    assert_eq!(db.count_push_subscriptions(a), 0, "a végpont átkerült");
    assert_eq!(db.get_push_subscriptions(b)[0]["lang"], "en");
    assert_eq!(db.delete_push_subscription("https://push.example/1", Some(a)), 0);
    assert_eq!(db.delete_push_subscription("https://push.example/1", Some(b)), 1);

    let id = db.save_game("r", "x", "{}", false, &[], "", None, false, false).unwrap();
    assert!(db.get_game_analysis(id).is_none());
    db.save_game_analysis(id, 6, "{\"a\":1}").unwrap();
    db.save_game_analysis(id, 7, "{\"a\":2}").unwrap();
    assert_eq!(db.get_game_analysis(id), Some((7, "{\"a\":2}".to_string())));
}

#[test]
fn counters_accumulate() {
    let db = db();
    db.bump_counter("quiz", 1);
    db.bump_counter("quiz", 2);
    let n: i64 = db.with(|tx| tx.query_row("SELECT count FROM usage_counters WHERE key = 'quiz'", [], |r| r.get(0))).unwrap();
    assert_eq!(n, 3);
}
