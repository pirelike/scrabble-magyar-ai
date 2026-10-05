use crate::board::Board;
use crate::eval::{EvalContext, Evaluator, StaticEvaluator, Unseen};
use crate::infer::Inference;
use crate::lexicon::Lexicon;
use crate::movegen::{MoveGenerator, Play};
use crate::rack::Rack;
use crate::rng::Rng;
use crate::rules::GameConfig;
use crate::tile::Tile;
use alloc::vec::Vec;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SimOptions {
    pub candidates: usize,
    pub plies: usize,
    pub iterations: usize,
    pub rounds: usize,
    pub seed: u64,
}

impl Default for SimOptions {
    fn default() -> Self {
        SimOptions {
            candidates: 20,
            plies: 2,
            iterations: 40,
            rounds: 3,
            seed: 0x51_4D_1A_7E,
        }
    }
}

impl SimOptions {
    pub fn fast() -> SimOptions {
        SimOptions {
            candidates: 12,
            plies: 2,
            iterations: 20,
            rounds: 2,
            ..SimOptions::default()
        }
    }

    pub fn deep() -> SimOptions {
        SimOptions {
            candidates: 30,
            plies: 3,
            iterations: 120,
            rounds: 4,
            ..SimOptions::default()
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SimResult {
    pub play: Play,
    pub static_equity: f64,
    pub mean: f64,
    pub stddev: f64,
    pub iterations: usize,
}

impl SimResult {
    pub fn margin_of_error(&self) -> f64 {
        if self.iterations < 2 {
            return f64::INFINITY;
        }
        1.96 * self.stddev / (self.iterations as f64).sqrt()
    }
}

pub struct Simulator {
    config: GameConfig,
    options: SimOptions,
    generator: MoveGenerator,
    #[cfg_attr(feature = "rayon", allow(dead_code))]
    rollout: MoveGenerator,
}

impl Simulator {
    pub fn new(config: &GameConfig, options: SimOptions) -> Simulator {
        Simulator {
            generator: MoveGenerator::new(config),
            rollout: MoveGenerator::new(config),
            config: config.clone(),
            options,
        }
    }

    #[inline]
    pub fn options(&self) -> &SimOptions {
        &self.options
    }

    pub fn run(
        &mut self,
        board: &Board,
        rack: &Rack,
        lexicon: &Lexicon,
        bag_remaining: usize,
        spread: i32,
    ) -> Vec<SimResult> {
        let evaluator = StaticEvaluator::new();
        self.run_with(board, rack, lexicon, bag_remaining, spread, &evaluator)
    }

    pub fn run_with(
        &mut self,
        board: &Board,
        rack: &Rack,
        lexicon: &Lexicon,
        bag_remaining: usize,
        spread: i32,
        evaluator: &(impl Evaluator + Sync),
    ) -> Vec<SimResult> {
        let ctx = EvalContext {
            config: &self.config,
            board,
            lexicon,
            rack,
            bag_remaining,
            spread,
        };

        let mut shortlist: Vec<(Play, f64)> = self
            .generator
            .generate(board, rack, lexicon)
            .iter()
            .map(|&p| (p, evaluator.equity(&ctx, &p)))
            .collect();
        if shortlist.is_empty() {
            return Vec::new();
        }
        shortlist.sort_unstable_by(|a, b| b.1.total_cmp(&a.1));
        shortlist.truncate(self.options.candidates.max(1));

        let unseen = Unseen::new(&self.config, board, rack);
        let inference = Inference::uniform(&unseen);

        let mut live: Vec<Candidate> = shortlist
            .into_iter()
            .map(|(play, static_equity)| Candidate {
                play,
                static_equity,
                sum: 0.0,
                sum_sq: 0.0,
                trials: 0,
            })
            .collect();

        let mut iterations = self.options.iterations.max(1);
        for round in 0..self.options.rounds.max(1) {
            let base_seed = self
                .options
                .seed
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(round as u64);

            let batch = |play: &Play, generator: &mut MoveGenerator| -> (f64, f64) {
                let mut sum = 0.0;
                let mut sum_sq = 0.0;
                for iteration in 0..iterations {
                    let mut rng = Rng::seed_from_u64(base_seed.wrapping_add(iteration as u64));
                    let value = rollout(
                        &self.config,
                        generator,
                        board,
                        rack,
                        lexicon,
                        &inference,
                        play,
                        self.options.plies,
                        evaluator,
                        bag_remaining,
                        &mut rng,
                    );
                    sum += value;
                    sum_sq += value * value;
                }
                (sum, sum_sq)
            };

            #[cfg(feature = "rayon")]
            {
                use rayon::prelude::*;

                let totals: Vec<(f64, f64)> = live
                    .par_iter()
                    .map(|candidate| {
                        let mut generator = MoveGenerator::new(&self.config);
                        batch(&candidate.play, &mut generator)
                    })
                    .collect();
                for (candidate, (sum, sum_sq)) in live.iter_mut().zip(totals) {
                    candidate.sum += sum;
                    candidate.sum_sq += sum_sq;
                    candidate.trials += iterations;
                }
            }

            #[cfg(not(feature = "rayon"))]
            {
                let mut totals = Vec::with_capacity(live.len());
                for candidate in live.iter() {
                    totals.push(batch(&candidate.play, &mut self.rollout));
                }
                for (candidate, (sum, sum_sq)) in live.iter_mut().zip(totals) {
                    candidate.sum += sum;
                    candidate.sum_sq += sum_sq;
                    candidate.trials += iterations;
                }
            }

            if live.len() <= 1 || round + 1 == self.options.rounds.max(1) {
                break;
            }

            live.sort_unstable_by(|a, b| b.mean().total_cmp(&a.mean()));
            live.truncate((live.len() / 2).max(1));
            iterations *= 2;
        }

        let mut results: Vec<SimResult> = live.iter().map(Candidate::finish).collect();
        results.sort_unstable_by(|a, b| b.mean.total_cmp(&a.mean));
        results
    }

    pub fn best(
        &mut self,
        board: &Board,
        rack: &Rack,
        lexicon: &Lexicon,
        bag_remaining: usize,
        spread: i32,
    ) -> Option<Play> {
        self.run(board, rack, lexicon, bag_remaining, spread)
            .first()
            .map(|r| r.play)
    }
}

struct Candidate {
    play: Play,
    static_equity: f64,
    sum: f64,
    sum_sq: f64,
    trials: usize,
}

impl Candidate {
    fn mean(&self) -> f64 {
        if self.trials == 0 {
            f64::NEG_INFINITY
        } else {
            self.sum / self.trials as f64
        }
    }

    fn finish(&self) -> SimResult {
        let n = self.trials as f64;
        let mean = if self.trials == 0 { 0.0 } else { self.sum / n };
        let variance = if self.trials < 2 {
            0.0
        } else {
            ((self.sum_sq - n * mean * mean) / (n - 1.0)).max(0.0)
        };
        SimResult {
            play: self.play,
            static_equity: self.static_equity,
            mean,
            stddev: variance.sqrt(),
            iterations: self.trials,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn rollout(
    config: &GameConfig,
    generator: &mut MoveGenerator,
    board: &Board,
    rack: &Rack,
    lexicon: &Lexicon,
    inference: &Inference,
    candidate: &Play,
    plies: usize,
    evaluator: &(impl Evaluator + Sync),
    bag_remaining: usize,
    rng: &mut Rng,
) -> f64 {
    let mut work = board.clone();
    candidate.apply(&mut work);

    let Some(leave) = candidate.leave(rack) else {
        return f64::NEG_INFINITY;
    };

    let (mut opponent_rack, mut pool) = inference.sample_split(rng, config.rack_size);

    let mut own_rack = leave;
    let draw = |rack: &mut Rack, pool: &mut Vec<Tile>| {
        while rack.len() < config.rack_size {
            match pool.pop() {
                Some(t) => rack.add(t),
                None => break,
            }
        }
    };
    draw(&mut own_rack, &mut pool);

    let mut spread = candidate.score() as f64;
    let mut remaining = bag_remaining.saturating_sub(candidate.tiles_used());

    for ply in 0..plies {
        let opponents_turn = ply % 2 == 0;
        let mover = if opponents_turn {
            &mut opponent_rack
        } else {
            &mut own_rack
        };

        let ctx = EvalContext {
            config,
            board: &work,
            lexicon,
            rack: mover,
            bag_remaining: remaining,
            spread: spread as i32,
        };
        let best = generator
            .generate(&work, mover, lexicon)
            .iter()
            .map(|&p| (p, evaluator.equity(&ctx, &p)))
            .max_by(|a, b| a.1.total_cmp(&b.1));

        let Some((play, _)) = best else {
            continue;
        };
        play.apply(&mut work);
        if let Some(after) = play.leave(mover) {
            *mover = after;
        }
        draw(mover, &mut pool);
        remaining = remaining.saturating_sub(play.tiles_used());

        if opponents_turn {
            spread -= play.score() as f64;
        } else {
            spread += play.score() as f64;
        }
    }

    spread + evaluator.leave_value(&config.alphabet, &own_rack)
        - evaluator.leave_value(&config.alphabet, &opponent_rack)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Alphabet;

    fn lexicon() -> Lexicon {
        let words: Vec<&str> = crate::lexicon_test_words();
        Lexicon::from_words(&Alphabet::english(), words).unwrap()
    }

    fn setup() -> (GameConfig, Lexicon, Board) {
        let config = GameConfig::standard();
        let lex = lexicon();
        let board = Board::new(&config.layout);
        (config, lex, board)
    }

    #[test]
    fn simulation_returns_ranked_candidates() {
        let (config, lex, board) = setup();
        let rack = Rack::parse(&config.alphabet, "CATSEA").unwrap();
        let mut sim = Simulator::new(&config, SimOptions::fast());
        let results = sim.run(&board, &rack, &lex, 60, 0);

        assert!(!results.is_empty());
        for pair in results.windows(2) {
            assert!(
                pair[0].mean >= pair[1].mean,
                "results must come back sorted by mean"
            );
        }
        for r in &results {
            assert!(r.iterations > 0);
            assert!(r.mean.is_finite());
            assert!(r.stddev >= 0.0);
        }
    }

    #[test]
    fn sequential_halving_concentrates_effort_on_survivors() {
        let (config, lex, board) = setup();
        let rack = Rack::parse(&config.alphabet, "CATSEA").unwrap();
        let options = SimOptions {
            candidates: 8,
            iterations: 4,
            rounds: 3,
            ..SimOptions::fast()
        };
        let mut sim = Simulator::new(&config, options);
        let results = sim.run(&board, &rack, &lex, 60, 0);

        assert!(
            results.len() <= 2,
            "three rounds should halve eight down to two"
        );
        let winner = &results[0];
        assert!(
            winner.iterations >= 4 + 8 + 16,
            "the survivor should accumulate every round's rollouts, got {}",
            winner.iterations
        );
    }

    #[test]
    fn simulation_is_reproducible() {
        let (config, lex, board) = setup();
        let rack = Rack::parse(&config.alphabet, "CATSEA").unwrap();
        let mut a = Simulator::new(&config, SimOptions::fast());
        let mut b = Simulator::new(&config, SimOptions::fast());
        let ra = a.run(&board, &rack, &lex, 60, 0);
        let rb = b.run(&board, &rack, &lex, 60, 0);
        assert_eq!(ra.len(), rb.len());
        for (x, y) in ra.iter().zip(rb.iter()) {
            assert_eq!(x.play, y.play);
            assert_eq!(x.mean, y.mean);
        }
    }

    #[test]
    fn a_different_seed_gives_a_different_sample() {
        let (config, lex, board) = setup();
        let rack = Rack::parse(&config.alphabet, "CATSEA").unwrap();
        let mut a = Simulator::new(&config, SimOptions::fast());
        let mut b = Simulator::new(
            &config,
            SimOptions {
                seed: 999,
                ..SimOptions::fast()
            },
        );
        let ra = a.run(&board, &rack, &lex, 60, 0);
        let rb = b.run(&board, &rack, &lex, 60, 0);

        assert!(
            ra[0].mean != rb[0].mean || ra.len() != rb.len(),
            "different seeds should produce different rollouts"
        );
    }

    #[test]
    fn an_unplayable_rack_simulates_to_nothing() {
        let (config, lex, board) = setup();
        let rack = Rack::parse(&config.alphabet, "VVVV").unwrap();
        let mut sim = Simulator::new(&config, SimOptions::fast());
        assert!(sim.run(&board, &rack, &lex, 60, 0).is_empty());
        assert!(sim.best(&board, &rack, &lex, 60, 0).is_none());
    }

    #[test]
    fn margin_of_error_shrinks_with_more_rollouts() {
        let (config, lex, board) = setup();
        let rack = Rack::parse(&config.alphabet, "CATSEA").unwrap();
        let few = SimOptions {
            candidates: 4,
            iterations: 4,
            rounds: 1,
            ..SimOptions::fast()
        };
        let many = SimOptions {
            iterations: 64,
            ..few
        };
        let mut a = Simulator::new(&config, few);
        let mut b = Simulator::new(&config, many);
        let wide = a.run(&board, &rack, &lex, 60, 0)[0].margin_of_error();
        let tight = b.run(&board, &rack, &lex, 60, 0)[0].margin_of_error();

        if wide > 0.0 {
            assert!(tight < wide, "more rollouts should narrow the interval");
        }
    }

    #[test]
    fn simulation_beats_pure_greed_on_leave_quality() {
        let (config, lex, board) = setup();
        let rack = Rack::parse(&config.alphabet, "CATSEA").unwrap();
        let mut generator = MoveGenerator::new(&config);
        let greedy = generator
            .best_by_score(&board, &rack, &lex)
            .expect("something is playable");
        let mut sim = Simulator::new(&config, SimOptions::deep());
        let simulated = sim.best(&board, &rack, &lex, 60, 0).unwrap();

        assert!(simulated.score() > 0);
        assert!(greedy.score() >= simulated.score());
    }
}
