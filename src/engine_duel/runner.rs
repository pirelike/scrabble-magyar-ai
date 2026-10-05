//! A párok futtatása szálakon, JSONL naplóval (megszakítás után folytatható).

use super::EVAL_SEED_LIMIT;
use super::referee::{End, GameRecord, play_game};
use super::sides::Side;
use super::spec::Env;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

pub struct DuelOptions {
    pub a: String,
    pub b: String,
    /// az első pár magja (a magok `first_pair`-től `first_pair + pairs - 1`-ig)
    pub first_pair: u64,
    pub pairs: u64,
    pub jobs: usize,
    pub out: PathBuf,
    /// a lépéslista is a naplóba kerül
    pub log_moves: bool,
}

#[derive(Debug)]
pub struct RunSummary {
    pub played: u64,
    pub skipped: u64,
    pub seconds: f64,
}

/// Egy játék naplósora. `before` / `after`: az `a` és a `b` oldal számlálói a játék előtt / után.
pub fn record_json(spec_a: &str, spec_b: &str, g: &GameRecord, counters_a: (u64, u64), counters_b: (u64, u64), log_moves: bool) -> Value {
    let side = |seat: u8, counters: (u64, u64)| {
        let (mut plays, mut exch, mut passes, mut bingos, mut points, mut cpu_ns) = (0u64, 0u64, 0u64, 0u64, 0i64, 0u64);
        let mut tags: BTreeMap<&str, u64> = BTreeMap::new();
        for t in g.turns.iter().filter(|t| t.seat == seat) {
            match t.kind {
                'P' => {
                    plays += 1;
                    points += t.score as i64;
                    if t.tiles as usize == crate::ai::HAND_SIZE {
                        bingos += 1;
                    }
                }
                'X' => exch += 1,
                _ => passes += 1,
            }
            cpu_ns += t.cpu_ns;
            *tags.entry(t.tag).or_insert(0) += 1;
        }
        json!({"plays": plays, "exch": exch, "pass": passes, "bingos": bingos, "points": points, "cpu_ms": cpu_ns as f64 / 1e6, "eg_calls": counters.0, "eg_exact": counters.1, "tags": tags})
    };
    let seat_b = 1 - g.seat_a;
    let mut value = json!({
        "v": 1,
        "spec_a": spec_a,
        "spec_b": spec_b,
        "pair": g.pair,
        "half": g.half,
        "seat_a": g.seat_a,
        "score_a": g.score_a(),
        "score_b": g.score_b(),
        "end": match g.end { End::PlayedOut => "out", End::Scoreless => "scoreless" },
        "plies": g.turns.len(),
        "side_a": side(g.seat_a, counters_a),
        "side_b": side(seat_b, counters_b),
        "spread_at_bag_empty": g.spread_a_at_bag_empty,
    });
    if log_moves {
        let moves: Vec<Value> = g.turns.iter().map(|t| json!([t.seat, t.kind.to_string(), t.score, t.tiles, t.tag])).collect();
        value["moves"] = Value::Array(moves);
    }
    value
}

/// A naplóban már kész (mindkét felével szereplő) párok az adott oldalpárra.
fn completed_pairs(path: &PathBuf, spec_a: &str, spec_b: &str) -> HashSet<u64> {
    let mut halves: BTreeMap<u64, u8> = BTreeMap::new();
    if let Ok(text) = std::fs::read_to_string(path) {
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            if v["spec_a"] == spec_a && v["spec_b"] == spec_b {
                *halves.entry(v["pair"].as_u64().unwrap_or(u64::MAX)).or_insert(0) |= 1 << v["half"].as_u64().unwrap_or(0);
            }
        }
    }
    halves.into_iter().filter(|(_, bits)| *bits == 0b11).map(|(p, _)| p).collect()
}

pub fn run_duel(env: &Env, opts: &DuelOptions, progress: bool) -> Result<RunSummary, String> {
    if opts.first_pair + opts.pairs > EVAL_SEED_LIMIT {
        return Err(format!("a kiértékelő pármagok {EVAL_SEED_LIMIT} alatt maradnak (a tanítóké afölött van)"));
    }
    // az oldalak specifikációját előre ellenőrizzük
    env.build_side(&opts.a)?;
    env.build_side(&opts.b)?;
    let done = completed_pairs(&opts.out, &opts.a, &opts.b);
    let todo: Vec<u64> = (opts.first_pair..opts.first_pair + opts.pairs).filter(|p| !done.contains(p)).collect();
    let skipped = opts.pairs - todo.len() as u64;
    if let Some(dir) = opts.out.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let file = Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(&opts.out).map_err(|e| format!("{}: {e}", opts.out.display()))?);
    let next = AtomicU64::new(0);
    let finished = AtomicU64::new(0);
    let failed = AtomicBool::new(false);
    let failure = Mutex::new(None::<String>);
    let sum_spread = Mutex::new(0.0f64);
    let started = Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..opts.jobs.max(1) {
            scope.spawn(|| {
                let build = |spec: &str| -> Result<Box<dyn Side>, String> { env.build_side(spec) };
                let (mut a, mut b) = match (build(&opts.a), build(&opts.b)) {
                    (Ok(a), Ok(b)) => (a, b),
                    (Err(e), _) | (_, Err(e)) => {
                        *failure.lock().unwrap() = Some(e);
                        failed.store(true, Ordering::SeqCst);
                        return;
                    }
                };
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst) as usize;
                    if i >= todo.len() || failed.load(Ordering::SeqCst) {
                        break;
                    }
                    let pair = todo[i];
                    let mut lines = Vec::with_capacity(2);
                    let mut pair_spread = 0.0;
                    for half in 0..2u8 {
                        let (ca0, cb0) = (a.counters(), b.counters());
                        match play_game(a.as_mut(), b.as_mut(), pair, half) {
                            Ok(game) => {
                                let (ca1, cb1) = (a.counters(), b.counters());
                                let delta = |x: (u64, u64), y: (u64, u64)| (x.0 - y.0, x.1 - y.1);
                                pair_spread += game.spread_a() as f64 / 2.0;
                                lines.push(record_json(&opts.a, &opts.b, &game, delta(ca1, ca0), delta(cb1, cb0), opts.log_moves).to_string());
                            }
                            Err(e) => {
                                *failure.lock().unwrap() = Some(e);
                                failed.store(true, Ordering::SeqCst);
                                return;
                            }
                        }
                    }
                    {
                        let mut f = file.lock().unwrap();
                        for line in &lines {
                            if writeln!(f, "{line}").is_err() {
                                *failure.lock().unwrap() = Some("a napló írása sikertelen".to_string());
                                failed.store(true, Ordering::SeqCst);
                                return;
                            }
                        }
                        let _ = f.flush();
                    }
                    let n = finished.fetch_add(1, Ordering::SeqCst) + 1;
                    let mut total = sum_spread.lock().unwrap();
                    *total += pair_spread;
                    if progress && (n.is_multiple_of(10) || n == todo.len() as u64) {
                        eprintln!("  {n} / {} pár kész, eddigi átlag Δ = {:+.1} ({:.0} mp)", todo.len(), *total / n as f64, started.elapsed().as_secs_f64());
                    }
                }
            });
        }
    });
    if let Some(e) = failure.into_inner().unwrap() {
        return Err(e);
    }
    Ok(RunSummary { played: finished.load(Ordering::SeqCst), skipped, seconds: started.elapsed().as_secs_f64() })
}
