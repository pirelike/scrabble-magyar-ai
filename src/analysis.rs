//! Játékelemzés: minden lépésnél megmutatja a legjobb lehetséges lépést és a kint maradt pontot.
//!
//! A lépésnapló minden lépésnél tartalmazza a játékos kezét a lépés előtt (`rack`) és a lépés utáni tábla
//! pillanatképét. Az elemzés a megelőző lépés pillanatképén a robot motorjával megkeresi a kézből rakható legjobb
//! lerakást, és összeveti a ténylegesen játszottal. Az eredmény gyorsítótárazott (`game_analysis` tábla); a
//! keresés tőszavakra épül, ezért a robot szókincsének bővítésekor az `ANALYSIS_VERSION`-t növelni kell.

use crate::ai::{self, Vocabulary};
use crate::board::Board;
use crate::db::StoredMove;
use crate::tiles::Tile;
use serde_json::{Map, Value, json};

/// 6: a kétjegyű betű csak a saját zsetonjával rakható ki (5: a szótár-építő második köre, 4: első kör, 3: a furcsa
/// alakok szűrése)
pub const ANALYSIS_VERSION: i64 = 6;
pub const SECONDS_PER_MOVE: f64 = 0.8;
/// Ezek a lépések számítanak a játékos körének (az elutasított lerakás nem).
const TURN_TYPES: [&str; 4] = ["place", "challenge_accept", "exchange", "pass"];

fn details_of(mv: &StoredMove) -> Value {
    mv.details_json
        .as_deref()
        .and_then(|t| serde_json::from_str::<Value>(t).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| json!({}))
}

fn is_turn(mv: &StoredMove, details: &Value) -> bool {
    TURN_TYPES.contains(&mv.action_type.as_str()) && details.get("rack").is_some_and(|r| r.is_array())
}

/// A lépésnapló elemezhető körei (régi játékokban nincs rögzített kéz).
pub fn analyzable_turns(moves: &[StoredMove]) -> Vec<&StoredMove> {
    moves.iter().filter(|m| is_turn(m, &details_of(m))).collect()
}

/// A játék elemzése. `progress`: opcionális `fn(kész, összes)` visszahívás.
/// Visszatér: {'version', 'moves': [...], 'players': [...]}
pub fn analyze_game(moves: &[StoredMove], vocab: &Vocabulary, seconds: f64, mut progress: Option<&mut dyn FnMut(usize, usize)>) -> Value {
    let total = analyzable_turns(moves).len();
    let mut board = Board::new(); // a tábla az aktuális lépés előtt
    let mut entries: Vec<Value> = Vec::new();
    for mv in moves {
        let details = details_of(mv);
        if is_turn(mv, &details) {
            entries.push(analyze_turn(&board, mv, &details, vocab, seconds));
            if let Some(cb) = progress.as_deref_mut() {
                cb(entries.len(), total);
            }
        }
        if let Some(snapshot) = mv.board_snapshot_json.as_deref().filter(|s| !s.is_empty()) {
            if let Ok(value) = serde_json::from_str::<Value>(snapshot) {
                board = Board::from_json(&value);
            }
        }
    }
    let players = summarize(&entries);
    json!({"version": ANALYSIS_VERSION, "moves": entries, "players": players})
}

fn analyze_turn(board: &Board, mv: &StoredMove, details: &Value, vocab: &Vocabulary, seconds: f64) -> Value {
    let rack: Vec<Tile> = details["rack"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().and_then(Tile::from_str)).collect()).unwrap_or_default();
    let is_place = mv.action_type == "place" || mv.action_type == "challenge_accept";
    let actual = if is_place { details.get("score").and_then(|s| s.as_i64()).unwrap_or(0) } else { 0 };
    let best = ai::best_moves(board, &rack, 1, vocab, Some(seconds));
    let (mut best_score, mut best_words, mut best_tiles): (i64, Value, Value) = (0, json!([]), json!([]));
    if let Some(first) = best.first() {
        best_score = first.score as i64;
        best_words = json!(first.words);
        best_tiles = first.tiles_json();
    }
    if actual >= best_score && actual > 0 {
        // A játszott lépés legalább olyan jó, mint amit a motor talált (a motor szókincse szűkebb)
        best_score = actual;
        best_words = details.get("words").cloned().unwrap_or_else(|| json!([]));
        best_tiles = details.get("tiles").cloned().unwrap_or_else(|| json!([]));
    }
    json!({
        "n": mv.move_number,
        "player": mv.player_name,
        "type": mv.action_type,
        "rack": details["rack"],
        "score": actual,
        "words": details.get("words").cloned().unwrap_or_else(|| json!([])),
        "best_score": best_score,
        "best_words": best_words,
        "best_tiles": best_tiles,
        "lost": (best_score - actual).max(0),
    })
}

/// Játékosonkénti összesítés: hány pontot ért el, és mennyit lehetett volna.
fn summarize(entries: &[Value]) -> Vec<Value> {
    let mut order: Vec<String> = Vec::new();
    let mut players: std::collections::HashMap<String, Map<String, Value>> = std::collections::HashMap::new();
    for e in entries {
        let name = e["player"].as_str().unwrap_or("").to_string();
        let p = players.entry(name.clone()).or_insert_with(|| {
            order.push(name.clone());
            let mut m = Map::new();
            m.insert("player".into(), json!(name));
            m.insert("turns".into(), json!(0));
            m.insert("scored".into(), json!(0));
            m.insert("possible".into(), json!(0));
            m.insert("optimal".into(), json!(0));
            m.insert("biggest_miss".into(), Value::Null);
            m
        });
        let score = e["score"].as_i64().unwrap_or(0);
        let best = e["best_score"].as_i64().unwrap_or(0);
        let lost = e["lost"].as_i64().unwrap_or(0);
        let add = |m: &mut Map<String, Value>, key: &str, by: i64| {
            let v = m[key].as_i64().unwrap_or(0) + by;
            m.insert(key.to_string(), json!(v));
        };
        add(p, "turns", 1);
        add(p, "scored", score);
        add(p, "possible", score.max(best));
        if lost == 0 {
            add(p, "optimal", 1);
        }
        let bigger = p["biggest_miss"].is_null() || lost > p["biggest_miss"]["lost"].as_i64().unwrap_or(0);
        if lost > 0 && bigger {
            p.insert(
                "biggest_miss".into(),
                json!({"n": e["n"], "lost": lost, "best_words": e["best_words"], "best_score": best, "score": score}),
            );
        }
    }
    order
        .into_iter()
        .map(|name| {
            let mut p = players.remove(&name).unwrap();
            let scored = p["scored"].as_i64().unwrap_or(0);
            let possible = p["possible"].as_i64().unwrap_or(0);
            p.insert("missed".into(), json!(possible - scored));
            let efficiency = if possible > 0 { ((1000.0 * scored as f64 / possible as f64).round_ties_even()) / 10.0 } else { 100.0 };
            p.insert("efficiency".into(), json!(efficiency));
            Value::Object(p)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(player: &str, n: i64, score: i64, best: i64) -> Value {
        json!({"n": n, "player": player, "score": score, "best_score": best, "lost": (best - score).max(0), "best_words": ["X"]})
    }

    #[test]
    fn summary_tracks_misses_and_efficiency() {
        let entries = vec![entry("A", 1, 10, 10), entry("A", 3, 5, 20), entry("B", 2, 30, 30), entry("A", 5, 0, 8)];
        let players = summarize(&entries);
        assert_eq!(players.len(), 2);
        let a = &players[0];
        assert_eq!(a["player"], "A");
        assert_eq!(a["turns"], 3);
        assert_eq!(a["scored"], 15);
        assert_eq!(a["possible"], 38);
        assert_eq!(a["optimal"], 1);
        assert_eq!(a["biggest_miss"]["n"], 3);
        assert_eq!(a["biggest_miss"]["lost"], 15);
        assert_eq!(a["missed"], 23);
        assert_eq!(a["efficiency"], 39.5);
        let b = &players[1];
        assert_eq!(b["efficiency"], 100.0);
        assert!(b["biggest_miss"].is_null());
    }

    #[test]
    fn old_games_without_racks_are_not_analyzable() {
        let old = StoredMove { move_number: 1, player_name: "A".into(), action_type: "place".into(), details_json: Some("{\"score\": 5}".into()), board_snapshot_json: None };
        let new = StoredMove {
            move_number: 2,
            player_name: "A".into(),
            action_type: "pass".into(),
            details_json: Some("{\"rack\": [\"A\"], \"score\": 0}".into()),
            board_snapshot_json: None,
        };
        let reject = StoredMove { move_number: 3, player_name: "A".into(), action_type: "challenge_reject".into(), details_json: Some("{\"rack\": [], \"score\": 0}".into()), board_snapshot_json: None };
        assert_eq!(analyzable_turns(&[old, new, reject]).len(), 1);
    }
}
