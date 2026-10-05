//! A parancssori eszközök (`word_review`, `bot_arena`, `build_attested`) valódi futtatással (a Python
//! `test_word_review.py` `TestTool` osztályának megfelelője).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scrabble-tools-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(bin: &str, args: &[&str]) -> Output {
    Command::new(bin).args(args).output().expect("eszköz")
}

fn word_review(args: &[&str]) -> Output {
    run(env!("CARGO_BIN_EXE_word_review"), args)
}

fn stdout(output: &Output) -> String {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path).unwrap().lines().filter(|l| !l.is_empty()).map(|l| l.to_string()).collect()
}

fn sample(dir: &Path, count: usize, seed: u64, extra: &[&str]) -> Vec<String> {
    let out = dir.join(format!("sample-{seed}.txt"));
    let rejected = dir.join("hu_rejected.txt");
    let (count, seed) = (count.to_string(), seed.to_string());
    let mut args = vec!["--rejected", rejected.to_str().unwrap(), "sample", &count, "--seed", &seed, "--out", out.to_str().unwrap()];
    args.extend_from_slice(extra);
    stdout(&word_review(&args));
    lines(&out)
}

#[test]
fn the_sample_is_valid_distinct_and_reproducible() {
    let dir = temp_dir("sample");
    let words = sample(&dir, 30, 1, &[]);
    assert_eq!(words.len(), 30);
    assert_eq!(words.iter().collect::<HashSet<_>>().len(), 30);
    assert!(words.iter().all(|w| *w == w.to_lowercase()));
    assert_eq!(sample(&dir, 30, 1, &[]), words);
    assert_ne!(sample(&dir, 30, 2, &[]), words);
}

#[test]
fn the_sample_can_skip_earlier_batches() {
    let dir = temp_dir("exclude");
    let first = sample(&dir, 60, 3, &[]);
    let skip = dir.join("done.txt");
    std::fs::write(&skip, first.join("\n") + "\n").unwrap();
    let again = sample(&dir, 60, 3, &["--exclude", skip.to_str().unwrap()]);
    assert!(again.iter().all(|w| !first.contains(w)));
}

#[test]
fn the_sample_skips_listed_words() {
    let dir = temp_dir("listed");
    let shipped = scrabble::dictionary::load_rejected(&scrabble::config::dict_dir().join("hu_rejected.txt"));
    let words = sample(&dir, 200, 4, &[]);
    // az alapértelmezett lista szavai a szótárból is hiányoznak, ezért nem kerülhetnek a mintába
    assert!(words.iter().all(|w| !shipped.contains(w)));
}

#[test]
fn apply_merges_sorted_and_keeps_the_header() {
    let dir = temp_dir("apply");
    let rejected = dir.join("hu_rejected.txt");
    std::fs::write(&rejected, "# saját fejléc\n# második sor\nbarack\n").unwrap();
    let proposal = dir.join("p.txt");
    std::fs::write(&proposal, "# ítéletek\nkörte\nAlma\nbarack\nalmtá\n").unwrap();
    let out = stdout(&word_review(&["--rejected", rejected.to_str().unwrap(), "apply", proposal.to_str().unwrap()]));
    assert_eq!(std::fs::read_to_string(&rejected).unwrap(), "# saját fejléc\n# második sor\nalma\nbarack\nkörte\n");
    assert!(out.contains("felvéve: 2") && out.contains("már a listán volt: 1") && out.contains("almtá"), "{out}");
}

#[test]
fn apply_is_idempotent() {
    let dir = temp_dir("idem");
    let rejected = dir.join("hu_rejected.txt");
    let proposal = dir.join("p.txt");
    std::fs::write(&proposal, "alma\nkörte\n").unwrap();
    let args = ["--rejected", rejected.to_str().unwrap(), "apply", proposal.to_str().unwrap()];
    stdout(&word_review(&args));
    let first = std::fs::read_to_string(&rejected).unwrap();
    stdout(&word_review(&args));
    assert_eq!(std::fs::read_to_string(&rejected).unwrap(), first);
    assert!(first.starts_with('#') && first.ends_with("alma\nkörte\n"));
}

#[test]
fn a_dry_run_changes_nothing() {
    let dir = temp_dir("dry");
    let rejected = dir.join("hu_rejected.txt");
    let proposal = dir.join("p.txt");
    std::fs::write(&proposal, "alma\n").unwrap();
    let out = stdout(&word_review(&["--rejected", rejected.to_str().unwrap(), "apply", proposal.to_str().unwrap(), "--dry-run"]));
    assert!(!rejected.exists());
    assert!(out.contains("próbafuttatás"), "{out}");
}

#[test]
fn stats_counts_the_list() {
    let dir = temp_dir("stats");
    let rejected = dir.join("hu_rejected.txt");
    std::fs::write(&rejected, "# x\nalma\nkörte\n").unwrap();
    assert!(stdout(&word_review(&["--rejected", rejected.to_str().unwrap(), "stats"])).contains(": 2"));
}

#[test]
fn invalid_usage_is_refused() {
    assert_eq!(word_review(&[]).status.code(), Some(2));
    assert_eq!(word_review(&["nincs-ilyen"]).status.code(), Some(2));
    assert_eq!(run(env!("CARGO_BIN_EXE_bot_arena"), &["match", "3"]).status.code(), Some(2));
    assert_eq!(run(env!("CARGO_BIN_EXE_bot_arena"), &["match", "3", "11"]).status.code(), Some(2));
}

#[test]
fn the_bot_arena_plays_games() {
    let out = stdout(&run(env!("CARGO_BIN_EXE_bot_arena"), &["match", "2", "9", "-n", "4", "-j", "2"]));
    assert!(out.contains("2. fokozat vs 9.:"), "{out}");
    // a magasabb fokozat nyer: az elsőnek (2.) alig van esélye
    let percent: f64 = out.split('(').nth(1).unwrap().split('%').next().unwrap().parse().unwrap();
    assert!(percent <= 25.0, "{out}");
    let adapt = stdout(&run(env!("CARGO_BIN_EXE_bot_arena"), &["adapt", "3", "-n", "2", "-j", "2"]));
    assert!(adapt.contains("igazodó robot vs 3. fokozat"), "{adapt}");
}

#[test]
fn build_attested_keeps_used_forms_and_drops_typos() {
    let dir = temp_dir("attested");
    let list = dir.join("freq.txt");
    std::fs::write(&list, "kedvesem 100\ntartom 5000\ntarom 10\nszomszédék 40\nalma 90000\n").unwrap();
    let out = dir.join("att.txt");
    let output = run(env!("CARGO_BIN_EXE_build_attested"), &[list.to_str().unwrap(), "--out", out.to_str().unwrap()]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let words: Vec<String> = std::fs::read_to_string(&out).unwrap().lines().filter(|l| !l.starts_with('#') && !l.is_empty()).map(|l| l.to_string()).collect();
    assert_eq!(words, vec!["kedvesem", "szomszédék"]); // a TAROM a TARTOM elírása; az ALMA nem kockázatos
}
