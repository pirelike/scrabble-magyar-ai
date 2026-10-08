//! A párharc két oldala. Mindkettő csak a `View`-t látja (a tábla, a saját kéz, a pontok és a nyilvános számok): az
//! ellenfél kezéhez és a zsák tartalmához nincs hozzáférésük.

use super::leaves::LinearLeaves;
use super::{bridge, derive};
use crate::ai::{self, HAND_SIZE, Vocabulary};
use crate::board::{Board, Placed};
use crate::tiles::Tile;
use pg_scrabble::endgame::EndgameSolver;
use pg_scrabble::eval::{EvalContext, Evaluator, StaticEvaluator, Unseen};
use pg_scrabble::movegen::{MoveGenerator, Play};
use pg_scrabble::prelude::*;
use pg_scrabble::rng::Rng;
use pg_scrabble::sim::{SimOptions, Simulator};
use rand::SeedableRng;
use rand::rngs::StdRng;
use std::sync::Arc;

/// Amit egy oldal a játékból lát.
pub struct View<'a> {
    pub board: &'a Board,
    pub hand: &'a [Tile],
    pub own_score: i32,
    pub opp_score: i32,
    pub bag_remaining: usize,
    pub opp_hand_len: usize,
    pub scoreless_turns: u32,
    pub ply: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Place(Vec<Placed>),
    /// a cserélendő zsetonok indexei a kézben
    Exchange(Vec<usize>),
    Pass,
}

#[derive(Clone, Debug)]
pub struct Decision {
    pub action: Action,
    /// a döntés módja (naplózáshoz): `greedy`, `static`, `sim`, `endgame`, `bot`, `no-move`…
    pub tag: &'static str,
}

pub trait Side: Send {
    fn label(&self) -> String;
    /// `seed`: ehhez a lépéshez származtatott mag (az oldal minden véletlent ebből vesz).
    fn choose(&mut self, view: &View<'_>, seed: u64) -> Result<Decision, String>;
    /// Számlálók a jelentéshez: (végjáték-kereső hívásai, ebből pontos eredményűek).
    fn counters(&self) -> (u64, u64) {
        (0, 0)
    }
}

/// Mindkét oldalon ugyanaz: ha nincs lépés, cserél (ha a zsákban legalább 7 zseton van), különben passzol.
fn no_move(view: &View<'_>, seed: u64, tag: &'static str) -> Decision {
    if view.bag_remaining >= 7 {
        let mut rng = StdRng::seed_from_u64(derive(seed, 0xE7, 0));
        Decision { action: Action::Exchange(ai::choose_exchange(view.hand, &mut rng)), tag }
    } else {
        Decision { action: Action::Pass, tag }
    }
}

// ===================================================================================================
// A robot
// ===================================================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BotMode {
    /// a robot a szókincsére korlátozva: a keresztszavak is a szókincsből valók (a motor ugyanezt a szótárt látja)
    Strict,
    /// a robot az éles működése szerint: a keresztszavak a teljes szótárból is jöhetnek (a motor ezeket nem tudja kirakni)
    Production,
    /// csak a pontszám számít (maradék-értékelés nélkül), a szókincsre korlátozva
    Greedy,
}

pub struct BotSide {
    pub level: u8,
    pub mode: BotMode,
    vocab: Arc<Vocabulary>,
}

impl BotSide {
    pub fn new(level: u8, mode: BotMode, vocab: Arc<Vocabulary>) -> BotSide {
        BotSide { level: level.clamp(ai::MIN_LEVEL, ai::MAX_LEVEL), mode, vocab }
    }
}

/// A keresés időkerete: gyakorlatilag végtelen, a robot a saját 1,5 mp-es védőkorlátja nélkül is befejezi.
const BOT_SEARCH_SECONDS: f64 = 60.0;

impl Side for BotSide {
    fn label(&self) -> String {
        match self.mode {
            BotMode::Strict => format!("bot:{}", self.level),
            BotMode::Production => format!("bot:{}:full", self.level),
            BotMode::Greedy => "bot:greedy".to_string(),
        }
    }

    fn choose(&mut self, view: &View<'_>, seed: u64) -> Result<Decision, String> {
        let mut rng = StdRng::seed_from_u64(derive(seed, 0xB07, 0));
        if self.mode == BotMode::Production {
            let action = ai::choose_action(view.board, view.hand, self.level, view.bag_remaining, &mut rng, &self.vocab, None, BOT_SEARCH_SECONDS);
            return Ok(match action {
                ai::Action::Place { tiles, .. } => Decision { action: Action::Place(tiles), tag: "bot" },
                ai::Action::Exchange { indices } => Decision { action: Action::Exchange(indices), tag: "bot" },
                ai::Action::Pass => Decision { action: Action::Pass, tag: "bot" },
            });
        }
        let mut moves = ai::generate_moves(view.board, view.hand, &self.vocab, true, HAND_SIZE, BOT_SEARCH_SECONDS);
        // a motor szótára a robot szókincse: a szókincsen kívüli keresztszavas lépéseket a robot sem rakhatja le
        moves.retain(|m| m.words.iter().all(|w| self.vocab.contains(w)));
        if self.mode == BotMode::Strict && ai::profile(self.level).sigma.is_some() {
            ai::rate(&mut moves, view.hand, view.bag_remaining);
        }
        let level = if self.mode == BotMode::Greedy { ai::MAX_LEVEL } else { self.level };
        let candidates = ai::ordered_candidates(moves, level, &mut rng);
        match ai::first_valid(&candidates, 1).into_iter().next() {
            Some(mv) => Ok(Decision { action: Action::Place(mv.tiles), tag: "bot" }),
            None => Ok(no_move(view, seed, "bot-no-move")),
        }
    }
}

// ===================================================================================================
// A motor
// ===================================================================================================

/// A motor lépésértékelése.
pub enum Eval {
    /// csak a pontszám
    Greedy,
    /// a motor gyári értékelése (magyar ábécén gyakorlatilag csak a joker és az ismétlés számít)
    Stock(StaticEvaluator),
    /// az önjátékból tanult lineáris maradék-érték
    Leaves(LinearLeaves),
}

impl Evaluator for Eval {
    fn equity(&self, ctx: &EvalContext<'_>, play: &Play) -> f64 {
        match self {
            Eval::Greedy => play.score() as f64,
            Eval::Stock(e) => e.equity(ctx, play),
            Eval::Leaves(e) => e.equity(ctx, play),
        }
    }

    fn leave_value(&self, alphabet: &Alphabet, leave: &Rack) -> f64 {
        match self {
            Eval::Greedy => 0.0,
            Eval::Stock(e) => e.leave_value(alphabet, leave),
            Eval::Leaves(e) => e.leave_value(alphabet, leave),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Search {
    /// egy lépéses: a legjobb értékelésű lépés
    Static,
    /// Monte-Carlo szimuláció a legjobb jelöltekre
    Sim(SimOptions),
}

/// Ennyi zseton (a két kéz együtt) alatt, üres zsák mellett a motor pontos végjáték-keresőt használ.
pub const ENDGAME_MAX_TILES: usize = 9;
pub const DEFAULT_ENDGAME_BUDGET: u64 = 300_000;

pub struct EngineSide {
    label: String,
    config: Arc<GameConfig>,
    lexicon: Arc<Lexicon>,
    eval: Arc<Eval>,
    search: Search,
    endgame_budget: Option<u64>,
    generator: MoveGenerator,
    /// a végjáték-kereső hívásai / ebből pontos eredménnyel (a jelentéshez)
    pub endgame_calls: u64,
    pub endgame_exact: u64,
}

impl EngineSide {
    pub fn new(label: String, config: Arc<GameConfig>, lexicon: Arc<Lexicon>, eval: Arc<Eval>, search: Search, endgame_budget: Option<u64>) -> EngineSide {
        let generator = MoveGenerator::new(&config);
        EngineSide { label, config, lexicon, eval, search, endgame_budget, generator, endgame_calls: 0, endgame_exact: 0 }
    }

    /// A pontszám szerinti legjobb, döntetlennél mag szerint sorsolt elem (a kulcs szerint rendezett halmazból).
    fn pick_best(&self, scored: Vec<(Play, f64)>, board_is_empty: bool, seed: u64) -> Option<Play> {
        let best = scored.iter().map(|(_, e)| *e).fold(f64::NEG_INFINITY, f64::max);
        if !best.is_finite() {
            return None;
        }
        let mut tied: Vec<(bridge::Key, Play)> = scored
            .into_iter()
            .filter(|(_, e)| (best - e).abs() < 1e-9)
            .map(|(p, _)| (bridge::key_of(&bridge::placed_from_play(&p, board_is_empty)), p))
            .collect();
        tied.sort_by(|a, b| a.0.cmp(&b.0));
        tied.dedup_by(|a, b| a.0 == b.0);
        let mut rng = Rng::seed_from_u64(derive(seed, 0x71E, 0));
        let pick = rng.below(tied.len() as u64) as usize;
        Some(tied[pick].1)
    }
}

impl Side for EngineSide {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn counters(&self) -> (u64, u64) {
        (self.endgame_calls, self.endgame_exact)
    }

    fn choose(&mut self, view: &View<'_>, seed: u64) -> Result<Decision, String> {
        let config = self.config.clone();
        let board = bridge::board_to_engine(view.board, &config);
        let rack = bridge::rack_to_engine(view.hand);
        let board_is_empty = view.board.is_empty;
        // a nem látott készlet (zsák + ellenfél keze) a táblából és a saját kézből számolt: az ellenfél kezét nem használja
        let unseen = Unseen::new(&config, &board, &rack);
        if unseen.len() != view.bag_remaining + view.opp_hand_len {
            return Err(format!(
                "a nem látott zsetonok száma ({}) nem egyezik a zsák + ellenfélkéz számával ({})",
                unseen.len(),
                view.bag_remaining + view.opp_hand_len
            ));
        }
        let bag = view.bag_remaining;
        let spread = view.own_score - view.opp_score;

        // pontos végjáték: üres zsák, kevés zseton — az ellenfél keze ilyenkor pontosan a nem látott készlet
        if let Some(budget) = self.endgame_budget
            && bag == 0
            && rack.len() + unseen.len() <= ENDGAME_MAX_TILES
            && !rack.is_empty()
        {
            let mut solver = EndgameSolver::new(&config).with_budget(budget);
            let result = solver.solve(&board, &rack, &unseen.as_rack(), &self.lexicon);
            self.endgame_calls += 1;
            if result.exact {
                self.endgame_exact += 1;
                return Ok(match result.best {
                    Some(play) => Decision { action: Action::Place(bridge::placed_from_play(&play, board_is_empty)), tag: "endgame" },
                    None => Decision { action: Action::Pass, tag: "endgame" },
                });
            }
        }

        let chosen: Option<(Play, &'static str)> = match self.search {
            Search::Static => {
                let ctx = EvalContext { config: &config, board: &board, lexicon: &self.lexicon, rack: &rack, bag_remaining: bag, spread };
                let plays: Vec<Play> = self.generator.generate(&board, &rack, &self.lexicon).to_vec();
                let tag = if matches!(*self.eval, Eval::Greedy) { "greedy" } else { "static" };
                let scored: Vec<(Play, f64)> = plays.into_iter().map(|p| (p, self.eval.equity(&ctx, &p))).collect();
                self.pick_best(scored, board_is_empty, seed).map(|p| (p, tag))
            }
            Search::Sim(mut options) => {
                options.seed = derive(seed, 0x51A, 0);
                let mut simulator = Simulator::new(&config, options);
                let results = simulator.run_with(&board, &rack, &self.lexicon, bag, spread, &*self.eval);
                results.first().map(|r| (r.play, "sim"))
            }
        };
        match chosen {
            Some((play, tag)) => Ok(Decision { action: Action::Place(bridge::placed_from_play(&play, board_is_empty)), tag }),
            None => Ok(no_move(view, seed, "no-move")),
        }
    }
}
