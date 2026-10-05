//! Robot-aréna: a fokozatok erejének mérése bot–bot játékokkal (a kalibrációhoz; nem része a szervernek).
//!
//! ```text
//! bot_arena ladder [-n 24] [-j 4]       # átlagos pont/kör fokozatonként (önjáték)
//! bot_arena match 3 6 [-n 40] [-j 4]    # két fokozat egymás ellen
//! bot_arena adapt 4 [-n 24] [-j 4]      # az "igazodik hozzám" robot egy 4. fokozatú ellen
//! ```
//!
//! Egy játék ~1 mp; a `-j` a párhuzamos szálak száma. A `ladder` eredményét az `ai::LEVEL_STRENGTH` értékeivel kell
//! összevetni, ha a robot paramétereit módosítod.

use rand::SeedableRng;
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use scrabble::ai::{self, Action, Difficulty};
use scrabble::game::Game;
use std::sync::atomic::{AtomicUsize, Ordering};

/// biztonsági korlát
const MAX_TURNS: usize = 500;

/// Egy lejátszott játék eredménye.
struct Outcome {
    scores: Vec<i32>,
    /// lerakott pontok összege és körök száma játékosonként
    points: Vec<i64>,
    turns: Vec<i64>,
    /// az „igazodik hozzám” robotok által használt fokozatok
    adaptive_levels: Vec<u8>,
}

/// Egy játék a megadott nehézségű robotok között (az `Auto` az ellenfelek utolsó köreihez igazodik).
fn play_game(levels: &[Difficulty], seed: u64) -> Outcome {
    let vocab = ai::get_vocabulary();
    let mut rng = StdRng::seed_from_u64(seed + 1);
    let mut game = Game::with_defaults(&format!("arena-{seed}"));
    for (i, level) in levels.iter().enumerate() {
        let _ = game.add_bot(&format!("Bot{}", i + 1), &level.to_json());
    }
    game.bag.tiles.shuffle(&mut StdRng::seed_from_u64(seed)); // a zsák keverése
    let _ = game.start();
    let mut outcome = Outcome { scores: Vec::new(), points: vec![0; levels.len()], turns: vec![0; levels.len()], adaptive_levels: Vec::new() };
    for _ in 0..MAX_TURNS {
        if game.finished {
            break;
        }
        let idx = game.current_player_idx;
        let player = game.current_player().expect("soron lévő játékos").clone();
        let others: Vec<String> = game.players.iter().filter(|p| p.id != player.id).map(|p| p.name.clone()).collect();
        let level = game.bot_level(&player, &mut rng, Some(&others));
        if player.difficulty == Some(Difficulty::Auto) {
            outcome.adaptive_levels.push(level);
        }
        let action = ai::choose_action(&game.board, &player.hand, level, game.bag.remaining(), &mut rng, &vocab, Some(&game.rejected_placements), ai::TIME_BUDGET);
        outcome.turns[idx] += 1;
        let mut ok = false;
        match &action {
            Action::Place { tiles, .. } => {
                if let Ok((_, score)) = game.place_tiles(&player.id, tiles) {
                    outcome.points[idx] += score as i64;
                    ok = true;
                }
            }
            Action::Exchange { indices } => {
                let indices: Vec<i64> = indices.iter().map(|i| *i as i64).collect();
                ok = game.exchange_tiles(&player.id, &indices).is_ok();
            }
            Action::Pass => {}
        }
        if !ok {
            let _ = game.pass_turn(&player.id, false);
        }
    }
    outcome.scores = game.players.iter().map(|p| p.score).collect();
    outcome
}

/// A feladatok párhuzamos futtatása `workers` szálon, az eredmények a feladatok sorrendjében.
fn run(jobs: &[(Vec<Difficulty>, u64)], workers: usize) -> Vec<Outcome> {
    let next = AtomicUsize::new(0);
    let results: Vec<parking_lot::Mutex<Option<Outcome>>> = jobs.iter().map(|_| parking_lot::Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..workers.max(1) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    if i >= jobs.len() {
                        break;
                    }
                    *results[i].lock() = Some(play_game(&jobs[i].0, jobs[i].1));
                }
            });
        }
    });
    results.into_iter().map(|r| r.into_inner().expect("kész játék")).collect()
}

fn ladder(games: usize, workers: usize, seed: u64) {
    println!("fokozat  pont/kör");
    for level in ai::MIN_LEVEL..=ai::MAX_LEVEL {
        let jobs: Vec<(Vec<Difficulty>, u64)> = (0..games).map(|g| (vec![Difficulty::Level(level); 2], seed * 1000 + g as u64)).collect();
        let results = run(&jobs, workers);
        let points: i64 = results.iter().flat_map(|r| r.points.iter()).sum();
        let turns: i64 = results.iter().flat_map(|r| r.turns.iter()).sum();
        println!("{level:7}  {:8.1}", points as f64 / turns.max(1) as f64);
    }
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len().max(1) as f64
}

fn match_levels(a: u8, b: u8, games: usize, workers: usize, seed: u64) {
    let jobs: Vec<(Vec<Difficulty>, u64)> = (0..games)
        .map(|g| (if g % 2 == 0 { vec![Difficulty::Level(a), Difficulty::Level(b)] } else { vec![Difficulty::Level(b), Difficulty::Level(a)] }, seed * 1000 + g as u64))
        .collect();
    let (mut wins, mut diffs) = (0.0, Vec::new());
    for ((order, _), outcome) in jobs.iter().zip(run(&jobs, workers)) {
        let a_idx = if a != b { order.iter().position(|d| *d == Difficulty::Level(a)).unwrap_or(0) } else { 0 };
        let diff = outcome.scores[a_idx] - outcome.scores[1 - a_idx];
        diffs.push(diff as f64);
        wins += if diff > 0 { 1.0 } else if diff == 0 { 0.5 } else { 0.0 };
    }
    println!(
        "{a}. fokozat vs {b}.: {wins}-{} ({:.0}% az elsőnek), átlagos pontkülönbség {:+.0}",
        games as f64 - wins,
        100.0 * wins / games as f64,
        mean(&diffs)
    );
}

/// Az „igazodik hozzám” robot egy rögzített fokozatú „ember” ellen: melyik fokozatot használja, mennyi pontot ér
/// el körönként, és hányszor nyer.
fn adapt(reference: u8, games: usize, workers: usize, seed: u64) {
    let jobs: Vec<(Vec<Difficulty>, u64)> = (0..games)
        .map(|g| (if g % 2 == 0 { vec![Difficulty::Auto, Difficulty::Level(reference)] } else { vec![Difficulty::Level(reference), Difficulty::Auto] }, seed * 1000 + g as u64))
        .collect();
    let (mut levels, mut auto_points, mut auto_turns, mut ref_points, mut ref_turns, mut wins) = (Vec::new(), 0i64, 0i64, 0i64, 0i64, 0.0);
    for ((order, _), outcome) in jobs.iter().zip(run(&jobs, workers)) {
        let a = order.iter().position(|d| *d == Difficulty::Auto).unwrap_or(0);
        levels.extend(outcome.adaptive_levels.iter().map(|l| *l as f64));
        auto_points += outcome.points[a];
        auto_turns += outcome.turns[a];
        ref_points += outcome.points[1 - a];
        ref_turns += outcome.turns[1 - a];
        let diff = outcome.scores[a] - outcome.scores[1 - a];
        wins += if diff > 0 { 1.0 } else if diff == 0 { 0.5 } else { 0.0 };
    }
    println!(
        "igazodó robot vs {reference}. fokozat: átlagosan a {:.1}. fokozatot használta, {:.1} pont/kör (az ellenfél {:.1}), győzelem {:.0}%",
        mean(&levels),
        auto_points as f64 / auto_turns.max(1) as f64,
        ref_points as f64 / ref_turns.max(1) as f64,
        100.0 * wins / games as f64
    );
}

fn usage() -> ! {
    eprintln!("használat: bot_arena ladder|match|adapt [fokozatok] [-n JÁTÉK] [-j SZÁL] [--seed MAG]\n  match: két fokozat (1–10); adapt: az ellenfél fokozata (1–10)");
    std::process::exit(2)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(mode) = args.first().cloned() else { usage() };
    let (mut games, mut workers, mut seed) = (24usize, 1usize, 1u64);
    let mut levels: Vec<u8> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let value = |i: usize| args.get(i + 1).and_then(|v| v.parse::<u64>().ok()).unwrap_or_else(|| usage());
        match args[i].as_str() {
            "-n" | "--games" => {
                games = value(i) as usize;
                i += 1;
            }
            "-j" | "--jobs" => {
                workers = value(i) as usize;
                i += 1;
            }
            "--seed" => {
                seed = value(i);
                i += 1;
            }
            other => match other.parse::<u8>() {
                Ok(level) if (ai::MIN_LEVEL..=ai::MAX_LEVEL).contains(&level) => levels.push(level),
                _ => usage(),
            },
        }
        i += 1;
    }
    match (mode.as_str(), levels.as_slice()) {
        ("ladder", []) => ladder(games, workers, seed),
        ("match", [a, b]) => match_levels(*a, *b, games, workers, seed),
        ("adapt", [reference]) => adapt(*reference, games, workers, seed),
        _ => usage(),
    }
}
