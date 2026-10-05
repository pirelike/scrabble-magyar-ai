//! A ténylegesen használt „kockázatos” szóalakok listájának előállítása (`dict/hu_attested.txt`; nem része a
//! szervernek).
//!
//! A beépített szóellenőrző néhány levezetést csak akkor fogad el, ha az alak a használatban ténylegesen előfordul:
//! melléknév + birtokos személyjel (KEDVESEM, de TAROM), -ék családi többes (SZOMSZÉDÉK, de ÉJÉK), -ul/-ül
//! (FELESÉGÜL, de BLÖKIÜL), -s foglalkozásnév és -né képző. Ez az eszköz egy szógyakorisági listából kiválogatja azokat
//! az alakokat, amelyek csak ilyen levezetéssel érvényesek, és legalább `--min-count`-szor előfordulnak. A
//! feliratkorpusz zaját az elírás-szűrő csökkenti: kimarad az alak, ha egy legalább `TYPO_RATIO`-szor gyakoribb,
//! kockázat nélkül érvényes szó ékezetek nélkül azonos vele, egy betű beszúrásával kapható belőle (KORUL ← KÖRÜL,
//! TAROM ← TARTOM), vagy egy megkettőzött betűje a hiba (PROFII ← PROFI). A tulajdonnévből képzett alakokat (ALISA ←
//! Ali) a szóellenőrző kis- és nagybetűérzékeny szótári keresése eleve kizárja: a nagybetűs szótő kisbetűs alakkal
//! nem található meg.
//!
//! Forrás: Hermit Dave, FrequencyWords (OpenSubtitles 2018, magyar), CC BY-SA 4.0
//! <https://github.com/hermitdave/FrequencyWords> — content/2018/hu/hu_full.txt (soronként: „szó darabszám”).
//!
//! Használat: `build_attested hu_full.txt [--min-count 2] [--out dict/hu_attested.txt] [--with-counts]`

use scrabble::config;
use scrabble::tiles::tokenize_word;
use std::collections::{BTreeMap, HashMap, HashSet};

const TYPO_RATIO: u64 = 10;
const LETTERS: &str = "aábcdeéfghiíjklmnoóöőpqrstuúüűvwxyz";

fn strip_accents(word: &str) -> String {
    word.chars()
        .map(|c| match c {
            'á' => 'a',
            'é' => 'e',
            'í' => 'i',
            'ó' | 'ö' | 'ő' => 'o',
            'ú' | 'ü' | 'ű' => 'u',
            other => other,
        })
        .collect()
}

fn is_hungarian_word(word: &str) -> bool {
    let n = word.chars().count();
    (2..=15).contains(&n) && word.chars().all(|c| "abcdefghijklmnopqrstuvwxyzáéíóöőúüű".contains(c)) && word.chars().any(|c| "aáeéiíoóöőuúüű".contains(c))
}

/// {szó: darabszám} a legalább `min_count`-szor előforduló kisbetűs szavakra.
fn load_counts(path: &str, min_count: u64) -> std::io::Result<HashMap<String, u64>> {
    let text = String::from_utf8_lossy(&std::fs::read(path)?).to_string();
    let mut counts = HashMap::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if let [word, count] = parts[..] {
            if let Ok(count) = count.parse::<u64>() {
                if count >= min_count {
                    counts.insert(word.to_string(), count);
                }
            }
        }
    }
    Ok(counts)
}

/// Egy sokkal gyakoribb, kockázat nélkül érvényes szó elírása-e (ékezet hiánya, kimaradt vagy megkettőzött betű)?
fn is_typo(checker: &scrabble::affix::AffixChecker, word: &str, count: u64, frequent: &HashMap<String, u64>, by_skeleton: &HashMap<String, Vec<String>>) -> bool {
    let limit = TYPO_RATIO * count;
    let chars: Vec<char> = word.chars().collect();
    let mut neighbours: HashSet<String> = by_skeleton.get(&strip_accents(word)).into_iter().flatten().cloned().collect();
    for i in 0..=chars.len() {
        for letter in LETTERS.chars() {
            let mut candidate: Vec<char> = chars.clone();
            candidate.insert(i, letter);
            neighbours.insert(candidate.into_iter().collect());
        }
    }
    // megkettőzött betű (PROFII ← PROFI)
    for i in 1..chars.len() {
        if chars[i] == chars[i - 1] {
            let mut candidate = chars.clone();
            candidate.remove(i);
            neighbours.insert(candidate.into_iter().collect());
        }
    }
    neighbours.remove(word);
    neighbours.iter().any(|n| frequent.get(n).copied().unwrap_or(0) >= limit && checker.valid_without_risky(n))
}

fn usage() -> ! {
    eprintln!("használat: build_attested GYAKORISÁGI_LISTA [--min-count 2] [--out dict/hu_attested.txt] [--with-counts]");
    std::process::exit(2)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut list, mut min_count, mut out, mut with_counts) = (None::<String>, 2u64, config::dict_dir().join("hu_attested.txt").to_string_lossy().to_string(), false);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--min-count" => {
                min_count = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or_else(|| usage());
                i += 1;
            }
            "--out" => {
                out = args.get(i + 1).cloned().unwrap_or_else(|| usage());
                i += 1;
            }
            "--with-counts" => with_counts = true,
            other if list.is_none() && !other.starts_with("--") => list = Some(other.to_string()),
            _ => usage(),
        }
        i += 1;
    }
    let Some(list) = list else { usage() };

    let dir = config::dict_dir();
    let checker = scrabble::affix::AffixChecker::load(&dir.join("hu_HU.aff"), &dir.join("hu_HU.dic"), None).unwrap_or_else(|e| {
        eprintln!("hiba: a szótár nem tölthető be: {e}");
        std::process::exit(1)
    });
    let frequent = load_counts(&list, TYPO_RATIO * min_count).unwrap_or_else(|e| {
        eprintln!("hiba: {list}: {e}");
        std::process::exit(1)
    });
    let mut by_skeleton: HashMap<String, Vec<String>> = HashMap::new();
    for word in frequent.keys() {
        by_skeleton.entry(strip_accents(word)).or_default().push(word.clone());
    }

    let all = load_counts(&list, min_count).unwrap_or_default();
    let mut attested: BTreeMap<String, u64> = BTreeMap::new();
    let mut typos = 0;
    let mut sorted: Vec<(&String, &u64)> = all.iter().collect();
    sorted.sort();
    for (n, (word, count)) in sorted.into_iter().enumerate() {
        if is_hungarian_word(word) && tokenize_word(&word.to_uppercase()).is_some() && checker.needs_attestation(word) {
            if is_typo(&checker, word, *count, &frequent, &by_skeleton) {
                typos += 1;
            } else {
                attested.insert(word.clone(), *count);
            }
        }
        if n % 100_000 == 0 {
            eprintln!("{n} szó, {} találat", attested.len());
        }
    }

    let mut text = format!(
        "# A beépített szóellenőrző \"kockázatos\" levezetéseinek (melléknév + birtokos személyjel, -ék,\n\
         # -ul/-ül, -s foglalkozásnév, -né) ténylegesen használt alakjai — lásd src/affix.rs.\n\
         # Előállítva: build_attested eszköz (legalább {min_count} előfordulás, elírásszűrővel; {} szó).\n\
         # Forrás: Hermit Dave, FrequencyWords (OpenSubtitles 2018, magyar), CC BY-SA 4.0\n\
         #   https://github.com/hermitdave/FrequencyWords\n\
         # Ez a fájl (a forrásból származtatott adat) szintén CC BY-SA 4.0 licencű.\n",
        attested.len()
    );
    for (word, count) in &attested {
        if with_counts {
            text.push_str(&format!("{word} {count}\n"));
        } else {
            text.push_str(&format!("{word}\n"));
        }
    }
    std::fs::write(&out, text).unwrap_or_else(|e| {
        eprintln!("hiba: {out}: {e}");
        std::process::exit(1)
    });
    eprintln!("{} szó -> {out} (kiszűrve: {typos} elírás)", attested.len());
}
