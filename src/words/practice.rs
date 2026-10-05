//! Gyakorló módok: „Melyik szó érvényes?” kvíz, betűvadászat / bingó-edző és a rövid szavak listája.
//!
//! Állapotmentes: a kvíz kérdései csak a szót tartalmazzák, a választ a szerver a játék saját szótárával
//! ellenőrzi (`check_answer`); a betűvadászat kezét (`make_rack`) a megoldásokkal együtt kapja a kliens, a nem
//! listázott szavakat `check_rack_word` bírálja el. Az érvénytelen kvízszavak hihető „félreírások”: egy érvényes
//! szó egy zsetonnyi módosítása, vagy (csapda mód) egy magánhangzó hosszúságának cseréje (a↔á...).

use crate::ai;
use crate::dictionary;
use crate::tiles::{self, TILE_DISTRIBUTION, Tile, forms_digraph, tokenize_word, word_base_score};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use rand::seq::SliceRandom;
use rand::{Rng, RngExt};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

pub const QUESTION_COUNT: usize = 10;
pub const MAX_QUESTIONS: usize = 30;
pub const QUIZ_MODES: [&str; 4] = ["mixed", "2", "3", "tricky"];
/// a vegyes kvíz szavainak hossza zsetonban
const MIN_TILES: usize = 3;
const MAX_TILES: usize = 9;
/// az érvényes kérdések ekkora része ragozott alak
const INFLECTED_SHARE: f64 = 0.3;
pub const SHORT_LENGTHS: [usize; 2] = [2, 3];

pub const RACK_SIZE: usize = 7;
/// mind a hét zseton kirakása (mint a játékban)
pub const BINGO_BONUS: u32 = 50;
pub const RACK_KINDS: [&str; 2] = ["hunt", "bingo"];
/// a betűvadászat szólistájának felső korlátja
const MAX_RACK_WORDS: usize = 200;
const HUNT_MIN_WORDS: usize = 12;
const HUNT_MAX_WORDS: usize = 150;
/// a „jó” kézben van legalább ekkora (zsetonos) szó
const HUNT_MIN_LONGEST: usize = 5;
/// a bingó-kezek ekkora része szótári tőszóból indul (a ragozott alakok gyakran furcsák)
const BINGO_STEM_SHARE: f64 = 0.75;

/// Hosszú ↔ rövid magánhangzó párok: a magyar helyesírás legtipikusabb csapdái.
fn length_pair(c: char) -> Option<char> {
    Some(match c {
        'a' => 'á',
        'e' => 'é',
        'i' => 'í',
        'o' => 'ó',
        'ö' => 'ő',
        'u' => 'ú',
        'ü' => 'ű',
        'á' => 'a',
        'é' => 'e',
        'í' => 'i',
        'ó' => 'o',
        'ő' => 'ö',
        'ú' => 'u',
        'ű' => 'ü',
        _ => return None,
    })
}

static VOWEL_TILES: Lazy<Vec<Tile>> = Lazy::new(|| tiles::letters().filter(|t| t.is_vowel()).collect());
static CONSONANT_TILES: Lazy<Vec<Tile>> = Lazy::new(|| tiles::letters().filter(|t| !t.is_vowel()).collect());

/// A 2–3 zsetonos szavak listáinak gyorsítótára ürítése (admin).
pub fn clear_short_cache() {
    SHORT_CACHE.lock().clear();
}

static SHORT_CACHE: Lazy<Mutex<HashMap<usize, (u64, Vec<Value>)>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static STEM_CACHE: OnceLock<Vec<String>> = OnceLock::new();

/// Véletlen, egymástól különböző indexek (mint a Python `random.sample`).
pub fn sample_indices<R: Rng + ?Sized>(rng: &mut R, n: usize, k: usize) -> Vec<usize> {
    let k = k.min(n);
    if k * 3 < n {
        let mut chosen = Vec::with_capacity(k);
        let mut seen = HashSet::new();
        while chosen.len() < k {
            let i = rng.random_range(0..n);
            if seen.insert(i) {
                chosen.push(i);
            }
        }
        chosen
    } else {
        let mut indices: Vec<usize> = (0..n).collect();
        for i in 0..k {
            let j = rng.random_range(i..n);
            indices.swap(i, j);
        }
        indices.truncate(k);
        indices
    }
}

fn choose<'a, T, R: Rng + ?Sized>(rng: &mut R, items: &'a [T]) -> &'a T {
    &items[rng.random_range(0..items.len())]
}

/// A gyakorló módok lusta adatainak előállítása (szerverindításkor: az első kérés ne várjon másodperceket).
pub fn warm_up() {
    stem_words();
    for length in SHORT_LENGTHS {
        short_words(length);
    }
}

fn is_valid(word: &str) -> bool {
    dictionary::is_word_valid(word)
}

fn has_vowel(word: &str) -> bool {
    word.chars().any(|c| "aáeéiíoóöőuúüű".contains(c))
}

/// A szótár tőszavai (nagybetűs): csak kirakható, magánhangzós szavak.
pub fn stem_words() -> &'static [String] {
    STEM_CACHE.get_or_init(|| {
        let Some(checker) = dictionary::get_checker() else { return Vec::new() };
        let mut stems: Vec<String> = checker
            .entry_words()
            .filter(|w| {
                let n = w.chars().count();
                (2..=15).contains(&n) && tokenize_word(w).is_some() && has_vowel(w) && **w == w.to_lowercase() && w.chars().any(|c| c.is_lowercase())
            })
            .map(|w| w.to_uppercase())
            .collect();
        stems.sort();
        stems
    })
}

/// Az összes érvényes, pontosan `length` zsetonos szó pontértékkel: [{'word', 'score'}], ábécé szerint.
pub fn short_words(length: usize) -> Vec<Value> {
    assert!(SHORT_LENGTHS.contains(&length), "a rövid szavak hossza 2 vagy 3 zseton");
    let version = dictionary::rejected_version();
    {
        let cache = SHORT_CACHE.lock();
        if let Some((v, list)) = cache.get(&length)
            && *v == version
        {
            return list.clone();
        }
    }
    // egy újonnan elutasított szó kikerül a listából
    let letters: Vec<Tile> = tiles::letters().collect();
    let mut candidates: HashSet<String> = HashSet::new();
    let mut indices = vec![0usize; length];
    loop {
        let word: String = indices.iter().map(|i| letters[*i].as_str()).collect();
        if tokenize_word(&word).map(|t| t.len()) == Some(length) && has_vowel_upper(&word) {
            candidates.insert(word);
        }
        // következő kombináció
        let mut pos = length;
        loop {
            if pos == 0 {
                break;
            }
            pos -= 1;
            indices[pos] += 1;
            if indices[pos] < letters.len() {
                break;
            }
            indices[pos] = 0;
            if pos == 0 {
                pos = usize::MAX;
                break;
            }
        }
        if pos == usize::MAX {
            break;
        }
    }
    let valid = dictionary::filter_valid(candidates.iter().map(|s| s.as_str()));
    let mut words: Vec<String> = valid.into_iter().collect();
    words.sort();
    let list: Vec<Value> = words.iter().map(|w| json!({"word": w, "score": word_base_score(w)})).collect();
    SHORT_CACHE.lock().insert(length, (version, list.clone()));
    list
}

fn has_vowel_upper(word: &str) -> bool {
    word.chars().any(|c| tiles::VOWELS.contains(c))
}

fn tiles_to_word(tokens: &[Tile]) -> String {
    tokens.iter().map(|t| t.as_str()).collect()
}

/// Egy érvényes szó egy zsetonnyi módosítása (hihető, de többnyire érvénytelen alak).
fn mutate<R: Rng + ?Sized>(word: &str, rng: &mut R) -> Option<String> {
    let mut tokens = tokenize_word(word)?;
    if tokens.len() < 2 {
        return None;
    }
    let kind = *choose(rng, &["replace", "replace", "swap", "insert", "delete"]);
    let i = rng.random_range(0..tokens.len());
    match kind {
        "replace" => {
            let pool: &Vec<Tile> = if tokens[i].is_vowel() { &VOWEL_TILES } else { &CONSONANT_TILES };
            let options: Vec<Tile> = pool.iter().copied().filter(|t| *t != tokens[i]).collect();
            tokens[i] = *choose(rng, &options);
        }
        "swap" if tokens.len() >= 2 => {
            let j = i.min(tokens.len() - 2);
            tokens.swap(j, j + 1);
        }
        "insert" => {
            let pool: &Vec<Tile> = if rng.random::<f64>() < 0.4 { &VOWEL_TILES } else { &CONSONANT_TILES };
            let tile = *choose(rng, pool);
            tokens.insert(i, tile);
        }
        "delete" if tokens.len() > 2 => {
            tokens.remove(i);
        }
        _ => {}
    }
    Some(tiles_to_word(&tokens))
}

/// Egy magánhangzó hosszúságának cseréje (a↔á, e↔é, o↔ó, ö↔ő, u↔ú, ü↔ű, i↔í): a csapda-mód félreírása.
fn mutate_length<R: Rng + ?Sized>(word: &str, rng: &mut R) -> Option<String> {
    let chars: Vec<char> = word.chars().collect();
    let spots: Vec<usize> = chars.iter().enumerate().filter(|(_, c)| c.to_lowercase().next().and_then(length_pair).is_some()).map(|(i, _)| i).collect();
    if spots.is_empty() {
        return None;
    }
    let i = *choose(rng, &spots);
    let lower = chars[i].to_lowercase().next().unwrap();
    let swapped = length_pair(lower).unwrap().to_uppercase().next().unwrap();
    let mut result = chars;
    result[i] = swapped;
    Some(result.into_iter().collect())
}

/// `count` érvénytelen, de hihető szó a `sources` szavaiból (módosítással). Egy forrásszóból lehetőleg csak egy
/// hamis szó készül (különben a kérdések egymás átírásai lennének).
fn fake_words<R: Rng + ?Sized>(
    sources: &[String],
    count: usize,
    rng: &mut R,
    length: Option<usize>,
    mutator: fn(&str, &mut R) -> Option<String>,
) -> Vec<String> {
    let mut fakes: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut order: Vec<String> = Vec::new();
    let mut attempts = 0;
    while fakes.len() < count && attempts < 200 {
        attempts += 1;
        if order.is_empty() {
            // kifogytak a forrásszavak: újra végigmegyünk rajtuk
            order = sources.to_vec();
            order.shuffle(rng);
        }
        let mut batch: Vec<String> = Vec::new();
        for _ in 0..count * 4 {
            let Some(source) = order.pop() else { break };
            let Some(fake) = mutator(&source, rng) else { continue };
            let Some(tokens) = tokenize_word(&fake).filter(|t| !t.is_empty()) else { continue };
            if seen.contains(&fake) || !tokens.iter().any(|t| t.is_vowel()) {
                continue;
            }
            if length.is_some_and(|l| tokens.len() != l) {
                continue;
            }
            if fake.chars().count() > 15 || tokens.len() > MAX_TILES {
                continue;
            }
            seen.insert(fake.clone());
            batch.push(fake);
        }
        let valid = dictionary::filter_valid(batch.iter().map(|s| s.as_str()));
        fakes.extend(batch.into_iter().filter(|w| !valid.contains(w)));
    }
    fakes.truncate(count);
    fakes
}

fn accent_sensitive(word: &str) -> bool {
    word.chars().any(|c| c.to_lowercase().next().and_then(length_pair).is_some())
}

fn tile_len(word: &str) -> usize {
    tokenize_word(word).map(|t| t.len()).unwrap_or(0)
}

/// Kvízkérdések: a fele érvényes, a fele érvénytelen szó, véletlen sorrendben.
///
/// `mode`: `mixed` (vegyes hosszúság), `2` / `3` (csak annyi zsetonos szavak) vagy `tricky` (csapdák: az
/// érvénytelen szavak egy érvényes szó magánhangzójának hosszúságcseréjével készülnek).
/// Visszatér: a szavak listája (a választ nem tartalmazza).
pub fn make_quiz<R: Rng + ?Sized>(count: usize, mode: &str, rng: &mut R) -> Result<Vec<String>, String> {
    if !QUIZ_MODES.contains(&mode) {
        return Err("ismeretlen kvízmód".to_string());
    }
    let count = count.clamp(2, MAX_QUESTIONS);
    let valid_count = count / 2 + usize::from(count % 2 == 1 && rng.random::<f64>() < 0.5);
    let fake_count = count - valid_count;

    let (valid, fakes): (Vec<String>, Vec<String>);
    if mode == "mixed" || mode == "tricky" {
        let tricky = mode == "tricky";
        let stems_all = stem_words();
        let take = stems_all.len().min(valid_count * if tricky { 14 } else { 6 });
        let stems: Vec<String> = sample_indices(rng, stems_all.len(), take)
            .into_iter()
            .map(|i| stems_all[i].clone())
            .filter(|w| (MIN_TILES..=MAX_TILES).contains(&tile_len(w)) && (!tricky || accent_sensitive(w)))
            .collect();
        let stem_set: HashSet<&String> = stems.iter().collect();
        let inflected_target = (valid_count as f64 * INFLECTED_SHARE).round_ties_even() as usize;
        let vocab = ai::get_vocabulary();
        let take = vocab.len().min(inflected_target * if tricky { 24 } else { 8 } + 8);
        let inflected: Vec<String> = sample_indices(rng, vocab.len(), take)
            .into_iter()
            .map(|i| vocab.word(i).to_string())
            .filter(|w| (MIN_TILES..=MAX_TILES).contains(&tile_len(w)) && !stem_set.contains(w) && (!tricky || accent_sensitive(w)))
            .collect();
        let inflected: Vec<String> = inflected.into_iter().filter(|w| is_valid(w)).take(inflected_target).collect();
        let want = valid_count - inflected.len();
        let mut valid_list: Vec<String> = stems.iter().filter(|w| is_valid(w)).take(want).cloned().collect();
        valid_list.extend(inflected);
        let sources = if valid_list.is_empty() { stems.clone() } else { valid_list.clone() };
        let mutator: fn(&str, &mut R) -> Option<String> = if tricky { mutate_length::<R> } else { mutate::<R> };
        fakes = fake_words(&sources, fake_count, rng, None, mutator);
        valid = valid_list;
    } else {
        let length: usize = mode.parse().map_err(|_| "ismeretlen kvízmód".to_string())?;
        let words: Vec<String> = short_words(length).iter().map(|e| e["word"].as_str().unwrap_or("").to_string()).collect();
        let picked = sample_indices(rng, words.len(), valid_count.min(words.len()));
        valid = picked.into_iter().map(|i| words[i].clone()).collect();
        fakes = fake_words(&words, fake_count, rng, Some(length), mutate::<R>);
    }

    let mut seen = HashSet::new();
    let mut questions: Vec<String> = valid.into_iter().chain(fakes).filter(|w| seen.insert(w.clone())).collect();
    questions.shuffle(rng);
    Ok(questions)
}

/// Egy kvíz-válasz kiértékelése.
///
/// Visszatér: {'word', 'valid', 'correct', 'score', 'tiles', 'suggestions'} — vagy None, ha a szó alakilag
/// érvénytelen (nem magyar betűk, túl hosszú).
pub fn check_answer(word: &str, answer: bool) -> Option<Value> {
    let word = word.trim().to_uppercase();
    let n = word.chars().count();
    if !(2..=15).contains(&n) {
        return None;
    }
    let tokens = tokenize_word(&word)?;
    let valid = is_valid(&word);
    let suggestions: Vec<String> = if valid { Vec::new() } else { dictionary::suggest_words(&word, 3) };
    Some(json!({
        "word": word,
        "valid": valid,
        "correct": answer == valid,
        "score": word_base_score(&word),
        "tiles": tokens.iter().map(|t| t.as_str()).collect::<Vec<_>>(),
        "suggestions": suggestions,
    }))
}

// --- Betűvadászat / bingó-edző ---
//
// A kéz 7 betűzseton (joker nélkül). A szerver a kézzel együtt kiadja az összes kirakható szót is (a robot
// szókincséből, a szótárral megerősítve), így a kliens azonnal tud pontozni; a szókincsben nem szereplő, de
// érvényes szavakat `check_rack_word` bírálja el.

fn rack_pool() -> Vec<Tile> {
    let mut pool = Vec::new();
    for (i, (letter, _value, count)) in TILE_DISTRIBUTION.iter().enumerate() {
        if !letter.is_empty() {
            for _ in 0..*count {
                pool.push(Tile(i as u8));
            }
        }
    }
    pool
}

/// A zsetonok darabszáma belefér-e a játék zsákjába (pl. nincs négy Ó).
fn count_ok(tokens: &[Tile]) -> bool {
    let mut counts: HashMap<Tile, usize> = HashMap::new();
    for t in tokens {
        *counts.entry(*t).or_insert(0) += 1;
    }
    counts.iter().all(|(t, n)| *n <= TILE_DISTRIBUTION[t.0 as usize].2 as usize)
}

/// Az összes szó, amely a kéz zsetonjaiból kirakható (legalább 2 zseton).
///
/// Visszatér: [{'word', 'score', 'tiles'}] pontszám (majd ábécé) szerint; a pont a zsetonértékek összege (+50,
/// ha mind a hét zseton fogy). Egy szó legjobb kirakását számoljuk. A kétjegyű betű csak a saját zsetonjával
/// rakható ki: külön S és Z zsetonból nem lesz SZ.
pub fn rack_words(rack: &[Tile]) -> Vec<Value> {
    let vocab = ai::get_vocabulary();
    let mut counts: HashMap<Tile, i32> = HashMap::new();
    let mut types: Vec<Tile> = Vec::new();
    for t in rack {
        let c = counts.entry(*t).or_insert(0);
        if *c == 0 && !types.contains(t) {
            types.push(*t);
        }
        *c += 1;
    }
    let mut best: HashMap<String, (u32, usize)> = HashMap::new();

    #[allow(clippy::too_many_arguments)]
    fn walk(
        prefix: &mut String,
        used: usize,
        score: u32,
        last: Option<Tile>,
        rack_len: usize,
        types: &[Tile],
        counts: &mut HashMap<Tile, i32>,
        vocab: &ai::Vocabulary,
        best: &mut HashMap<String, (u32, usize)>,
    ) {
        if used >= 2 && vocab.contains(prefix) {
            let total = score + if used == RACK_SIZE { BINGO_BONUS } else { 0 };
            match best.get(prefix.as_str()) {
                Some((b, _)) if *b >= total => {}
                _ => {
                    best.insert(prefix.clone(), (total, used));
                }
            }
        }
        if used >= rack_len {
            return;
        }
        for &tile in types {
            if counts[&tile] <= 0 {
                continue;
            }
            if last.is_some_and(|l| forms_digraph(l.as_str(), tile.as_str())) {
                continue;
            }
            let len = prefix.len();
            prefix.push_str(tile.as_str());
            if vocab.has_prefix(prefix) {
                *counts.get_mut(&tile).unwrap() -= 1;
                walk(prefix, used + 1, score + tile.value(), Some(tile), rack_len, types, counts, vocab, best);
                *counts.get_mut(&tile).unwrap() += 1;
            }
            prefix.truncate(len);
        }
    }

    let mut prefix = String::new();
    walk(&mut prefix, 0, 0, None, rack.len(), &types, &mut counts, &vocab, &mut best);
    let valid = if best.is_empty() { HashSet::new() } else { dictionary::filter_valid(best.keys().map(|s| s.as_str())) };
    let mut words: Vec<(String, u32, usize)> = valid
        .into_iter()
        .map(|w| {
            let (score, used) = best[&w];
            (w, score, used)
        })
        .collect();
    words.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    words.into_iter().map(|(w, score, used)| json!({"word": w, "score": score, "tiles": used})).collect()
}

fn draw_rack<R: Rng + ?Sized>(rng: &mut R) -> Vec<Tile> {
    let pool = rack_pool();
    loop {
        let rack: Vec<Tile> = sample_indices(rng, pool.len(), RACK_SIZE).into_iter().map(|i| pool[i]).collect();
        let vowels = rack.iter().filter(|t| t.is_vowel()).count();
        if (2..=4).contains(&vowels) {
            return rack;
        }
    }
}

/// Olyan kéz, amelyből biztosan kirakható egy 7 zsetonos szó (a szótár egy véletlen szavából).
fn bingo_rack<R: Rng + ?Sized>(rng: &mut R) -> Result<Vec<Tile>, String> {
    let stems = stem_words();
    let vocab = ai::get_vocabulary();
    for _ in 0..400 {
        let use_stems = rng.random::<f64>() < BINGO_STEM_SHARE;
        let (n, get): (usize, Box<dyn Fn(usize) -> String>) =
            if use_stems { (stems.len(), Box::new(|i| stems[i].clone())) } else { (vocab.len(), Box::new(|i| vocab.word(i).to_string())) };
        for i in sample_indices(rng, n, 300) {
            let word = get(i);
            let chars = word.chars().count();
            if !(7..=14).contains(&chars) {
                continue;
            }
            if let Some(mut tokens) = tokenize_word(&word)
                && tokens.len() == RACK_SIZE
                && count_ok(&tokens)
                && is_valid(&word)
            {
                tokens.shuffle(rng);
                return Ok(tokens);
            }
        }
    }
    Err("nem sikerült bingó-kezet készíteni".to_string())
}

/// Egy gyakorló kéz a megoldásokkal.
///
/// `kind`: `hunt` (betűvadászat: sok szó, legalább egy 5+ zsetonos) vagy `bingo` (biztosan van 7 zsetonos szó).
/// Visszatér: {'kind', 'rack': [zsetonok], 'words': [{'word', 'score', 'tiles'}], 'total_score'}; bingónál a
/// `words` csak a 7 zsetonos (bingó) szavakat tartalmazza.
pub fn make_rack<R: Rng + ?Sized>(kind: &str, rng: &mut R) -> Result<Value, String> {
    if !RACK_KINDS.contains(&kind) {
        return Err("ismeretlen kéztípus".to_string());
    }
    let total = |words: &[Value]| words.iter().map(|w| w["score"].as_i64().unwrap_or(0)).sum::<i64>();
    if kind == "bingo" {
        let rack = bingo_rack(rng)?;
        let words: Vec<Value> = rack_words(&rack).into_iter().filter(|w| w["tiles"].as_u64() == Some(RACK_SIZE as u64)).collect();
        return Ok(json!({
            "kind": kind, "rack": rack.iter().map(|t| t.as_str()).collect::<Vec<_>>(), "total_score": total(&words), "words": words,
        }));
    }
    let mut best: Option<(Vec<Tile>, Vec<Value>)> = None;
    for _ in 0..40 {
        let rack = draw_rack(rng);
        let words = rack_words(&rack);
        if words.is_empty() {
            continue;
        }
        let longest = words.iter().map(|w| w["tiles"].as_u64().unwrap_or(0)).max().unwrap_or(0) as usize;
        if (HUNT_MIN_WORDS..=HUNT_MAX_WORDS).contains(&words.len()) && longest >= HUNT_MIN_LONGEST {
            best = Some((rack, words));
            break;
        }
        if best.as_ref().is_none_or(|(_, b)| words.len() > b.len()) {
            best = Some((rack, words));
        }
    }
    let (rack, mut words) = best.ok_or("nem sikerült kezet készíteni")?;
    words.truncate(MAX_RACK_WORDS);
    Ok(json!({
        "kind": kind, "rack": rack.iter().map(|t| t.as_str()).collect::<Vec<_>>(), "total_score": total(&words), "words": words,
    }))
}

/// A szó kirakása a kéz zsetonjaiból: (legjobb pontszám, felhasznált zsetonok) vagy None. A kétjegyű betű (SZ,
/// CS...) csak a saját zsetonjával rakható ki, külön S + Z zsetonból nem (`split_ok`: csak annak eldöntésére,
/// hogy ez volt-e az akadály).
fn assign(word: &[char], counts: &mut HashMap<String, i32>, score: u32, last: &str, split_ok: bool) -> Option<(u32, Vec<String>)> {
    if word.is_empty() {
        return Some((score, Vec::new()));
    }
    let mut best: Option<(u32, Vec<String>)> = None;
    for size in 1..=2usize {
        if word.len() < size {
            continue;
        }
        let piece: String = word[..size].iter().collect();
        if counts.get(&piece).copied().unwrap_or(0) <= 0 || (!split_ok && forms_digraph(last, &piece)) {
            continue;
        }
        *counts.get_mut(&piece).unwrap() -= 1;
        let value = tiles::tile_value(&piece);
        let rest = assign(&word[size..], counts, score + value, &piece, split_ok);
        *counts.get_mut(&piece).unwrap() += 1;
        if let Some((rest_score, rest_tiles)) = rest
            && best.as_ref().is_none_or(|(b, _)| rest_score > *b)
        {
            let mut used = vec![piece];
            used.extend(rest_tiles);
            best = Some((rest_score, used));
        }
    }
    best
}

/// Egy beírt szó bírálata a kézhez.
///
/// Visszatér: {'ok', 'reason', 'word', 'tiles', 'score', 'bingo'}; a `reason`: `too_short`, `invalid_chars`,
/// `not_in_rack`, `split_digraph` (a kétjegyű betű külön zsetonokból állna), `not_a_word` (ok=False esetén).
pub fn check_rack_word(rack: &[String], word: &str) -> Value {
    let word = word.trim().to_uppercase();
    let fail = |reason: &str| json!({"ok": false, "reason": reason, "word": word, "tiles": [], "score": 0, "bingo": false});
    let chars: Vec<char> = word.chars().collect();
    if chars.len() < 2 {
        return fail("too_short");
    }
    if chars.len() > 15 || tokenize_word(&word).is_none() {
        return fail("invalid_chars");
    }
    let fresh_counts = || {
        let mut counts: HashMap<String, i32> = HashMap::new();
        for t in rack {
            *counts.entry(t.clone()).or_insert(0) += 1;
        }
        counts
    };
    let Some((score, tiles)) = assign(&chars, &mut fresh_counts(), 0, "", false) else {
        let split = assign(&chars, &mut fresh_counts(), 0, "", true);
        return fail(if split.is_some() { "split_digraph" } else { "not_in_rack" });
    };
    if !is_valid(&word) {
        return fail("not_a_word");
    }
    let bingo = tiles.len() == RACK_SIZE;
    json!({
        "ok": true, "reason": null, "word": word, "tiles": tiles,
        "score": score + if bingo { BINGO_BONUS } else { 0 }, "bingo": bingo,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    #[test]
    fn length_swap_mutation() {
        let mut rng = StdRng::seed_from_u64(0);
        assert_eq!(mutate_length("KRT", &mut rng), None);
        let swapped = mutate_length("ALMA", &mut rng).unwrap();
        assert!(swapped != "ALMA" && swapped.chars().count() == 4);
        assert_eq!(swapped.chars().zip("ALMA".chars()).filter(|(a, b)| a != b).count(), 1);
        assert_eq!(mutate_length("ŐZ", &mut rng).as_deref(), Some("ÖZ"));
    }

    #[test]
    fn every_length_pair_is_symmetric() {
        for c in "aáeéiíoóöőuúüű".chars() {
            assert_eq!(length_pair(length_pair(c).unwrap()), Some(c), "{c}");
        }
        assert_eq!(length_pair('k'), None);
    }

    #[test]
    fn tricky_fakes_come_from_different_words() {
        dictionary::warm_up();
        let sources: Vec<String> = ["ARANYÉR", "TERVEZD", "CELLULÓZ", "DEZODOR"].iter().map(|s| s.to_string()).collect();
        let fakes = fake_words(&sources, 4, &mut StdRng::seed_from_u64(1), None, mutate_length::<StdRng>);
        // négy különböző forrásszóból négy különböző hamis szó (nincs két átírás ugyanabból)
        assert_eq!(fakes.iter().collect::<HashSet<_>>().len(), 4, "{fakes:?}");
    }

    #[test]
    fn fake_words_are_plausible_edits() {
        dictionary::warm_up();
        let sources: Vec<String> = ["ALMA", "KÖRTE", "SZÉK"].iter().map(|s| s.to_string()).collect();
        let fakes = fake_words(&sources, 8, &mut StdRng::seed_from_u64(7), None, mutate::<StdRng>);
        assert_eq!(fakes.len(), 8);
        assert!(dictionary::filter_valid(fakes.iter().map(|s| s.as_str())).is_empty(), "{fakes:?}");
        assert!(fakes.iter().all(|w| has_vowel_upper(w)), "{fakes:?}");
    }

    #[test]
    fn a_rack_must_fit_the_bag() {
        let tiles = |letters: &[&str]| -> Vec<Tile> { letters.iter().map(|l| Tile::from_str(l).unwrap()).collect() };
        assert!(count_ok(&tiles(&["A", "A", "A", "Ó", "Ó", "Ó"])));
        assert!(!count_ok(&tiles(&["Ó", "Ó", "Ó", "Ó"])));
        assert!(!count_ok(&tiles(&["CS", "CS"])));
    }
}
