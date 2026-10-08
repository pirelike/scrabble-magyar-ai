//! Robot (10. fokozat) és egy független külső Scrabble motor párharca. Leírás, módszertan, eredmények: docs/ENGINE_DUEL.md
//!
//! ```text
//! engine_duel duel A B [--pairs 200] [--first-pair 1] [-j 4] [--leaves FÁJL] [--out FÁJL] [--log-moves]
//! engine_duel train --out FÁJL [--games 100000] [-j 4] [--policy greedy|stock|leaves] [--from FÁJL] [--epsilon 0.1] [--lambda 100]
//!                   [--holdout 10] [--save-normal ELŐTAG]
//! engine_duel solve --normal ELŐTAG[,ELŐTAG…] --out FÁJL [--lambdas 10,30,100,300,1000]
//! engine_duel crosscheck [--games 6] [--seed 1]
//! engine_duel report FÁJL...
//! engine_duel info
//! ```
//!
//! Az oldalak leírása (`A`, `B`): `bot:10`, `bot:9`, `bot:greedy`, `bot:10:full`, `eng:greedy`, `eng:stock`, `eng:leaves`,
//! `eng:leaves:sim-fast+eg` … (lásd a `src/engine_duel/spec.rs` fejlécét). Fordítás: `--features engine-duel`.

use pg_scrabble::eval::{EvalContext, Evaluator, StaticEvaluator};
use pg_scrabble::movegen::Play;
use scrabble::engine_duel::crosscheck;
use scrabble::engine_duel::leaves::{self, LinearLeaves, TrainOptions};
use scrabble::engine_duel::report;
use scrabble::engine_duel::runner::{self, DuelOptions};
use scrabble::engine_duel::sides::Eval;
use scrabble::engine_duel::spec::Env;
use scrabble::engine_duel::{EVAL_SEED_LIMIT, TRAIN_SEED_START};
use serde_json::json;
use std::path::PathBuf;
use std::process::exit;

fn usage() -> ! {
    eprintln!(
        "használat:\n  engine_duel duel A B [--pairs N] [--first-pair S] [-j N] [--leaves FÁJL] [--out FÁJL] [--log-moves]\n  engine_duel train --out FÁJL [--games N] [-j N] [--policy greedy|stock|leaves] [--from FÁJL] [--epsilon E] [--lambda L] [--seed-start S] [--holdout K] [--save-normal ELŐTAG]\n  engine_duel solve --normal ELŐTAG[,ELŐTAG…] --out FÁJL [--lambdas 10,30,100,300,1000]\n  engine_duel crosscheck [--games N] [--seed S]\n  engine_duel report FÁJL...\n  engine_duel info\n\nAz oldalak: bot:10 bot:9 bot:greedy bot:10:full eng:greedy eng:stock eng:leaves[:sim-fast|:sim|:sim-deep][+eg]"
    );
    exit(2)
}

fn fail(message: &str) -> ! {
    eprintln!("hiba: {message}");
    exit(1)
}

struct Args {
    positional: Vec<String>,
    options: Vec<(String, Option<String>)>,
}

impl Args {
    fn parse(raw: &[String]) -> Args {
        let mut positional = Vec::new();
        let mut options = Vec::new();
        let flags = ["--log-moves"];
        let mut i = 0;
        while i < raw.len() {
            let a = &raw[i];
            if a.starts_with('-') && a.len() > 1 && a.parse::<f64>().is_err() {
                if flags.contains(&a.as_str()) {
                    options.push((a.clone(), None));
                } else {
                    i += 1;
                    match raw.get(i) {
                        Some(v) => options.push((a.clone(), Some(v.clone()))),
                        None => {
                            eprintln!("hiányzó érték: {a}");
                            usage()
                        }
                    }
                }
            } else {
                positional.push(a.clone());
            }
            i += 1;
        }
        Args { positional, options }
    }

    fn get(&self, names: &[&str]) -> Option<&str> {
        self.options.iter().find(|(k, _)| names.contains(&k.as_str())).and_then(|(_, v)| v.as_deref())
    }

    fn flag(&self, name: &str) -> bool {
        self.options.iter().any(|(k, _)| k == name)
    }

    fn number<T: std::str::FromStr>(&self, names: &[&str], default: T) -> T {
        match self.get(names) {
            Some(text) => text.parse().unwrap_or_else(|_| fail(&format!("hibás szám: {} {text}", names[0]))),
            None => default,
        }
    }

    fn check(&self, allowed: &[&str]) {
        for (k, _) in &self.options {
            if !allowed.contains(&k.as_str()) {
                eprintln!("ismeretlen kapcsoló: {k}");
                usage()
            }
        }
    }
}

fn default_jobs() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2)
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = raw.first().cloned() else { usage() };
    let args = Args::parse(&raw[1..]);
    match command.as_str() {
        "duel" => cmd_duel(&args),
        "train" => cmd_train(&args),
        "solve" => cmd_solve(&args),
        "crosscheck" => cmd_crosscheck(&args),
        "report" => cmd_report(&args),
        "info" => cmd_info(&args),
        "help" | "--help" | "-h" => usage(),
        other => {
            eprintln!("ismeretlen parancs: {other}");
            usage()
        }
    }
}

fn load_env(args: &Args) -> Env {
    let leaves = args.get(&["--leaves"]).map(PathBuf::from);
    Env::load(leaves.as_deref()).unwrap_or_else(|e| fail(&e))
}

fn cmd_info(args: &Args) {
    args.check(&["--leaves"]);
    let env = load_env(args);
    println!("a robot szókincse: {} szó; a motor szótára: {} zsetonsor", env.vocab.len(), env.sequences);
    println!("kiértékelő pármagok: 1..{EVAL_SEED_LIMIT}; tanító magok: {TRAIN_SEED_START}-tól");
}

fn cmd_duel(args: &Args) {
    args.check(&["--pairs", "--first-pair", "-j", "--leaves", "--out", "--log-moves"]);
    if args.positional.len() != 2 {
        usage()
    }
    let env = load_env(args);
    let opts = DuelOptions {
        a: args.positional[0].clone(),
        b: args.positional[1].clone(),
        first_pair: args.number(&["--first-pair"], 1),
        pairs: args.number(&["--pairs"], 200),
        jobs: args.number(&["-j"], default_jobs()),
        out: PathBuf::from(args.get(&["--out"]).unwrap_or(".perf/duel/duel.jsonl")),
        log_moves: args.flag("--log-moves"),
    };
    eprintln!("{} — {}: {} pár ({}. magtól), {} szál, napló: {}", opts.a, opts.b, opts.pairs, opts.first_pair, opts.jobs, opts.out.display());
    match runner::run_duel(&env, &opts, true) {
        Ok(summary) => {
            eprintln!("kész: {} új pár, {} már megvolt, {:.0} mp", summary.played, summary.skipped, summary.seconds);
            let matchups = report::load(std::slice::from_ref(&opts.out)).unwrap_or_else(|e| fail(&e));
            let analyses: Vec<report::Analysis> = matchups.iter().filter(|m| m.spec_a == opts.a && m.spec_b == opts.b).filter_map(report::analyze).collect();
            print!("{}", report::render(&analyses));
        }
        Err(e) => fail(&e),
    }
}

fn cmd_crosscheck(args: &Args) {
    args.check(&["--games", "--seed"]);
    let env = load_env(args);
    let games: u64 = args.number(&["--games"], 6);
    let seed: u64 = args.number(&["--seed"], 1);
    let report = crosscheck::run(&env, games, seed).unwrap_or_else(|e| fail(&e));
    for (k, v) in report.summary() {
        println!("{k:36} {v}");
    }
    println!("{:36} {} ms", "bot_gen_ms_max", report.bot_ms_max);
    for e in &report.examples {
        println!("  {e}");
    }
    if report.clean() {
        println!("ELLENŐRZÉS: rendben (a két generátor azonos, a játékvezető minden motorlépést elfogadott)");
    } else {
        eprintln!("ELLENŐRZÉS: ELTÉRÉS");
        exit(1)
    }
}

fn cmd_report(args: &Args) {
    args.check(&[]);
    if args.positional.is_empty() {
        usage()
    }
    let paths: Vec<PathBuf> = args.positional.iter().map(PathBuf::from).collect();
    let matchups = report::load(&paths).unwrap_or_else(|e| fail(&e));
    let analyses: Vec<report::Analysis> = matchups.iter().filter_map(report::analyze).collect();
    if analyses.is_empty() {
        fail("nincs teljes (mindkét felével szereplő) pár a naplókban");
    }
    print!("{}", report::render(&analyses));
}

fn cmd_train(args: &Args) {
    args.check(&["--out", "--games", "-j", "--policy", "--from", "--epsilon", "--lambda", "--seed-start", "--min-bag", "--holdout", "--save-normal"]);
    let Some(out) = args.get(&["--out"]) else { usage() };
    let from = args.get(&["--from"]).map(PathBuf::from);
    let env = Env::load(from.as_deref()).unwrap_or_else(|e| fail(&e));
    let policy_name = args.get(&["--policy"]).unwrap_or(if from.is_some() { "leaves" } else { "greedy" });
    let eval = match policy_name {
        "greedy" => Eval::Greedy,
        "stock" => Eval::Stock(StaticEvaluator::new()),
        "leaves" => Eval::Leaves((*env.leaves.clone().unwrap_or_else(|| fail("a --policy leaves-hez kell a --from FÁJL"))).clone()),
        other => fail(&format!("ismeretlen szabály: {other}")),
    };
    let policy = |ctx: &EvalContext<'_>, play: &Play| eval.equity(ctx, play);
    let opts = TrainOptions {
        games: args.number(&["--games"], 100_000),
        jobs: args.number(&["-j"], default_jobs()),
        seed_start: args.number(&["--seed-start"], TRAIN_SEED_START),
        epsilon: args.number(&["--epsilon"], 0.1),
        lambda: args.number(&["--lambda"], 100.0),
        min_bag: args.number(&["--min-bag"], 7),
        holdout_every: args.number(&["--holdout"], 0),
    };
    eprintln!("tanítás: {} játék, {} szál, szabály: {policy_name}, ε = {}, λ = {}", opts.games, opts.jobs, opts.epsilon, opts.lambda);
    let started = std::time::Instant::now();
    let report = leaves::train(&env.config, &env.lexicon, &policy, &opts, true).unwrap_or_else(|e| fail(&e));
    eprintln!(
        "kész {:.0} mp alatt: {} minta, a kimenet átlaga {:+.2}, szórása {:.1}",
        started.elapsed().as_secs_f64(),
        report.samples,
        report.mean_y,
        report.sd_y
    );
    let meta = json!({"games": report.games, "samples": report.samples, "policy": policy_name, "epsilon": opts.epsilon, "lambda": opts.lambda, "seed_start": opts.seed_start, "min_bag": opts.min_bag, "mean_y": report.mean_y, "sd_y": report.sd_y});
    if let Some(dir) = std::path::Path::new(out).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(out, serde_json::to_string(&report.leaves.to_json(meta)).unwrap()).unwrap_or_else(|e| fail(&format!("{out}: {e}")));
    print_tile_values(&env, &report.leaves);
    if let Some(prefix) = args.get(&["--save-normal"]) {
        report.train.save(&PathBuf::from(format!("{prefix}.train.bin"))).unwrap_or_else(|e| fail(&e));
        if let Some(validation) = &report.validation {
            validation.save(&PathBuf::from(format!("{prefix}.val.bin"))).unwrap_or_else(|e| fail(&e));
        }
        eprintln!("normálegyenlet mentve: {prefix}.train.bin (+ .val.bin)");
    }
    eprintln!("mentve: {out}");
}

/// Mentett normálegyenletekből a λ kiválasztása a kivárt halmaz hibája alapján, majd a végleges megoldás.
fn cmd_solve(args: &Args) {
    args.check(&["--normal", "--out", "--lambdas"]);
    let (Some(prefixes), Some(out)) = (args.get(&["--normal"]), args.get(&["--out"])) else { usage() };
    let lambdas: Vec<f64> =
        args.get(&["--lambdas"]).unwrap_or("10,30,100,300,1000").split(',').map(|x| x.trim().parse().unwrap_or_else(|_| fail("hibás λ"))).collect();
    let (mut train, mut validation) = (leaves::Normal::new(), leaves::Normal::new());
    for prefix in prefixes.split(',') {
        train.merge(&leaves::Normal::load(&PathBuf::from(format!("{prefix}.train.bin"))).unwrap_or_else(|e| fail(&e)));
        validation.merge(&leaves::Normal::load(&PathBuf::from(format!("{prefix}.val.bin"))).unwrap_or_else(|e| fail(&e)));
    }
    if validation.samples == 0 {
        fail("nincs kivárt halmaz (tanítsd --holdout K-val)");
    }
    println!("tanító minták: {}, kivárt minták: {}", train.samples, validation.samples);
    let baseline = {
        let mut only_bias = leaves::LinearLeaves::zero();
        *only_bias.weights.last_mut().unwrap() = train.mean_y();
        validation.mse(&only_bias)
    };
    println!("a kivárt halmaz szórásnégyzete (csak tengelymetszet): {baseline:.2}");
    let mut best: Option<(f64, f64)> = None;
    for lambda in &lambdas {
        let fit = train.solve_full(*lambda).unwrap_or_else(|e| fail(&e));
        let mse = validation.mse(&fit);
        println!("  λ = {lambda:>7}: kivárt MSE = {mse:.3} ({:.3}% magyarázott szórás)", (1.0 - mse / baseline) * 100.0);
        if best.is_none_or(|(_, m)| mse < m) {
            best = Some((*lambda, mse));
        }
    }
    let (lambda, mse) = best.unwrap();
    println!("a legjobb λ = {lambda} (kivárt MSE {mse:.3}); a végleges modell a tanító + kivárt adatból");
    train.merge(&validation);
    let leaves = train.solve(lambda).unwrap_or_else(|e| fail(&e));
    let meta = json!({"normal": prefixes, "lambda": lambda, "validation_mse": mse, "samples": train.samples});
    std::fs::write(out, serde_json::to_string(&leaves.to_json(meta)).unwrap()).unwrap_or_else(|e| fail(&format!("{out}: {e}")));
    eprintln!("mentve: {out}");
}

/// A zsetononkénti (egyedi) érték: a tanult modell egy zseton maradékának értéke.
fn print_tile_values(env: &Env, leaves: &LinearLeaves) {
    let alphabet = &env.config.alphabet;
    let mut rows: Vec<(String, f64)> = (0..alphabet.len()).map(|i| (alphabet.display(i as u8).to_string(), leaves.weights[i])).collect();
    rows.push(("?".to_string(), leaves.weights[leaves::TILE_SLOTS - 1]));
    rows.sort_by(|a, b| b.1.total_cmp(&a.1));
    let line: Vec<String> = rows.iter().map(|(l, w)| format!("{l} {w:+.1}")).collect();
    println!("zsetononkénti értékek: {}", line.join(", "));
}
