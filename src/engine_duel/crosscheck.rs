//! A két független lépésgenerátor (a robot horgonykeresése és a motor GADDAG-ja) összevetése, és a motor minden lépésének
//! ellenőrzése a játékvezetővel. Ez a biztonsági háló: ha itt eltérés van, a párharc számai nem értelmezhetők.

use super::bridge::{self, Key};
use super::derive;
use super::spec::Env;
use crate::ai;
use crate::board::Board;
use crate::game::Game;
use crate::tiles::Tile;
use pg_scrabble::eval::Unseen;
use pg_scrabble::movegen::MoveGenerator;
use pg_scrabble::rng::Rng;
use std::collections::{BTreeMap, HashMap};

#[derive(Default, Debug)]
pub struct PositionDiff {
    pub bot_total: usize,
    pub bot_nonvocab: usize,
    pub bot_vocab: usize,
    pub engine_moves: usize,
    pub only_bot: Vec<String>,
    pub only_engine: Vec<String>,
    pub score_diff: Vec<String>,
    pub engine_rejected: Vec<String>,
    pub engine_score_mismatch: Vec<String>,
    pub bot_ms: u128,
}

impl PositionDiff {
    pub fn identical(&self) -> bool {
        self.only_bot.is_empty() && self.only_engine.is_empty() && self.score_diff.is_empty()
    }
}

/// Egy állás összevetése: a robot (a szókincsére korlátozott) és a motor lépéshalmaza ugyanaz-e, és a játékvezető
/// minden motorlépést érvényesnek talál-e, ugyanazzal a ponttal.
pub fn compare_position(env: &Env, generator: &mut MoveGenerator, board: &Board, hand: &[Tile]) -> PositionDiff {
    let mut diff = PositionDiff::default();
    let started = std::time::Instant::now();
    let all = ai::generate_moves(board, hand, &env.vocab, true, ai::HAND_SIZE, 120.0);
    diff.bot_ms = started.elapsed().as_millis();
    diff.bot_total = all.len();
    let mut bot: HashMap<Key, i32> = HashMap::new();
    for m in all {
        if m.words.iter().all(|w| env.vocab.contains(w)) {
            bot.insert(bridge::key_of(&m.tiles), m.score);
        } else {
            diff.bot_nonvocab += 1;
        }
    }
    diff.bot_vocab = bot.len();

    let ebrd = bridge::board_to_engine(board, &env.config);
    let erack = bridge::rack_to_engine(hand);
    let plays = generator.generate(&ebrd, &erack, &env.lexicon).to_vec();
    let mut engine: HashMap<Key, i32> = HashMap::new();
    for play in &plays {
        let placed = bridge::placed_from_play(play, board.is_empty);
        let key = bridge::key_of(&placed);
        if let Some(old) = engine.insert(key.clone(), play.score())
            && old != play.score()
        {
            diff.engine_score_mismatch.push(format!("ugyanaz a lerakás két pontszámmal a motornál: {key:?}"));
        }
        // a játékvezető (teljes szótár + kétjegyű betűk szabálya) minden motorlépést el kell fogadjon, azonos ponttal
        match board.validate_placement(&placed, false) {
            Ok(formed) => {
                let total: i32 = formed.iter().map(|w| w.score as i32).sum::<i32>() + if placed.len() == ai::HAND_SIZE { ai::BONUS_ALL_TILES } else { 0 };
                if total != play.score() {
                    diff.engine_score_mismatch.push(format!("pontszám: a játékvezető {total}, a motor {} — {key:?}", play.score()));
                }
            }
            Err(e) => diff.engine_rejected.push(format!("{e} — {key:?}")),
        }
    }
    diff.engine_moves = engine.len();
    for (key, score) in &bot {
        match engine.get(key) {
            None => diff.only_bot.push(format!("{key:?} ({score} pont)")),
            Some(s) if s != score => diff.score_diff.push(format!("{key:?}: robot {score}, motor {s}")),
            _ => {}
        }
    }
    for (key, score) in &engine {
        if !bot.contains_key(key) {
            diff.only_engine.push(format!("{key:?} ({score} pont)"));
        }
    }
    diff
}

#[derive(Default, Debug)]
pub struct CrossReport {
    pub games: u64,
    pub positions: u64,
    pub identical_positions: u64,
    pub only_bot: u64,
    pub only_engine: u64,
    pub score_diff: u64,
    pub engine_rejected: u64,
    pub engine_score_mismatch: u64,
    pub bot_raw_moves: u64,
    pub bot_nonvocab_moves: u64,
    pub engine_moves: u64,
    pub blank_positions: u64,
    pub roundtrip_failures: u64,
    pub unseen_failures: u64,
    pub bot_ms_max: u128,
    pub examples: Vec<String>,
}

impl CrossReport {
    pub fn clean(&self) -> bool {
        self.only_bot == 0
            && self.only_engine == 0
            && self.score_diff == 0
            && self.engine_rejected == 0
            && self.engine_score_mismatch == 0
            && self.roundtrip_failures == 0
            && self.unseen_failures == 0
    }

    pub fn summary(&self) -> BTreeMap<&'static str, u64> {
        BTreeMap::from([
            ("games", self.games),
            ("positions", self.positions),
            ("identical_positions", self.identical_positions),
            ("only_in_bot", self.only_bot),
            ("only_in_engine", self.only_engine),
            ("score_diff", self.score_diff),
            ("engine_plays_rejected_by_referee", self.engine_rejected),
            ("engine_score_mismatch", self.engine_score_mismatch),
            ("bot_moves_raw", self.bot_raw_moves),
            ("bot_moves_nonvocab_crossword", self.bot_nonvocab_moves),
            ("engine_moves", self.engine_moves),
            ("positions_with_blank_in_rack", self.blank_positions),
            ("roundtrip_failures", self.roundtrip_failures),
            ("unseen_failures", self.unseen_failures),
        ])
    }
}

/// A valódi nem látott készlet (zsák + az ellenfél keze) zsetonindexenként — csak ellenőrzéshez.
fn truth_unseen(game: &Game, me: usize) -> [u8; 39] {
    let mut counts = [0u8; 39];
    for t in &game.bag.tiles {
        counts[t.0 as usize] += 1;
    }
    for (i, p) in game.players.iter().enumerate() {
        if i != me {
            for t in &p.hand {
                counts[t.0 as usize] += 1;
            }
        }
    }
    counts
}

/// `games` önjáték-játék véletlen (a motor legjobb hat lépése közül választott) lépésekkel; minden állásban az összevetés.
pub fn run(env: &Env, games: u64, seed0: u64) -> Result<CrossReport, String> {
    let mut report = CrossReport::default();
    let mut generator = MoveGenerator::new(&env.config);
    for g in 0..games {
        let seed = seed0 + g;
        let mut rng = Rng::seed_from_u64(derive(seed, 0xC805, 0));
        let mut game = Game::with_defaults("crosscheck");
        game.add_player("a", "A")?;
        game.add_player("b", "B")?;
        game.bag.tiles = super::referee::seeded_bag(seed);
        game.start()?;
        report.games += 1;
        for ply in 0..200 {
            if game.finished {
                break;
            }
            let idx = game.current_player_idx;
            let id = game.players[idx].id.clone();
            let hand = game.players[idx].hand.clone();
            let ebrd = bridge::board_to_engine(&game.board, &env.config);
            let erack = bridge::rack_to_engine(&hand);
            // veszteségmentes átalakítások
            let mut back = bridge::rack_from_engine(&erack);
            let mut sorted_hand = hand.clone();
            back.sort();
            sorted_hand.sort();
            if bridge::board_from_engine(&ebrd) != game.board || back != sorted_hand {
                report.roundtrip_failures += 1;
            }
            // a tisztességes nem látott készlet = a valóság = a motor saját számolása
            let honest = bridge::honest_unseen(&game.board, &hand);
            let unseen = Unseen::new(&env.config, &ebrd, &erack);
            if honest != truth_unseen(&game, idx)
                || honest != bridge::unseen_counts_from_engine(&unseen)
                || unseen.len() != game.bag.remaining() + game.players[1 - idx].hand.len()
            {
                report.unseen_failures += 1;
            }
            let diff = compare_position(env, &mut generator, &game.board, &hand);
            report.positions += 1;
            if diff.identical() {
                report.identical_positions += 1;
            }
            report.only_bot += diff.only_bot.len() as u64;
            report.only_engine += diff.only_engine.len() as u64;
            report.score_diff += diff.score_diff.len() as u64;
            report.engine_rejected += diff.engine_rejected.len() as u64;
            report.engine_score_mismatch += diff.engine_score_mismatch.len() as u64;
            report.bot_raw_moves += diff.bot_total as u64;
            report.bot_nonvocab_moves += diff.bot_nonvocab as u64;
            report.engine_moves += diff.engine_moves as u64;
            report.bot_ms_max = report.bot_ms_max.max(diff.bot_ms);
            if hand.iter().any(|t| t.is_blank()) {
                report.blank_positions += 1;
            }
            for (label, list) in [
                ("csak a robotnál", &diff.only_bot),
                ("csak a motornál", &diff.only_engine),
                ("pont eltér", &diff.score_diff),
                ("a játékvezető elutasítja", &diff.engine_rejected),
                ("pontszám a játékvezetővel", &diff.engine_score_mismatch),
            ] {
                for item in list.iter().take(2) {
                    if report.examples.len() < 12 {
                        report.examples.push(format!("{g}. játék, {ply}. lépés — {label}: {item}"));
                    }
                }
            }
            // továbblépés: a motor legjobb hat lépése közül egy véletlen
            let plays = generator.generate(&ebrd, &erack, &env.lexicon).to_vec();
            let mut ranked: Vec<(i32, bridge::Key, Vec<crate::board::Placed>)> = plays
                .iter()
                .map(|p| {
                    let placed = bridge::placed_from_play(p, game.board.is_empty);
                    (p.score(), bridge::key_of(&placed), placed)
                })
                .collect();
            ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            ranked.dedup_by(|a, b| a.1 == b.1);
            if ranked.is_empty() {
                if game.bag.remaining() >= 7 {
                    game.exchange_tiles(&id, &[0, 1, 2])?;
                } else {
                    game.pass_turn(&id, false)?;
                }
                continue;
            }
            let pick = rng.below(ranked.len().min(6) as u64) as usize;
            if let Err(e) = game.place_tiles(&id, &ranked[pick].2) {
                return Err(format!("{g}. játék, {ply}. lépés: a játékvezető elutasította a motor lépését: {e}"));
            }
        }
    }
    Ok(report)
}
