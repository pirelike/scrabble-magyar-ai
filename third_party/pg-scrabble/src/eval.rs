use crate::board::Board;
use crate::lexicon::Lexicon;
use crate::movegen::Play;
use crate::rack::Rack;
use crate::rules::{Alphabet, GameConfig};
use crate::tile::TILE_CODES;
use alloc::vec::Vec;

#[cfg(feature = "std")]
use std::collections::HashMap;

pub struct EvalContext<'a> {
    pub config: &'a GameConfig,
    pub board: &'a Board,
    pub lexicon: &'a Lexicon,
    pub rack: &'a Rack,
    pub bag_remaining: usize,
    pub spread: i32,
}

pub trait Evaluator {
    fn equity(&self, ctx: &EvalContext<'_>, play: &Play) -> f64;

    fn pass_equity(&self, ctx: &EvalContext<'_>) -> f64 {
        -self.leave_value(&ctx.config.alphabet, ctx.rack) * 0.5
    }

    fn exchange_equity(&self, ctx: &EvalContext<'_>, tiles: &Rack) -> f64 {
        let mut leave = *ctx.rack;
        if !leave.remove_all(tiles) {
            return f64::NEG_INFINITY;
        }
        self.leave_value(&ctx.config.alphabet, &leave)
    }

    fn leave_value(&self, alphabet: &Alphabet, leave: &Rack) -> f64;
}

#[cfg(feature = "std")]
#[derive(Clone, Default, Debug)]
pub struct LeaveTable {
    values: HashMap<u64, f32>,
}

#[cfg(feature = "std")]
impl LeaveTable {
    pub fn new() -> LeaveTable {
        LeaveTable::default()
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn insert(&mut self, leave: &Rack, value: f32) {
        self.values.insert(leave.key(), value);
    }

    #[inline]
    pub fn get(&self, leave: &Rack) -> Option<f32> {
        self.values.get(&leave.key()).copied()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + self.values.len() * 12);
        out.extend_from_slice(b"SLV1");
        out.extend_from_slice(&(self.values.len() as u32).to_le_bytes());

        let mut entries: Vec<(&u64, &f32)> = self.values.iter().collect();
        entries.sort_unstable_by_key(|(k, _)| **k);
        for (key, value) in entries {
            out.extend_from_slice(&key.to_le_bytes());
            out.extend_from_slice(&value.to_le_bytes());
        }
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<LeaveTable> {
        if bytes.len() < 8 || &bytes[..4] != b"SLV1" {
            return None;
        }
        let count = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
        if bytes.len() < 8 + count * 12 {
            return None;
        }
        let mut values = HashMap::with_capacity(count);
        for i in 0..count {
            let at = 8 + i * 12;
            let mut k = [0u8; 8];
            k.copy_from_slice(&bytes[at..at + 8]);
            let v =
                f32::from_le_bytes([bytes[at + 8], bytes[at + 9], bytes[at + 10], bytes[at + 11]]);
            values.insert(u64::from_le_bytes(k), v);
        }
        Some(LeaveTable { values })
    }
}

const ENGLISH_LEAVE: [f64; 26] = [
    0.5, -1.6, -0.4, 0.3, 1.6, -1.6, -1.9, 0.4, -0.4, -1.6, -0.6, -0.3, 0.1, 0.5, -0.5, -0.7, -7.0,
    1.0, 7.0, 0.7, -2.2, -5.2, -3.2, 2.4, -0.8, 1.9,
];

const BLANK_LEAVE: f64 = 24.0;

const DUPLICATE_PENALTY: f64 = -1.2;

const BALANCE_PENALTY: f64 = -1.6;

const IDEAL_VOWEL_SHARE: f64 = 0.4;

pub fn heuristic_leave(alphabet: &Alphabet, leave: &Rack) -> f64 {
    if leave.is_empty() {
        return 0.0;
    }
    let english = alphabet.len() == 26 && alphabet.display(0) == "A";
    let counts = leave.counts();

    let mut value = BLANK_LEAVE * leave.blanks() as f64;
    let mut vowels = 0.0;
    let mut consonants = 0.0;

    for letter in 0..alphabet.len() {
        let n = counts[letter] as usize;
        if n == 0 {
            continue;
        }
        if english {
            value += ENGLISH_LEAVE[letter];

            value += DUPLICATE_PENALTY * (n - 1) as f64 * (n as f64 / 2.0);
            if is_english_vowel(letter as u8) {
                vowels += n as f64;
            } else {
                consonants += n as f64;
            }
        } else {
            value += DUPLICATE_PENALTY * (n - 1) as f64;
        }
    }

    let letters = vowels + consonants;
    if letters > 0.0 {
        let ideal = letters * IDEAL_VOWEL_SHARE;
        value += BALANCE_PENALTY * (vowels - ideal).abs();

        if letters >= 3.0 && (vowels == 0.0 || consonants == 0.0) {
            value -= 4.0;
        }
    }

    if english {
        let q = counts[16] > 0;
        let u = counts[20] > 0;
        if q && !u && leave.blanks() == 0 {
            value -= 6.0;
        }
    }

    value
}

#[inline]
fn is_english_vowel(letter: u8) -> bool {
    matches!(letter, 0 | 4 | 8 | 14 | 20)
}

#[derive(Default)]
pub struct StaticEvaluator {
    #[cfg(feature = "std")]
    pub leaves: Option<LeaveTable>,
    pub score_weight: f64,
}

impl StaticEvaluator {
    pub fn new() -> StaticEvaluator {
        StaticEvaluator {
            #[cfg(feature = "std")]
            leaves: None,
            score_weight: 1.0,
        }
    }

    #[cfg(feature = "std")]
    pub fn with_leaves(leaves: LeaveTable) -> StaticEvaluator {
        StaticEvaluator {
            leaves: Some(leaves),
            score_weight: 1.0,
        }
    }

    pub fn greedy() -> StaticEvaluator {
        StaticEvaluator {
            #[cfg(feature = "std")]
            leaves: None,
            score_weight: f64::INFINITY,
        }
    }
}

impl Evaluator for StaticEvaluator {
    fn equity(&self, ctx: &EvalContext<'_>, play: &Play) -> f64 {
        let score = play.score() as f64;
        if self.score_weight.is_infinite() {
            return score;
        }

        let Some(leave) = play.leave(ctx.rack) else {
            return f64::NEG_INFINITY;
        };

        if ctx.bag_remaining == 0 {
            let stuck = leave.value(&ctx.config.alphabet) as f64;
            let out_bonus = if leave.is_empty() { 10.0 } else { 0.0 };
            return score * self.score_weight - stuck + out_bonus;
        }

        score * self.score_weight + self.leave_value(&ctx.config.alphabet, &leave)
    }

    fn leave_value(&self, alphabet: &Alphabet, leave: &Rack) -> f64 {
        #[cfg(feature = "std")]
        if let Some(table) = &self.leaves {
            if let Some(v) = table.get(leave) {
                return v as f64;
            }
        }
        heuristic_leave(alphabet, leave)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Unseen {
    counts: [u8; TILE_CODES],
    total: usize,
}

impl Unseen {
    pub fn new(config: &GameConfig, board: &Board, own_rack: &Rack) -> Unseen {
        let mut counts = *config.distribution.counts();
        let mut remove = |code: usize| {
            counts[code] = counts[code].saturating_sub(1);
        };
        for square in board.squares() {
            if square.is_occupied() {
                if square.is_blank() {
                    remove(crate::tile::BLANK_CODE as usize);
                } else {
                    remove(square.index_unchecked() as usize);
                }
            }
        }
        for (code, n) in own_rack.counts().iter().enumerate() {
            for _ in 0..*n {
                remove(code);
            }
        }
        let total = counts.iter().map(|&c| c as usize).sum();
        Unseen { counts, total }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.total
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    #[inline]
    pub fn counts(&self) -> &[u8; TILE_CODES] {
        &self.counts
    }

    pub fn as_rack(&self) -> Rack {
        let mut rack = Rack::new();
        for (code, &n) in self.counts.iter().enumerate() {
            for _ in 0..n {
                rack.add(crate::tile::Tile::from_code(code as u8));
            }
        }
        rack
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Alphabet;

    fn alpha() -> Alphabet {
        Alphabet::english()
    }

    fn leave(s: &str) -> Rack {
        Rack::parse(&alpha(), s).unwrap()
    }

    #[test]
    fn an_empty_leave_is_worth_nothing() {
        assert_eq!(heuristic_leave(&alpha(), &Rack::new()), 0.0);
    }

    #[test]
    fn blanks_dominate_every_other_consideration() {
        let with = heuristic_leave(&alpha(), &leave("?"));
        let without = heuristic_leave(&alpha(), &leave("V"));
        assert!(
            with > 20.0,
            "a blank should be worth around 25 points, got {with}"
        );
        assert!(with > without + 25.0);
    }

    #[test]
    fn an_s_beats_an_ordinary_consonant() {
        assert!(heuristic_leave(&alpha(), &leave("S")) > heuristic_leave(&alpha(), &leave("N")));
        assert!(heuristic_leave(&alpha(), &leave("N")) > heuristic_leave(&alpha(), &leave("V")));
    }

    #[test]
    fn balanced_leaves_beat_lopsided_ones() {
        let balanced = heuristic_leave(&alpha(), &leave("AENRT"));
        let vowels = heuristic_leave(&alpha(), &leave("AEIOU"));
        let consonants = heuristic_leave(&alpha(), &leave("BCDFG"));
        assert!(
            balanced > vowels && balanced > consonants,
            "AENRT {balanced} should beat AEIOU {vowels} and BCDFG {consonants}"
        );
    }

    #[test]
    fn duplicates_cost_something() {
        let single = heuristic_leave(&alpha(), &leave("E"));
        let double = heuristic_leave(&alpha(), &leave("EE"));
        let triple = heuristic_leave(&alpha(), &leave("EEE"));
        assert!(
            double < 2.0 * single,
            "a second E is worth less than the first"
        );
        assert!(triple < double, "a third E is worse still");
    }

    #[test]
    fn a_q_without_a_u_is_punished() {
        let stuck = heuristic_leave(&alpha(), &leave("QI"));
        let rescued = heuristic_leave(&alpha(), &leave("QU"));
        assert!(rescued > stuck, "QU {rescued} should beat QI {stuck}");
    }

    #[test]
    fn leave_values_survive_a_non_english_alphabet() {
        let small = Alphabet::new(
            "toy",
            alloc::vec!["X".into(), "Y".into(), "Z".into()],
            alloc::vec![1, 1, 1],
        );
        let mut r = Rack::new();
        r.add(crate::tile::Tile::letter(0));
        r.add(crate::tile::Tile::letter(0));

        assert!(heuristic_leave(&small, &r) < 0.0);
    }

    #[test]
    fn unseen_accounts_for_the_board_and_your_rack() {
        let config = GameConfig::standard();
        let mut board = Board::new(&config.layout);
        board.set(7, 7, crate::tile::Square::letter(16));
        let rack = leave("AAAAA");
        let unseen = Unseen::new(&config, &board, &rack);

        assert_eq!(unseen.len(), 100 - 1 - 5);
        assert_eq!(unseen.counts()[16], 0, "the Q is on the board");
        assert_eq!(unseen.counts()[0], 9 - 5, "five A's are in hand");
        assert_eq!(unseen.as_rack().len(), unseen.len());
    }

    #[test]
    fn unseen_treats_a_played_blank_as_a_blank() {
        let config = GameConfig::standard();
        let mut board = Board::new(&config.layout);
        board.set(7, 7, crate::tile::Square::blank_letter(16));
        let unseen = Unseen::new(&config, &board, &Rack::new());
        assert_eq!(unseen.counts()[crate::tile::BLANK_CODE as usize], 1);
        assert_eq!(unseen.counts()[16], 1, "the real Q is still out there");
    }

    #[cfg(feature = "std")]
    #[test]
    fn a_leave_table_round_trips() {
        let mut table = LeaveTable::new();
        table.insert(&leave("AEINRST"), 12.5);
        table.insert(&leave("QZ"), -14.25);
        let bytes = table.to_bytes();
        let back = LeaveTable::from_bytes(&bytes).expect("round trip");
        assert_eq!(back.len(), 2);
        assert_eq!(back.get(&leave("AEINRST")), Some(12.5));
        assert_eq!(back.get(&leave("QZ")), Some(-14.25));
        assert_eq!(back.get(&leave("A")), None);
        assert!(LeaveTable::from_bytes(b"junk").is_none());
    }

    #[cfg(feature = "std")]
    #[test]
    fn a_table_overrides_the_heuristic_and_falls_back_when_it_cannot() {
        let mut table = LeaveTable::new();
        table.insert(&leave("QQ"), 99.0);
        let evaluator = StaticEvaluator::with_leaves(table);
        assert_eq!(evaluator.leave_value(&alpha(), &leave("QQ")), 99.0);
        assert_eq!(
            evaluator.leave_value(&alpha(), &leave("AE")),
            heuristic_leave(&alpha(), &leave("AE")),
            "an uncovered rack falls back to the heuristic"
        );
    }
}
