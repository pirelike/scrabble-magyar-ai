//! Számítógépes ellenfél (AI): fokozatok, lépésgenerátor és döntési stratégia.
//!
//! Működés:
//!  * Szókincs: a beágyazott hu_HU szótár tőszavai és gyakori ragozott alakjai, egy rendezett, tömör
//!    pufferben (bináris kereséssel): nincs szükség nagy memóriájú trie-ra.
//!  * Lépésgenerálás: horgonyalapú (Appel–Jacobson) keresés vízszintes és függőleges irányban. A
//!    keresztszavak érvényességét a szótár dönti el (egyetlen tömeges híváson belül). A kiválasztott
//!    lépés minden szavát a játék saját szótára is ellenőrzi, mielőtt a robot lerakja.
//!  * Nehézségi szintek: 10 fokozat (1 = újonc … 10 = mester). Az alsó fokozatokon a robot minden
//!    körben egy véletlen célpontszámot vesz fel, és a hozzá legközelebbi pontszámú lépést rakja le;
//!    a felső fokozatokon a legjobban értékelt (pont + maradék zsetonok) lépést választja, egyre
//!    kisebb zajjal. A fokozatok erejét bot–bot mérés kalibrálta (lásd `profile`).

use rand::RngExt;
use serde_json::Value;

pub const MIN_LEVEL: u8 = 1;
pub const MAX_LEVEL: u8 = 10;
pub const DEFAULT_LEVEL: u8 = 6;

/// "Igazodik hozzám" mód: a robot az ember utolsó köreinek átlagához állítja az erejét.
pub const ADAPT_WINDOW: usize = 6; // az ember utolsó ennyi köre számít
pub const ADAPT_START_LEVEL: f64 = 5.0; // induló fokozat, amíg nincs adat
/// Mért átlagos pont/kör fokozatonként (bot–bot önjáték, ragozott alakokkal a szókincsben).
pub const LEVEL_STRENGTH: [f64; 10] = [4.3, 6.1, 7.4, 10.1, 12.7, 15.7, 18.4, 21.2, 24.6, 27.0];

/// A robot beállított nehézsége: fokozat (1–10) vagy "igazodik hozzám".
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum Difficulty {
    Level(u8),
    Auto,
}

impl Difficulty {
    pub fn to_json(self) -> Value {
        match self {
            Difficulty::Level(n) => Value::from(n),
            Difficulty::Auto => Value::from("auto"),
        }
    }

    /// A fokozat egész száma; az `Auto` nem fokozat.
    pub fn level(self) -> Option<u8> {
        match self {
            Difficulty::Level(n) => Some(n),
            Difficulty::Auto => None,
        }
    }
}

/// A korábbi háromfokozatú skála helye az új skálán.
fn legacy_level(text: &str) -> Option<u8> {
    match text {
        "easy" => Some(3),
        "medium" => Some(6),
        "hard" => Some(10),
        _ => None,
    }
}

/// A robot fokozata (1–10) a megadott értékből, vagy None, ha érvénytelen. Elfogadja az egész számot, a számot
/// szövegként ("6"), és a korábbi neveket ('easy' / 'medium' / 'hard').
pub fn parse_level(value: &Value) -> Option<u8> {
    match value {
        Value::Bool(_) => None,
        Value::Number(n) => {
            let int = n.as_i64().or_else(|| n.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64))?;
            if (MIN_LEVEL as i64..=MAX_LEVEL as i64).contains(&int) { Some(int as u8) } else { None }
        }
        Value::String(s) => {
            let text = s.trim().to_lowercase();
            legacy_level(&text).or_else(|| text.parse::<u8>().ok().filter(|n| (MIN_LEVEL..=MAX_LEVEL).contains(n) && n.to_string() == text))
        }
        _ => None,
    }
}

/// Mint a `parse_level`, de érvénytelen vagy hiányzó érték esetén az alapértelmezett fokozat.
pub fn normalize_level(value: &Value) -> u8 {
    parse_level(value).unwrap_or(DEFAULT_LEVEL)
}

/// A robot beállított nehézsége: fokozat (1–10) vagy `Auto`; None, ha érvénytelen.
pub fn parse_difficulty(value: &Value) -> Option<Difficulty> {
    if let Value::String(s) = value {
        let text = s.trim().to_lowercase();
        if text == "auto" || text == "adaptive" {
            return Some(Difficulty::Auto);
        }
    }
    parse_level(value).map(Difficulty::Level)
}

/// Mint a `parse_difficulty`, de érvénytelen érték esetén az alapértelmezett fokozat.
pub fn normalize_difficulty(value: &Value) -> Difficulty {
    parse_difficulty(value).unwrap_or(Difficulty::Level(DEFAULT_LEVEL))
}

/// Az átlagos pont/körnek megfelelő (tört) fokozat a mért `LEVEL_STRENGTH` skálán.
pub fn level_for_average(average: f64) -> f64 {
    let strength = LEVEL_STRENGTH;
    if average <= strength[0] {
        return MIN_LEVEL as f64;
    }
    if average >= strength[strength.len() - 1] {
        return MAX_LEVEL as f64;
    }
    for i in 0..strength.len() - 1 {
        if average <= strength[i + 1] {
            return (i + 1) as f64 + (average - strength[i]) / (strength[i + 1] - strength[i]);
        }
    }
    MAX_LEVEL as f64
}

/// Az "igazodik hozzám" robot fokozata (egész szám) az ember legutóbbi köreinek alapján.
///
/// `average`: az ember átlagos pont/köre (None, ha még nem lépett); `turns`: a figyelembe vett körök száma.
/// Kevés adatnál az induló fokozat felé húz, `ADAPT_WINDOW` körtől a mért átlagot követi. A tört fokozatot a
/// két szomszédos fokozat véletlenszerű keverése adja.
pub fn adaptive_level<R: rand::Rng + ?Sized>(average: Option<f64>, turns: usize, rng: &mut R) -> u8 {
    let target = match average {
        Some(avg) if turns > 0 => {
            let weight = turns.min(ADAPT_WINDOW) as f64 / ADAPT_WINDOW as f64;
            ADAPT_START_LEVEL * (1.0 - weight) + level_for_average(avg) * weight
        }
        _ => ADAPT_START_LEVEL,
    };
    let low = target.floor();
    let level = if rng.random::<f64>() < target - low { low + 1.0 } else { low };
    (level as i64).clamp(MIN_LEVEL as i64, MAX_LEVEL as i64) as u8
}

// ===================================================================================================
// Szókincs
// ===================================================================================================

use crate::board::{BOARD_SIZE, Board, CENTER, Placed};
use crate::dictionary;
use crate::tiles::{self, TILE_DISTRIBUTION, Tile, tokenize_word};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

pub const HAND_SIZE: usize = 7;
pub const BONUS_ALL_TILES: i32 = 50;

/// Gondolkodási időkeret másodpercben (a keresés ennyi után az eddigi legjobbal folytatja). A robot erejét a
/// lépéskiválasztás szabja meg, nem az időkeret: a keresés a legtöbb táblán ennél jóval hamarabb véget ér.
pub const TIME_BUDGET: f64 = 1.5;
pub const HINT_TIME_BUDGET: f64 = 3.5;
/// A célpontszám relatív szórása.
const TARGET_CV: f64 = 0.5;

/// Ragozott alakok a szókincsben: szótő + egy végződés a hunspell szabályaival. A teljes szabályrendszer több
/// tízmillió alakot adna, ezért csak a leggyakoribb esetragokat, a birtokos és az igei végződéseket vesszük
/// fel, a rövidebb (legfeljebb `INFLECT_MAX_STEM` betűs) szótövekhez.
const INFLECT_ADDS: &str =
    // többes szám, tárgyrag (a magánhangzó-nyúlásos alakokkal) és esetragok
    "k ak ek ok ök ák ék t at et ot öt át ét ban ben ba be ból ből ra re ról ről on en ön hoz hez höz \
     tól től nak nek val vel ig ért nál nél ul ül ként \
     m d a e ja je unk ünk uk ük om em öm od ed öd \
     ik sz ol el öl tok tek tök ott ett ött tam tem tál tél tunk tünk tak ni va ve ó ő ás és";
pub const INFLECT_MAX_STEM: usize = 6;
pub const INFLECT_MAX_FORM: usize = 12;

/// Rendezett szólista egyetlen pufferben, gyors prefix- és tagsági kereséssel (kevés memória).
pub struct Vocabulary {
    data: String,
    /// a szavak kezdőbájt-pozíciója; az utolsó elem a puffer hossza
    offsets: Vec<u32>,
}

impl Vocabulary {
    pub fn new<I: IntoIterator<Item = String>>(words: I) -> Vocabulary {
        let mut list: Vec<String> = words.into_iter().map(|w| w.to_uppercase()).collect();
        list.sort();
        list.dedup();
        let mut data = String::with_capacity(list.iter().map(|w| w.len()).sum());
        let mut offsets = Vec::with_capacity(list.len() + 1);
        for word in &list {
            offsets.push(data.len() as u32);
            data.push_str(word);
        }
        offsets.push(data.len() as u32);
        Vocabulary { data, offsets }
    }

    pub fn len(&self) -> usize {
        self.offsets.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn word(&self, index: usize) -> &str {
        &self.data[self.offsets[index] as usize..self.offsets[index + 1] as usize]
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        (0..self.len()).map(|i| self.word(i))
    }

    /// Az első szó indexe, amely nem kisebb, mint `key`.
    fn lower_bound(&self, key: &str) -> usize {
        let (mut lo, mut hi) = (0usize, self.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.word(mid) < key {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    pub fn has_prefix(&self, prefix: &str) -> bool {
        let i = self.lower_bound(prefix);
        i < self.len() && self.word(i).starts_with(prefix)
    }

    pub fn contains(&self, word: &str) -> bool {
        let i = self.lower_bound(word);
        i < self.len() && self.word(i) == word
    }
}

static VOCABULARY: RwLock<Option<Arc<Vocabulary>>> = RwLock::new(None);

/// Kirakható-e a szó zsetonokkal? (Az idegen szavak q / w / x / y betűihez nincs zseton.)
fn placeable(word: &str) -> bool {
    tokenize_word(word).is_some()
}

/// A stem alakja: `^[a-záéíóöőúüű]{2,15}$`.
fn stem_shape(word: &str) -> bool {
    let n = word.chars().count();
    (2..=15).contains(&n) && word.chars().all(|c| c.is_ascii_lowercase() || "áéíóöőúüű".contains(c))
}

fn has_vowel(word: &str) -> bool {
    word.chars().any(|c| "aáeéiíoóöőuúüű".contains(c))
}

/// A hunspell .dic fájl tőszavaiból (és azok gyakori ragozott alakjaiból, ha `inflect`) építi a szókincset —
/// csak kisbetűs, a táblán kirakható szavak.
pub fn load_vocabulary(dic_path: &Path, inflect: bool) -> std::io::Result<Vocabulary> {
    let bytes = std::fs::read(dic_path)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut stems: HashSet<String> = HashSet::new();
    for line in text.lines().skip(1) {
        let entry = line.trim().split('\t').next().unwrap_or("").split('/').next().unwrap_or("");
        // A magánhangzó nélküli bejegyzések rövidítések (kkv, tb, kg): a robot nem rak le ilyet
        if stem_shape(entry) && has_vowel(entry) && placeable(entry) {
            stems.insert(entry.to_uppercase());
        }
    }
    let checker = dictionary::get_checker();
    if let Some(checker) = &checker {
        // a csak toldalékkal álló, a rövidítés- és idegen írásmódú szócikkek (VU, COS, MADAME) nélkül
        stems.retain(|s| checker.has_stem(&s.to_lowercase()));
    }
    let mut words: HashSet<String> = stems.clone();
    if let (Some(checker), true) = (&checker, inflect) {
        let short: Vec<String> = stems.iter().filter(|s| s.chars().count() <= INFLECT_MAX_STEM).map(|s| s.to_lowercase()).collect();
        let adds: HashSet<&str> = INFLECT_ADDS.split_whitespace().collect();
        // A kockázatos alakokat (KEDVESEM, SZOMSZÉDÉK, FELESÉGÜL) a robot akkor sem rakja le, ha a szótár
        // elfogadja őket: így a lépései (és a napi feladvány táblája) természetes szavakból állnak
        let forms = checker.inflected_forms(&short, &adds, INFLECT_MAX_FORM, false);
        for form in forms {
            if stem_shape(&form) && has_vowel(&form) && placeable(&form) {
                words.insert(form.to_uppercase());
            }
        }
    }
    // a szótár-építő átnézésén elutasított szavakat a robot sem keresi (a játék szótára úgyis elvetné őket)
    for rejected in dictionary::listed_rejected() {
        words.remove(&rejected.to_uppercase());
    }
    Ok(Vocabulary::new(words))
}

/// A szókincs lusta betöltése (az első robotlépésnél, egyszer).
pub fn get_vocabulary() -> Arc<Vocabulary> {
    if let Some(v) = VOCABULARY.read().unwrap().as_ref() {
        return v.clone();
    }
    let mut guard = VOCABULARY.write().unwrap();
    if let Some(v) = guard.as_ref() {
        return v.clone();
    }
    let vocabulary = Arc::new(load_vocabulary(&crate::config::dict_dir().join("hu_HU.dic"), true).unwrap_or_else(|e| {
        eprintln!("[bot] A szókincs nem tölthető be: {e}");
        Vocabulary::new(Vec::new())
    }));
    *guard = Some(vocabulary.clone());
    vocabulary
}

/// Szókincs cseréje (tesztekhez / előtöltéshez).
pub fn set_vocabulary(vocabulary: Option<Arc<Vocabulary>>) {
    *VOCABULARY.write().unwrap() = vocabulary;
}

/// Betöltve van-e már a szókincs.
pub fn vocabulary_loaded() -> bool {
    VOCABULARY.read().unwrap().is_some()
}

// ===================================================================================================
// Lépés
// ===================================================================================================

/// Egy lehetséges lerakás: zsetonok, a képzett szavak és a pontszám.
#[derive(Clone, Debug)]
pub struct Move {
    pub tiles: Vec<Placed>,
    pub words: Vec<String>,
    pub score: i32,
    pub equity: f64,
}

impl Move {
    pub fn tiles_json(&self) -> Value {
        Value::Array(self.tiles.iter().map(|p| p.to_json()).collect())
    }
}

// ===================================================================================================
// Keresés
// ===================================================================================================

struct OutOfTime;

/// Időkeret a hosszú keresés alatt.
struct Budget {
    deadline: Instant,
    ticks: u64,
}

impl Budget {
    fn new(seconds: f64) -> Budget {
        Budget { deadline: Instant::now() + Duration::from_secs_f64(seconds.max(0.0)), ticks: 0 }
    }

    fn tick(&mut self) -> Result<(), OutOfTime> {
        self.ticks += 1;
        if self.ticks & 511 == 0 && Instant::now() > self.deadline {
            return Err(OutOfTime);
        }
        Ok(())
    }
}

type Grid = [[Option<Tile>; BOARD_SIZE]; BOARD_SIZE];
/// Az üres mezők függőleges környezete: (sor, oszlop) → (fent, lent); csak ahol van szomszéd betű.
type Contexts = HashMap<(usize, usize), (String, String)>;
/// Mely betűk rakhatók a mezőre érvényes keresztszóval (bitmaszk a zseton-azonosítók szerint).
type Allowed = HashMap<(usize, usize), u64>;

fn contexts(grid: &Grid) -> Contexts {
    let mut ctx = HashMap::new();
    for r in 0..BOARD_SIZE {
        for c in 0..BOARD_SIZE {
            if grid[r][c].is_some() {
                continue;
            }
            let mut up = String::new();
            let mut rr = r as i32 - 1;
            let mut above: Vec<&str> = Vec::new();
            while rr >= 0 && grid[rr as usize][c].is_some() {
                above.push(grid[rr as usize][c].unwrap().as_str());
                rr -= 1;
            }
            for s in above.iter().rev() {
                up.push_str(s);
            }
            let mut down = String::new();
            let mut rr = r + 1;
            while rr < BOARD_SIZE && grid[rr][c].is_some() {
                down.push_str(grid[rr][c].unwrap().as_str());
                rr += 1;
            }
            if !up.is_empty() || !down.is_empty() {
                ctx.insert((r, c), (up, down));
            }
        }
    }
    ctx
}

fn tile_bit(tile: Tile) -> u64 {
    1u64 << tile.0
}

/// Minden keresztszavas mezőre kiszámolja, mely betűk rakhatók oda érvényes keresztszóval. Az összes irány
/// egyetlen szótárhívással ellenőrződik.
fn allowed_letters(ctxs: [&Contexts; 2], letters: &[Tile], vocab: &Vocabulary) -> [Allowed; 2] {
    struct Entry {
        which: usize,
        square: (usize, usize),
        letter: Tile,
        word: String,
    }
    let mut entries: Vec<Entry> = Vec::new();
    let mut unknown: HashSet<String> = HashSet::new();
    for (which, ctx) in ctxs.iter().enumerate() {
        for (square, (up, down)) in ctx.iter() {
            for &letter in letters {
                let word = format!("{up}{}{down}", letter.as_str());
                if !vocab.contains(&word) {
                    unknown.insert(word.clone());
                }
                entries.push(Entry { which, square: *square, letter, word });
            }
        }
    }
    let valid_unknown = if unknown.is_empty() { HashSet::new() } else { dictionary::filter_valid(unknown.iter().map(|s| s.as_str())) };
    let mut result: [Allowed; 2] = [ctxs[0].keys().map(|k| (*k, 0u64)).collect(), ctxs[1].keys().map(|k| (*k, 0u64)).collect()];
    for e in entries {
        if vocab.contains(&e.word) || valid_unknown.contains(&e.word) {
            *result[e.which].get_mut(&e.square).unwrap() |= tile_bit(e.letter);
        }
    }
    result
}

/// Horgonyok: az üres mezők, amelyek szomszédosak egy betűvel (üres táblán a közép).
fn anchors(grid: &Grid, empty_board: bool) -> Vec<(usize, usize)> {
    if empty_board {
        return vec![(CENTER as usize, CENTER as usize)];
    }
    let mut result = Vec::new();
    for r in 0..BOARD_SIZE {
        for c in 0..BOARD_SIZE {
            if grid[r][c].is_some() {
                continue;
            }
            for (dr, dc) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                let (rr, cc) = (r as i32 + dr, c as i32 + dc);
                if (0..BOARD_SIZE as i32).contains(&rr) && (0..BOARD_SIZE as i32).contains(&cc) && grid[rr as usize][cc as usize].is_some() {
                    result.push((r, c));
                    break;
                }
            }
        }
    }
    result
}

/// A talált lerakások (a zsetonok halmaza szerint egyediek), a megtalálás sorrendjében.
#[derive(Default)]
struct Found {
    index: HashMap<Vec<Placed>, usize>,
    list: Vec<Vec<Placed>>,
}

struct Searcher<'a> {
    vocab: &'a Vocabulary,
    allowed: &'a Allowed,
    transposed: bool,
    budget: &'a mut Budget,
    found: &'a mut Found,
    // az aktuális sor és horgony
    row: [Option<Tile>; BOARD_SIZE],
    r: usize,
    anchor: usize,
    counts: [u8; 39],
    rack_types: Vec<Tile>,
    prefix: String,
}

impl Searcher<'_> {
    fn record(&mut self, placed: &[(usize, Tile, bool)]) {
        let mut tiles: Vec<Placed> = placed
            .iter()
            .map(|&(col, letter, is_blank)| {
                if self.transposed {
                    Placed::new(col as i32, self.r as i32, letter, is_blank)
                } else {
                    Placed::new(self.r as i32, col as i32, letter, is_blank)
                }
            })
            .collect();
        let mut key = tiles.clone();
        key.sort();
        if !self.found.index.contains_key(&key) {
            self.found.index.insert(key, self.found.list.len());
            // az eredeti sorrend marad (a pontozás nem függ tőle)
            self.found.list.push(std::mem::take(&mut tiles));
        }
    }

    fn extend_right(&mut self, col: usize, placed: &mut Vec<(usize, Tile, bool)>, free_blanks: u8) -> Result<(), OutOfTime> {
        self.budget.tick()?;
        if col >= BOARD_SIZE || self.row[col].is_none() {
            if col > self.anchor && self.prefix.chars().count() >= 2 && self.vocab.contains(&self.prefix) {
                self.record(placed);
            }
            if col >= BOARD_SIZE {
                return Ok(());
            }
            let allowed_here = self.allowed.get(&(self.r, col)).copied();
            let prefix_len = self.prefix.len();
            for ti in 0..self.rack_types.len() {
                let letter = self.rack_types[ti];
                if self.counts[letter.0 as usize] == 0 {
                    continue;
                }
                if allowed_here.is_some_and(|mask| mask & tile_bit(letter) == 0) {
                    continue;
                }
                self.prefix.push_str(letter.as_str());
                if !self.vocab.has_prefix(&self.prefix) {
                    self.prefix.truncate(prefix_len);
                    continue;
                }
                self.counts[letter.0 as usize] -= 1;
                placed.push((col, letter, false));
                let result = self.extend_right(col + 1, placed, free_blanks);
                placed.pop();
                self.counts[letter.0 as usize] += 1;
                self.prefix.truncate(prefix_len);
                result?;
            }
            if free_blanks > 0 {
                for letter in tiles::letters() {
                    if allowed_here.is_some_and(|mask| mask & tile_bit(letter) == 0) {
                        continue;
                    }
                    self.prefix.push_str(letter.as_str());
                    if !self.vocab.has_prefix(&self.prefix) {
                        self.prefix.truncate(prefix_len);
                        continue;
                    }
                    placed.push((col, letter, true));
                    let result = self.extend_right(col + 1, placed, free_blanks - 1);
                    placed.pop();
                    self.prefix.truncate(prefix_len);
                    result?;
                }
            }
        } else {
            let existing = self.row[col].unwrap();
            let prefix_len = self.prefix.len();
            self.prefix.push_str(existing.as_str());
            let result = if self.vocab.has_prefix(&self.prefix) { self.extend_right(col + 1, placed, free_blanks) } else { Ok(()) };
            self.prefix.truncate(prefix_len);
            result?;
        }
        Ok(())
    }

    fn left_part(&mut self, left: &mut Vec<(Tile, bool)>, limit: usize, free_blanks: u8) -> Result<(), OutOfTime> {
        self.budget.tick()?;
        let anchor = self.anchor;
        let mut placed: Vec<(usize, Tile, bool)> =
            left.iter().enumerate().map(|(i, &(letter, is_blank))| (anchor - left.len() + i, letter, is_blank)).collect();
        self.extend_right(anchor, &mut placed, free_blanks)?;
        if left.len() >= limit {
            return Ok(());
        }
        let prefix_len = self.prefix.len();
        for ti in 0..self.rack_types.len() {
            let letter = self.rack_types[ti];
            if self.counts[letter.0 as usize] == 0 {
                continue;
            }
            self.prefix.push_str(letter.as_str());
            if !self.vocab.has_prefix(&self.prefix) {
                self.prefix.truncate(prefix_len);
                continue;
            }
            self.counts[letter.0 as usize] -= 1;
            left.push((letter, false));
            let result = self.left_part(left, limit, free_blanks);
            left.pop();
            self.counts[letter.0 as usize] += 1;
            self.prefix.truncate(prefix_len);
            result?;
        }
        if free_blanks > 0 {
            for letter in tiles::letters() {
                self.prefix.push_str(letter.as_str());
                if !self.vocab.has_prefix(&self.prefix) {
                    self.prefix.truncate(prefix_len);
                    continue;
                }
                left.push((letter, true));
                let result = self.left_part(left, limit, free_blanks - 1);
                left.pop();
                self.prefix.truncate(prefix_len);
                result?;
            }
        }
        Ok(())
    }
}

/// Vízszintes keresés a `grid` soraiban (a függőleges irányt az átfordított rács adja). A talált
/// lerakásokat a `found`-ba gyűjti.
#[allow(clippy::too_many_arguments)]
fn search(
    grid: &Grid,
    allowed: &Allowed,
    anchor_list: &[(usize, usize)],
    rack_counts: &[u8; 39],
    blanks: u8,
    vocab: &Vocabulary,
    budget: &mut Budget,
    transposed: bool,
    found: &mut Found,
) -> Result<(), OutOfTime> {
    let anchor_set: HashSet<(usize, usize)> = anchor_list.iter().copied().collect();
    let rack_types: Vec<Tile> = (1..39u8).filter(|i| rack_counts[*i as usize] > 0).map(Tile).collect();
    for &(r, anchor) in anchor_list {
        let mut searcher = Searcher {
            vocab,
            allowed,
            transposed,
            budget,
            found,
            row: grid[r],
            r,
            anchor,
            counts: *rack_counts,
            rack_types: rack_types.clone(),
            prefix: String::new(),
        };
        // A horgonytól balra álló összefüggő betűk a szó fix eleje
        let mut start = anchor;
        while start > 0 && searcher.row[start - 1].is_some() {
            start -= 1;
        }
        if start < anchor {
            for c in start..anchor {
                let letter = searcher.row[c].unwrap();
                searcher.prefix.push_str(letter.as_str());
            }
            let mut placed = Vec::new();
            searcher.extend_right(anchor, &mut placed, blanks)?;
            continue;
        }
        // Egyébként a bal oldali üres (nem horgony) mezők hossza korlátozza a bal részt
        let mut limit = 0usize;
        let mut cc = anchor as i32 - 1;
        while cc >= 0 && searcher.row[cc as usize].is_none() && !anchor_set.contains(&(r, cc as usize)) && limit < HAND_SIZE - 1 {
            limit += 1;
            cc -= 1;
        }
        let mut left = Vec::new();
        searcher.left_part(&mut left, limit, blanks)?;
    }
    Ok(())
}

/// Az összes (a szókincs alapján) lehetséges lerakás a megadott kézzel.
///
/// A fő szó a szókincsből való, a keresztszavak a hunspell szerint érvényesek. A fő szavakat a hívónak még
/// ellenőriznie kell a játék szótárával (lásd `first_valid`). `rack`: a kéz zsetonjai (`Tile::BLANK` = üres).
pub fn generate_moves(board: &Board, rack: &[Tile], vocab: &Vocabulary, allow_blanks: bool, max_tiles: usize, seconds: f64) -> Vec<Move> {
    let mut rack_counts = [0u8; 39];
    let mut blanks = 0u8;
    for t in rack {
        if t.is_blank() {
            if allow_blanks {
                blanks += 1;
            }
        } else {
            rack_counts[t.0 as usize] += 1;
        }
    }
    if rack_counts.iter().all(|c| *c == 0) && blanks == 0 {
        return Vec::new();
    }

    let mut grid: Grid = [[None; BOARD_SIZE]; BOARD_SIZE];
    let mut transposed_grid: Grid = [[None; BOARD_SIZE]; BOARD_SIZE];
    for r in 0..BOARD_SIZE {
        for c in 0..BOARD_SIZE {
            grid[r][c] = board.cells[r][c].map(|cell| cell.letter);
            transposed_grid[c][r] = grid[r][c];
        }
    }
    let empty_board = board.is_empty;

    let letters: Vec<Tile> = if blanks > 0 { tiles::letters().collect() } else { (1..39u8).filter(|i| rack_counts[*i as usize] > 0).map(Tile).collect() };
    let mut budget = Budget::new(seconds);
    let mut found = Found::default();

    let ctx_h = contexts(&transposed_grid); // vízszintes irány keresztszavai = oszlopok az átfordított rácson
    let ctx_v = contexts(&grid); // függőleges irány keresztszavai
    // Vízszintes keresés: keresztszavak függőlegesek (ctx_v); függőleges keresésnél fordítva
    let [allowed_v, allowed_h] = allowed_letters([&ctx_v, &ctx_h], &letters, vocab);

    let result = (|| -> Result<(), OutOfTime> {
        search(&grid, &allowed_v, &anchors(&grid, empty_board), &rack_counts, blanks, vocab, &mut budget, false, &mut found)?;
        if !empty_board {
            let t_anchors = anchors(&transposed_grid, false);
            search(&transposed_grid, &allowed_h, &t_anchors, &rack_counts, blanks, vocab, &mut budget, true, &mut found)?;
        }
        Ok(())
    })();
    let _ = result; // időkeret lejárt: az eddigi találatokkal dolgozunk

    // Pontozás a játék saját táblaszámításával
    let mut moves = Vec::new();
    for tiles in found.list {
        if tiles.len() > max_tiles {
            continue;
        }
        let Ok(formed) = board.validate_placement(&tiles, true) else { continue };
        let mut score: i32 = formed.iter().map(|w| w.score as i32).sum();
        if tiles.len() == HAND_SIZE {
            score += BONUS_ALL_TILES;
        }
        moves.push(Move { tiles, words: formed.into_iter().map(|w| w.word).collect(), score, equity: score as f64 });
    }
    moves
}

// ===================================================================================================
// Értékelés és döntés
// ===================================================================================================

/// A lerakás után kézben maradó zsetonok (heurisztikus) értéke.
pub fn leave_value(leave: &[Tile]) -> f64 {
    if leave.is_empty() {
        return 0.0;
    }
    let mut value = 0.0;
    let mut counts: HashMap<Tile, i32> = HashMap::new();
    for t in leave {
        *counts.entry(*t).or_insert(0) += 1;
    }
    value += 12.0 * *counts.get(&Tile::BLANK).unwrap_or(&0) as f64;
    for (&letter, &count) in &counts {
        if !letter.is_blank() && count > 1 {
            value -= 2.5 * (count - 1) as f64;
        }
        if !letter.is_blank() && letter.value() >= 7 {
            value -= 1.0 * count as f64;
        }
    }
    let vowels: i32 = counts.iter().filter(|(t, _)| !t.is_blank() && t.is_vowel()).map(|(_, c)| *c).sum();
    let consonants: i32 = counts.iter().filter(|(t, _)| !t.is_blank() && !t.is_vowel()).map(|(_, c)| *c).sum();
    // Python round(): a .5 a páros felé kerekít
    let ideal = (0.42 * (vowels + consonants) as f64).round_ties_even();
    value -= 1.5 * (vowels as f64 - ideal).abs();
    value
}

fn remaining_after(rack: &[Tile], mv: &Move) -> Vec<Tile> {
    let mut left = rack.to_vec();
    for p in &mv.tiles {
        let target = p.hand_tile();
        if let Some(i) = left.iter().position(|t| *t == target) {
            left.remove(i);
        }
    }
    left
}

/// Kitölti a lépések `equity` értékét (az értékeléses fokozatokhoz).
fn rate(moves: &mut [Move], rack: &[Tile], bag_remaining: usize) {
    for mv in moves.iter_mut() {
        let leave = remaining_after(rack, mv);
        if bag_remaining == 0 {
            // Végjáték: a kézben maradó zsetonok pontja levonódik, a kiürítés bónuszt ér
            mv.equity = mv.score as f64 - leave.iter().map(|t| t.value() as f64).sum::<f64>() * 0.8;
        } else {
            mv.equity = mv.score as f64 + leave_value(&leave);
        }
    }
}

/// A rendezett jelöltek közül az első `wanted` darabot adja, amelynek minden szava érvényes a játék szótárában.
pub fn first_valid(candidates: &[Move], wanted: usize) -> Vec<Move> {
    const BATCH: usize = 8;
    let mut chosen = Vec::new();
    for chunk in candidates.chunks(BATCH) {
        let valid = dictionary::filter_valid(chunk.iter().flat_map(|m| m.words.iter().map(|w| w.as_str())));
        for mv in chunk {
            if chosen.len() < wanted && mv.words.iter().all(|w| valid.contains(w)) {
                chosen.push(mv.clone());
            }
        }
        if chosen.len() >= wanted {
            break;
        }
    }
    chosen
}

/// Egy fokozat erőssége.
#[derive(Copy, Clone, Debug)]
pub struct Profile {
    /// célpontszámos: a körönkénti célpontszám átlaga (lognormális szórással)
    pub mu: Option<f64>,
    /// értékeléses: az értékelésre rakott Gauss-zaj (0 = mindig a legjobb)
    pub sigma: Option<f64>,
    /// esély, hogy a robot "nem talál" lépést, és passzol / cserél
    pub pass_p: f64,
}

/// Fokozatonkénti erősség. Mért átlagos pontszám körönként (bot–bot önjáték, a passzok is számítanak,
/// ragozott alakokkal a szókincsben, a furcsa alakok nélkül): 4,3 · 6,1 · 7,4 · 10,1 · 12,7 · 15,7 · 18,4 ·
/// 21,2 · 24,6 · 27,0 (`LEVEL_STRENGTH`). Újramérés: `bot_arena ladder`; utána a `LEVEL_STRENGTH` értékeit is
/// frissíteni kell.
pub fn profile(level: u8) -> Profile {
    match level {
        1 => Profile { mu: Some(2.0), sigma: None, pass_p: 0.15 },
        2 => Profile { mu: Some(5.0), sigma: None, pass_p: 0.0 },
        3 => Profile { mu: Some(8.0), sigma: None, pass_p: 0.0 },
        4 => Profile { mu: Some(12.0), sigma: None, pass_p: 0.0 },
        5 => Profile { mu: Some(16.0), sigma: None, pass_p: 0.0 },
        6 => Profile { mu: Some(20.0), sigma: None, pass_p: 0.0 },
        7 => Profile { mu: Some(26.0), sigma: None, pass_p: 0.0 },
        8 => Profile { mu: None, sigma: Some(8.0), pass_p: 0.0 },
        9 => Profile { mu: None, sigma: Some(4.0), pass_p: 0.0 },
        _ => Profile { mu: None, sigma: Some(0.0), pass_p: 0.0 },
    }
}

/// Normális eloszlású véletlen (Box–Muller), mint a Python `random.gauss`.
pub fn gauss<R: rand::Rng + ?Sized>(rng: &mut R, mu: f64, sigma: f64) -> f64 {
    let u1: f64 = 1.0 - rng.random::<f64>(); // (0, 1]
    let u2: f64 = rng.random::<f64>();
    mu + sigma * (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
}

/// Értékeléses (nem célpontszámos) fokozat: a lépések `equity` értékét ki kell tölteni.
fn uses_equity(level: u8) -> bool {
    profile(level).sigma.is_some()
}

/// A jelöltek preferencia szerinti sorrendje a fokozat alapján (az első a legkedvezőbb).
///
/// Célpontszámos fokozatnál a körönként sorsolt célpontszámhoz legközelebbi pontszámú lépés az első;
/// értékeléses fokozatnál a legnagyobb (zajjal terhelt) `equity`. Az összes lépés megmarad tartaléknak arra
/// az esetre, ha a legkedvezőbbet a játék szótára elutasítja.
fn ordered_candidates<R: rand::Rng + ?Sized>(moves: Vec<Move>, level: u8, rng: &mut R) -> Vec<Move> {
    let p = profile(level);
    let mut keyed: Vec<(f64, f64, Move)> = if let Some(mu) = p.mu {
        let target = mu * gauss(rng, -TARGET_CV * TARGET_CV / 2.0, TARGET_CV).exp();
        moves.into_iter().map(|m| ((m.score as f64 - target).abs(), rng.random::<f64>(), m)).collect()
    } else {
        let sigma = p.sigma.unwrap_or(0.0);
        if sigma != 0.0 {
            moves.into_iter().map(|m| (-(m.equity + gauss(rng, 0.0, sigma)), 0.0, m)).collect()
        } else {
            moves.into_iter().map(|m| (-m.equity, rng.random::<f64>(), m)).collect()
        }
    };
    keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)));
    keyed.into_iter().map(|(_, _, m)| m).collect()
}

/// Cserére kiválasztott kéz-indexek: a jokert és a jó betűket megtartja.
pub fn choose_exchange<R: rand::Rng + ?Sized>(rack: &[Tile], rng: &mut R) -> Vec<usize> {
    fn keep_worth(tile: Tile) -> i32 {
        if tile.is_blank() {
            return 100;
        }
        let value = tile.value();
        let base = if value <= 1 {
            6
        } else if value <= 3 {
            3
        } else {
            0
        };
        base + if tile.is_vowel() { 2 } else { 0 }
    }
    let mut order: Vec<(i32, f64, usize)> = (0..rack.len()).map(|i| (-keep_worth(rack[i]), rng.random::<f64>(), i)).collect();
    order.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)));
    let mut keep: Vec<usize> = Vec::new();
    let mut seen: HashSet<Tile> = HashSet::new();
    for (_, _, i) in order {
        if keep.len() >= 3 {
            break;
        }
        if !seen.contains(&rack[i]) {
            keep.push(i);
            seen.insert(rack[i]);
        }
    }
    let exchange: Vec<usize> = (0..rack.len()).filter(|i| !keep.contains(i)).collect();
    if exchange.is_empty() { vec![0] } else { exchange }
}

/// A robot döntése.
#[derive(Clone, Debug)]
pub enum Action {
    Place { tiles: Vec<Placed>, words: Vec<String>, score: i32 },
    Exchange { indices: Vec<usize> },
    Pass,
}

/// Eldönti, mit lép a robot.
///
/// `level`: a robot fokozata (1–10). `avoid`: kerülendő lerakások (a tiles halmaza) — pl. a szavazással
/// elutasítottak (`game::placement_key`).
pub fn choose_action<R: rand::Rng + ?Sized>(
    board: &Board,
    rack: &[Tile],
    level: u8,
    bag_remaining: usize,
    rng: &mut R,
    vocab: &Vocabulary,
    avoid: Option<&HashSet<Vec<Placed>>>,
    seconds: f64,
) -> Action {
    let level = level.clamp(MIN_LEVEL, MAX_LEVEL);
    let pass_p = profile(level).pass_p;
    if pass_p > 0.0 && rng.random::<f64>() < pass_p {
        // "Nem talál" lépést (a legalsó fokozat néha elakad, mint a kezdő játékosok)
        if bag_remaining >= HAND_SIZE {
            return Action::Exchange { indices: choose_exchange(rack, rng) };
        }
        return Action::Pass;
    }

    let mut moves = generate_moves(board, rack, vocab, true, HAND_SIZE, seconds);
    if let Some(avoid) = avoid {
        moves.retain(|m| !avoid.contains(&crate::game::placement_key(&m.tiles)));
    }
    if uses_equity(level) {
        rate(&mut moves, rack, bag_remaining);
    }
    let candidates = ordered_candidates(moves, level, rng);
    let best = first_valid(&candidates, 1);

    if let Some(mv) = best.into_iter().next() {
        return Action::Place { tiles: mv.tiles, words: mv.words, score: mv.score };
    }
    if bag_remaining >= HAND_SIZE {
        return Action::Exchange { indices: choose_exchange(rack, rng) };
    }
    Action::Pass
}

/// A `count` legjobb (szótár szerint érvényes) lépés tippként, pontszám szerint rendezve.
pub fn best_moves(board: &Board, rack: &[Tile], count: usize, vocab: &Vocabulary, seconds: Option<f64>) -> Vec<Move> {
    let mut moves = generate_moves(board, rack, vocab, true, HAND_SIZE, seconds.unwrap_or(HINT_TIME_BUDGET));
    moves.sort_by_key(|m| std::cmp::Reverse(m.score));
    first_valid(&moves, count)
}

/// A zseton-eloszlás darabszáma (a zsák és a kéz ellenőrzéséhez).
pub fn tile_total() -> usize {
    TILE_DISTRIBUTION.iter().map(|(_, _, c)| *c as usize).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_level_accepts_numbers_texts_and_legacy_names() {
        assert_eq!(parse_level(&json!(6)), Some(6));
        assert_eq!(parse_level(&json!("7")), Some(7));
        assert_eq!(parse_level(&json!(" 10 ")), Some(10));
        assert_eq!(parse_level(&json!(6.0)), Some(6));
        assert_eq!(parse_level(&json!("easy")), Some(3));
        assert_eq!(parse_level(&json!("Medium")), Some(6));
        assert_eq!(parse_level(&json!("hard")), Some(10));
        assert_eq!(parse_level(&json!(0)), None);
        assert_eq!(parse_level(&json!(11)), None);
        assert_eq!(parse_level(&json!(true)), None);
        assert_eq!(parse_level(&json!(6.5)), None);
        assert_eq!(parse_level(&json!("zzz")), None);
        assert_eq!(parse_level(&json!(null)), None);
        assert_eq!(parse_level(&json!("+5")), None);
    }

    #[test]
    fn difficulty_parsing() {
        assert_eq!(parse_difficulty(&json!("auto")), Some(Difficulty::Auto));
        assert_eq!(parse_difficulty(&json!("Adaptive")), Some(Difficulty::Auto));
        assert_eq!(parse_difficulty(&json!(3)), Some(Difficulty::Level(3)));
        assert_eq!(normalize_difficulty(&json!("nonsense")), Difficulty::Level(DEFAULT_LEVEL));
        assert_eq!(Difficulty::Auto.to_json(), json!("auto"));
        assert_eq!(Difficulty::Level(4).to_json(), json!(4));
    }

    #[test]
    fn level_for_average_interpolates() {
        assert_eq!(level_for_average(0.0), 1.0);
        assert_eq!(level_for_average(100.0), 10.0);
        assert!((level_for_average(LEVEL_STRENGTH[4]) - 5.0).abs() < 1e-9);
        let mid = (LEVEL_STRENGTH[4] + LEVEL_STRENGTH[5]) / 2.0;
        assert!((level_for_average(mid) - 5.5).abs() < 1e-9);
    }

    #[test]
    fn adaptive_level_without_data_hovers_around_start() {
        let mut rng = rand::rng();
        for _ in 0..50 {
            assert_eq!(adaptive_level(None, 0, &mut rng), 5);
        }
        let mut high = 0;
        for _ in 0..200 {
            let level = adaptive_level(Some(40.0), 10, &mut rng);
            assert_eq!(level, 10);
            high += 1;
        }
        assert_eq!(high, 200);
    }
}
