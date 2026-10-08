use crate::board::Board;
use crate::lexicon::Lexicon;
use crate::movegen::{MoveGenerator, Play};
use crate::rack::Rack;
use crate::rng::Rng;
use crate::rules::GameConfig;
use crate::tile::Square;
use alloc::vec::Vec;
use std::collections::HashMap;

#[derive(Clone, PartialEq, Debug)]
pub struct EndgameResult {
    pub best: Option<Play>,
    pub spread: i32,
    pub nodes: u64,
    pub exact: bool,
}

pub struct EndgameSolver {
    config: GameConfig,
    generator: MoveGenerator,
    table: HashMap<u64, Entry>,
    zobrist: Zobrist,
    nodes: u64,
    budget: u64,
    exhausted: bool,
    max_depth: u32,
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    depth: u32,
    value: i32,
    kind: Bound,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Bound {
    Exact,
    Lower,
    Upper,
}

impl EndgameSolver {
    pub fn new(config: &GameConfig) -> EndgameSolver {
        EndgameSolver {
            generator: MoveGenerator::new(config),
            zobrist: Zobrist::new(config),
            config: config.clone(),
            table: HashMap::new(),
            nodes: 0,
            budget: 8_000_000,
            exhausted: false,
            max_depth: 32,
        }
    }

    pub fn with_budget(mut self, nodes: u64) -> EndgameSolver {
        self.budget = nodes;
        self
    }

    pub fn solve(
        &mut self,
        board: &Board,
        mover: &Rack,
        opponent: &Rack,
        lexicon: &Lexicon,
    ) -> EndgameResult {
        self.nodes = 0;
        self.exhausted = false;
        self.table.clear();

        let mut work = board.clone();
        let mut best: Option<Play> = None;
        let mut best_value = i32::MIN;

        let pass_value = -self.search(
            &mut work,
            opponent,
            mover,
            lexicon,
            1,
            -i32::MAX,
            i32::MAX,
            1,
        );
        best_value = best_value.max(pass_value);

        let plays = self.ordered_plays(&work, mover, lexicon);
        for play in plays {
            let Some(leave) = play.leave(mover) else {
                continue;
            };
            play.apply(&mut work);
            let value = if leave.is_empty() {
                play.score() + 2 * opponent.value(&self.config.alphabet)
            } else {
                play.score()
                    - self.search(
                        &mut work,
                        opponent,
                        &leave,
                        lexicon,
                        0,
                        -i32::MAX,
                        i32::MAX,
                        1,
                    )
            };
            play.undo(&mut work);

            if value > best_value {
                best_value = value;
                best = Some(play);
            }
        }

        EndgameResult {
            best,
            spread: best_value,
            nodes: self.nodes,
            exact: !self.exhausted,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn search(
        &mut self,
        board: &mut Board,
        mover: &Rack,
        opponent: &Rack,
        lexicon: &Lexicon,
        passes: u32,
        mut alpha: i32,
        beta: i32,
        depth: u32,
    ) -> i32 {
        self.nodes += 1;
        if self.nodes >= self.budget || depth >= self.max_depth {
            self.exhausted = true;

            return opponent.value(&self.config.alphabet) - mover.value(&self.config.alphabet);
        }
        if mover.is_empty() {
            return opponent.value(&self.config.alphabet) * 2;
        }
        if passes >= 2 {
            return opponent.value(&self.config.alphabet) - mover.value(&self.config.alphabet);
        }

        let key = self.zobrist.key(board, mover, opponent, passes);
        let alpha_orig = alpha;
        if let Some(entry) = self.table.get(&key) {
            if entry.depth >= depth {
                match entry.kind {
                    Bound::Exact => return entry.value,
                    Bound::Lower if entry.value >= beta => return entry.value,
                    Bound::Upper if entry.value <= alpha => return entry.value,
                    _ => {}
                }
            }
        }

        let mut best = -self.search(
            board,
            opponent,
            mover,
            lexicon,
            passes + 1,
            -beta,
            -alpha,
            depth + 1,
        );
        alpha = alpha.max(best);

        if alpha < beta {
            let plays = self.ordered_plays(board, mover, lexicon);
            for play in plays {
                let Some(leave) = play.leave(mover) else {
                    continue;
                };
                play.apply(board);
                let value = if leave.is_empty() {
                    play.score() + 2 * opponent.value(&self.config.alphabet)
                } else {
                    play.score()
                        - self.search(
                            board,
                            opponent,
                            &leave,
                            lexicon,
                            0,
                            -beta,
                            -alpha,
                            depth + 1,
                        )
                };
                play.undo(board);

                if value > best {
                    best = value;
                }
                alpha = alpha.max(best);
                if alpha >= beta {
                    break;
                }
            }
        }

        let kind = if best <= alpha_orig {
            Bound::Upper
        } else if best >= beta {
            Bound::Lower
        } else {
            Bound::Exact
        };
        self.table.insert(
            key,
            Entry {
                depth,
                value: best,
                kind,
            },
        );
        best
    }

    fn ordered_plays(&mut self, board: &Board, rack: &Rack, lexicon: &Lexicon) -> Vec<Play> {
        let mut plays = self.generator.generate(board, rack, lexicon).to_vec();
        plays.sort_unstable_by_key(|play| core::cmp::Reverse((play.score(), play.tiles_used())));
        plays
    }

    #[inline]
    pub fn nodes(&self) -> u64 {
        self.nodes
    }
}

/// Zobrist keys for the transposition table.
///
/// Upstream `scrabble` 0.1.0 drew its keys from one random stream sized for 31
/// letters. To keep every key (and so every table hit, collision and node
/// count) identical for alphabets of up to 31 letters, that stream is still
/// drawn first, in the same order and with the same shape: board squares
/// (`LEGACY_STATES` per cell), then the per-side rack keys (`LEGACY_CODES`
/// codes, the blank being code 31 there), then the pass counters. Keys for the
/// letters that did not exist back then (31..`MAX_LETTERS`) come from the same
/// generator afterwards. The values are then laid out in flat tables indexed
/// by the *new* numbering, so a lookup needs no branch.
struct Zobrist {
    /// `squares[cell * SQUARE_STATES + state]`, state 0 (empty) is unused.
    squares: Vec<u64>,
    /// `racks[side][code * MAX_RACK_COUNT + n]`, indexed by the new tile code.
    racks: [Vec<u64>; 2],
    passes: [u64; 3],
    cells: usize,
}

const LEGACY_LETTERS: usize = crate::tile::LEGACY_MAX_LETTERS;
const LEGACY_STATES: usize = 1 + 2 * LEGACY_LETTERS;
const LEGACY_CODES: usize = LEGACY_LETTERS + 1;
const WIDE_LETTERS: usize = crate::tile::MAX_LETTERS - LEGACY_LETTERS;

const SQUARE_STATES: usize = 1 + 2 * crate::tile::MAX_LETTERS;

const MAX_RACK_COUNT: usize = 8;

impl Zobrist {
    fn new(config: &GameConfig) -> Zobrist {
        let mut rng = Rng::seed_from_u64(0x5CAB_B1E5_1234_9ABCu64);
        let cells = config.layout.cells();

        // 1. the stream exactly as upstream drew it
        let mut legacy_squares = Vec::with_capacity(cells * LEGACY_STATES);
        for _ in 0..cells * LEGACY_STATES {
            legacy_squares.push(rng.next_u64());
        }
        let mut legacy_racks = [Vec::new(), Vec::new()];
        for side in &mut legacy_racks {
            for _ in 0..LEGACY_CODES * MAX_RACK_COUNT {
                side.push(rng.next_u64());
            }
        }
        let passes = [rng.next_u64(), rng.next_u64(), rng.next_u64()];

        // 2. keys for the letters upstream could not represent
        let mut wide_squares = Vec::with_capacity(cells * 2 * WIDE_LETTERS);
        for _ in 0..cells * 2 * WIDE_LETTERS {
            wide_squares.push(rng.next_u64());
        }
        let mut wide_racks = [Vec::new(), Vec::new()];
        for side in &mut wide_racks {
            for _ in 0..WIDE_LETTERS * MAX_RACK_COUNT {
                side.push(rng.next_u64());
            }
        }

        // 3. flatten into the new numbering
        let mut squares = vec![0u64; cells * SQUARE_STATES];
        for cell in 0..cells {
            for letter in 0..crate::tile::MAX_LETTERS {
                for blank in [false, true] {
                    let state = 1 + letter + if blank { crate::tile::MAX_LETTERS } else { 0 };
                    squares[cell * SQUARE_STATES + state] = if letter < LEGACY_LETTERS {
                        let old = 1 + letter + if blank { LEGACY_LETTERS } else { 0 };
                        legacy_squares[cell * LEGACY_STATES + old]
                    } else {
                        let w = letter - LEGACY_LETTERS + if blank { WIDE_LETTERS } else { 0 };
                        wide_squares[cell * 2 * WIDE_LETTERS + w]
                    };
                }
            }
        }
        let mut racks = [Vec::new(), Vec::new()];
        for side in 0..2 {
            let mut flat = vec![0u64; crate::tile::TILE_CODES * MAX_RACK_COUNT];
            for code in 0..crate::tile::TILE_CODES {
                for n in 0..MAX_RACK_COUNT {
                    flat[code * MAX_RACK_COUNT + n] = if code == crate::tile::BLANK_CODE as usize {
                        legacy_racks[side][LEGACY_LETTERS * MAX_RACK_COUNT + n]
                    } else if code < LEGACY_LETTERS {
                        legacy_racks[side][code * MAX_RACK_COUNT + n]
                    } else {
                        wide_racks[side][(code - LEGACY_LETTERS) * MAX_RACK_COUNT + n]
                    };
                }
            }
            racks[side] = flat;
        }
        Zobrist {
            squares,
            racks,
            passes,
            cells,
        }
    }

    #[inline]
    fn square_key(&self, cell: usize, square: Square) -> u64 {
        match square.index() {
            None => 0,
            Some(letter) => {
                let state = 1
                    + letter as usize
                    + if square.is_blank() {
                        crate::tile::MAX_LETTERS
                    } else {
                        0
                    };
                self.squares[cell * SQUARE_STATES + state]
            }
        }
    }

    #[inline]
    fn rack_key(&self, side: usize, code: usize, n: usize) -> u64 {
        self.racks[side][code * MAX_RACK_COUNT + n.min(MAX_RACK_COUNT - 1)]
    }

    fn key(&self, board: &Board, mover: &Rack, opponent: &Rack, passes: u32) -> u64 {
        let mut h = self.passes[(passes as usize).min(2)];
        for (i, square) in board.squares().iter().enumerate() {
            h ^= self.square_key(i, *square);
        }
        debug_assert_eq!(board.squares().len(), self.cells);
        for (side, rack) in [mover, opponent].iter().enumerate() {
            // only tile kinds that are present contribute; the rack's letter
            // mask lists them, so there is no need to scan every tile code
            let counts = rack.counts();
            let mut present = rack.mask();
            while present != 0 {
                let code = present.trailing_zeros() as usize;
                present &= present - 1;
                h ^= self.rack_key(side, code, counts[code] as usize);
            }
            let blanks = counts[crate::tile::BLANK_CODE as usize];
            if blanks > 0 {
                h ^= self.rack_key(side, crate::tile::BLANK_CODE as usize, blanks as usize);
            }
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Alphabet;

    fn lexicon() -> Lexicon {
        Lexicon::from_words(
            &Alphabet::english(),
            [
                "AT", "ATE", "CAT", "CATS", "EAT", "EATS", "SAT", "SEAT", "SEATS", "TA", "TAE",
                "TAN", "TAB", "NAB", "AB", "BA", "AN", "NA", "AX", "XI", "ZA", "QI", "JO",
            ],
        )
        .unwrap()
    }

    fn config() -> GameConfig {
        GameConfig::standard()
    }

    fn board_with(config: &GameConfig, row: usize, col: usize, word: &str) -> Board {
        let mut b = Board::new(&config.layout);
        for (i, ch) in word.chars().enumerate() {
            let (letter, _) = config.alphabet.parse_char(ch).unwrap();
            b.set(row, col + i, Square::letter(letter));
        }
        b
    }

    #[test]
    fn going_out_collects_twice_the_opponents_rack() {
        let config = config();
        let lex = lexicon();
        let board = board_with(&config, 7, 6, "CAT");
        let mover = Rack::parse(&config.alphabet, "S").unwrap();

        let opponent = Rack::parse(&config.alphabet, "QZ").unwrap();

        let mut solver = EndgameSolver::new(&config);
        let result = solver.solve(&board, &mover, &opponent, &lex);

        let play = result.best.expect("playing the S goes out");
        assert_eq!(play.tiles_used(), 1);

        assert_eq!(result.spread, play.score() + 40);
        assert!(result.exact);
    }

    #[test]
    fn a_smaller_play_that_goes_out_beats_a_bigger_one_that_does_not() {
        let config = config();
        let lex = lexicon();
        let board = board_with(&config, 7, 6, "CAT");

        let mover = Rack::parse(&config.alphabet, "S").unwrap();
        let opponent = Rack::parse(&config.alphabet, "QZ").unwrap();
        let mut solver = EndgameSolver::new(&config);
        let out = solver.solve(&board, &mover, &opponent, &lex).spread;

        let stuck = Rack::parse(&config.alphabet, "QZ").unwrap();
        let light = Rack::parse(&config.alphabet, "S").unwrap();
        let held = solver.solve(&board, &stuck, &light, &lex).spread;
        assert!(
            out > held,
            "going out ({out}) must beat being stuck with the same tiles ({held})"
        );
    }

    #[test]
    fn a_hopeless_position_still_returns_the_least_bad_line() {
        let config = config();
        let lex = lexicon();
        let board = Board::new(&config.layout);

        let mover = Rack::parse(&config.alphabet, "VVV").unwrap();
        let opponent = Rack::parse(&config.alphabet, "A").unwrap();
        let mut solver = EndgameSolver::new(&config);
        let result = solver.solve(&board, &mover, &opponent, &lex);

        assert!(result.best.is_none(), "there is nothing to play");

        assert_eq!(result.spread, 1 - 12);
        assert!(result.exact);
    }

    #[test]
    fn the_solver_is_deterministic() {
        let config = config();
        let lex = lexicon();
        let board = board_with(&config, 7, 6, "CAT");
        let mover = Rack::parse(&config.alphabet, "SE").unwrap();
        let opponent = Rack::parse(&config.alphabet, "AB").unwrap();
        let mut solver = EndgameSolver::new(&config);
        let first = solver.solve(&board, &mover, &opponent, &lex);
        let second = solver.solve(&board, &mover, &opponent, &lex);
        assert_eq!(first.spread, second.spread);
        assert_eq!(first.best, second.best);
        assert_eq!(
            first.nodes, second.nodes,
            "the node count must be reproducible"
        );
    }

    #[test]
    fn a_tiny_budget_truncates_rather_than_hanging() {
        let config = config();
        let lex = lexicon();
        let board = board_with(&config, 7, 6, "CAT");
        let mover = Rack::parse(&config.alphabet, "SEAT").unwrap();
        let opponent = Rack::parse(&config.alphabet, "ABTN").unwrap();
        let mut solver = EndgameSolver::new(&config).with_budget(50);
        let result = solver.solve(&board, &mover, &opponent, &lex);
        assert!(!result.exact, "50 nodes cannot solve this");
        assert!(result.nodes >= 50);
    }

    #[test]
    fn transposition_keys_separate_different_positions() {
        let config = config();
        let z = Zobrist::new(&config);
        let empty = Board::new(&config.layout);
        let played = board_with(&config, 7, 6, "CAT");
        let a = Rack::parse(&config.alphabet, "AB").unwrap();
        let b = Rack::parse(&config.alphabet, "CD").unwrap();

        assert_ne!(z.key(&empty, &a, &b, 0), z.key(&played, &a, &b, 0));
        assert_ne!(z.key(&empty, &a, &b, 0), z.key(&empty, &b, &a, 0));
        assert_ne!(z.key(&empty, &a, &b, 0), z.key(&empty, &a, &b, 1));
        assert_eq!(z.key(&played, &a, &b, 0), z.key(&played, &a, &b, 0));
    }

    #[test]
    fn a_blank_on_the_board_hashes_differently_from_the_letter() {
        let config = config();
        let z = Zobrist::new(&config);
        let mut plain = Board::new(&config.layout);
        plain.set(7, 7, Square::letter(4));
        let mut blank = Board::new(&config.layout);
        blank.set(7, 7, Square::blank_letter(4));
        let r = Rack::new();
        assert_ne!(z.key(&plain, &r, &r, 0), z.key(&blank, &r, &r, 0));
    }

    /// The Zobrist code exactly as upstream `scrabble` 0.1.0 wrote it
    /// (31-letter numbering, blank = code 31), used as the oracle for the
    /// "keys are unchanged for alphabets that fit the old limit" guarantee.
    struct LegacyZobrist {
        squares: Vec<u64>,
        racks: [Vec<u64>; 2],
        passes: [u64; 3],
    }

    impl LegacyZobrist {
        const MAX_LETTERS: usize = 31;
        const SQUARE_STATES: usize = 1 + 2 * Self::MAX_LETTERS;
        const TILE_CODES: usize = Self::MAX_LETTERS + 1;

        fn new(cells: usize) -> LegacyZobrist {
            let mut rng = Rng::seed_from_u64(0x5CAB_B1E5_1234_9ABCu64);
            let mut squares = Vec::new();
            for _ in 0..cells * Self::SQUARE_STATES {
                squares.push(rng.next_u64());
            }
            let mut racks = [Vec::new(), Vec::new()];
            for side in &mut racks {
                for _ in 0..Self::TILE_CODES * MAX_RACK_COUNT {
                    side.push(rng.next_u64());
                }
            }
            LegacyZobrist {
                squares,
                racks,
                passes: [rng.next_u64(), rng.next_u64(), rng.next_u64()],
            }
        }

        fn key(&self, board: &Board, mover: &Rack, opponent: &Rack, passes: u32) -> u64 {
            let mut h = self.passes[(passes as usize).min(2)];
            for (i, square) in board.squares().iter().enumerate() {
                let state = match square.index() {
                    None => 0,
                    Some(letter) => {
                        1 + letter as usize
                            + if square.is_blank() {
                                Self::MAX_LETTERS
                            } else {
                                0
                            }
                    }
                };
                if state != 0 {
                    h ^= self.squares[i * Self::SQUARE_STATES + state];
                }
            }
            for (side, rack) in [mover, opponent].iter().enumerate() {
                for code in 0..=LEGACY_LETTERS {
                    // the new blank code (63) is the old blank code (31)
                    let tile_code = if code == LEGACY_LETTERS {
                        crate::tile::BLANK_CODE as usize
                    } else {
                        code
                    };
                    let n = rack.counts()[tile_code];
                    if n > 0 {
                        let n = (n as usize).min(MAX_RACK_COUNT - 1);
                        h ^= self.racks[side][code * MAX_RACK_COUNT + n];
                    }
                }
            }
            h
        }
    }

    #[test]
    fn keys_are_identical_to_upstream_for_alphabets_that_fit_the_old_limit() {
        let config = config();
        let new = Zobrist::new(&config);
        let old = LegacyZobrist::new(config.layout.cells());
        let mut rng = Rng::seed_from_u64(77);
        for _ in 0..300 {
            let mut board = Board::new(&config.layout);
            for _ in 0..rng.below(40) {
                let cell = rng.below(config.layout.cells() as u64) as usize;
                let letter = rng.below(30) as u8; // legacy alphabets: up to 30 letters
                let sq = if rng.below(4) == 0 {
                    Square::blank_letter(letter)
                } else {
                    Square::letter(letter)
                };
                board.set(cell / config.width(), cell % config.width(), sq);
            }
            let mut mover = Rack::new();
            let mut opp = Rack::new();
            for _ in 0..rng.below(8) {
                let t = if rng.below(6) == 0 {
                    crate::tile::Tile::BLANK
                } else {
                    crate::tile::Tile::letter(rng.below(30) as u8)
                };
                mover.add(t);
            }
            for _ in 0..rng.below(8) {
                opp.add(crate::tile::Tile::letter(rng.below(30) as u8));
            }
            let passes = rng.below(4) as u32;
            assert_eq!(
                new.key(&board, &mover, &opp, passes),
                old.key(&board, &mover, &opp, passes),
                "Zobrist key changed for a legacy-sized alphabet"
            );
        }
    }

    #[test]
    fn every_square_state_and_rack_slot_has_its_own_key() {
        use crate::tile::{Tile, BLANK_CODE, MAX_LETTERS};
        let config = config();
        let z = Zobrist::new(&config);
        let mut seen = std::collections::HashSet::new();
        for cell in 0..config.layout.cells() {
            for letter in 0..MAX_LETTERS as u8 {
                for sq in [Square::letter(letter), Square::blank_letter(letter)] {
                    let k = z.square_key(cell, sq);
                    assert_ne!(k, 0);
                    assert!(
                        seen.insert(k),
                        "square key collision at cell {cell} letter {letter}"
                    );
                }
            }
            assert_eq!(z.square_key(cell, Square::EMPTY), 0);
        }
        let mut rack_keys = std::collections::HashSet::new();
        for side in 0..2 {
            for code in 0..=BLANK_CODE as usize {
                for n in 1..MAX_RACK_COUNT {
                    assert!(
                        rack_keys.insert(z.rack_key(side, code, n)),
                        "rack key collision"
                    );
                }
            }
        }
        // the highest letter must differ from the blank and from its neighbour
        let mut hi = Rack::new();
        hi.add(Tile::letter(MAX_LETTERS as u8 - 1));
        let mut blank = Rack::new();
        blank.add(Tile::BLANK);
        let empty = Board::new(&config.layout);
        let r = Rack::new();
        assert_ne!(z.key(&empty, &hi, &r, 0), z.key(&empty, &blank, &r, 0));
    }
}
