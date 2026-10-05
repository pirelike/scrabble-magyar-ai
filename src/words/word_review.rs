//! Szótár-építő: a játék szótárának emberi átnézése.
//!
//! A hu_HU szótár helyesírás-ellenőrzésre készült, ezért a Scrabble-ban sok olyan szót is elfogad, amelyet senki
//! sem tekintene rendes szónak. A szótár-építő véletlen szavakat mutat (a játék által éppen elfogadottakat), a
//! felhasználó pedig eldönti, rendes szó-e. A „nem szó” döntés azonnal kizárja a szót a játékból
//! (`dictionary::mark_voted_rejected`): a lerakás, a robot, a kvíz és a szólisták sem fogadják el többé.
//!
//! A döntések az adatbázisban élnek (`word_reviews`, felhasználónként egy szavazat szavanként); egy szót akkor zár
//! ki a szótár, ha a „nem szó” szavazatok száma legalább a küszöbbel több a „rendes szó” szavazatoknál. Az
//! elutasítás pontos szóalakra vonatkozik (a ragozott alakokat nem érinti). A kizárt szavak indításkor az
//! adatbázisból töltődnek vissza (`refresh`).

use crate::ai;
use crate::db::Db;
use crate::dictionary;
use crate::practice;
use crate::settings::Settings;
use crate::tiles::tokenize_word;
use parking_lot::Mutex;
use rand::Rng;
use rand::seq::SliceRandom;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;

pub const BATCH_SIZE: usize = 20;
pub const MAX_BATCH: usize = 50;
/// a mutatott szavak ekkora része ragozott alak (a többi szótári tő)
const INFLECTED_SHARE: f64 = 0.3;
const MIN_LENGTH: usize = 2;
const MAX_LENGTH: usize = 15;

/// A mintavétel két készlete: a szókincs és benne a (tőszónak nem számító) ragozott alakok indexei.
struct Pools {
    vocabulary: Arc<ai::Vocabulary>,
    inflected: Vec<u32>,
}

static POOLS: Mutex<Option<Arc<Pools>>> = Mutex::new(None);

/// A kizáráshoz szükséges „nem szó” többlet: az admin panelről állítható.
pub fn threshold(settings: &Settings) -> i64 {
    settings.get_i64("word_reject_threshold")
}

/// A szavazatok alapján elutasított szavak és az admin szótári felülbírálatai (engedélyezések, tiltások, saját
/// szavak) betöltése az adatbázisból (szerverindításkor és az admin módosításai után).
pub fn refresh(db: &Db, settings: &Settings) {
    dictionary::set_voted_rejected(db.get_voted_rejected_words(threshold(settings)));
    let (allow, reject, additions) = db.get_admin_word_lists();
    dictionary::set_admin_lists(allow, reject, additions);
}

/// A szó nagybetűs, ellenőrzött alakja, vagy None, ha nem lehet magyar szó a táblán.
pub fn normalize(word: &Value) -> Option<String> {
    let word = word.as_str()?.trim().to_uppercase();
    let n = word.chars().count();
    if !(MIN_LENGTH..=MAX_LENGTH).contains(&n) || tokenize_word(&word).is_none() {
        return None;
    }
    Some(word)
}

pub fn normalize_str(word: &str) -> Option<String> {
    normalize(&Value::from(word))
}

/// A szó elutasított állapotának újraszámolása a szavazatokból (a szótárba is átvezetve).
fn apply(db: &Db, settings: &Settings, word: &str) -> bool {
    let (good, bad) = db.get_word_review_votes(&word.to_lowercase());
    let rejected = bad - good >= threshold(settings);
    dictionary::mark_voted_rejected(word, rejected);
    rejected
}

/// A felhasználó döntése egy szóról (`valid`: rendes szó-e). Csak olyan szóra lehet szavazni, amelyet a szótár
/// éppen elfogad. Visszatér: a szó elutasított-e most, vagy None, ha a szó nem szavazható.
pub fn record_vote(db: &Db, settings: &Settings, user_id: i64, word: &str, valid: bool) -> Option<bool> {
    let word = normalize_str(word)?;
    if !dictionary::is_word_valid(&word) {
        return None;
    }
    db.save_word_review(user_id, &word.to_lowercase(), valid).ok()?;
    Some(apply(db, settings, &word))
}

/// A felhasználó saját döntésének visszavonása. Visszatér: (volt-e mit visszavonni, elutasított-e most a szó).
pub fn undo_vote(db: &Db, settings: &Settings, user_id: i64, word: &str) -> (bool, bool) {
    let Some(word) = normalize_str(word) else { return (false, false) };
    if db.delete_word_review(user_id, &word.to_lowercase()) == 0 {
        return (false, false);
    }
    (true, apply(db, settings, &word))
}

/// A felhasználó összesítője: összes / rendes / nem szó döntés és a ma átnézettek száma.
pub fn stats(db: &Db, user_id: i64) -> Value {
    db.get_word_review_stats(user_id)
}

// --- Mintavétel ---

fn word_pools() -> Arc<Pools> {
    let vocabulary = ai::get_vocabulary();
    let mut guard = POOLS.lock();
    if let Some(pools) = guard.as_ref()
        && Arc::ptr_eq(&pools.vocabulary, &vocabulary)
    {
        return pools.clone();
    }
    let stem_set: HashSet<&String> = practice::stem_words().iter().collect();
    let inflected: Vec<u32> = (0..vocabulary.len()).filter(|i| !stem_set.contains(&vocabulary.word(*i).to_string())).map(|i| i as u32).collect();
    let pools = Arc::new(Pools { vocabulary, inflected });
    *guard = Some(pools.clone());
    pools
}

/// `want` darab, a `taken`-ben még nem szereplő, a szótár szerint érvényes szó a készletből.
fn draw<R: Rng + ?Sized>(size: usize, get: &dyn Fn(usize) -> String, want: usize, taken: &mut HashSet<String>, rng: &mut R) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for _ in 0..12 {
        if found.len() >= want {
            break;
        }
        let k = size.min((want - found.len()) * 4 + 8);
        let sample: Vec<String> = practice::sample_indices(rng, size, k)
            .into_iter()
            .map(get)
            .filter(|w| !taken.contains(&w.to_lowercase()) && (MIN_LENGTH..=MAX_LENGTH).contains(&w.chars().count()))
            .collect();
        let valid = dictionary::filter_valid(sample.iter().map(|s| s.as_str()));
        for word in sample {
            if valid.contains(&word) && !taken.contains(&word.to_lowercase()) && found.len() < want {
                taken.insert(word.to_lowercase());
                found.push(word);
            }
        }
    }
    found
}

/// `count` véletlen szó átnézésre: a szótár tőszavaiból és gyakori ragozott alakjaiból (a robot szókincse), csak
/// olyanok, amelyeket a szótár most elfogad és az `exclude`-ban (kisbetűs) nem szerepelnek.
/// Visszatér: nagybetűs szavak, véletlen sorrendben (kevesebb is lehet, ha elfogytak a szavak).
pub fn sample_words<R: Rng + ?Sized>(count: usize, rng: &mut R, exclude: &HashSet<String>) -> Vec<String> {
    let count = count.max(1);
    let mut taken = exclude.clone();
    let inflected_count = (count as f64 * INFLECTED_SHARE).round_ties_even() as usize;
    let pools = word_pools();
    let stems = practice::stem_words();
    let mut words = draw(stems.len(), &|i| stems[i].clone(), count - inflected_count, &mut taken, rng);
    let inflected = &pools.inflected;
    let vocab = &pools.vocabulary;
    let more = draw(inflected.len(), &|i| vocab.word(inflected[i] as usize).to_string(), count - words.len(), &mut taken, rng);
    words.extend(more);
    words.shuffle(rng);
    words
}

/// Átnézésre váró szavak (legfeljebb `MAX_BATCH`): olyanok, amelyeket még senki sem nézett át (az elfogadottak
/// sem térnek vissza).
pub fn next_words<R: Rng + ?Sized>(db: &Db, count: usize, rng: &mut R) -> Vec<String> {
    sample_words(count.clamp(1, MAX_BATCH), rng, &db.get_reviewed_words())
}
