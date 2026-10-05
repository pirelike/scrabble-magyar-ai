//! A robot lépésgenerátora a Python referencia-implementáció lépéshalmazával egyezik (állásonként a lépések
//! száma és az ellenőrző összege). A teljes Pythonos kimenet (`SCRABBLE_PY_MOVES`) megléte esetén a pontos
//! különbséget is megmutatja.

use scrabble::ai::{self, Vocabulary};
use scrabble::board::Board;
use scrabble::tiles::Tile;
use scrabble::{config, dictionary};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

fn vocab() -> Arc<Vocabulary> {
    static V: OnceLock<Arc<Vocabulary>> = OnceLock::new();
    V.get_or_init(|| {
        dictionary::warm_up();
        Arc::new(ai::load_vocabulary(&config::dict_dir().join("hu_HU.dic"), true).unwrap())
    })
    .clone()
}

fn canonical(moves: &[ai::Move]) -> Vec<String> {
    let mut lines: Vec<String> = moves
        .iter()
        .map(|m| {
            let mut tiles = m.tiles.clone();
            tiles.sort_by_key(|p| (p.row, p.col));
            let tiles: Vec<String> =
                tiles.iter().map(|p| format!("{},{},{},{}", p.row, p.col, p.letter.as_str(), p.is_blank as i32)).collect();
            format!("{}|{}|{}", tiles.join(";"), m.words.join(","), m.score)
        })
        .collect();
    lines.sort();
    lines
}

#[test]
fn move_generation_matches_python() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/moves.json");
    let cases: Vec<Value> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let full: Option<Vec<Value>> = std::env::var("SCRABBLE_PY_MOVES")
        .ok()
        .map(|p| serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap());
    let vocab = vocab();
    let mut total = 0;
    for (i, case) in cases.iter().enumerate() {
        let mut board = Board::from_json(&case["board"]);
        board.is_empty = board.cells.iter().all(|row| row.iter().all(|c| c.is_none()));
        let rack: Vec<Tile> = case["rack"].as_array().unwrap().iter().map(|t| Tile::from_str(t.as_str().unwrap()).unwrap()).collect();
        let moves = ai::generate_moves(&board, &rack, &vocab, true, 7, 600.0);
        let lines = canonical(&moves);
        total += lines.len();
        let mut hasher = Sha256::new();
        hasher.update(lines.join("\n").as_bytes());
        let digest = hex::encode(hasher.finalize());
        if lines.len() as u64 != case["count"].as_u64().unwrap() || digest != case["sha256"].as_str().unwrap() {
            let mut detail = String::new();
            if let Some(full) = &full {
                let python: std::collections::HashSet<String> =
                    full[i]["lines"].as_array().unwrap().iter().map(|l| l.as_str().unwrap().to_string()).collect();
                let rust: std::collections::HashSet<String> = lines.iter().cloned().collect();
                let mut only_rust: Vec<&String> = rust.difference(&python).collect();
                let mut only_python: Vec<&String> = python.difference(&rust).collect();
                only_rust.sort();
                only_python.sort();
                detail = format!(
                    "\ncsak Rust ({}): {:?}\ncsak Python ({}): {:?}",
                    only_rust.len(),
                    &only_rust[..only_rust.len().min(5)],
                    only_python.len(),
                    &only_python[..only_python.len().min(5)]
                );
            }
            panic!("{}. eset (kéz: {:?}): Rust {} lépés, Python {}{detail}", i, case["rack"], lines.len(), case["count"]);
        }
    }
    assert!(total > 50_000, "túl kevés lépés: {total}");
}
