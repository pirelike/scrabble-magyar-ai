//! Szótár-építő: az elutasított szavak a szótárban, a szavazás és a küszöb, a mintavétel és az API (a Python
//! `test_word_review.py` megfelelője).
//!
//! A szótár állapota (tartós lista, szavazatok, szókincs) a folyamat egészére hatós: a tesztek egymás után futnak
//! (`SERIAL`), és mindegyik a tartós lista ideiglenes másolatán dolgozik.

mod common;
use common::admin::*;
use common::*;
use rand::SeedableRng;
use rand::rngs::StdRng;
use scrabble::dictionary;
use scrabble::tiles::tokenize_word;
use scrabble::word_review;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::PathBuf;

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Isolated {
    _guard: tokio::sync::MutexGuard<'static, ()>,
    dir: PathBuf,
}

impl Drop for Isolated {
    fn drop(&mut self) {
        dictionary::set_rejected_path(None);
        dictionary::reload_rejected(None);
        dictionary::set_admin_lists([], [], []);
        dictionary::reset_runtime_lists();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A tartós lista ideiglenes másolatával dolgozó, tiszta szótár-állapot.
async fn isolated() -> Isolated {
    let guard = SERIAL.lock().await;
    dictionary::warm_up();
    scrabble::ai::get_vocabulary();
    let dir = temp_dir("review");
    let path = dir.join("hu_rejected.txt");
    std::fs::copy(scrabble::config::dict_dir().join("hu_rejected.txt"), &path).unwrap();
    dictionary::set_rejected_path(Some(path));
    dictionary::reload_rejected(None);
    dictionary::set_admin_lists([], [], []);
    dictionary::reset_runtime_lists();
    Isolated { _guard: guard, dir }
}

struct Env {
    server: TestServer,
    _iso: Isolated,
}

async fn env() -> Env {
    let iso = isolated().await;
    let server = TestServer::start().await;
    Env { server, _iso: iso }
}

impl Env {
    fn user(&self, name: &str) -> i64 {
        self.server.create_user(&format!("{}@example.com", name.to_lowercase()), name)
    }

    fn vote(&self, user: i64, word: &str, valid: bool) -> Option<bool> {
        word_review::record_vote(&self.server.app.db, &self.server.app.settings, user, word, valid)
    }

    fn undo(&self, user: i64, word: &str) -> (bool, bool) {
        word_review::undo_vote(&self.server.app.db, &self.server.app.settings, user, word)
    }

    fn stats(&self, user: i64) -> Value {
        word_review::stats(&self.server.app.db, user)
    }
}

fn valid(word: &str) -> bool {
    dictionary::filter_valid([word]).contains(word)
}

fn set(words: &[&str]) -> HashSet<String> {
    words.iter().map(|w| w.to_string()).collect()
}

// ===================================================================================================
// A tartós lista
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_file_is_parsed() {
    let iso = isolated().await;
    let path = iso.dir.join("rejected.txt");
    std::fs::write(&path, "# megjegyzés\n\nAlma  # inline\nKÖRTE\n  barack \n").unwrap();
    assert_eq!(dictionary::load_rejected(&path), set(&["alma", "körte", "barack"]));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_file_is_empty() {
    let iso = isolated().await;
    assert!(dictionary::load_rejected(&iso.dir.join("nincs.txt")).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn listed_words_are_not_valid() {
    let iso = isolated().await;
    let path = iso.dir.join("only-alma.txt");
    std::fs::write(&path, "alma\n").unwrap();
    dictionary::set_rejected_path(Some(path));
    dictionary::reload_rejected(None);
    assert!(!valid("ALMA"));
    assert_eq!(dictionary::check_words(&["ALMA"]), (false, vec!["ALMA".to_string()]));
    assert!(dictionary::is_rejected("Alma"));
    assert!(valid("ALMÁT")); // a ragozott alak nem esik ki
    assert!(!dictionary::suggest_words("ALMAA", 20).contains(&"ALMA".to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_shipped_list_is_clean() {
    let _iso = isolated().await;
    let listed = dictionary::listed_rejected();
    assert!(listed.len() >= 100, "{}", listed.len()); // az AI első átnézésének ítéletei
    let checker = dictionary::get_checker().unwrap();
    for word in &listed {
        assert!(*word == word.to_lowercase() && tokenize_word(&word.to_uppercase()).is_some(), "{word}");
        // csak olyan szó kerülhet a listára, amelyet a szótár egyébként elfogadna (nem elírás)
        assert!(checker.check(word), "{word}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_shipped_list_takes_effect() {
    let _iso = isolated().await;
    let mut listed: Vec<String> = dictionary::listed_rejected().into_iter().collect();
    listed.sort();
    for word in listed.iter().take(25) {
        assert!(dictionary::is_rejected(word) && !valid(&word.to_uppercase()), "{word}");
    }
}

#[test]
fn the_shipped_list_is_sorted_and_unique() {
    let text = std::fs::read_to_string(scrabble::config::dict_dir().join("hu_rejected.txt")).unwrap();
    let words: Vec<&str> = text.lines().map(|l| l.trim()).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    let mut sorted = words.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(words, sorted);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_robot_vocabulary_leaves_them_out() {
    let _iso = isolated().await;
    let vocabulary = scrabble::ai::get_vocabulary();
    assert!(dictionary::listed_rejected().iter().all(|w| !vocabulary.contains(&w.to_uppercase())));
    assert!(vocabulary.contains("ALMA"));
}

// ===================================================================================================
// A szavazatok szerinti kizárás a szótárban
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn mark_and_unmark() {
    let _iso = isolated().await;
    assert!(valid("ALMA"));
    let version = dictionary::rejected_version();
    dictionary::mark_voted_rejected("alma", true);
    assert!(!valid("ALMA") && dictionary::is_rejected("ALMA"));
    assert_eq!(dictionary::rejected_version(), version + 1);
    dictionary::mark_voted_rejected("ALMA", false);
    assert!(valid("ALMA") && !dictionary::is_rejected("ALMA"));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unchanged_state_keeps_the_version() {
    let _iso = isolated().await;
    let version = dictionary::rejected_version();
    dictionary::mark_voted_rejected("alma", false);
    assert_eq!(dictionary::rejected_version(), version);
}

#[tokio::test(flavor = "multi_thread")]
async fn set_replaces_everything() {
    let _iso = isolated().await;
    dictionary::mark_voted_rejected("alma", true);
    dictionary::set_voted_rejected(["körte".to_string(), "BARACK".to_string()]);
    assert!(valid("ALMA") && !valid("KÖRTE") && !valid("BARACK"));
}

#[tokio::test(flavor = "multi_thread")]
async fn short_word_lists_follow_the_rejections() {
    let _iso = isolated().await;
    let has_az = || scrabble::practice::short_words(2).iter().any(|e| e["word"] == "AZ");
    assert!(has_az());
    dictionary::mark_voted_rejected("az", true);
    assert!(!has_az());
    dictionary::mark_voted_rejected("az", false);
    assert!(has_az());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_quiz_answer_treats_rejected_words_as_invalid() {
    let _iso = isolated().await;
    assert_eq!(scrabble::practice::check_answer("ALMA", true).unwrap()["valid"], true);
    dictionary::mark_voted_rejected("alma", true);
    assert_eq!(scrabble::practice::check_answer("ALMA", true).unwrap()["valid"], false);
}

// ===================================================================================================
// Szavazás
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn a_no_vote_rejects_the_word() {
    let e = env().await;
    let uid = e.user("Anna");
    assert_eq!(e.vote(uid, "ALMA", false), Some(true));
    assert!(!valid("ALMA"));
    assert_eq!(e.server.app.db.get_voted_rejected_words(1), set(&["alma"]));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_yes_vote_keeps_the_word() {
    let e = env().await;
    let uid = e.user("Anna");
    assert_eq!(e.vote(uid, "alma", true), Some(false));
    assert!(valid("ALMA"));
}

#[tokio::test(flavor = "multi_thread")]
async fn changing_the_vote() {
    let e = env().await;
    let uid = e.user("Anna");
    e.vote(uid, "ALMA", false);
    assert!(!valid("ALMA"));
    // a kizárt szót már nem lehet újra megszavazni, de a döntés visszavonható
    assert_eq!(e.vote(uid, "ALMA", true), None);
    assert_eq!(e.undo(uid, "ALMA"), (true, false));
    assert!(valid("ALMA"));
    assert_eq!(e.vote(uid, "ALMA", true), Some(false));
}

#[tokio::test(flavor = "multi_thread")]
async fn words_the_dictionary_does_not_accept_cannot_be_voted() {
    let e = env().await;
    let uid = e.user("Anna");
    assert_eq!(e.vote(uid, "ALMTÁ", true), None);
    assert_eq!(e.vote(uid, "QWX", false), None);
    assert_eq!(e.vote(uid, "A", false), None);
    assert!(e.server.app.db.get_reviewed_words().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn undo_without_a_vote() {
    let e = env().await;
    let uid = e.user("Anna");
    assert_eq!(e.undo(uid, "ALMA"), (false, false));
    assert_eq!(e.undo(uid, "nem szó!"), (false, false));
}

#[tokio::test(flavor = "multi_thread")]
async fn undo_only_touches_your_own_vote() {
    let e = env().await;
    let (a, b) = (e.user("Anna"), e.user("Bela"));
    e.vote(a, "ALMA", false);
    assert_eq!(e.undo(b, "ALMA"), (false, false));
    assert!(!valid("ALMA"));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_threshold_needs_more_than_one_reviewer() {
    let e = env().await;
    set_setting(&e.server, "word_reject_threshold", json!(2));
    let (a, b) = (e.user("Anna"), e.user("Bela"));
    assert_eq!(e.vote(a, "ALMA", false), Some(false));
    assert!(valid("ALMA"));
    assert_eq!(e.vote(b, "ALMA", false), Some(true));
    assert!(!valid("ALMA"));
}

#[tokio::test(flavor = "multi_thread")]
async fn accepting_votes_offset_rejections() {
    let e = env().await;
    set_setting(&e.server, "word_reject_threshold", json!(2));
    let (a, b, c) = (e.user("Anna"), e.user("Bela"), e.user("Csaba"));
    e.vote(a, "ALMA", false);
    e.vote(b, "ALMA", false);
    assert!(!valid("ALMA"));
    // a harmadik szerint rendes szó: a különbség 1 < 2, a szó visszakerül
    e.server.app.db.save_word_review(c, "alma", true).unwrap();
    word_review::refresh(&e.server.app.db, &e.server.app.settings);
    assert!(valid("ALMA"));
}

#[tokio::test(flavor = "multi_thread")]
async fn refresh_restores_the_rejections_after_a_restart() {
    let e = env().await;
    let uid = e.user("Anna");
    e.vote(uid, "ALMA", false);
    e.vote(uid, "KÖRTE", true);
    dictionary::set_voted_rejected([]); // újraindulás: a memória üres
    assert!(valid("ALMA"));
    word_review::refresh(&e.server.app.db, &e.server.app.settings);
    assert!(!valid("ALMA") && valid("KÖRTE"));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_stats() {
    let e = env().await;
    let uid = e.user("Anna");
    assert_eq!(e.stats(uid), json!({"total": 0, "valid": 0, "invalid": 0, "today": 0}));
    e.vote(uid, "ALMA", true);
    e.vote(uid, "KÖRTE", true);
    e.vote(uid, "BARACK", false);
    assert_eq!(e.stats(uid), json!({"total": 3, "valid": 2, "invalid": 1, "today": 3}));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deleted_user_takes_the_votes_along() {
    let e = env().await;
    let uid = e.user("Anna");
    e.vote(uid, "ALMA", false);
    e.server.app.db.with(|tx| tx.execute("DELETE FROM users WHERE id = ?", [uid]).map(|_| ())).unwrap();
    assert!(e.server.app.db.get_reviewed_words().is_empty());
}

// ===================================================================================================
// Mintavétel
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn sampled_words_are_valid_distinct_and_playable() {
    let _iso = isolated().await;
    let words = word_review::sample_words(30, &mut StdRng::seed_from_u64(3), &HashSet::new());
    assert_eq!(words.len(), 30);
    assert_eq!(words.iter().collect::<HashSet<_>>().len(), 30);
    assert!(words.iter().all(|w| valid(w) && tokenize_word(w).is_some()));
    assert!(words.iter().all(|w| (2..=15).contains(&w.chars().count())));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_sample_mixes_stems_and_inflected_forms() {
    let _iso = isolated().await;
    let stems: HashSet<&String> = scrabble::practice::stem_words().iter().collect();
    let words = word_review::sample_words(50, &mut StdRng::seed_from_u64(5), &HashSet::new());
    let inflected = words.iter().filter(|w| !stems.contains(w)).count();
    assert!((5..=25).contains(&inflected), "{inflected}");
}

#[tokio::test(flavor = "multi_thread")]
async fn excluded_words_never_appear() {
    let _iso = isolated().await;
    let first = word_review::sample_words(50, &mut StdRng::seed_from_u64(7), &HashSet::new());
    let exclude: HashSet<String> = first.iter().map(|w| w.to_lowercase()).collect();
    let again = word_review::sample_words(50, &mut StdRng::seed_from_u64(7), &exclude);
    assert!(again.iter().all(|w| !first.contains(w)));
}

#[tokio::test(flavor = "multi_thread")]
async fn reviewed_words_are_skipped() {
    let e = env().await;
    let uid = e.user("Anna");
    let shown = word_review::next_words(&e.server.app.db, 20, &mut StdRng::seed_from_u64(11));
    for word in &shown {
        e.vote(uid, word, true);
    }
    let next = word_review::next_words(&e.server.app.db, 50, &mut StdRng::seed_from_u64(11));
    assert!(next.iter().all(|w| !shown.contains(w)));
}

#[tokio::test(flavor = "multi_thread")]
async fn rejected_words_are_not_offered() {
    let _iso = isolated().await;
    dictionary::set_voted_rejected(["alma".to_string()]);
    for seed in 0..5 {
        assert!(!word_review::sample_words(50, &mut StdRng::seed_from_u64(seed), &HashSet::new()).contains(&"ALMA".to_string()));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_count_is_bounded() {
    let e = env().await;
    let mut rng = StdRng::seed_from_u64(1);
    assert_eq!(word_review::next_words(&e.server.app.db, 1000, &mut rng).len(), word_review::MAX_BATCH);
    assert_eq!(word_review::next_words(&e.server.app.db, 0, &mut rng).len(), 1);
    assert_eq!(word_review::sample_words(120, &mut StdRng::seed_from_u64(2), &HashSet::new()).len(), 120); // az eszköz nagyobb mintát is kér
}

#[tokio::test(flavor = "multi_thread")]
async fn the_sample_is_random_each_time() {
    let _iso = isolated().await;
    let (a, b) = (word_review::sample_words(20, &mut rand::rng(), &HashSet::new()), word_review::sample_words(20, &mut rand::rng(), &HashSet::new()));
    assert_ne!(a, b);
}

#[test]
fn normalize() {
    assert_eq!(word_review::normalize_str(" alma ").as_deref(), Some("ALMA"));
    assert_eq!(word_review::normalize_str("szék").as_deref(), Some("SZÉK"));
    assert_eq!(word_review::normalize_str("a"), None);
    assert_eq!(word_review::normalize_str("qwx"), None);
    assert_eq!(word_review::normalize_str(&"a".repeat(16)), None);
    assert_eq!(word_review::normalize(&Value::Null), None);
}

// ===================================================================================================
// API
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn login_is_required() {
    let e = env().await;
    let http = e.server.http();
    assert_eq!(http.get("/api/practice/word-review").await.status, 401);
    assert_eq!(http.post("/api/practice/word-review", json!({"word": "ALMA", "valid": false})).await.status, 401);
    assert_eq!(http.post("/api/practice/word-review/undo", json!({"word": "ALMA"})).await.status, 401);
    assert_eq!(http.get("/api/practice/word-review/stats").await.status, 401);
    assert!(valid("ALMA"));
}

#[tokio::test(flavor = "multi_thread")]
async fn next_words_over_http() {
    let e = env().await;
    e.user("Anna");
    let http = e.server.http_as("anna@example.com").await;
    let data = http.get("/api/practice/word-review?n=12").await.json();
    assert_eq!(data["success"], true);
    let words: Vec<String> = data["words"].as_array().unwrap().iter().map(|w| w.as_str().unwrap().to_string()).collect();
    assert_eq!(words.len(), 12);
    let tiles: Vec<Value> = words.iter().map(|w| json!(tokenize_word(w).unwrap().iter().map(|t| t.as_str()).collect::<Vec<_>>())).collect();
    assert_eq!(data["tiles"], json!(tiles));
    assert_eq!(data["stats"], json!({"total": 0, "valid": 0, "invalid": 0, "today": 0}));
    assert_eq!(http.get("/api/practice/word-review").await.json()["words"].as_array().unwrap().len(), word_review::BATCH_SIZE);
    assert_eq!(http.get("/api/practice/word-review?n=abc").await.json()["words"].as_array().unwrap().len(), word_review::BATCH_SIZE);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_no_vote_removes_the_word_from_the_game() {
    let e = env().await;
    e.user("Anna");
    let http = e.server.http_as("anna@example.com").await;
    let data = http.post("/api/practice/word-review", json!({"word": "alma", "valid": false})).await.json();
    assert_eq!((data["success"].clone(), data["rejected"].clone(), data["word"].clone()), (json!(true), json!(true), json!("ALMA")));
    assert_eq!(data["stats"]["invalid"], 1);
    assert_eq!(http.post("/api/practice/answer", json!({"word": "ALMA", "answer": true})).await.json()["valid"], false);
    assert_eq!(http.get("/api/dictionary/check?q=alma").await.json()["results"][0]["valid"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_yes_vote_over_http() {
    let e = env().await;
    e.user("Anna");
    let http = e.server.http_as("anna@example.com").await;
    let data = http.post("/api/practice/word-review", json!({"word": "ALMA", "valid": true})).await.json();
    assert_eq!((data["success"].clone(), data["rejected"].clone(), data["stats"]["valid"].clone()), (json!(true), json!(false), json!(1)));
    assert!(valid("ALMA"));
}

#[tokio::test(flavor = "multi_thread")]
async fn undo_brings_the_word_back() {
    let e = env().await;
    e.user("Anna");
    let http = e.server.http_as("anna@example.com").await;
    http.post("/api/practice/word-review", json!({"word": "ALMA", "valid": false})).await;
    let data = http.post("/api/practice/word-review/undo", json!({"word": "ALMA"})).await.json();
    assert_eq!((data["success"].clone(), data["rejected"].clone(), data["stats"]["total"].clone()), (json!(true), json!(false), json!(0)));
    assert!(valid("ALMA"));
    let again = http.post("/api/practice/word-review/undo", json!({"word": "ALMA"})).await;
    assert_eq!(again.status, 404);
    assert_eq!(again.json()["success"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn vote_validation() {
    let e = env().await;
    e.user("Anna");
    let http = e.server.http_as("anna@example.com").await;
    let post = |body: Value| {
        let http = http.clone();
        async move { http.post("/api/practice/word-review", body).await.status }
    };
    assert_eq!(post(json!({"word": "ALMA"})).await, 400); // nincs döntés
    assert_eq!(post(json!({"word": "ALMA", "valid": "nem"})).await, 400);
    assert_eq!(post(json!({"valid": true})).await, 400);
    assert_eq!(post(json!({"word": 5, "valid": true})).await, 400);
    assert_eq!(post(json!({"word": "QWX", "valid": false})).await, 400);
    assert_eq!(post(json!({"word": "ALMTÁ", "valid": false})).await, 409); // a szótár már nem fogadja el
    let bad = http.request("POST", "/api/practice/word-review", None, &[("Content-Type", "application/json")]).await;
    assert_eq!(bad.status, 400);
    assert_eq!(http.post("/api/practice/word-review/undo", json!({"word": 5})).await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_stats_endpoint_is_per_user() {
    let e = env().await;
    e.user("Anna");
    e.user("Bela");
    let anna = e.server.http_as("anna@example.com").await;
    anna.post("/api/practice/word-review", json!({"word": "ALMA", "valid": true})).await;
    assert_eq!(anna.get("/api/practice/word-review/stats").await.json()["stats"]["total"], 1);
    let bela = e.server.http_as("bela@example.com").await;
    assert_eq!(bela.get("/api/practice/word-review/stats").await.json()["stats"]["total"], 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_endpoints_are_rate_limited() {
    let e = env().await;
    e.user("Anna");
    let http = e.server.http_as("anna@example.com").await;
    restore_default_limits(&e.server.app);
    let mut codes = Vec::new();
    for _ in 0..241 {
        codes.push(http.get("/api/practice/word-review/stats").await.status);
    }
    assert_eq!((codes[0], codes[240]), (200, 429));
}
