//! A robot szókincse a Python referencia-implementáció szókincsével egyezik (darabszám, ellenőrző összeg).

use scrabble::{ai, config, dictionary};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

fn golden() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/vocabulary.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn vocabulary_matches_python() {
    dictionary::warm_up();
    let vocab = ai::load_vocabulary(&config::dict_dir().join("hu_HU.dic"), true).unwrap();
    let golden = golden();
    let words: Vec<&str> = vocab.iter().collect();

    // ha megvan a Python kimenete, pontos különbséget is mutat
    if let Ok(path) = std::env::var("SCRABBLE_PY_VOCAB") {
        let python: std::collections::HashSet<String> = std::fs::read_to_string(path).unwrap().split('\n').map(|s| s.to_string()).collect();
        let rust: std::collections::HashSet<String> = words.iter().map(|s| s.to_string()).collect();
        let mut only_rust: Vec<&String> = rust.difference(&python).collect();
        let mut only_python: Vec<&String> = python.difference(&rust).collect();
        only_rust.sort();
        only_python.sort();
        assert!(
            only_rust.is_empty() && only_python.is_empty(),
            "csak Rust: {} (pl. {:?}), csak Python: {} (pl. {:?})",
            only_rust.len(),
            &only_rust[..only_rust.len().min(10)],
            only_python.len(),
            &only_python[..only_python.len().min(10)]
        );
    }

    assert_eq!(words.len() as u64, golden["count"].as_u64().unwrap(), "a szókincs mérete eltér");
    let mut hasher = Sha256::new();
    hasher.update(words.join("\n").as_bytes());
    assert_eq!(hex::encode(hasher.finalize()), golden["sha256"].as_str().unwrap(), "a szókincs tartalma eltér");
    for word in golden["sample"].as_array().unwrap() {
        assert!(vocab.contains(word.as_str().unwrap()));
    }
}

#[test]
fn vocabulary_prefix_and_membership() {
    let vocab = ai::Vocabulary::new(vec!["alma".into(), "almafa".into(), "körte".into(), "korte".into()]);
    assert_eq!(vocab.len(), 4);
    assert!(vocab.contains("ALMA"));
    assert!(vocab.contains("ALMAFA"));
    assert!(!vocab.contains("ALM"));
    assert!(vocab.has_prefix("ALM"));
    assert!(vocab.has_prefix("KÖR"));
    assert!(!vocab.has_prefix("KÖRTÉ"));
    assert!(!vocab.has_prefix("Z"));
}
