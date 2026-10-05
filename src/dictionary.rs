//! Magyar szótár-ellenőrzés a beágyazott `affix` ellenőrzővel (nincs rendszerfüggőség), az elutasított szavak
//! (tartós lista + szótár-építő szavazatok + admin felülbírálatok) és a javaslatok.

use crate::affix::AffixChecker;
use crate::config;
use once_cell::sync::Lazy;
use parking_lot::{Mutex, RwLock};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const VOWELLESS_INTERJECTIONS: [&str; 8] = ["brr", "brrr", "hm", "hmm", "hmmm", "khm", "khmm", "pszt"];
const MAX_WORD_LENGTH: usize = 15;
const VALID_CACHE_LIMIT: usize = 200_000;

#[derive(Default)]
struct Lists {
    rejected_listed: HashSet<String>,
    rejected_voted: HashSet<String>,
    override_allow: HashSet<String>,
    override_reject: HashSet<String>,
    additions: HashSet<String>,
}

struct Dict {
    checker: Option<Arc<AffixChecker>>,
}

static DICT: RwLock<Option<Dict>> = RwLock::new(None);
static INIT_ATTEMPTED: AtomicBool = AtomicBool::new(false);
static INIT_LOCK: Mutex<()> = Mutex::new(());
static LISTS: Lazy<RwLock<Lists>> = Lazy::new(|| RwLock::new(Lists::default()));
static VERSION: AtomicU64 = AtomicU64::new(0);
static VALID_CACHE: Lazy<Mutex<HashMap<String, bool>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn has_vowel(word: &str) -> bool {
    word.chars().any(|c| "aáeéiíoóöőuúüű".contains(c))
}

/// Van-e magánhangzó a szóban, vagy magánhangzó nélküli indulatszó (a szó-vizsgálóhoz).
pub fn has_vowel_or_interjection(word: &str) -> bool {
    has_vowel(word) || VOWELLESS_INTERJECTIONS.contains(&word)
}

static REJECTED_OVERRIDE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// A tartós kizárt lista helye: a `set_rejected_path` felülbírálata (a tesztek ideiglenes másolatot használnak, hogy ne
/// írják a repó fájlját), vagy a `SCRABBLE_REJECTED_FILE` környezeti változó, különben a `dict/hu_rejected.txt`.
pub fn rejected_path() -> PathBuf {
    if let Some(path) = REJECTED_OVERRIDE.lock().clone() {
        return path;
    }
    if let Some(path) = std::env::var_os("SCRABBLE_REJECTED_FILE").filter(|p| !p.is_empty()) {
        return PathBuf::from(path);
    }
    config::dict_dir().join("hu_rejected.txt")
}

/// A kizárt lista helyének felülbírálása (None → az alapértelmezett).
pub fn set_rejected_path(path: Option<PathBuf>) {
    *REJECTED_OVERRIDE.lock() = path;
}

/// Az elutasított szavak listája (soronként egy szó, `#` megjegyzés): kisbetűs szavak. Hiányzó fájlnál üres.
pub fn load_rejected(path: &Path) -> HashSet<String> {
    let mut words = HashSet::new();
    if let Ok(bytes) = std::fs::read(path) {
        for line in String::from_utf8_lossy(&bytes).lines() {
            let word = line.split('#').next().unwrap_or("").trim().to_lowercase();
            if !word.is_empty() {
                words.insert(word);
            }
        }
    }
    words
}

/// A beágyazott hu_HU szótár ellenőrzője a használati lista (`hu_attested.txt`) nélkül is betölthető.
pub fn load_checker(dir: &Path) -> std::io::Result<AffixChecker> {
    let attested = dir.join("hu_attested.txt");
    AffixChecker::load(
        &dir.join("hu_HU.aff"),
        &dir.join("hu_HU.dic"),
        if attested.exists() { Some(attested.as_path()) } else { None },
    )
}

fn init_checker() {
    let _guard = INIT_LOCK.lock();
    if INIT_ATTEMPTED.load(Ordering::SeqCst) {
        return;
    }
    match load_checker(&config::dict_dir()) {
        Ok(checker) => {
            let listed = load_rejected(&rejected_path());
            eprintln!("Szótár: beágyazott hu_HU ({} szótő, {} elutasított szó)", checker.entry_count(), listed.len());
            LISTS.write().rejected_listed = listed;
            VERSION.fetch_add(1, Ordering::SeqCst);
            *DICT.write() = Some(Dict { checker: Some(Arc::new(checker)) });
        }
        Err(error) => {
            println!(
                "FIGYELEM: A szótár nem tölthető be ({error}) — a szavak ellenőrzése ki van kapcsolva! \
                 Ellenőrizd a dict/hu_HU.aff és dict/hu_HU.dic fájlokat."
            );
            *DICT.write() = Some(Dict { checker: None });
        }
    }
    INIT_ATTEMPTED.store(true, Ordering::SeqCst);
}

fn ensure_init() {
    if !INIT_ATTEMPTED.load(Ordering::SeqCst) {
        init_checker();
    }
}

/// Tesztekhez / eszközökhöz: a szótár betöltése másik mappából.
pub fn init_from_dir(dir: &Path) -> std::io::Result<()> {
    let _guard = INIT_LOCK.lock();
    let checker = load_checker(dir)?;
    LISTS.write().rejected_listed = load_rejected(&dir.join("hu_rejected.txt"));
    VERSION.fetch_add(1, Ordering::SeqCst);
    VALID_CACHE.lock().clear();
    *DICT.write() = Some(Dict { checker: Some(Arc::new(checker)) });
    INIT_ATTEMPTED.store(true, Ordering::SeqCst);
    Ok(())
}

/// A beépített szóellenőrző (a ragozott alakok előállításához), vagy None, ha nem töltődött be.
pub fn get_checker() -> Option<Arc<AffixChecker>> {
    ensure_init();
    DICT.read().as_ref().and_then(|d| d.checker.clone())
}

/// Betölti a szótárat (szerverindításkor, hogy az első lerakásnál ne kelljen várni).
pub fn warm_up() -> bool {
    is_available()
}

/// Igaz, ha működik a szótár-ellenőrzés (különben minden szó elfogadásra kerül).
pub fn is_available() -> bool {
    get_checker().is_some()
}

fn lookup_with(checker: &AffixChecker, lists: &Lists, word: &str) -> bool {
    if lists.override_reject.contains(word) {
        return false;
    }
    if !lists.override_allow.contains(word) && (lists.rejected_listed.contains(word) || lists.rejected_voted.contains(word)) {
        return false;
    }
    if !has_vowel(word) && !VOWELLESS_INTERJECTIONS.contains(&word) {
        return false;
    }
    if lists.additions.contains(word) {
        return true;
    }
    checker.check(word)
}

/// Egyetlen kisbetűs szó keresése a szótárban (ragozott alakokkal együtt).
fn lookup(word: &str) -> bool {
    let Some(checker) = get_checker() else { return true };
    let lists = LISTS.read();
    lookup_with(&checker, &lists, word)
}

// --- Elutasított szavak ---

/// Az elutasított szavak állapotának verziója (a számolt gyorsítótárak érvénytelenítéséhez).
pub fn rejected_version() -> u64 {
    ensure_init();
    VERSION.load(Ordering::SeqCst)
}

/// A `dict/hu_rejected.txt` szavai (kisbetűs halmaz).
pub fn listed_rejected() -> HashSet<String> {
    ensure_init();
    LISTS.read().rejected_listed.clone()
}

/// Igaz, ha a (bármilyen kis/nagybetűs) szót elutasították — akkor is, ha a szótár elfogadná.
pub fn is_rejected(word: &str) -> bool {
    ensure_init();
    let word = word.to_lowercase();
    let lists = LISTS.read();
    if lists.override_reject.contains(&word) {
        return true;
    }
    !lists.override_allow.contains(&word) && (lists.rejected_listed.contains(&word) || lists.rejected_voted.contains(&word))
}

/// A szavazatok alapján elutasított szavak (kisbetűs halmaz).
pub fn voted_rejected() -> HashSet<String> {
    ensure_init();
    LISTS.read().rejected_voted.clone()
}

/// Az admin felülbírálatok: (allow, reject, additions) kisbetűs halmazok.
pub fn admin_lists() -> (HashSet<String>, HashSet<String>, HashSet<String>) {
    ensure_init();
    let lists = LISTS.read();
    (lists.override_allow.clone(), lists.override_reject.clone(), lists.additions.clone())
}

/// Az admin felülbírálatok és saját szavak teljes cseréje (indításkor és minden módosítás után).
pub fn set_admin_lists(allow: impl IntoIterator<Item = String>, reject: impl IntoIterator<Item = String>, additions: impl IntoIterator<Item = String>) {
    ensure_init();
    {
        let mut lists = LISTS.write();
        lists.override_allow = allow.into_iter().map(|w| w.to_lowercase()).collect();
        lists.override_reject = reject.into_iter().map(|w| w.to_lowercase()).collect();
        lists.additions = additions.into_iter().map(|w| w.to_lowercase()).collect();
    }
    VERSION.fetch_add(1, Ordering::SeqCst);
    VALID_CACHE.lock().clear();
}

/// A tartós lista (`dict/hu_rejected.txt`) újraolvasása a fájlból (az admin panel módosítása után).
pub fn reload_rejected(path: Option<&Path>) -> usize {
    ensure_init();
    let listed = load_rejected(&path.map(|p| p.to_path_buf()).unwrap_or_else(rejected_path));
    let count = listed.len();
    LISTS.write().rejected_listed = listed;
    VERSION.fetch_add(1, Ordering::SeqCst);
    VALID_CACHE.lock().clear();
    count
}

/// A szavak érvényességének gyorsítótára (a következő ellenőrzés újraszámol). Visszatér: a törölt tételek száma.
pub fn clear_valid_cache() -> usize {
    let mut cache = VALID_CACHE.lock();
    let count = cache.len();
    cache.clear();
    count
}

pub fn valid_cache_len() -> usize {
    VALID_CACHE.lock().len()
}

/// A szavazatok alapján elutasított szavak teljes cseréje (indításkor, az adatbázisból).
pub fn set_voted_rejected(words: impl IntoIterator<Item = String>) {
    ensure_init();
    LISTS.write().rejected_voted = words.into_iter().map(|w| w.to_lowercase()).collect();
    VERSION.fetch_add(1, Ordering::SeqCst);
    VALID_CACHE.lock().clear();
}

/// Egy szó felvétele az elutasítottak közé (vagy kivétele belőle) a szavazatok alapján.
pub fn mark_voted_rejected(word: &str, rejected: bool) {
    ensure_init();
    let word = word.to_lowercase();
    {
        let mut lists = LISTS.write();
        if lists.rejected_voted.contains(&word) == rejected {
            return;
        }
        if rejected {
            lists.rejected_voted.insert(word.clone());
        } else {
            lists.rejected_voted.remove(&word);
        }
    }
    VERSION.fetch_add(1, Ordering::SeqCst);
    VALID_CACHE.lock().remove(&word);
}

/// A tesztek után: a szavazatból és az admin felülbírálatokból kizárt szavak törlése.
pub fn reset_runtime_lists() {
    ensure_init();
    {
        let mut lists = LISTS.write();
        lists.rejected_voted.clear();
        lists.override_allow.clear();
        lists.override_reject.clear();
        lists.additions.clear();
    }
    VERSION.fetch_add(1, Ordering::SeqCst);
    VALID_CACHE.lock().clear();
}

/// Érvényes magyar szó karakterek (nagybetűk + ékezetes betűk): `^[A-ZÁÉÍÓÖŐÚÜŰ]+$`.
pub fn valid_word_shape(word: &str) -> bool {
    !word.is_empty() && word.chars().all(|c| c.is_ascii_uppercase() || "ÁÉÍÓÖŐÚÜŰ".contains(c))
}

/// Ellenőrzi a szavak helyességét a magyar szótárral. `words`: nagybetűs szavak (pl. ["ALMA", "SZÉKEK"]).
/// Visszatér: (minden érvényes-e, az érvénytelen szavak).
pub fn check_words<S: AsRef<str>>(words: &[S]) -> (bool, Vec<String>) {
    if words.is_empty() {
        return (true, Vec::new());
    }
    // Input sanitizálás: csak érvényes magyar betűkből álló szavakat engedünk
    for word in words {
        let word = word.as_ref();
        if word.chars().count() > MAX_WORD_LENGTH || !valid_word_shape(word) {
            return (false, vec![word.to_string()]);
        }
    }
    let Some(checker) = get_checker() else {
        // Nincs elérhető szótár — minden szót elfogadunk
        return (true, Vec::new());
    };
    // A táblán a szavak nagybetűsek, de kisbetűvel kell keresni: a nagy kezdőbetűs alak a tulajdonneveket
    // (BUDAPEST, DUNA) is elfogadná, a Scrabble-ban pedig azok nem érvényesek.
    let lists = LISTS.read();
    let invalid: Vec<String> = words
        .iter()
        .map(|w| w.as_ref())
        .filter(|w| !lookup_with(&checker, &lists, &w.to_lowercase()))
        .map(|w| w.to_string())
        .collect();
    (invalid.is_empty(), invalid)
}

// --- Tömeges ellenőrzés és javaslatok (AI ellenfél, szótár-böngésző) ---

fn remember(cache: &mut HashMap<String, bool>, word: &str, valid: bool) {
    if cache.len() >= VALID_CACHE_LIMIT {
        cache.clear();
    }
    cache.insert(word.to_string(), valid);
}

/// Visszaadja a megadott (nagybetűs) szavak közül az érvényeseket, gyorsítótárral. Hibás alakú (nem magyar
/// betűs, túl hosszú) szó sosem érvényes. Visszatér: az érvényes szavak halmaza, az eredeti (nagybetűs) alakban.
pub fn filter_valid<'a, I: IntoIterator<Item = &'a str>>(words: I) -> HashSet<String> {
    let checker = get_checker();
    let mut valid = HashSet::new();
    let mut unknown: Vec<(String, &str)> = Vec::new();
    let unique: HashSet<&str> = words.into_iter().collect();
    {
        let cache = VALID_CACHE.lock();
        for word in unique {
            if word.chars().count() > MAX_WORD_LENGTH || !valid_word_shape(word) {
                continue;
            }
            let key = word.to_lowercase();
            match cache.get(&key) {
                None => unknown.push((key, word)),
                Some(true) => {
                    valid.insert(word.to_string());
                }
                Some(false) => {}
            }
        }
    }
    if unknown.is_empty() {
        return valid;
    }
    // A keresés a gyorsítótár zárján kívül fut (az ellenőrző önmagában is szálbiztos)
    let results: Vec<(String, &str, bool)> = match &checker {
        Some(checker) => {
            let lists = LISTS.read();
            unknown.into_iter().map(|(key, word)| {
                let ok = lookup_with(checker, &lists, &key);
                (key, word, ok)
            }).collect()
        }
        None => unknown.into_iter().map(|(key, word)| (key, word, true)).collect(), // szótár híján minden érvényes
    };
    let mut cache = VALID_CACHE.lock();
    for (key, word, ok) in results {
        remember(&mut cache, &key, ok);
        if ok {
            valid.insert(word.to_string());
        }
    }
    valid
}

/// Egyetlen (nagybetűs) szó ellenőrzése.
pub fn is_word_valid(word: &str) -> bool {
    filter_valid([word]).contains(word)
}

const SUGGEST_ALPHABET: &str = "aábcdeéfghiíjklmnoóöőpqrstuúüűvwxyz";

fn accent_base(c: char) -> char {
    match c {
        'á' => 'a',
        'é' => 'e',
        'í' => 'i',
        'ó' | 'ö' | 'ő' => 'o',
        'ú' | 'ü' | 'ű' => 'u',
        other => other,
    }
}

/// A szóból egyetlen szerkesztéssel (betűcsere, törlés, beszúrás, felcserélés) kapható alakok, jóság szerint
/// rendezve: ékezetcsere, két betű felcserélése, más betűcsere, törlés, beszúrás.
fn edit_candidates(word: &str) -> Vec<String> {
    let chars: Vec<char> = word.chars().collect();
    let n = chars.len();
    let mut ranked: HashMap<String, u8> = HashMap::new();
    let mut add = |candidate: String, rank: u8| {
        if candidate != word {
            ranked.entry(candidate).or_insert(rank);
        }
    };
    let alphabet: Vec<char> = SUGGEST_ALPHABET.chars().collect();
    for i in 0..n {
        for &letter in &alphabet {
            if letter != chars[i] {
                let same_base = accent_base(letter) == accent_base(chars[i]);
                let mut c = chars.clone();
                c[i] = letter;
                add(c.into_iter().collect(), if same_base { 0 } else { 2 });
            }
        }
    }
    for i in 0..n.saturating_sub(1) {
        let mut c = chars.clone();
        c.swap(i, i + 1);
        add(c.into_iter().collect(), 1);
    }
    for i in 0..n {
        let mut c = chars.clone();
        c.remove(i);
        add(c.into_iter().collect(), 3);
    }
    for i in 0..=n {
        for &letter in &alphabet {
            let mut c = chars.clone();
            c.insert(i, letter);
            add(c.into_iter().collect(), 4);
        }
    }
    let mut list: Vec<(u8, String)> = ranked.into_iter().map(|(c, r)| (r, c)).collect();
    list.sort();
    list.into_iter().map(|(_, c)| c).collect()
}

/// Javaslatok egy hibás szóra (nagybetűs, a táblán kirakható szavak). Üres lista, ha nincs.
/// A javaslatok a szóhoz egy betűnyi szerkesztéssel kapható, érvényes szavak.
pub fn suggest_words(word: &str, limit: usize) -> Vec<String> {
    let upper = word.to_uppercase();
    if !valid_word_shape(&upper) || word.chars().count() > MAX_WORD_LENGTH {
        return Vec::new();
    }
    if get_checker().is_none() {
        return Vec::new();
    }
    let mut suggestions = Vec::new();
    for candidate in edit_candidates(&word.to_lowercase()) {
        let len = candidate.chars().count();
        if (2..=MAX_WORD_LENGTH).contains(&len) && lookup(&candidate) {
            suggestions.push(candidate.to_uppercase());
            if suggestions.len() >= limit {
                break;
            }
        }
    }
    suggestions
}

/// Egy kisbetűs szó érvényessége a szótár szerint (admin szó-vizsgáló).
pub fn lookup_word(word: &str) -> bool {
    lookup(word)
}
