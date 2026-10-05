//! Egy játék lejátszása a saját `Game`-mel (a játékvezető): tükrözött, magból épített zsák, invariánsok, és hiba esetén
//! megszakítás — sosem passzol csendben, és hibát sosem számol győzelemnek.

use super::sides::{Action, Decision, Side, View};
use super::{cpu, derive, mix};
use crate::game::{Game, SCORELESS_TURNS_LIMIT};
use crate::tiles::{TILE_DISTRIBUTION, TileBag};
use pg_scrabble::rng::Rng;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Biztonsági korlát: egy játék legfeljebb ennyi lépésből állhat.
pub const MAX_PLIES: usize = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// valaki kirakta az összes zsetonját (a zsák üres)
    PlayedOut,
    /// 6 egymást követő pont nélküli kör
    Scoreless,
}

#[derive(Clone, Debug)]
pub struct TurnRecord {
    /// 0 / 1: a játékos ülése (0 kezd)
    pub seat: u8,
    /// `P` lerakás, `X` csere, `S` passz
    pub kind: char,
    pub score: i32,
    pub tiles: u8,
    pub tag: &'static str,
    pub cpu_ns: u64,
}

#[derive(Clone, Debug)]
pub struct GameRecord {
    pub pair: u64,
    pub half: u8,
    /// az `a` oldal ülése ebben a játékban
    pub seat_a: u8,
    /// végső pontok ülés szerint
    pub scores: [i32; 2],
    pub end: End,
    pub turns: Vec<TurnRecord>,
    /// a pontkülönbség (a − b) abban a pillanatban, amikor a zsák először kiürült (ha kiürült)
    pub spread_a_at_bag_empty: Option<i32>,
}

impl GameRecord {
    pub fn score_a(&self) -> i32 {
        self.scores[self.seat_a as usize]
    }

    pub fn score_b(&self) -> i32 {
        self.scores[1 - self.seat_a as usize]
    }

    pub fn spread_a(&self) -> i32 {
        self.score_a() - self.score_b()
    }
}

/// A pár zsákja: a kanonikus (rendezett) zsetonlistából a magból Fisher–Yates keverés. A pár mindkét fele ugyanezt kapja.
pub fn seeded_bag(pair_seed: u64) -> Vec<crate::tiles::Tile> {
    let mut tiles = TileBag::new().tiles;
    tiles.sort();
    Rng::seed_from_u64(mix(pair_seed)).shuffle(&mut tiles);
    tiles
}

fn conservation(game: &Game) -> Result<(), String> {
    let on_board = game.board.cells.iter().flatten().filter(|c| c.is_some()).count();
    let in_hands: usize = game.players.iter().map(|p| p.hand.len()).sum();
    let total = on_board + in_hands + game.bag.remaining();
    let expected: usize = TILE_DISTRIBUTION.iter().map(|(_, _, c)| *c as usize).sum();
    if total != expected {
        return Err(format!("a zsetonok száma {total} lett {expected} helyett (tábla {on_board}, kéz {in_hands}, zsák {})", game.bag.remaining()));
    }
    Ok(())
}

/// Lejátssza a pár egyik felét: az `a` oldal a 0. ülésen kezd a `half == 0` játékban, a `half == 1` játékban a `b`.
pub fn play_game(a: &mut dyn Side, b: &mut dyn Side, pair_seed: u64, half: u8) -> Result<GameRecord, String> {
    let ctx = |ply: usize, msg: &str| format!("{}. pár, {}. fél, {}. lépés: {msg}", pair_seed, half, ply);
    let mut game = Game::with_defaults("duel");
    game.add_player("p0", "P0")?;
    game.add_player("p1", "P1")?;
    game.bag.tiles = seeded_bag(pair_seed);
    game.start()?;
    let seat_a: u8 = if half == 0 { 0 } else { 1 };
    let mut turns: Vec<TurnRecord> = Vec::with_capacity(48);
    let mut spread_a_at_bag_empty = None;

    for ply in 0..MAX_PLIES {
        if game.finished {
            break;
        }
        conservation(&game).map_err(|e| ctx(ply, &e))?;
        let idx = game.current_player_idx;
        let id = game.players[idx].id.clone();
        let hand = game.players[idx].hand.clone();
        let view = View {
            board: &game.board,
            hand: &hand,
            own_score: game.players[idx].score,
            opp_score: game.players[1 - idx].score,
            bag_remaining: game.bag.remaining(),
            opp_hand_len: game.players[1 - idx].hand.len(),
            scoreless_turns: game.scoreless_turns,
            ply,
        };
        let is_a = idx as u8 == seat_a;
        let seed = derive(pair_seed, half as u64 * 1000 + ply as u64, idx as u64);
        let side: &mut dyn Side = if is_a { &mut *a } else { &mut *b };
        let started = cpu::thread_cpu_ns();
        let decision = catch_unwind(AssertUnwindSafe(|| side.choose(&view, seed)))
            .map_err(|_| ctx(ply, &format!("{} döntése pánikolt", side.label())))?
            .map_err(|e| ctx(ply, &format!("{} döntése hibás: {e}", side.label())))?;
        let cpu_ns = cpu::thread_cpu_ns().saturating_sub(started);
        let Decision { action, tag } = decision;
        let (kind, score, tiles) = match &action {
            Action::Place(placed) => {
                let (_, score) = game
                    .place_tiles(&id, placed)
                    .map_err(|e| ctx(ply, &format!("a(z) {} lépését a játékvezető elutasította: {e}", side_label(is_a, &*a, &*b))))?;
                ('P', score, placed.len() as u8)
            }
            Action::Exchange(indices) => {
                let list: Vec<i64> = indices.iter().map(|i| *i as i64).collect();
                game.exchange_tiles(&id, &list)
                    .map_err(|e| ctx(ply, &format!("a(z) {} cseréjét a játékvezető elutasította: {e}", side_label(is_a, &*a, &*b))))?;
                // a játék a visszatett zsetonokat véletlenül keveri: a magból újrakeverjük, hogy a futás megismételhető legyen
                game.bag.tiles.sort();
                Rng::seed_from_u64(derive(pair_seed, 0x5AB + half as u64, ply as u64)).shuffle(&mut game.bag.tiles);
                ('X', 0, indices.len() as u8)
            }
            Action::Pass => {
                game.pass_turn(&id, false).map_err(|e| ctx(ply, &format!("a(z) {} passzát a játékvezető elutasította: {e}", side_label(is_a, &*a, &*b))))?;
                ('S', 0, 0)
            }
        };
        turns.push(TurnRecord { seat: idx as u8, kind, score, tiles, tag, cpu_ns });
        if spread_a_at_bag_empty.is_none() && game.bag.remaining() == 0 && !game.finished {
            let sa = game.players[seat_a as usize].score;
            let sb = game.players[1 - seat_a as usize].score;
            spread_a_at_bag_empty = Some(sa - sb);
        }
    }
    if !game.finished {
        return Err(ctx(MAX_PLIES, "a játék nem ért véget"));
    }
    let end = if game.players.iter().any(|p| p.hand.is_empty()) {
        End::PlayedOut
    } else if game.scoreless_turns >= SCORELESS_TURNS_LIMIT {
        End::Scoreless
    } else {
        return Err(ctx(turns.len(), "ismeretlen játékvége"));
    };
    let scores = [game.players[0].score, game.players[1].score];
    Ok(GameRecord { pair: pair_seed, half, seat_a, scores, end, turns, spread_a_at_bag_empty })
}

fn side_label(is_a: bool, a: &dyn Side, b: &dyn Side) -> String {
    if is_a { a.label() } else { b.label() }
}
