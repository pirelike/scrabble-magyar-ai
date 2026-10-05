//! Szótár-építő eszközök: véletlen szavak mintavétele átnézésre (AI-val vagy kézzel) és az elutasított szavak tartós
//! listájának (`dict/hu_rejected.txt`) kezelése.
//!
//! A hu_HU szótár helyesírás-ellenőrzésre készült, ezért a Scrabble-ban sok olyan szót is elfogad, amelyet senki sem
//! tekintene rendes szónak. Az alkalmazásban a gyakorló módok között a „Szótár-építő” ad véletlen szavakat az
//! embereknek; ez az eszköz ugyanezt teszi tömegesen, hogy egy AI-modell is átnézhessen ezreket:
//!
//! ```text
//! word_review sample 5000 --seed 1 --out sample.txt   # átnézendő szavak, soronként egy
//! # ... az átnézés után a „nem rendes szó” ítéletek egy fájlba, soronként egy szó ...
//! word_review apply rejected.txt                      # felvétel a dict/hu_rejected.txt-be
//! word_review stats
//! ```
//!
//! A `sample` csak olyan szavakat ad, amelyeket a szótár most elfogad (a tőszavak és a gyakori ragozott alakok 7:3
//! arányban, mint az alkalmazásban). Az `apply` ellenőriz: csak a szótár által éppen elfogadott szó kerülhet a listára
//! (az elírt szavakat jelzi és kihagyja). A listán lévő szavakat a szótár pontos alakban zárja ki, és a robot
//! szókincséből is kimaradnak. Az ítéletek az alkalmazás Szótár-építőjében emberileg is felülvizsgálhatók.
//!
//! Közös kapcsoló: `--rejected FÁJL` (az elutasított szavak listája; alapból a `dict/hu_rejected.txt`).

use rand::SeedableRng;
use rand::rngs::StdRng;
use scrabble::{dictionary, word_review};
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

const DEFAULT_HEADER: &str = "\
# Elutasított szavak: a hu_HU szótár elfogadná őket, de átnézés szerint nem rendes (használatos) szavak.
# A szótár a pontos szóalakot zárja ki (a ragozott alakokat nem), a robot szókincséből is kimaradnak.
# Karbantartás: word_review eszköz (sample → átnézés → apply). Soronként egy kisbetűs szó, `#` megjegyzés.
";

fn usage() -> ! {
    eprintln!(
        "használat: word_review [--rejected FÁJL] sample DARAB [--seed MAG] [--out FÁJL] [--exclude FÁJL]...\n\
         \x20          word_review [--rejected FÁJL] apply FÁJL [--dry-run]\n\
         \x20          word_review [--rejected FÁJL] stats"
    );
    std::process::exit(2)
}

/// A fájl elején álló megjegyzés-blokk (a szavak előtt); hiányzó fájlnál az alapértelmezett fejléc.
fn split_header(path: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(path) else { return DEFAULT_HEADER.to_string() };
    let header: Vec<&str> = text.lines().take_while(|l| l.trim().is_empty() || l.trim_start().starts_with('#')).collect();
    if header.iter().any(|h| !h.trim().is_empty()) { format!("{}\n", header.join("\n").trim_end_matches('\n')) } else { DEFAULT_HEADER.to_string() }
}

fn write_rejected(path: &Path, words: &BTreeSet<String>) -> std::io::Result<()> {
    let mut text = split_header(path);
    for word in words {
        text.push_str(word);
        text.push('\n');
    }
    std::fs::write(path, text)
}

fn cmd_sample(rejected: &Path, args: &[String]) {
    let (mut count, mut seed, mut out, mut exclude_files) = (None::<usize>, None::<u64>, None::<String>, Vec::<String>::new());
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--seed" => {
                seed = args.get(i + 1).and_then(|v| v.parse().ok());
                i += 1;
            }
            "--out" => {
                out = args.get(i + 1).cloned();
                i += 1;
            }
            "--exclude" => {
                exclude_files.extend(args.get(i + 1).cloned());
                i += 1;
            }
            other if count.is_none() => count = other.parse().ok(),
            _ => usage(),
        }
        i += 1;
    }
    let Some(count) = count else { usage() };
    let mut exclude: HashSet<String> = dictionary::load_rejected(rejected);
    for path in exclude_files {
        exclude.extend(dictionary::load_rejected(Path::new(&path)));
    }
    let mut rng: StdRng = match seed {
        Some(seed) => StdRng::seed_from_u64(seed),
        None => StdRng::from_rng(&mut rand::rng()),
    };
    let words = word_review::sample_words(count, &mut rng, &exclude);
    let text: String = words.iter().map(|w| format!("{}\n", w.to_lowercase())).collect();
    match out {
        Some(path) => {
            std::fs::write(&path, text).unwrap_or_else(|e| fail(&format!("{path}: {e}")));
            println!("{} szó -> {path}", words.len());
        }
        None => print!("{text}"),
    }
}

fn cmd_apply(rejected: &Path, args: &[String]) {
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let Some(file) = args.iter().find(|a| !a.starts_with("--")) else { usage() };
    let proposed: BTreeSet<String> = dictionary::load_rejected(Path::new(file)).into_iter().collect();
    let current: BTreeSet<String> = dictionary::load_rejected(rejected).into_iter().collect();
    let checker = dictionary::get_checker().unwrap_or_else(|| fail("a szótár nem tölthető be"));
    let (mut added, mut already, mut unknown) = (Vec::new(), Vec::new(), Vec::new());
    for word in &proposed {
        if current.contains(word) {
            already.push(word.clone());
        } else if word_review::normalize_str(word).is_none() || !checker.check(word) {
            unknown.push(word.clone()); // elírás vagy a szótár úgysem fogadja el: felesleges a listára tenni
        } else {
            added.push(word.clone());
        }
    }
    if !added.is_empty() && !dry_run {
        let mut all = current.clone();
        all.extend(added.iter().cloned());
        write_rejected(rejected, &all).unwrap_or_else(|e| fail(&format!("{}: {e}", rejected.display())));
    }
    println!(
        "felvéve: {}, már a listán volt: {}, a szótár úgysem fogadja el: {}{}",
        added.len(),
        already.len(),
        unknown.len(),
        if dry_run { " (próbafuttatás, a lista nem módosult)" } else { "" }
    );
    if !unknown.is_empty() {
        println!("kihagyva (nincs a szótárban): {}", unknown.join(", "));
    }
}

fn cmd_stats(rejected: &Path) {
    let shown = std::env::current_dir().ok().and_then(|cwd| rejected.strip_prefix(cwd).ok().map(|p| p.to_path_buf())).unwrap_or_else(|| rejected.to_path_buf());
    println!("elutasított szavak a listán ({}): {}", shown.display(), dictionary::load_rejected(rejected).len());
}

fn fail(message: &str) -> ! {
    eprintln!("hiba: {message}");
    std::process::exit(1)
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut rejected: PathBuf = dictionary::rejected_path();
    if let Some(i) = args.iter().position(|a| a == "--rejected") {
        let Some(path) = args.get(i + 1).cloned() else { usage() };
        rejected = PathBuf::from(path);
        args.drain(i..i + 2);
    }
    let Some(command) = args.first().cloned() else { usage() };
    let rest = &args[1..];
    match command.as_str() {
        "sample" | "apply" => {
            if !dictionary::warm_up() {
                fail("a szótár nem tölthető be");
            }
            if command == "sample" { cmd_sample(&rejected, rest) } else { cmd_apply(&rejected, rest) }
        }
        "stats" => cmd_stats(&rejected),
        _ => usage(),
    }
}
