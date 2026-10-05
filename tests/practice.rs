//! Gyakorló módok: szókvíz (csapdákkal), betűvadászat / bingó-edző és a rövid szavak (a Python `test_practice.py`
//! megfelelője). A belső segédfüggvények tesztjei a `src/practice.rs` egységtesztjei között vannak.

mod common;
use common::*;
use rand::SeedableRng;
use rand::rngs::StdRng;
use scrabble::dictionary;
use scrabble::practice::{self, BINGO_BONUS, MAX_QUESTIONS, RACK_SIZE};
use scrabble::tiles::{self, TILE_DISTRIBUTION, Tile, tokenize_word};
use serde_json::{Value, json};
use std::collections::HashSet;

fn rng(seed: u64) -> StdRng {
    StdRng::seed_from_u64(seed)
}

fn ready() {
    dictionary::warm_up();
    scrabble::ai::get_vocabulary();
}

fn tile_count(word: &str) -> usize {
    tokenize_word(word).unwrap().len()
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| s.to_string()).collect()
}

fn rack_of(letters: &[&str]) -> Vec<Tile> {
    letters.iter().map(|l| Tile::from_str(l).unwrap()).collect()
}

fn words_of(list: &[Value]) -> HashSet<String> {
    list.iter().map(|e| e["word"].as_str().unwrap().to_string()).collect()
}

fn is_vowel_word(word: &str) -> bool {
    word.chars().any(|c| tiles::VOWELS.contains(c))
}

// ===================================================================================================
// Rövid szavak
// ===================================================================================================

#[test]
fn two_tile_words() {
    ready();
    let words = practice::short_words(2);
    let names = words_of(&words);
    assert!(words.len() > 100 && words.len() < 400, "{}", words.len());
    for expected in ["AZ", "EZ", "BE", "EL", "AGY", "CSŐ"] {
        assert!(names.contains(expected), "{expected}"); // AGY = A+GY, CSŐ = CS+Ő: két zseton
    }
    assert!(names.iter().all(|w| tile_count(w) == 2));
    assert!(!names.contains("XY") && !names.contains("BK"));
}

#[test]
fn every_listed_word_is_valid_and_scored() {
    ready();
    let words = practice::short_words(2);
    for entry in words.iter().take(60) {
        let word = entry["word"].as_str().unwrap();
        assert!(dictionary::filter_valid([word]).contains(word), "{word}");
        assert!(entry["score"].as_u64().unwrap() >= 2, "{word}");
    }
    assert_eq!(words.iter().find(|e| e["word"] == "AZ").unwrap()["score"], 5);
}

#[test]
fn three_tile_words_are_a_superset_in_size() {
    ready();
    let three = practice::short_words(3);
    assert!(three.len() > practice::short_words(2).len());
    assert!(three.iter().all(|e| tile_count(e["word"].as_str().unwrap()) == 3));
}

#[test]
fn short_words_are_sorted_and_cached() {
    ready();
    let words = practice::short_words(2);
    let names: Vec<&str> = words.iter().map(|e| e["word"].as_str().unwrap()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
    assert_eq!(practice::short_words(2), words);
}

#[test]
fn other_lengths_are_refused() {
    ready();
    assert!(std::panic::catch_unwind(|| practice::short_words(4)).is_err());
}

// ===================================================================================================
// Szókvíz
// ===================================================================================================

fn classify(questions: &[String]) -> Vec<bool> {
    questions.iter().map(|w| practice::check_answer(w, true).unwrap()["valid"] == true).collect()
}

fn count(valid: &[bool], wanted: bool) -> usize {
    valid.iter().filter(|v| **v == wanted).count()
}

#[test]
fn a_mixed_quiz_is_half_valid_half_invalid() {
    ready();
    for seed in 0..5 {
        let questions = practice::make_quiz(10, "mixed", &mut rng(seed)).unwrap();
        assert_eq!(questions.len(), 10);
        assert_eq!(questions.iter().collect::<HashSet<_>>().len(), 10);
        let valid = classify(&questions);
        assert_eq!((count(&valid, true), count(&valid, false)), (5, 5), "seed {seed}");
    }
}

#[test]
fn odd_counts_split_unevenly_but_cover_both() {
    ready();
    let questions = practice::make_quiz(7, "mixed", &mut rng(1)).unwrap();
    let valid = classify(&questions);
    assert!(questions.len() == 7 && (3..=4).contains(&count(&valid, true)));
}

#[test]
fn questions_are_words_the_game_can_place() {
    ready();
    for word in practice::make_quiz(20, "mixed", &mut rng(2)).unwrap() {
        let tokens = tokenize_word(&word).expect(&word);
        assert!((2..=9).contains(&tokens.len()), "{word}");
        assert!(tokens.iter().any(|t| tiles::VOWELS.contains(t.as_str().chars().next().unwrap())), "{word}");
    }
}

#[test]
fn a_short_quiz_uses_only_that_length() {
    ready();
    for length in [2usize, 3] {
        let questions = practice::make_quiz(10, &length.to_string(), &mut rng(4)).unwrap();
        assert!(questions.iter().all(|w| tile_count(w) == length));
        let valid = classify(&questions);
        assert_eq!((count(&valid, true), count(&valid, false)), (5, 5), "{length}");
    }
}

#[test]
fn an_unknown_mode_is_refused() {
    ready();
    assert!(practice::make_quiz(10, "nehéz", &mut rng(0)).is_err());
}

/// A magánhangzók hosszú / rövid párja (a csapda-mód átírásai).
fn length_pair(c: char) -> Option<char> {
    Some(match c {
        'a' => 'á',
        'á' => 'a',
        'e' => 'é',
        'é' => 'e',
        'i' => 'í',
        'í' => 'i',
        'o' => 'ó',
        'ó' => 'o',
        'ö' => 'ő',
        'ő' => 'ö',
        'u' => 'ú',
        'ú' => 'u',
        'ü' => 'ű',
        'ű' => 'ü',
        _ => return None,
    })
}

#[test]
fn tricky_quiz_fakes_are_vowel_length_swaps_of_valid_words() {
    ready();
    for seed in 0..4 {
        let questions = practice::make_quiz(10, "tricky", &mut rng(seed)).unwrap();
        let valid = classify(&questions);
        assert!((4..=6).contains(&count(&valid, true)) && questions.len() >= 8, "seed {seed}");
        for (word, ok) in questions.iter().zip(&valid) {
            assert!(is_vowel_word(word), "{word}");
            if !ok {
                // a hamis szó egyetlen magánhangzó hosszúságának cseréjével lesz érvényes
                let chars: Vec<char> = word.to_lowercase().chars().collect();
                let twins: Vec<String> = (0..chars.len())
                    .filter_map(|i| {
                        length_pair(chars[i]).map(|p| chars.iter().enumerate().map(|(j, c)| if j == i { p } else { *c }).collect::<String>().to_uppercase())
                    })
                    .collect();
                let valid_twins = dictionary::filter_valid(twins.iter().map(|s| s.as_str()));
                assert!(twins.iter().any(|t| valid_twins.contains(t)), "{word}");
            }
        }
    }
}

#[test]
fn a_quiz_is_random() {
    ready();
    assert_ne!(practice::make_quiz(10, "mixed", &mut rng(5)).unwrap(), practice::make_quiz(10, "mixed", &mut rng(6)).unwrap());
}

#[test]
fn the_count_is_clamped() {
    ready();
    assert_eq!(practice::make_quiz(1, "mixed", &mut rng(1)).unwrap().len(), 2);
    assert!(practice::make_quiz(500, "mixed", &mut rng(1)).unwrap().len() <= MAX_QUESTIONS);
}

// ===================================================================================================
// Betűvadász: kirakható szavak
// ===================================================================================================

const RACK: [&str; 7] = ["K", "Ú", "P", "H", "O", "Z", "N"];

#[test]
fn finds_words_that_fit_the_rack() {
    ready();
    let words = words_of(&practice::rack_words(&rack_of(&["A", "L", "M", "Á", "T", "K", "E"])));
    for expected in ["ALMÁT", "ALMÁK", "MÁK", "TÁL", "KELT"] {
        assert!(words.contains(expected), "{expected}");
    }
    assert!(!words.contains("ALMÁKAT")); // nincs elég zseton
}

#[test]
fn every_rack_word_is_valid_and_buildable() {
    ready();
    for entry in practice::rack_words(&rack_of(&RACK)) {
        let word = entry["word"].as_str().unwrap();
        assert!(!dictionary::filter_valid([word]).is_empty(), "{word}");
        assert_eq!(practice::check_rack_word(&strings(&RACK), word)["ok"], true, "{word}");
        assert!((2..=RACK_SIZE as u64).contains(&entry["tiles"].as_u64().unwrap()), "{word}");
    }
}

#[test]
fn rack_words_are_sorted_by_score_and_scored_by_tile_values() {
    ready();
    let words = practice::rack_words(&rack_of(&RACK));
    let scores: Vec<u64> = words.iter().map(|e| e["score"].as_u64().unwrap()).collect();
    let mut sorted = scores.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(scores, sorted);
    assert_eq!(words.iter().find(|e| e["word"] == "HÚZ").unwrap()["score"], 3 + 7 + 4);
}

#[test]
fn digraph_tiles_count_as_one_tile() {
    ready();
    let words = practice::rack_words(&rack_of(&["SZ", "Ó", "T", "A", "L", "M", "K"]));
    let szo = words.iter().find(|e| e["word"] == "SZÓ").unwrap();
    assert_eq!((szo["tiles"].clone(), szo["score"].clone()), (json!(2), json!(3 + 2)));
}

#[test]
fn separate_single_tiles_do_not_form_a_digraph() {
    ready();
    // S + Z külön zsetonként nem adja az SZ betűt: SZÓ csak az SZ zsetonnal rakható ki
    let split = words_of(&practice::rack_words(&rack_of(&["S", "Z", "Ó", "T", "A", "L", "M"])));
    assert!(!split.contains("SZÓ") && !split.contains("SZAL"));
    assert!(words_of(&practice::rack_words(&rack_of(&["SZ", "Ó", "T", "A", "L", "M", "K"]))).contains("SZÓ"));
    // a sorrend nem számít: Z után S sem alkothat ZS-t
    assert!(!words_of(&practice::rack_words(&rack_of(&["Z", "S", "Á", "K", "A", "T", "L"]))).contains("ZSÁK"));
    assert!(words_of(&practice::rack_words(&rack_of(&["ZS", "Á", "K", "A", "T", "L", "M"]))).contains("ZSÁK"));
}

#[test]
fn the_bingo_bonus_when_all_seven_tiles_are_used() {
    ready();
    let words = practice::rack_words(&rack_of(&["K", "Ó", "B", "O", "R", "R", "A"]));
    let word = words.iter().find(|e| e["word"] == "KÓBORRA").unwrap();
    assert_eq!(word["tiles"], 7);
    assert_eq!(word["score"], 1 + 2 + 2 + 1 + 1 + 1 + 1 + BINGO_BONUS);
}

// ===================================================================================================
// Betűvadász / bingó-edző: a kéz előállítása
// ===================================================================================================

fn rack_strings(data: &Value) -> Vec<String> {
    data["rack"].as_array().unwrap().iter().map(|t| t.as_str().unwrap().to_string()).collect()
}

#[test]
fn a_hunt_rack_is_playable() {
    ready();
    for seed in 0..6 {
        let data = practice::make_rack("hunt", &mut rng(seed)).unwrap();
        let rack = rack_strings(&data);
        assert_eq!((data["kind"].clone(), rack.len()), (json!("hunt"), RACK_SIZE));
        assert!(rack.iter().all(|t| !t.is_empty() && Tile::from_str(t).is_some()));
        // a zsák darabszámait nem lépi túl
        for letter in &rack {
            let available = TILE_DISTRIBUTION.iter().find(|(l, _, _)| l == letter).unwrap().2 as usize;
            assert!(rack.iter().filter(|t| *t == letter).count() <= available, "{rack:?}");
        }
        let vowels = rack.iter().filter(|t| tiles::VOWELS.contains(t.chars().next().unwrap())).count();
        assert!((2..=4).contains(&vowels), "{rack:?}");
        let words = data["words"].as_array().unwrap();
        assert!(!words.is_empty() && words.iter().map(|w| w["tiles"].as_u64().unwrap()).max().unwrap() >= 2);
        assert_eq!(data["total_score"].as_u64().unwrap(), words.iter().map(|w| w["score"].as_u64().unwrap()).sum::<u64>());
    }
}

#[test]
fn a_hunt_rack_has_enough_words() {
    ready();
    let counts: Vec<usize> = (0..8).map(|s| practice::make_rack("hunt", &mut rng(s)).unwrap()["words"].as_array().unwrap().len()).collect();
    assert!(counts.iter().filter(|n| (12..=150).contains(*n)).count() >= 6, "{counts:?}");
}

#[test]
fn a_bingo_rack_always_has_a_seven_tile_word() {
    ready();
    for seed in 0..8 {
        let data = practice::make_rack("bingo", &mut rng(seed)).unwrap();
        let rack = rack_strings(&data);
        assert_eq!((data["kind"].clone(), rack.len()), (json!("bingo"), RACK_SIZE));
        let words = data["words"].as_array().unwrap();
        assert!(!words.is_empty() && words.iter().all(|w| w["tiles"] == RACK_SIZE));
        for entry in words {
            assert_eq!(practice::check_rack_word(&rack, entry["word"].as_str().unwrap())["bingo"], true);
            assert!(entry["score"].as_u64().unwrap() >= BINGO_BONUS as u64);
        }
    }
}

#[test]
fn racks_are_random_and_an_unknown_kind_is_refused() {
    ready();
    let racks: HashSet<Vec<String>> = (0..6).map(|s| rack_strings(&practice::make_rack("hunt", &mut rng(s)).unwrap())).collect();
    assert!(racks.len() > 1);
    assert!(practice::make_rack("szokas", &mut rng(0)).is_err());
}

// ===================================================================================================
// Betűvadász: egy szó bírálata
// ===================================================================================================

const FIT_RACK: [&str; 7] = ["A", "L", "M", "Á", "T", "K", "E"];

#[test]
fn a_valid_word() {
    ready();
    let result = practice::check_rack_word(&strings(&FIT_RACK), "almát");
    assert_eq!((result["ok"].clone(), result["word"].clone(), result["score"].clone()), (json!(true), json!("ALMÁT"), json!(5)));
    assert_eq!((result["tiles"].clone(), result["bingo"].clone()), (json!(["A", "L", "M", "Á", "T"]), json!(false)));
}

#[test]
fn not_a_word_and_not_in_rack() {
    ready();
    let rack = strings(&FIT_RACK);
    assert_eq!(practice::check_rack_word(&rack, "ALMTÁ")["reason"], "not_a_word");
    assert_eq!(practice::check_rack_word(&rack, "ALMAK")["reason"], "not_in_rack"); // csak egy A van
    assert_eq!(practice::check_rack_word(&rack, "KÖRTE")["reason"], "not_in_rack");
}

#[test]
fn too_short_and_bad_characters() {
    ready();
    let rack = strings(&FIT_RACK);
    assert_eq!(practice::check_rack_word(&rack, "A")["reason"], "too_short");
    assert_eq!(practice::check_rack_word(&rack, "")["reason"], "too_short");
    assert_eq!(practice::check_rack_word(&rack, "QWX")["reason"], "invalid_chars");
    assert_eq!(practice::check_rack_word(&rack, &"A".repeat(20))["reason"], "invalid_chars");
}

#[test]
fn a_digraph_needs_its_own_tile() {
    ready();
    let one = practice::check_rack_word(&strings(&["SZ", "Ó", "A"]), "SZÓ");
    assert_eq!((one["ok"].clone(), one["tiles"].clone(), one["score"].clone()), (json!(true), json!(["SZ", "Ó"]), json!(5)));
    // külön S + Z zsetonból nem lesz SZ
    let two = practice::check_rack_word(&strings(&["S", "Z", "Ó"]), "SZÓ");
    assert_eq!((two["ok"].clone(), two["reason"].clone()), (json!(false), json!("split_digraph")));
    // ha mindkettő megvan, a kétjegyű zseton számít (nem a külön S + Z)
    let both = practice::check_rack_word(&strings(&["SZ", "S", "Z", "Ó"]), "SZÓ");
    assert_eq!((both["ok"].clone(), both["tiles"].clone(), both["score"].clone()), (json!(true), json!(["SZ", "Ó"]), json!(5)));
}

#[test]
fn every_digraph_is_refused_from_two_single_tiles() {
    ready();
    // (a GY, LY, NY, TY második betűjéből, az Y-ból nincs önálló zseton: csak ezek fordulhatnak elő)
    for (first, second) in [("C", "S"), ("S", "Z"), ("Z", "S")] {
        let word = format!("{first}{second}AK");
        let result = practice::check_rack_word(&strings(&[first, second, "A", "K"]), &word);
        assert_eq!(result["reason"], "split_digraph", "{first}{second}");
    }
}

#[test]
fn single_tiles_next_to_a_digraph_tile_are_fine() {
    ready();
    // asszony = A + S + SZ + O + NY: az S egyjegyű, az SZ viszont egy zseton
    let result = practice::check_rack_word(&strings(&["A", "S", "SZ", "O", "NY"]), "ASSZONY");
    assert_eq!((result["ok"].clone(), result["tiles"].clone()), (json!(true), json!(["A", "S", "SZ", "O", "NY"])));
}

#[test]
fn the_bingo_bonus_in_the_check() {
    ready();
    let result = practice::check_rack_word(&strings(&["K", "Ó", "B", "O", "R", "R", "A"]), "KÓBORRA");
    assert_eq!((result["ok"].clone(), result["bingo"].clone(), result["score"].clone()), (json!(true), json!(true), json!(9 + BINGO_BONUS)));
}

// ===================================================================================================
// Kvíz-válasz
// ===================================================================================================

#[test]
fn a_valid_answer() {
    ready();
    let result = practice::check_answer("ALMÁT", true).unwrap();
    assert_eq!((result["valid"].clone(), result["correct"].clone(), result["score"].clone()), (json!(true), json!(true), json!(5)));
    assert_eq!((result["tiles"].clone(), result["suggestions"].clone()), (json!(["A", "L", "M", "Á", "T"]), json!([])));
}

#[test]
fn a_wrong_guess_on_a_valid_word() {
    ready();
    assert_eq!(practice::check_answer("alma", false).unwrap()["correct"], false);
}

#[test]
fn an_invalid_word_comes_with_suggestions() {
    ready();
    let result = practice::check_answer("ALMTÁ", false).unwrap();
    assert_eq!((result["valid"].clone(), result["correct"].clone()), (json!(false), json!(true)));
    assert!(result["suggestions"].as_array().unwrap().contains(&json!("ALMÁT")));
}

#[test]
fn malformed_words() {
    ready();
    for word in ["", "x", "QWXY", &"A".repeat(20)] {
        assert!(practice::check_answer(word, true).is_none(), "{word}");
    }
}

// ===================================================================================================
// API
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_quiz_endpoint() {
    let server = TestServer::start().await;
    let http = server.http();
    let data = http.get("/api/practice/quiz").await.json();
    assert_eq!((data["success"].clone(), data["mode"].clone(), data["questions"].as_array().unwrap().len()), (json!(true), json!("mixed"), 10));
    // a kérdésekhez a zsetonok is járnak (a kliens ezekből rajzolja a szót)
    let tiles: Vec<Value> = data["questions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|q| json!(tokenize_word(q.as_str().unwrap()).unwrap().iter().map(|t| t.as_str()).collect::<Vec<_>>()))
        .collect();
    assert_eq!(data["tiles"], json!(tiles));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_quiz_options() {
    let server = TestServer::start().await;
    let http = server.http();
    let data = http.get("/api/practice/quiz?n=6&mode=2").await.json();
    assert_eq!((data["questions"].as_array().unwrap().len(), data["mode"].clone()), (6, json!("2")));
    assert!(data["questions"].as_array().unwrap().iter().all(|q| tile_count(q.as_str().unwrap()) == 2));
    assert!(http.get("/api/practice/quiz?n=8&mode=tricky").await.json()["questions"].as_array().unwrap().len() >= 6);
    assert_eq!(http.get("/api/practice/quiz?mode=9").await.status, 400);
    assert_eq!(http.get("/api/practice/quiz?mode=").await.status, 400);
    assert_eq!(http.get("/api/practice/quiz?n=abc").await.json()["questions"].as_array().unwrap().len(), 10);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_rack_endpoint() {
    let server = TestServer::start().await;
    let http = server.http();
    let data = http.get("/api/practice/rack").await.json();
    assert_eq!((data["success"].clone(), data["kind"].clone(), data["rack"].as_array().unwrap().len()), (json!(true), json!("hunt"), 7));
    let first = data["words"][0].as_object().unwrap();
    assert!(["word", "score", "tiles"].iter().all(|k| first.contains_key(*k)));
    let bingo = http.get("/api/practice/rack?kind=bingo").await.json();
    assert_eq!(bingo["kind"], "bingo");
    assert!(bingo["words"].as_array().unwrap().iter().all(|w| w["tiles"] == 7));
    assert_eq!(http.get("/api/practice/rack?kind=x").await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_rack_word_endpoint() {
    let server = TestServer::start().await;
    let http = server.http();
    let rack = json!(["A", "L", "M", "Á", "T", "K", "E"]);
    let ok = http.post("/api/practice/rack-word", json!({"rack": rack, "word": "almát"})).await.json();
    assert_eq!((ok["success"].clone(), ok["ok"].clone(), ok["score"].clone()), (json!(true), json!(true), json!(5)));
    let bad = http.post("/api/practice/rack-word", json!({"rack": rack, "word": "almtá"})).await.json();
    assert_eq!((bad["success"].clone(), bad["ok"].clone(), bad["reason"].clone()), (json!(true), json!(false), json!("not_a_word")));
    assert_eq!(http.post("/api/practice/rack-word", json!({"rack": rack, "word": "körte"})).await.json()["reason"], "not_in_rack");
}

#[tokio::test(flavor = "multi_thread")]
async fn rack_word_validation() {
    let server = TestServer::start().await;
    let http = server.http();
    let post = |body: Value| {
        let http = http.clone();
        async move { http.post("/api/practice/rack-word", body).await.status }
    };
    assert_eq!(post(json!({"word": "ALMA"})).await, 400); // nincs kéz
    assert_eq!(post(json!({"rack": "ALMA", "word": "ALMA"})).await, 400); // a kéz nem lista
    assert_eq!(post(json!({"rack": ["A", "Q"], "word": "A"})).await, 400); // nincs ilyen zseton
    assert_eq!(post(json!({"rack": vec!["A"; 9], "word": "AA"})).await, 400); // túl sok zseton
    assert_eq!(post(json!({"rack": ["A", ""], "word": "AA"})).await, 400); // joker nem szerepelhet
    assert_eq!(post(json!({"rack": ["A", "L"], "word": 5})).await, 400);
    assert_eq!(http.request("POST", "/api/practice/rack-word", None, &[("Content-Type", "application/json")]).await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_answer_endpoint() {
    let server = TestServer::start().await;
    let http = server.http();
    let data = http.post("/api/practice/answer", json!({"word": "ALMÁT", "answer": true})).await.json();
    assert_eq!((data["success"].clone(), data["valid"].clone(), data["correct"].clone()), (json!(true), json!(true), json!(true)));
    let wrong = http.post("/api/practice/answer", json!({"word": "ALMTÁ", "answer": true})).await.json();
    assert_eq!((wrong["valid"].clone(), wrong["correct"].clone()), (json!(false), json!(false)));
    assert!(!wrong["suggestions"].as_array().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn answer_validation() {
    let server = TestServer::start().await;
    let http = server.http();
    for body in [json!({"word": "ALMA"}), json!({"word": "ALMA", "answer": "igen"}), json!({"word": 5, "answer": true}), json!({"word": "QWX", "answer": true})]
    {
        assert_eq!(http.post("/api/practice/answer", body.clone()).await.status, 400, "{body}");
    }
    assert_eq!(http.request("POST", "/api/practice/answer", None, &[("Content-Type", "application/json")]).await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_short_words_endpoint() {
    let server = TestServer::start().await;
    let http = server.http();
    let data = http.get("/api/practice/short-words?length=2").await.json();
    assert_eq!((data["success"].clone(), data["length"].clone()), (json!(true), json!(2)));
    assert!(data["words"].as_array().unwrap().iter().any(|e| e["word"] == "AZ"));
    assert_eq!(http.get("/api/practice/short-words").await.json()["length"], 2);
    assert_eq!(http.get("/api/practice/short-words?length=5").await.status, 400);
    assert_eq!(http.get("/api/practice/short-words?length=x").await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_practice_endpoints_are_rate_limited() {
    let server = TestServer::start().await;
    let http = server.http();
    restore_default_limits(&server.app);
    let mut last = 0;
    for _ in 0..121 {
        last = http.post("/api/practice/answer", json!({"word": "ALMA", "answer": true})).await.status;
    }
    assert_eq!(last, 429);
}
