//! Admin panel: a szótár — szó-vizsgáló, kizárt szavak (tartós lista és szavazatok), felülbírálat, saját szavak,
//! szótár-építő felügyelet és gyorsítótárak (a Python `test_admin_dictionary.py` megfelelője).
//!
//! A szótár állapota (tartós lista, szavazatok, felülbírálatok, szókincs) a folyamat egészére hatós: a tesztek
//! egymás után futnak (`SERIAL`), és mindegyik a tartós lista ideiglenes másolatán dolgozik.

mod common;
use common::admin::*;
use common::*;
use scrabble::dictionary;
use scrabble::word_review;
use serde_json::{Value, json};
use std::path::PathBuf;

const REASON: &str = "Szótári teszt";

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Egy teszt elszigetelt szótára: a lista ideiglenes másolata; a végén az állapot visszaáll.
struct Isolated {
    _guard: tokio::sync::MutexGuard<'static, ()>,
    path: PathBuf,
    dir: PathBuf,
}

impl Isolated {
    fn text(&self) -> String {
        std::fs::read_to_string(&self.path).unwrap()
    }

    fn words(&self) -> Vec<String> {
        self.text().split_whitespace().map(|s| s.to_string()).collect()
    }
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

async fn isolated() -> Isolated {
    let guard = SERIAL.lock().await;
    dictionary::warm_up();
    let dir = temp_dir("dict");
    let path = dir.join("hu_rejected.txt");
    std::fs::copy(scrabble::config::dict_dir().join("hu_rejected.txt"), &path).unwrap();
    dictionary::set_rejected_path(Some(path.clone()));
    dictionary::reload_rejected(None);
    dictionary::set_admin_lists([], [], []);
    dictionary::reset_runtime_lists();
    scrabble::admin::dict::remember_baseline();
    Isolated { _guard: guard, path, dir }
}

struct Env {
    server: TestServer,
    api: Http,
    _iso: Isolated,
}

async fn env() -> Env {
    let iso = isolated().await;
    let server = TestServer::start().await;
    let api = server.admin_http().await;
    Env { server, api, _iso: iso }
}

/// Egy tartós listán szereplő szó, amelyet a szótár egyébként elfogadna.
fn accepted_but_listed() -> String {
    let checker = dictionary::get_checker().expect("szótár");
    let mut listed: Vec<String> = dictionary::listed_rejected().into_iter().collect();
    listed.sort();
    listed.into_iter().find(|w| checker.check(w)).expect("van ilyen szó")
}

/// Egy biztosan érvényes, szokásos szó (nem a listán).
fn valid_word(index: usize) -> String {
    let candidates: Vec<&str> = ["almás", "körte", "szilva", "barack", "szőlő", "dinnye"].into_iter().filter(|w| dictionary::is_word_valid(&w.to_uppercase())).collect();
    candidates[index].to_string()
}

fn word_url(word: &str) -> String {
    format!("/dictionary/word{}", qs(&[("w", word)]))
}

fn arr(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

fn strings(value: &Value) -> Vec<String> {
    arr(value).iter().map(|v| v.as_str().unwrap_or("").to_string()).collect()
}

// ===================================================================================================
// Szó-vizsgáló
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn a_valid_word() {
    let e = env().await;
    let data = e.api.admin_get(&word_url("almát")).await.json();
    assert_eq!((data["word"].clone(), data["valid"].clone(), data["in_dictionary"].clone()), (json!("ALMÁT"), json!(true), json!(true)));
    assert_eq!((data["tiles"].clone(), data["score"].clone()), (json!(["A", "L", "M", "Á", "T"]), json!(5)));
    assert!(arr(&data["derivations"]).iter().any(|d| d["stem"] == "alma" && d["suffixes"] == json!(["át"])));
    assert_eq!((data["rejected_listed"].clone(), data["rejected_voted"].clone(), data["override"].clone()), (json!(false), json!(false), Value::Null));
    assert_eq!(data["balance"], json!({"good": 0, "bad": 0, "threshold": 1, "diff": 0}));
    assert_eq!(data["suggestions"], json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn digraph_tiles() {
    let e = env().await;
    assert_eq!(e.api.admin_get(&word_url("szék")).await.json()["tiles"], json!(["SZ", "É", "K"]));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_invalid_word_has_suggestions() {
    let e = env().await;
    let data = e.api.admin_get(&word_url("almaa")).await.json();
    assert_eq!((data["valid"].clone(), data["in_dictionary"].clone()), (json!(false), json!(false)));
    assert!(strings(&data["suggestions"]).contains(&"ALMA".to_string()), "{data}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_risky_derivation_needs_the_usage_list() {
    let e = env().await;
    let data = e.api.admin_get(&word_url("tarom")).await.json();
    assert_eq!((data["needs_attestation"].clone(), data["attested"].clone(), data["valid"].clone()), (json!(true), json!(false), json!(false)));
    assert_eq!(data["derivations"][0]["risky"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_listed_rejection_is_explained() {
    let e = env().await;
    let word = accepted_but_listed();
    let data = e.api.admin_get(&word_url(&word)).await.json();
    assert_eq!((data["rejected_listed"].clone(), data["valid"].clone(), data["in_dictionary"].clone()), (json!(true), json!(false), json!(true)));
}

#[tokio::test(flavor = "multi_thread")]
async fn votes_are_listed() {
    let e = env().await;
    let uid = e.server.create_user("a@example.com", "Anna");
    let word = valid_word(0);
    word_review::record_vote(&e.server.app.db, &e.server.app.settings, uid, &word.to_uppercase(), false);
    let data = e.api.admin_get(&word_url(&word)).await.json();
    assert_eq!((data["rejected_voted"].clone(), data["valid"].clone()), (json!(true), json!(false)));
    assert_eq!((data["votes"][0]["display_name"].clone(), data["votes"][0]["valid"].clone()), (json!("Anna"), json!(false)));
    assert_eq!(data["balance"]["diff"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn vocabulary_membership() {
    let e = env().await;
    assert_eq!(e.api.admin_get(&word_url("alma")).await.json()["in_vocabulary"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_input() {
    let e = env().await;
    for word in ["", &"x".repeat(16), "abc123", "qwx"] {
        assert_eq!(e.api.admin_get(&word_url(word)).await.status, 400, "{word:?}");
    }
}

// ===================================================================================================
// A kizárt szavak listája
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn listing_and_search() {
    let e = env().await;
    let data = e.api.admin_get("/dictionary/rejected").await.json();
    assert_eq!(data["counts"]["listed"], dictionary::listed_rejected().len());
    assert!(data["total"].as_i64().unwrap() >= 600);
    let first = data["items"][0]["word"].as_str().unwrap().to_string();
    let prefix: String = first.chars().take(4).collect();
    let found = e.api.admin_get(&format!("/dictionary/rejected{}", qs(&[("q", &prefix), ("source", "listed")]))).await.json();
    assert!(arr(&found["items"]).iter().any(|i| i["word"] == json!(first)));
    assert_eq!(e.api.admin_get("/dictionary/rejected?source=rossz").await.status, 400);
    let page = e.api.admin_get("/dictionary/rejected?limit=5&offset=5").await.json();
    assert_eq!((page["items"].as_array().unwrap().len(), page["offset"].clone()), (5, json!(5)));
}

#[tokio::test(flavor = "multi_thread")]
async fn voted_words_are_listed_with_their_votes() {
    let e = env().await;
    let uid = e.server.create_user("a@example.com", "Anna");
    let word = valid_word(0);
    word_review::record_vote(&e.server.app.db, &e.server.app.settings, uid, &word.to_uppercase(), false);
    let item = e.api.admin_get("/dictionary/rejected?source=voted").await.json()["items"][0].clone();
    assert_eq!((item["word"].clone(), item["voted"].clone(), item["bad"].clone(), item["listed"].clone()), (json!(word.to_uppercase()), json!(true), json!(1), json!(false)));
}

#[tokio::test(flavor = "multi_thread")]
async fn rejected_csv() {
    let e = env().await;
    assert!(e.api.admin_get("/dictionary/rejected?format=csv").await.text().starts_with("word,listed,voted,allowed,good,bad"));
}

// ===================================================================================================
// Felvétel a listára
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_preview_classifies_the_words() {
    let e = env().await;
    let listed = accepted_but_listed();
    let fresh = valid_word(0);
    let text = format!("{fresh}, {listed}\nnemszó almaa # megjegyzés ide\nqqq 123");
    let data = e.api.admin_post("/dictionary/rejected/preview", json!({"words": text})).await.json();
    assert_eq!(data["new"], json!([fresh]));
    assert_eq!(data["already"], json!([listed]));
    let invalid = strings(&data["invalid"]);
    assert!(invalid.contains(&"almaa".to_string()) && invalid.contains(&"nemszó".to_string()) && !invalid.contains(&"megjegyzés".to_string()), "{data}");
    assert!(strings(&data["bad"]).contains(&"qqq".to_string()), "{data}");
}

#[tokio::test(flavor = "multi_thread")]
async fn adding_writes_the_file_atomically_and_applies_immediately() {
    let e = env().await;
    let word = valid_word(0);
    let before = dictionary::rejected_version();
    assert!(dictionary::is_word_valid(&word.to_uppercase()));
    let response = e.api.admin_post("/dictionary/rejected", json!({"words": format!("{word} almaa"), "reason": REASON})).await;
    assert_eq!(response.status, 200, "{}", response.text());
    assert_eq!(response.json()["new"], json!([word]));
    let iso = &e._iso;
    assert!(iso.words().contains(&word));
    assert!(!iso.text().contains("almaa")); // érvénytelen szót nem vesz fel
    assert!(!dictionary::is_word_valid(&word.to_uppercase()));
    assert!(dictionary::rejected_version() > before);
    let temporary: Vec<String> = std::fs::read_dir(&iso.dir).unwrap().filter_map(|f| f.ok()).map(|f| f.file_name().to_string_lossy().to_string()).filter(|n| n.starts_with(".hu_rejected")).collect();
    assert!(temporary.is_empty(), "{temporary:?}");
    let row = audit_of(&e.server, "dict.reject_add")[0]["details"].clone();
    assert_eq!((row["words"].clone(), row["skipped_invalid"].clone(), row["reason"].clone()), (json!([word]), json!(1), json!(REASON)));
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_to_add() {
    let e = env().await;
    assert_eq!(e.api.admin_post("/dictionary/rejected", json!({"words": "almaa", "reason": REASON})).await.status, 409);
    assert_eq!(e.api.admin_post("/dictionary/rejected", json!({"words": accepted_but_listed(), "reason": REASON})).await.status, 409);
    assert_eq!(e.api.admin_post("/dictionary/rejected", json!({"words": valid_word(0)})).await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_audit_write_restores_the_file() {
    let e = env().await;
    let original = e._iso.text();
    let word = valid_word(0);
    e.server.app.db.with(|tx| tx.execute_batch("ALTER TABLE admin_audit RENAME TO admin_audit_elmozgatva")).unwrap();
    let ctx = scrabble::admin::AdminContext { admin_user_id: e.server.user_id(ADMIN_EMAIL), ip: "1.1.1.1".into(), user_agent: "t".into() };
    let result = scrabble::admin::dict::add_rejected(&e.server.app, &ctx, Some(&json!(word)), Some(&json!(REASON)), false);
    assert!(result.is_err());
    assert_eq!(e._iso.text(), original);
    assert!(dictionary::is_word_valid(&word.to_uppercase()));
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_keeps_the_comments() {
    let e = env().await;
    let word = valid_word(0);
    e.api.admin_post("/dictionary/rejected", json!({"words": word, "reason": REASON})).await;
    assert_eq!(e.api.admin_delete("/dictionary/rejected", json!({"words": word, "reason": REASON})).await.status, 200);
    assert!(dictionary::is_word_valid(&word.to_uppercase()));
    let text = e._iso.text();
    assert!(!e._iso.words().contains(&word) && text.contains("# --- admin panel"), "{text}");
    assert_eq!(e.api.admin_delete("/dictionary/rejected", json!({"words": word, "reason": REASON})).await.status, 404);
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_an_original_entry() {
    let e = env().await;
    let word = accepted_but_listed();
    let original = e._iso.text();
    e.api.admin_delete("/dictionary/rejected", json!({"words": word, "reason": REASON})).await;
    assert_eq!(e._iso.text().lines().count(), original.lines().count() - 1);
    assert!(dictionary::is_word_valid(&word.to_uppercase()));
}

#[tokio::test(flavor = "multi_thread")]
async fn download_and_diff() {
    let e = env().await;
    let word = valid_word(0);
    e.api.admin_post("/dictionary/rejected", json!({"words": word, "reason": REASON})).await;
    let text = e.api.admin_get("/dictionary/rejected/download").await.text();
    assert!(text.split_whitespace().any(|w| w == word));
    let diff = e.api.admin_get("/dictionary/rejected/download?diff=1").await.text();
    assert!(diff.contains(&format!("+{word}")) && diff.contains("--- a/dict/hu_rejected.txt"), "{diff}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reverted_change_disappears_from_the_diff() {
    let e = env().await;
    let word = valid_word(0);
    e.api.admin_post("/dictionary/rejected", json!({"words": word, "reason": REASON})).await;
    e.api.admin_delete("/dictionary/rejected", json!({"words": word, "reason": REASON})).await;
    let diff = e.api.admin_get("/dictionary/rejected/download?diff=1").await.text();
    assert!(!diff.contains(&format!("+{word}")), "{diff}");
}

// ===================================================================================================
// Felülbírálatok
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn a_reject_override() {
    let e = env().await;
    let word = valid_word(0);
    let before = dictionary::rejected_version();
    assert_eq!(e.api.admin_post("/dictionary/overrides", json!({"word": word, "verdict": "reject", "reason": REASON})).await.status, 200);
    assert!(!dictionary::is_word_valid(&word.to_uppercase()) && dictionary::rejected_version() > before);
    assert_eq!(e.api.admin_get(&word_url(&word)).await.json()["override"]["verdict"], "reject");
    e.api.admin_delete("/dictionary/overrides", json!({"word": word, "reason": REASON})).await;
    assert!(dictionary::is_word_valid(&word.to_uppercase()));
}

#[tokio::test(flavor = "multi_thread")]
async fn allow_overrides_the_votes_and_the_list() {
    let e = env().await;
    let uid = e.server.create_user("a@example.com", "Anna");
    let voted = valid_word(0);
    let listed = accepted_but_listed();
    let app = &e.server.app;
    word_review::record_vote(&app.db, &app.settings, uid, &voted.to_uppercase(), false);
    assert!(!dictionary::is_word_valid(&voted.to_uppercase()) && !dictionary::is_word_valid(&listed.to_uppercase()));
    for word in [&voted, &listed] {
        assert_eq!(e.api.admin_post("/dictionary/overrides", json!({"word": word, "verdict": "allow", "reason": REASON})).await.status, 200);
        assert!(dictionary::is_word_valid(&word.to_uppercase()), "{word}");
    }
    word_review::refresh(&app.db, &app.settings); // újraindítás után is érvényben marad
    assert!(dictionary::is_word_valid(&voted.to_uppercase()));
    let mut listed_words: Vec<String> = arr(&e.api.admin_get("/dictionary/overrides").await.json()["items"]).iter().map(|o| o["word"].as_str().unwrap().to_string()).collect();
    listed_words.sort();
    let mut expected = vec![voted.to_uppercase(), listed.to_uppercase()];
    expected.sort();
    assert_eq!(listed_words, expected);
}

#[tokio::test(flavor = "multi_thread")]
async fn allow_does_not_make_an_unknown_word_valid() {
    let e = env().await;
    let response = e.api.admin_post("/dictionary/overrides", json!({"word": "almaa", "verdict": "allow", "reason": REASON})).await;
    assert_eq!(response.status, 409);
    assert!(!dictionary::is_word_valid("ALMAA"));
}

#[tokio::test(flavor = "multi_thread")]
async fn override_validation() {
    let e = env().await;
    let word = valid_word(0);
    assert_eq!(e.api.admin_post("/dictionary/overrides", json!({"word": word, "verdict": "talán", "reason": REASON})).await.status, 400);
    assert_eq!(e.api.admin_post("/dictionary/overrides", json!({"word": word, "verdict": "reject"})).await.status, 400);
    assert_eq!(e.api.admin_delete("/dictionary/overrides", json!({"word": word, "reason": REASON})).await.status, 404);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_override_is_logged_with_before_and_after() {
    let e = env().await;
    let word = valid_word(0);
    e.api.admin_post("/dictionary/overrides", json!({"word": word, "verdict": "reject", "reason": REASON})).await;
    e.api.admin_post("/dictionary/overrides", json!({"word": word, "verdict": "allow", "reason": REASON})).await;
    let rows = audit_of(&e.server, "dict.override");
    assert_eq!(rows.iter().map(|r| r["details"]["after"].clone()).collect::<Vec<_>>(), vec![json!({"verdict": "allow"}), json!({"verdict": "reject"})]);
    assert_eq!(rows[0]["details"]["before"], json!({"verdict": "reject"}));
}

// ===================================================================================================
// Saját szavak
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn a_custom_word_is_accepted_but_not_in_the_robot_vocabulary() {
    let e = env().await;
    let word = "zsugorbab";
    assert!(!dictionary::is_word_valid(&word.to_uppercase()));
    assert_eq!(e.api.admin_post("/dictionary/additions", json!({"word": word, "reason": REASON})).await.status, 200);
    assert!(dictionary::is_word_valid(&word.to_uppercase()));
    assert_eq!(dictionary::check_words(&[word.to_uppercase()]), (true, vec![]));
    assert!(!scrabble::ai::get_vocabulary().contains(&word.to_uppercase()));
    assert_eq!(e.api.admin_get("/dictionary/additions").await.json()["items"][0]["word"], json!(word.to_uppercase()));
    assert_eq!(e.api.admin_post("/dictionary/additions", json!({"word": word, "reason": REASON})).await.status, 409);
    assert_eq!(e.api.admin_delete("/dictionary/additions", json!({"word": word, "reason": REASON})).await.status, 200);
    assert!(!dictionary::is_word_valid(&word.to_uppercase()));
}

#[tokio::test(flavor = "multi_thread")]
async fn rejections_still_win_over_a_custom_word() {
    let e = env().await;
    let word = "zsugorbab";
    e.api.admin_post("/dictionary/additions", json!({"word": word, "reason": REASON})).await;
    e.api.admin_post("/dictionary/overrides", json!({"word": word, "verdict": "reject", "reason": REASON})).await;
    assert!(!dictionary::is_word_valid(&word.to_uppercase()));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_custom_word_needs_a_vowel_and_two_tiles() {
    let e = env().await;
    for word in ["bcdfg", "a"] {
        assert_eq!(e.api.admin_post("/dictionary/additions", json!({"word": word, "reason": REASON})).await.status, 400, "{word}");
    }
}

// ===================================================================================================
// Szavazatok
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn deleting_all_votes_of_a_word() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    let b = e.server.create_user("b@example.com", "Béla");
    let word = valid_word(0);
    let app = &e.server.app;
    word_review::record_vote(&app.db, &app.settings, a, &word.to_uppercase(), false);
    app.db.save_word_review(b, &word, false).unwrap(); // a második szavazat már csak az adatbázisból jöhet
    assert_eq!(e.api.admin_delete("/dictionary/votes", json!({"word": word, "reason": REASON})).await.status, 401);
    e.api.sudo().await;
    let data = e.api.admin_delete("/dictionary/votes", json!({"word": word, "reason": REASON})).await.json();
    assert_eq!(data["deleted"], 2);
    assert!(dictionary::is_word_valid(&word.to_uppercase()));
    assert_eq!(e.api.admin_delete("/dictionary/votes", json!({"word": word, "reason": REASON})).await.status, 404);
    assert_eq!(audit_of(&e.server, "dict.votes_delete")[0]["details"]["votes"].as_array().unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn exporting_the_voted_words_to_the_persistent_list() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    let (w1, w2) = (valid_word(0), valid_word(1));
    let app = &e.server.app;
    for word in [&w1, &w2] {
        word_review::record_vote(&app.db, &app.settings, a, &word.to_uppercase(), false);
    }
    e.api.sudo().await;
    let data = e.api.admin_post("/dictionary/rejected/export-votes", json!({"reason": REASON})).await.json();
    let mut new = strings(&data["new"]);
    new.sort();
    let mut expected = vec![w1.clone(), w2.clone()];
    expected.sort();
    assert_eq!(new, expected);
    let words = e._iso.words();
    assert!(words.contains(&w1) && words.contains(&w2));
    assert_eq!(e.api.admin_post("/dictionary/rejected/export-votes", json!({"reason": REASON})).await.status, 409);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_threshold_comes_from_the_settings() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    let word = valid_word(0);
    let app = &e.server.app;
    assert_eq!(e.api.admin_get("/dictionary/threshold").await.json(), json!({"success": true, "value": 1, "default": 1, "overridden": false}));
    let response = e.api.admin_patch("/settings", json!({"changes": {"word_reject_threshold": 2}, "reason": REASON})).await;
    assert_eq!(response.status, 200, "{}", response.text());
    word_review::record_vote(&app.db, &app.settings, a, &word.to_uppercase(), false);
    assert!(dictionary::is_word_valid(&word.to_uppercase())); // egy szavazat már kevés
    let b = e.server.create_user("b@example.com", "Béla");
    app.db.save_word_review(b, &word, false).unwrap();
    word_review::refresh(&app.db, &app.settings);
    assert!(!dictionary::is_word_valid(&word.to_uppercase()));
    e.api.admin_patch("/settings", json!({"changes": {"word_reject_threshold": null}, "reason": REASON})).await;
    assert_eq!(e.api.admin_get("/dictionary/threshold").await.json()["value"], 1);
}

// ===================================================================================================
// Bírálók
// ===================================================================================================

fn votes(e: &Env, user: i64, verdicts: &[(&str, bool)]) {
    for (word, verdict) in verdicts {
        e.server.app.db.save_word_review(user, word, *verdict).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn reviewer_statistics() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    let b = e.server.create_user("b@example.com", "Béla");
    votes(&e, a, &[("w1", false), ("w2", true), ("w3", false)]);
    votes(&e, b, &[("w1", false), ("w2", false), ("w4", true)]);
    let data = e.api.admin_get("/dictionary/reviewers").await.json();
    let item = |name: &str| arr(&data["items"]).into_iter().find(|i| i["display_name"] == name).unwrap();
    let (anna, bela) = (item("Anna"), item("Béla"));
    assert_eq!((anna["total"].clone(), anna["invalid"].clone()), (json!(3), json!(2)));
    assert_eq!(anna["invalid_share"].as_f64(), Some(66.7));
    // közös szavak: w1 (egyezik), w2 (ellentétes) → az egyezés 50%
    assert_eq!(anna["compared"], 2);
    assert_eq!(anna["agreement"].as_f64(), Some(50.0));
    assert_eq!(bela["agreement"].as_f64(), Some(50.0));
    assert_eq!(anna["suspicious"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_suspicious_reviewer_is_highlighted() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    for i in 0..60 {
        e.server.app.db.save_word_review(a, &format!("w{i}"), i % 10 == 0).unwrap();
    }
    let item = e.api.admin_get("/dictionary/reviewers").await.json()["items"][0].clone();
    assert_eq!(item["total"], 60);
    assert!(item["invalid_share"].as_f64().unwrap() > 80.0);
    assert_eq!(item["suspicious"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_summary() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    votes(&e, a, &[("w1", false), ("w2", true)]);
    let data = e.api.admin_get("/dictionary/review-summary").await.json();
    assert_eq!((data["decisions"].clone(), data["words_reviewed"].clone(), data["reviewers"].clone()), (json!(2), json!(2), json!(1)));
    assert_eq!((data["daily"][0]["decisions"].clone(), data["daily"][0]["rejections"].clone()), (json!(2), json!(1)));
    assert_eq!((data["active_reviewers"].clone(), data["threshold"].clone()), (json!(1), json!(1)));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_second_opinion_sample() {
    let e = env().await;
    let data = arr(&e.api.admin_get("/dictionary/second-opinion?n=7").await.json()["items"]);
    assert_eq!(data.len(), 7);
    let shape = regex::Regex::new("^[A-ZÁÉÍÓÖŐÚÜŰ]+$").unwrap();
    assert!(data.iter().all(|i| shape.is_match(i["word"].as_str().unwrap())));
    let listed = dictionary::listed_rejected();
    assert!(data.iter().all(|i| listed.contains(&i["word"].as_str().unwrap().to_lowercase())));
    assert!(data[0].get("tiles").is_some() && data[0].get("dictionary_accepts").is_some());
}

// ===================================================================================================
// Gyorsítótárak
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_valid_cache() {
    let e = env().await;
    dictionary::filter_valid(["ALMA", "KÖRTE"]);
    let data = e.api.admin_post("/dictionary/caches/clear", json!({"which": "valid", "reason": REASON})).await.json();
    assert_eq!(data["which"], "valid");
    assert!(data["detail"].as_i64().unwrap() >= 2 && data["seconds"].as_f64().unwrap() >= 0.0);
    assert_eq!(dictionary::valid_cache_len(), 0);
    assert_eq!(audit_of(&e.server, "dict.cache_clear")[0]["target_id"], "valid");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_short_words_are_recomputed() {
    let e = env().await;
    assert_eq!(e.api.admin_post("/dictionary/caches/clear", json!({"which": "short_words", "reason": REASON})).await.status, 200);
    assert!(!scrabble::practice::short_words(2).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_robot_vocabulary_is_rebuilt() {
    let e = env().await;
    let real = scrabble::ai::get_vocabulary();
    let data = e.api.admin_post("/dictionary/caches/clear", json!({"which": "vocabulary", "reason": REASON})).await.json();
    assert_eq!(data["detail"].as_u64().unwrap() as usize, scrabble::ai::get_vocabulary().len());
    assert_eq!(scrabble::ai::get_vocabulary().len(), real.len());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_analysis_cache() {
    let e = env().await;
    let a = e.server.create_user("a@example.com", "Anna");
    let game_id = finished_game(&e.server, "r1", &[(Some(a), "Anna", 10), (None, "Vendég", 5)], false, &[], 0);
    e.server.app.db.save_game_analysis(game_id, 6, "{}").unwrap();
    let data = e.api.admin_post("/dictionary/caches/clear", json!({"which": "analysis", "reason": REASON})).await.json();
    assert_eq!(data["detail"], 1);
    assert!(e.server.app.db.get_game_analysis(game_id).is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn cache_validation() {
    let e = env().await;
    assert_eq!(e.api.admin_post("/dictionary/caches/clear", json!({"which": "minden", "reason": REASON})).await.status, 400);
    assert_eq!(e.api.admin_post("/dictionary/caches/clear", json!({"which": "valid"})).await.status, 400);
}
