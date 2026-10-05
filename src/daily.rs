//! Napi feladvány: naponta egy közös táblaállás és kéz, a cél a lehető legtöbb pont egyetlen lépéssel.
//!
//! A feladványt a nap dátumából indított, determinisztikus bot–bot játék adja (néhány lépés után a soron lévő
//! robot keze + a tábla), a "legjobb lépést" a robot motorja keresi meg. Az első kérésnél készül el, és az
//! adatbázisban rögzül, így mindenkinek ugyanaz. Játék közben az ellenőrzést és a pontozást a játék saját logikája
//! végzi (egy játékos, egy lerakás után vége).

use crate::ai::{self, Action, Vocabulary};
use crate::board::Board;
use crate::db::{Db, DailyPuzzle};
use crate::game::{Game, HAND_SIZE};
use crate::tiles::{TILE_DISTRIBUTION, Tile};
use crate::util;
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Weekday};
use rand::seq::SliceRandom;
use rand::{RngExt, SeedableRng, rngs::StdRng};
use serde_json::{Value, json};

/// a legjobb lépés legalább ennyit érjen (különben unalmas a feladvány)
pub const MIN_BEST_SCORE: i32 = 30;
/// és ne legyen túl szerencsés / abszurd
pub const MAX_BEST_SCORE: i32 = 250;
/// ennyi lépést játszanak le a robotok a feladvány előtt
pub const PLIES: (usize, usize) = (8, 16);
const MAX_ATTEMPTS: u64 = 40;
const SEARCH_SECONDS: f64 = 4.0;
/// A robotok lépéskeresésének időkerete a feladvány előállításakor (a feladvány ne függjön a gép sebességétől).
const GENERATION_SECONDS: f64 = 10.0;

/// Az utolsó vasárnap (hónap, év) dátuma.
fn last_sunday(year: i32, month: u32) -> NaiveDate {
    let mut day = NaiveDate::from_ymd_opt(year, month, 31).unwrap_or_else(|| NaiveDate::from_ymd_opt(year, month, 30).unwrap());
    while day.weekday() != Weekday::Sun {
        day -= Duration::days(1);
    }
    day
}

/// Az Európa/Budapest időzóna eltérése az UTC-hez képest órában (az EU nyári időszámítása: március utolsó
/// vasárnapjának 01:00 UTC-jétől október utolsó vasárnapjának 01:00 UTC-jéig CEST, +2).
pub fn budapest_offset_hours(utc: NaiveDateTime) -> i64 {
    let year = utc.year();
    let start = last_sunday(year, 3).and_hms_opt(1, 0, 0).unwrap();
    let end = last_sunday(year, 10).and_hms_opt(1, 0, 0).unwrap();
    if utc >= start && utc < end { 2 } else { 1 }
}

/// A megadott UTC idő magyar idő szerinti dátuma ÉÉÉÉ-HH-NN formában.
pub fn date_str_at(utc: NaiveDateTime) -> String {
    (utc + Duration::hours(budapest_offset_hours(utc))).format("%Y-%m-%d").to_string()
}

/// A mai dátum (magyar idő szerint) ÉÉÉÉ-HH-NN formában.
pub fn today_str() -> String {
    date_str_at(util::utcnow())
}

pub fn previous_date(date_str: &str) -> String {
    NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
        .map(|d| (d - Duration::days(1)).format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

pub fn is_valid_date(text: &str) -> bool {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").map(|d| d.format("%Y-%m-%d").to_string() == text).unwrap_or(false)
}

/// Az elkészült feladvány (mentés előtt).
pub struct Generated {
    pub board: Value,
    pub rack: Vec<String>,
    pub best_score: i64,
    pub best: Value,
}

fn tile_dicts(tiles: &[crate::board::Placed]) -> Value {
    Value::Array(tiles.iter().map(|p| p.to_json()).collect())
}

/// Egy próbálkozás: lejátszott játék, majd a soron lévő robot keze a feladvány.
fn try_generate(rng: &mut StdRng, vocab: &Vocabulary) -> Option<Generated> {
    let mut game = Game::with_defaults("daily");
    let levels = [rng.random_range(5..=8i64), rng.random_range(5..=8i64)];
    for (i, level) in levels.iter().enumerate() {
        let _ = game.add_bot(&format!("Bot{}", i + 1), &json!(level));
    }
    game.bag.tiles.sort_by_key(|t| t.as_str());
    game.bag.tiles.shuffle(rng); // a zsák sorrendje csak a dátumtól függ
    let _ = game.start();

    let plies = rng.random_range(PLIES.0..=PLIES.1);
    for _ in 0..plies {
        if game.finished {
            return None;
        }
        let player = game.current_player()?.clone();
        let level = player.difficulty.and_then(|d| d.level()).unwrap_or(6);
        let action = ai::choose_action(&game.board, &player.hand, level, game.bag.remaining(), rng, vocab, None, GENERATION_SECONDS);
        let mut ok = false;
        if let Action::Place { tiles, .. } = &action {
            ok = game.place_tiles(&player.id, tiles).is_ok();
        }
        if !ok {
            // Csere helyett passz: a zsák visszakeverése a globális véletlent használná, a feladvány viszont
            // csak a dátumtól függhet
            let _ = game.pass_turn(&player.id, false);
        }
    }

    let rack = game.current_player()?.hand.clone();
    if game.finished || rack.len() < HAND_SIZE || game.board.is_empty {
        return None;
    }
    let best = ai::best_moves(&game.board, &rack, 1, vocab, Some(SEARCH_SECONDS));
    let first = best.first()?;
    if !(MIN_BEST_SCORE..=MAX_BEST_SCORE).contains(&first.score) {
        return None;
    }
    Some(Generated {
        board: game.board.to_json(),
        rack: rack.iter().map(|t| t.as_str().to_string()).collect(),
        best_score: first.score as i64,
        best: json!({"tiles": tile_dicts(&first.tiles), "words": first.words, "score": first.score}),
    })
}

/// A dátumhoz tartozó feladvány előállítása (determinisztikus a dátumból).
pub fn generate_puzzle(date_str: &str, vocab: &Vocabulary) -> Result<Generated, String> {
    let digits: String = date_str.chars().filter(|c| c.is_ascii_digit()).collect();
    let seed: u64 = digits.parse::<u64>().unwrap_or(0) * 1000;
    for attempt in 0..MAX_ATTEMPTS {
        let mut rng = StdRng::seed_from_u64(seed + attempt);
        if let Some(puzzle) = try_generate(&mut rng, vocab) {
            return Ok(puzzle);
        }
    }
    Err(format!("nem sikerült feladványt készíteni ({date_str})"))
}

/// A nap feladványa: az adatbázisból, ennek híján elkészíti és elmenti (hosszan számol: háttérszálból hívandó).
pub fn ensure_puzzle(db: &Db, date_str: Option<&str>, vocab: &Vocabulary) -> Result<DailyPuzzle, String> {
    let date = date_str.map(|d| d.to_string()).unwrap_or_else(today_str);
    if let Some(puzzle) = db.get_daily_puzzle(&date) {
        return Ok(puzzle);
    }
    let generated = generate_puzzle(&date, vocab)?;
    db.save_daily_puzzle(&date, &generated.board, &generated.rack, generated.best_score, &generated.best).map_err(|e| e.to_string())?;
    // egyidejű kérésnél az először mentett marad
    db.get_daily_puzzle(&date).ok_or_else(|| "a feladvány nem menthető".to_string())
}

fn seed_from_text(text: &str) -> u64 {
    // FNV-1a: a dátum szövegéből determinisztikus mag
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// A zsákban maradt (a táblán és a kézben nem látható) zsetonok, a dátumtól függő sorrendben.
fn remaining_tiles(board: &Board, rack: &[Tile], date_str: &str) -> Vec<Tile> {
    let mut counts: Vec<i32> = TILE_DISTRIBUTION.iter().map(|(_, _, c)| *c as i32).collect();
    for row in &board.cells {
        for cell in row.iter().flatten() {
            let tile = if cell.is_blank { Tile::BLANK } else { cell.letter };
            counts[tile.0 as usize] -= 1;
        }
    }
    for t in rack {
        counts[t.0 as usize] -= 1;
    }
    let mut tiles: Vec<Tile> = Vec::new();
    for (i, n) in counts.iter().enumerate() {
        for _ in 0..(*n).max(0) {
            tiles.push(Tile(i as u8));
        }
    }
    tiles.sort_by_key(|t| t.as_str());
    tiles.shuffle(&mut StdRng::seed_from_u64(seed_from_text(date_str)));
    tiles
}

/// A játékot a feladvány kezdőállapotára állítja (indításkor és újrapróbálkozáskor).
pub fn reset_game(game: &mut Game, puzzle: &DailyPuzzle) {
    game.board = Board::from_json(&puzzle.board);
    let hand: Vec<Tile> = puzzle.rack.iter().filter_map(|t| Tile::from_str(t)).collect();
    game.bag.tiles = remaining_tiles(&game.board, &hand, &puzzle.date);
    if let Some(player) = game.players.first_mut() {
        player.hand = hand;
        player.score = 0;
    }
    game.current_player_idx = 0;
    game.finished = false;
    game.winners = Vec::new();
    game.move_log = Vec::new();
    game.turn_number = 0;
    game.scoreless_turns = 0;
    game.pending_challenge = None;
    game.last_action = None;
    game.last_action_info = None;
    game.puzzle = Some(crate::game::Puzzle { date: puzzle.date.clone(), score: None });
}

/// Egyjátékos játék a feladvány állásából.
pub fn build_game(room_id: &str, sid: &str, name: &str, puzzle: &DailyPuzzle) -> Game {
    let mut game = Game::new(room_id, false, 0, 0);
    let _ = game.add_player(sid, name);
    let _ = game.start();
    reset_game(&mut game, puzzle);
    game
}

/// Egy beküldött lépés feldolgozása: ranglista (csak regisztrált), új rekord, helyezés.
///
/// Visszatér: {'score', 'best_score' (a feladvány legjobbja), 'is_best', 'recorded', 'attempts', 'my_best',
/// 'rank'}; vendégnél `recorded` hamis.
pub fn record_result(db: &Db, puzzle: &DailyPuzzle, user_id: Option<i64>, score: i64, tiles: &Value, words: &Value) -> Value {
    let mut best_score = puzzle.best_score;
    if score > best_score {
        db.raise_daily_best(&puzzle.date, score, &json!({"tiles": tiles, "words": words, "score": score}));
        best_score = score;
    }
    let mut result = json!({
        "score": score, "best_score": best_score, "is_best": score >= best_score,
        "recorded": false, "attempts": null, "my_best": null, "rank": null,
    });
    if let Some(uid) = user_id.filter(|u| *u != 0) {
        if let Ok(rec) = db.record_daily_score(&puzzle.date, uid, score) {
            result["recorded"] = json!(rec.recorded);
            result["attempts"] = json!(rec.attempts);
            result["my_best"] = json!(rec.best);
        }
        let (_entries, me) = db.get_daily_leaderboard(&puzzle.date, 1, Some(uid));
        result["rank"] = me.map(|m| m["rank"].clone()).unwrap_or(Value::Null);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(y: i32, m: u32, d: u32, h: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, 0, 0).unwrap()
    }

    #[test]
    fn budapest_time_follows_eu_dst() {
        // tél: UTC+1; nyár: UTC+2
        assert_eq!(budapest_offset_hours(utc(2026, 1, 15, 12)), 1);
        assert_eq!(budapest_offset_hours(utc(2026, 7, 15, 12)), 2);
        // 2026-ban az átállás március 29-én és október 25-én van 01:00 UTC-kor
        assert_eq!(budapest_offset_hours(utc(2026, 3, 29, 0)), 1);
        assert_eq!(budapest_offset_hours(utc(2026, 3, 29, 1)), 2);
        assert_eq!(budapest_offset_hours(utc(2026, 10, 25, 0)), 2);
        assert_eq!(budapest_offset_hours(utc(2026, 10, 25, 1)), 1);
    }

    #[test]
    fn date_rolls_over_at_hungarian_midnight() {
        assert_eq!(date_str_at(utc(2026, 7, 15, 21)), "2026-07-15");
        assert_eq!(date_str_at(utc(2026, 7, 15, 22)), "2026-07-16");
        assert_eq!(date_str_at(utc(2026, 1, 15, 22)), "2026-01-15");
        assert_eq!(date_str_at(utc(2026, 1, 15, 23)), "2026-01-16");
    }

    #[test]
    fn date_helpers() {
        assert_eq!(previous_date("2026-03-01"), "2026-02-28");
        assert_eq!(previous_date("2026-01-01"), "2025-12-31");
        assert!(is_valid_date("2026-10-05"));
        assert!(!is_valid_date("2026-13-05"));
        assert!(!is_valid_date("2026-1-5"));
        assert!(!is_valid_date("nem dátum"));
    }

    #[test]
    fn remaining_tiles_are_deterministic_and_complete() {
        let mut board = Board::new();
        board.set(7, 7, Tile::from_str("A").unwrap(), false);
        board.set(7, 8, Tile::from_str("K").unwrap(), true);
        board.is_empty = false;
        let rack: Vec<Tile> = ["A", "L", "M", "A", "S", "Z", ""].iter().map(|t| Tile::from_str(t).unwrap()).collect();
        let first = remaining_tiles(&board, &rack, "2026-10-05");
        let second = remaining_tiles(&board, &rack, "2026-10-05");
        assert_eq!(first, second);
        assert_eq!(first.len(), 100 - 2 - 7);
        assert_ne!(first, remaining_tiles(&board, &rack, "2026-10-06"));
        let blanks = first.iter().filter(|t| t.is_blank()).count();
        assert_eq!(blanks, 2 - 1 - 1, "egy joker a táblán, egy a kézben");
    }
}
