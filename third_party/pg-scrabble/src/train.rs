use crate::bag::Bag;
use crate::board::Board;
use crate::eval::{EvalContext, Evaluator, LeaveTable, StaticEvaluator};
use crate::lexicon::Lexicon;
use crate::movegen::MoveGenerator;
use crate::rack::Rack;
use crate::rules::GameConfig;
use alloc::vec::Vec;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TrainOptions {
    pub games: usize,
    pub max_turns: usize,
    pub max_leave: usize,
    pub min_samples: usize,
    pub seed: u64,
}

impl Default for TrainOptions {
    fn default() -> Self {
        TrainOptions {
            games: 2_000,
            max_turns: 40,
            max_leave: 4,
            min_samples: 30,
            seed: 0x1EA7E,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TrainReport {
    pub table: LeaveTable,
    pub games: usize,
    pub observations: usize,
    pub distinct_leaves: usize,
    pub kept: usize,
    pub mean_turn_score: f64,
}

pub fn train_leaves(
    config: &GameConfig,
    lexicon: &Lexicon,
    options: TrainOptions,
    mut on_game: impl FnMut(usize),
) -> TrainReport {
    let evaluator = StaticEvaluator::new();
    let mut generator = MoveGenerator::new(config);

    let mut samples: HashMap<u64, (f64, usize)> = HashMap::new();
    let mut leave_for_key: HashMap<u64, Rack> = HashMap::new();
    let mut total_score = 0.0;
    let mut total_turns = 0usize;

    for game in 0..options.games {
        let seed = options
            .seed
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(game as u64);
        let mut board = Board::new(&config.layout);
        let mut bag = Bag::new(&config.distribution, seed);
        let mut racks = [Rack::new(), Rack::new()];
        for rack in &mut racks {
            bag.refill(rack, config.rack_size);
        }

        let mut pending: [Option<Rack>; 2] = [None, None];
        let mut scoreless = 0u32;

        for turn in 0..options.max_turns {
            let side = turn % 2;
            let ctx = EvalContext {
                config,
                board: &board,
                lexicon,
                rack: &racks[side],
                bag_remaining: bag.len(),
                spread: 0,
            };
            let best = generator
                .generate(&board, &racks[side], lexicon)
                .iter()
                .map(|&p| (p, evaluator.equity(&ctx, &p)))
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(p, _)| p);

            let Some(play) = best else {
                scoreless += 1;
                if scoreless >= config.max_consecutive_zeros {
                    break;
                }
                continue;
            };
            scoreless = 0;

            if let Some(leave) = pending[side].take() {
                if !leave.is_empty() && leave.len() <= options.max_leave {
                    let key = leave.key();
                    let entry = samples.entry(key).or_insert((0.0, 0));
                    entry.0 += play.score() as f64;
                    entry.1 += 1;
                    leave_for_key.entry(key).or_insert(leave);
                }
            }
            total_score += play.score() as f64;
            total_turns += 1;

            play.apply(&mut board);
            let leave = play
                .leave(&racks[side])
                .expect("a generated play fits its rack");
            pending[side] = Some(leave);
            racks[side] = leave;
            bag.refill(&mut racks[side], config.rack_size);

            if racks[side].is_empty() && bag.is_empty() {
                break;
            }
        }
        on_game(game + 1);
    }

    let mean = if total_turns == 0 {
        0.0
    } else {
        total_score / total_turns as f64
    };

    let mut table = LeaveTable::new();
    let mut kept = 0;
    for (key, (sum, count)) in &samples {
        if *count < options.min_samples {
            continue;
        }
        let leave = &leave_for_key[key];
        table.insert(leave, (sum / *count as f64 - mean) as f32);
        kept += 1;
    }

    TrainReport {
        table,
        games: options.games,
        observations: total_turns,
        distinct_leaves: samples.len(),
        kept,
        mean_turn_score: mean,
    }
}

pub fn head_to_head(
    config: &GameConfig,
    lexicon: &Lexicon,
    a: &impl Evaluator,
    b: &impl Evaluator,
    games: usize,
    seed: u64,
) -> MatchReport {
    let mut generator = MoveGenerator::new(config);
    let mut wins = [0usize; 2];
    let mut ties = 0usize;
    let mut spreads: Vec<i32> = Vec::with_capacity(games);

    for game in 0..games {
        let a_first = game % 2 == 0;
        let game_seed = seed
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add((game / 2) as u64);

        let mut board = Board::new(&config.layout);
        let mut bag = Bag::new(&config.distribution, game_seed);
        let mut racks = [Rack::new(), Rack::new()];
        for rack in &mut racks {
            bag.refill(rack, config.rack_size);
        }
        let mut scores = [0i32; 2];
        let mut scoreless = 0u32;

        for turn in 0..60 {
            let side = turn % 2;

            let uses_a = (side == 0) == a_first;
            let ctx = EvalContext {
                config,
                board: &board,
                lexicon,
                rack: &racks[side],
                bag_remaining: bag.len(),
                spread: scores[side] - scores[1 - side],
            };
            let plays = generator.generate(&board, &racks[side], lexicon);
            let best = if uses_a {
                plays
                    .iter()
                    .map(|&p| (p, a.equity(&ctx, &p)))
                    .max_by(|x, y| x.1.total_cmp(&y.1))
            } else {
                plays
                    .iter()
                    .map(|&p| (p, b.equity(&ctx, &p)))
                    .max_by(|x, y| x.1.total_cmp(&y.1))
            };

            let Some((play, _)) = best else {
                scoreless += 1;
                if scoreless >= config.max_consecutive_zeros {
                    break;
                }
                continue;
            };
            scoreless = 0;
            scores[side] += play.score();
            play.apply(&mut board);
            racks[side] = play.leave(&racks[side]).expect("a generated play fits");
            bag.refill(&mut racks[side], config.rack_size);
            if racks[side].is_empty() && bag.is_empty() {
                let leftover = racks[1 - side].value(&config.alphabet);
                scores[side] += leftover;
                scores[1 - side] -= leftover;
                break;
            }
        }

        let (a_score, b_score) = if a_first {
            (scores[0], scores[1])
        } else {
            (scores[1], scores[0])
        };
        spreads.push(a_score - b_score);
        match a_score.cmp(&b_score) {
            core::cmp::Ordering::Greater => wins[0] += 1,
            core::cmp::Ordering::Less => wins[1] += 1,
            core::cmp::Ordering::Equal => ties += 1,
        }
    }

    let mean_spread = spreads.iter().map(|&s| s as f64).sum::<f64>() / games.max(1) as f64;
    MatchReport {
        games,
        a_wins: wins[0],
        b_wins: wins[1],
        ties,
        mean_spread,
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct MatchReport {
    pub games: usize,
    pub a_wins: usize,
    pub b_wins: usize,
    pub ties: usize,
    pub mean_spread: f64,
}

impl MatchReport {
    pub fn win_rate(&self) -> f64 {
        if self.games == 0 {
            return 0.5;
        }
        (self.a_wins as f64 + self.ties as f64 / 2.0) / self.games as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Alphabet;

    fn setup() -> (GameConfig, Lexicon) {
        let config = GameConfig::standard();
        let lexicon =
            Lexicon::from_words(&Alphabet::english(), crate::lexicon_test_words()).unwrap();
        (config, lexicon)
    }

    #[test]
    fn training_produces_a_table_and_a_report() {
        let (config, lexicon) = setup();
        let options = TrainOptions {
            games: 30,
            max_turns: 12,
            min_samples: 2,
            max_leave: 3,
            seed: 1,
        };
        let mut seen = 0;
        let report = train_leaves(&config, &lexicon, options, |n| seen = n);

        assert_eq!(seen, 30, "the progress callback should fire once per game");
        assert_eq!(report.games, 30);
        assert!(report.observations > 0);
        assert!(report.distinct_leaves > 0);
        assert!(report.kept <= report.distinct_leaves);
        assert!(report.mean_turn_score > 0.0);
    }

    #[test]
    fn a_higher_sample_threshold_keeps_fewer_leaves() {
        let (config, lexicon) = setup();
        let base = TrainOptions {
            games: 40,
            max_turns: 12,
            min_samples: 1,
            max_leave: 3,
            seed: 2,
        };
        let loose = train_leaves(&config, &lexicon, base, |_| {});
        let strict = train_leaves(
            &config,
            &lexicon,
            TrainOptions {
                min_samples: 15,
                ..base
            },
            |_| {},
        );
        assert!(
            strict.kept < loose.kept,
            "raising the threshold from 1 to 15 should drop leaves ({} vs {})",
            strict.kept,
            loose.kept
        );
    }

    #[test]
    fn training_is_reproducible() {
        let (config, lexicon) = setup();
        let options = TrainOptions {
            games: 20,
            max_turns: 10,
            min_samples: 2,
            max_leave: 3,
            seed: 7,
        };
        let a = train_leaves(&config, &lexicon, options, |_| {});
        let b = train_leaves(&config, &lexicon, options, |_| {});
        assert_eq!(a.table.to_bytes(), b.table.to_bytes());
        assert_eq!(a.mean_turn_score, b.mean_turn_score);
    }

    #[test]
    fn two_evaluators_that_differ_produce_different_games() {
        let (config, lexicon) = setup();
        let thoughtful = StaticEvaluator::new();
        let greedy = StaticEvaluator::greedy();
        let report = head_to_head(&config, &lexicon, &thoughtful, &greedy, 40, 3);

        assert_eq!(report.games, 40);
        assert_eq!(report.a_wins + report.b_wins + report.ties, 40);

        assert!(
            report.mean_spread != 0.0,
            "different evaluators should reach different positions: {report:?}"
        );
    }

    #[test]
    fn an_evaluator_matched_against_itself_is_exactly_even() {
        let (config, lexicon) = setup();
        let a = StaticEvaluator::new();
        let b = StaticEvaluator::new();
        let report = head_to_head(&config, &lexicon, &a, &b, 20, 5);

        assert_eq!(
            report.mean_spread, 0.0,
            "mirrored self-play must cancel exactly: {report:?}"
        );
        assert_eq!(report.win_rate(), 0.5);
        assert_eq!(report.a_wins, report.b_wins);
    }
}
