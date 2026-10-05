//! A gyakorló módok és a zsetonokra bontás a Python referencia-implementációval egyezik.

use scrabble::tiles::{Tile, tokenize_word, word_base_score};
use scrabble::{dictionary, practice};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

fn golden() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/practice.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn sha(lines: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(lines.join("\n").as_bytes());
    hex::encode(hasher.finalize())
}

#[test]
fn tokenization_matches_python() {
    let golden = golden();
    let items = golden["tokenize"]["items"].as_array().unwrap();
    assert!(items.len() > 3000);
    for item in items {
        let word = item[0].as_str().unwrap();
        let expected_tokens = item[1].as_str().unwrap();
        let expected_score = item[2].as_str().unwrap();
        let tokens = tokenize_word(word);
        let actual = tokens.as_ref().map(|t| t.iter().map(|x: &Tile| x.as_str()).collect::<Vec<_>>().join(",")).unwrap_or_default();
        assert_eq!(actual, expected_tokens, "{word}: zsetonok");
        let score = word_base_score(word).map(|s| s.to_string()).unwrap_or_else(|| "None".to_string());
        assert_eq!(score, expected_score, "{word}: pontérték");
    }
}

#[test]
fn short_words_match_python() {
    dictionary::warm_up();
    let golden = golden();
    for length in [2usize, 3] {
        let words = practice::short_words(length);
        let lines: Vec<String> = words.iter().map(|e| format!("{}:{}", e["word"].as_str().unwrap(), e["score"])).collect();
        let key = format!("short_{length}");
        assert_eq!(lines.len() as u64, golden[&key]["count"].as_u64().unwrap(), "{key}: darabszám");
        assert_eq!(sha(&lines), golden[&key]["sha256"].as_str().unwrap(), "{key}: tartalom");
    }
}

#[test]
fn stem_words_match_python() {
    dictionary::warm_up();
    let golden = golden();
    let mut stems: Vec<String> = practice::stem_words().to_vec();
    stems.sort();
    assert_eq!(stems.len() as u64, golden["stems"]["count"].as_u64().unwrap());
    assert_eq!(sha(&stems), golden["stems"]["sha256"].as_str().unwrap());
}

fn tiles(items: &Value) -> Vec<Tile> {
    items.as_array().unwrap().iter().map(|t| Tile::from_str(t.as_str().unwrap()).unwrap()).collect()
}

#[test]
fn rack_words_match_python() {
    dictionary::warm_up();
    let golden = golden();
    for case in golden["rack_words"].as_array().unwrap() {
        let rack = tiles(&case["rack"]);
        let words = practice::rack_words(&rack);
        let lines: Vec<String> =
            words.iter().map(|w| format!("{}:{}:{}", w["word"].as_str().unwrap(), w["score"], w["tiles"])).collect();
        assert_eq!(lines.len() as u64, case["count"].as_u64().unwrap(), "{:?}: darabszám", case["rack"]);
        assert_eq!(sha(&lines), case["sha256"].as_str().unwrap(), "{:?}: tartalom", case["rack"]);
    }
}

#[test]
fn rack_word_checks_match_python() {
    dictionary::warm_up();
    let golden = golden();
    for case in golden["check_rack_word"].as_array().unwrap() {
        let rack: Vec<String> = case["rack"].as_array().unwrap().iter().map(|t| t.as_str().unwrap().to_string()).collect();
        let actual = practice::check_rack_word(&rack, case["word"].as_str().unwrap());
        assert_eq!(actual, case["result"], "{:?} / {:?}", case["rack"], case["word"]);
    }
}

#[test]
fn quiz_answers_match_python() {
    dictionary::warm_up();
    let golden = golden();
    for case in golden["check_answer"].as_array().unwrap() {
        let actual = practice::check_answer(case["word"].as_str().unwrap(), case["answer"].as_bool().unwrap());
        let expected = &case["result"];
        match actual {
            None => assert!(expected.is_null(), "{}", case["word"]),
            Some(actual) => assert_eq!(&actual, expected, "{}", case["word"]),
        }
    }
}

#[test]
fn quiz_generation_produces_requested_shape() {
    dictionary::warm_up();
    let mut rng = rand::rng();
    for mode in ["mixed", "2", "3", "tricky"] {
        for count in [2usize, 10, 11, 30, 100] {
            let questions = practice::make_quiz(count, mode, &mut rng).unwrap();
            let expected = count.clamp(2, 30);
            assert!(questions.len() <= expected && questions.len() + 3 >= expected, "{mode}/{count}: {}", questions.len());
            let unique: std::collections::HashSet<&String> = questions.iter().collect();
            assert_eq!(unique.len(), questions.len(), "nincs ismétlődő kérdés");
            let valid = questions.iter().filter(|q| dictionary::is_word_valid(q)).count();
            assert!(valid >= 1 && valid < questions.len(), "{mode}: vegyes érvényes / érvénytelen kérdések");
        }
    }
    assert!(practice::make_quiz(10, "nincs", &mut rng).is_err());
}

#[test]
fn rack_generation_hunt_and_bingo() {
    dictionary::warm_up();
    let mut rng = rand::rng();
    let hunt = practice::make_rack("hunt", &mut rng).unwrap();
    assert_eq!(hunt["rack"].as_array().unwrap().len(), 7);
    assert!(!hunt["words"].as_array().unwrap().is_empty());
    let bingo = practice::make_rack("bingo", &mut rng).unwrap();
    let words = bingo["words"].as_array().unwrap();
    assert!(!words.is_empty());
    assert!(words.iter().all(|w| w["tiles"] == json!(7) && w["score"].as_i64().unwrap() >= 50));
    assert!(practice::make_rack("nincs", &mut rng).is_err());
}
