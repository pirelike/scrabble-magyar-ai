//! A szótár-ellenőrző összevetése a Python referencia-implementáció eredményeivel (`tests/golden/*.json`).

use scrabble::{affix::AffixChecker, config, dictionary};
use serde_json::Value;
use std::path::PathBuf;

fn golden(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(name);
    serde_json::from_slice(&std::fs::read(path).expect("golden fájl")).unwrap()
}

fn checker() -> AffixChecker {
    dictionary::load_checker(&config::dict_dir()).expect("a szótár betölthető")
}

#[test]
fn affix_checker_matches_python_on_16k_words() {
    let checker = checker();
    let golden = golden("dictionary.json");
    let rows = golden["words"].as_array().unwrap();
    assert!(rows.len() > 10_000);
    let mut mismatches = Vec::new();
    for row in rows {
        let word = row[0].as_str().unwrap();
        let expected = (row[1].as_i64().unwrap() == 1, row[2].as_i64().unwrap() == 1, row[3].as_i64().unwrap() == 1);
        let actual = (checker.check(word), checker.has_stem(word), checker.needs_attestation(word));
        if expected != actual {
            mismatches.push(format!("{word}: várt {expected:?}, kapott {actual:?}"));
        }
    }
    assert!(mismatches.is_empty(), "{} eltérés, pl.: {:#?}", mismatches.len(), &mismatches[..mismatches.len().min(15)]);
}

#[test]
fn dictionary_lookup_matches_python_including_rejected_lists() {
    dictionary::warm_up();
    let golden = golden("dictionary.json");
    let rows = golden["words"].as_array().unwrap();
    let mut mismatches = Vec::new();
    for row in rows {
        let word = row[0].as_str().unwrap();
        let expected = row[4].as_i64().unwrap() == 1;
        if dictionary::lookup_word(word) != expected {
            mismatches.push(word.to_string());
        }
    }
    assert!(mismatches.is_empty(), "{} eltérés, pl.: {:?}", mismatches.len(), &mismatches[..mismatches.len().min(15)]);
}

#[test]
fn explanations_match_python() {
    let checker = checker();
    let golden = golden("explain.json");
    for (word, expected) in golden.as_object().unwrap() {
        let actual: Vec<Value> = checker
            .explain(word, 12)
            .into_iter()
            .map(|e| {
                serde_json::json!({
                    "stem": e.stem, "prefix": e.prefix, "suffixes": e.suffixes, "risky": e.risky,
                    "risk": e.risk, "adjective": e.adjective, "derived": e.derived,
                })
            })
            .collect();
        assert_eq!(&Value::Array(actual), expected, "levezetés eltér: {word}");
    }
}

#[test]
fn suggestions_match_python() {
    dictionary::warm_up();
    let golden = golden("suggest.json");
    for (word, expected) in golden.as_object().unwrap() {
        let actual = dictionary::suggest_words(word, 6);
        let expected: Vec<String> = expected.as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
        assert_eq!(actual, expected, "javaslatok eltérnek: {word}");
    }
}

#[test]
fn check_words_rules() {
    dictionary::warm_up();
    assert_eq!(dictionary::check_words(&["ALMA", "SZÉK"]), (true, vec![]));
    let (ok, invalid) = dictionary::check_words(&["ALMA", "ALAMAA"]);
    assert!(!ok);
    assert_eq!(invalid, vec!["ALAMAA".to_string()]);
    // nem szó-alak: kisbetű, szám, túl hosszú
    assert!(!dictionary::check_words(&["alma"]).0);
    assert!(!dictionary::check_words(&["ABCDEFGHIJKLMNOP"]).0);
    // tulajdonnév és rövidítés nem érvényes
    assert!(!dictionary::check_words(&["BUDAPEST"]).0);
    assert!(!dictionary::check_words(&["KG"]).0);
    assert!(dictionary::check_words(&["BRR"]).0);
    assert!(dictionary::check_words::<&str>(&[]).0);
}
