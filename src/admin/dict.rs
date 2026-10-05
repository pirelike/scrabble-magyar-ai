//! Admin panel: a szótár — szó-vizsgáló, kizárt szavak, felülbírálatok, szótár-építő felügyelet, gyorsítótárak.
//!
//! A tartós lista (`dict/hu_rejected.txt`) git-ben követett fájl: a panel módosításai a szerveren élnek (a fájl
//! atomikusan íródik, a memóriában azonnal érvényesülnek), a repóba való átvezetéshez diff tölthető le.

use super::*;
use crate::app::App;
use crate::db::{Row, RowExt, fetch_all, fetch_one, placeholders};
use crate::dictionary;
use crate::tiles::{tile_value, tokenize_word};
use crate::word_review;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use regex::Regex;
use std::collections::{HashMap, HashSet};

pub const MAX_BULK_WORDS: usize = 5000;
pub const MAX_WORD_LEN: usize = 15;
pub const SECOND_OPINION_DEFAULT: i64 = 20;
const SUSPICIOUS_MIN_DECISIONS: i64 = 50;
const SUSPICIOUS_INVALID_SHARE: f64 = 80.0;
const LIST_PAGE_DEFAULT: i64 = 100;
pub const CACHE_NAMES: [&str; 4] = ["valid", "short_words", "vocabulary", "analysis"];

static WORD_SPLIT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\s,;]+").expect("érvényes regex"));
static VOWEL_RE: Lazy<Regex> = Lazy::new(|| Regex::new("[aáeéiíoóöőuúüű]").expect("érvényes regex"));

/// Az eredeti (a repóban lévő) lista tartalma: a „Változások letöltése” diff ehhez viszonyít. A szerver indulásakor
/// olvasódik be, így a diff azt mutatja, mi változott azóta a panelről.
static BASELINE: Lazy<Mutex<(Option<std::path::PathBuf>, String)>> = Lazy::new(|| Mutex::new((None, String::new())));

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

// ===== Szó normalizálása =====

/// Szó → nagybetűs, ellenőrzött alak; AdminError, ha nem lehet magyar szó a táblán.
pub fn normalize(raw: Option<&Value>) -> AdminResult<String> {
    let word = str_of(raw).map(|w| w.trim().to_uppercase()).unwrap_or_default();
    if word.is_empty() || word.chars().count() > MAX_WORD_LEN {
        return Err(AdminError::field("Érvénytelen szó.", 400, "word"));
    }
    if tokenize_word(&word).is_none() {
        return Err(AdminError::field("A szó nem rakható ki a táblán.", 400, "word"));
    }
    Ok(word)
}

/// Beillesztett lista (szóköz / vessző / sortörés) → egyedi, kisbetűs szavak sorrendben; (szavak, hibás).
pub fn parse_words(text: Option<&Value>) -> AdminResult<(Vec<String>, Vec<String>)> {
    let Some(text) = str_of(text) else { return Err(AdminError::field("Érvénytelen kérés.", 400, "words")) };
    let mut seen: HashSet<String> = HashSet::new();
    let (mut words, mut bad): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    for line in text.lines() {
        let content = line.split('#').next().unwrap_or(""); // a `#` után megjegyzés áll (mint a fájlban)
        for part in WORD_SPLIT_RE.split(content) {
            let part = part.trim().to_lowercase();
            if part.is_empty() {
                continue;
            }
            if part.chars().count() > MAX_WORD_LEN || tokenize_word(&part.to_uppercase()).is_none() {
                bad.push(part.chars().take(30).collect());
                continue;
            }
            if seen.insert(part.clone()) {
                words.push(part);
            }
        }
    }
    if words.len() + bad.len() > MAX_BULK_WORDS {
        return Err(AdminError::field("Túl sok szó egyszerre.", 400, "words"));
    }
    Ok((words, bad))
}

// ===== Szó-vizsgáló =====

fn votes(tx: &Transaction, word: &str) -> AdminResult<(Vec<Value>, i64, i64)> {
    let rows = fetch_all(
        tx,
        "SELECT r.user_id, u.display_name, u.review_blocked, r.verdict, r.created_at FROM word_reviews r \
         LEFT JOIN users u ON u.id = r.user_id WHERE r.word = ? ORDER BY r.created_at",
        [word],
    )?;
    let good = rows.iter().filter(|r| r.flag("verdict") && !r.flag("review_blocked")).count() as i64;
    let bad = rows.iter().filter(|r| !r.flag("verdict") && !r.flag("review_blocked")).count() as i64;
    let list = rows
        .iter()
        .map(|r| json!({"user_id": r.get("user_id"), "display_name": r.get("display_name"), "valid": r.flag("verdict"), "blocked": r.flag("review_blocked"), "at": r.text("created_at")}))
        .collect();
    Ok((list, good, bad))
}

/// A teljes döntési lánc egy szóra: érvényes-e, zsetonok és pont, a szótár levezetései (szótő, előtag, toldalékok,
/// kockázat), a használati lista, az elutasítások (lista, szavazatok, felülbírálat), a robot szókincse, javaslatok.
pub fn inspect_word(app: &App, raw: Option<&Value>) -> AdminResult<Value> {
    let word = normalize(raw)?;
    let lower = word.to_lowercase();
    let tiles = tokenize_word(&word).unwrap_or_default();
    let checker = dictionary::get_checker();
    let valid = dictionary::is_word_valid(&word);
    let mut result = json!({
        "word": word, "tiles": tiles.iter().map(|t| t.as_str()).collect::<Vec<_>>(),
        "score": tiles.iter().map(|t| tile_value(t.as_str())).sum::<u32>(),
        "valid": valid, "dictionary_available": dictionary::is_available(),
    });
    result["has_vowel"] = json!(VOWEL_RE.is_match(&lower) || dictionary::has_vowel_or_interjection(&lower));
    match &checker {
        Some(checker) => {
            result["in_dictionary"] = json!(checker.check(&lower));
            result["derivations"] = json!(checker
                .explain(&lower, 12)
                .iter()
                .map(|e| json!({"stem": e.stem, "prefix": e.prefix, "suffixes": e.suffixes, "risky": e.risky, "risk": e.risk, "adjective": e.adjective, "derived": e.derived}))
                .collect::<Vec<_>>());
            result["needs_attestation"] = json!(checker.needs_attestation(&lower));
            result["attested"] = if checker.has_attested_list() { json!(checker.is_attested(&lower)) } else { Value::Null };
        }
        None => {
            result["in_dictionary"] = Value::Null;
            result["derivations"] = json!([]);
            result["needs_attestation"] = Value::Null;
            result["attested"] = Value::Null;
        }
    }
    let (_, _, additions) = dictionary::admin_lists();
    result["rejected_listed"] = json!(dictionary::listed_rejected().contains(&lower));
    result["rejected_voted"] = json!(dictionary::voted_rejected().contains(&lower));
    result["override"] = Value::Null;
    result["addition"] = json!(additions.contains(&lower));
    let (override_row, vote_list, good, bad) = txn(&app.db, |tx| {
        let row = fetch_one(
            tx,
            "SELECT o.verdict, o.reason, o.created_at, u.display_name AS admin_name FROM word_overrides o LEFT JOIN users u ON u.id = o.admin_user_id WHERE o.word = ?",
            [&lower],
        )?;
        let (list, good, bad) = votes(tx, &lower)?;
        Ok((row, list, good, bad))
    })?;
    if let Some(row) = override_row {
        result["override"] = json!({"verdict": row.text("verdict"), "reason": row.get("reason"), "at": row.text("created_at"), "admin": row.get("admin_name")});
    }
    result["votes"] = json!(vote_list);
    result["balance"] = json!({"good": good, "bad": bad, "threshold": word_review::threshold(&app.settings), "diff": bad - good});
    result["in_vocabulary"] = json!(crate::ai::get_vocabulary().contains(&word));
    result["suggestions"] = if valid { json!([]) } else { json!(dictionary::suggest_words(&word, 8)) };
    Ok(result)
}

// ===== A tartós lista (dict/hu_rejected.txt) =====

pub fn read_list_text() -> String {
    std::fs::read_to_string(dictionary::rejected_path()).unwrap_or_default()
}

/// A lista jelenlegi tartalmának megjegyzése a diffhez (szerverindításkor; már rögzítettet nem ír felül).
pub fn remember_baseline() {
    let path = dictionary::rejected_path();
    let mut baseline = BASELINE.lock();
    if baseline.0.as_ref() != Some(&path) {
        *baseline = (Some(path), read_list_text());
    }
}

/// Atomikus írás: ideiglenes fájl ugyanabban a mappában, majd átnevezés.
fn write_list_text(text: &str) -> std::io::Result<()> {
    let path = dictionary::rejected_path();
    let folder = path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| std::path::PathBuf::from("."));
    let tmp = folder.join(format!(".hu_rejected-{}.tmp", util::short_id()));
    let result = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn write_or_fail(text: &str) -> AdminResult<()> {
    write_list_text(text).map_err(|e| AdminError::new(&format!("A lista nem írható: {e}"), 500))
}

/// A tartós lista változásai a szerver indulása óta (unified diff) — a repóba való átvezetéshez.
pub fn list_diff() -> String {
    remember_baseline();
    let old = BASELINE.lock().1.clone();
    let new = read_list_text();
    similar::TextDiff::from_lines(&old, &new).unified_diff().header("a/dict/hu_rejected.txt", "b/dict/hu_rejected.txt").to_string()
}

/// A kizárt szavak: forrás (`listed` tartós lista / `voted` szavazatok / összes), keresés és lapozás.
pub fn list_rejected(app: &App, args: &HashMap<String, String>) -> AdminResult<Value> {
    let source = args.get("source").map(|s| s.as_str()).filter(|s| !s.is_empty()).unwrap_or("all");
    if !["all", "listed", "voted"].contains(&source) {
        return Err(AdminError::new("Érvénytelen szűrő.", 400));
    }
    let q = args.get("q").map(|q| q.trim().to_lowercase()).unwrap_or_default();
    let listed = dictionary::listed_rejected();
    let voted = dictionary::voted_rejected();
    let allow = dictionary::admin_lists().0;
    let mut words: HashSet<String> = HashSet::new();
    if source == "all" || source == "listed" {
        words.extend(listed.iter().cloned());
    }
    if source == "all" || source == "voted" {
        words.extend(voted.iter().cloned());
    }
    if !q.is_empty() {
        words.retain(|w| w.contains(&q));
    }
    let mut ordered: Vec<String> = words.into_iter().collect();
    ordered.sort();
    let (limit, offset) = page_args(args, LIST_PAGE_DEFAULT, 500);
    let page: Vec<String> = ordered.iter().skip(offset as usize).take(limit as usize).cloned().collect();
    let mut counts: HashMap<String, (i64, i64)> = HashMap::new();
    if !page.is_empty() {
        let rows = txn(&app.db, |tx| {
            Ok(fetch_all(
                tx,
                &format!(
                    "SELECT r.word AS word, COALESCE(SUM(1 - r.verdict), 0) AS bad, COALESCE(SUM(r.verdict), 0) AS good FROM word_reviews r \
                     JOIN users u ON u.id = r.user_id AND u.review_blocked = 0 WHERE r.word IN ({}) GROUP BY r.word",
                    placeholders(page.len())
                ),
                rusqlite::params_from_iter(page.iter()),
            )?)
        })?;
        for r in rows {
            counts.insert(r.text("word"), (r.int("good"), r.int("bad")));
        }
    }
    let items: Vec<Value> = page
        .iter()
        .map(|w| {
            let (good, bad) = counts.get(w).copied().unwrap_or((0, 0));
            json!({"word": w.to_uppercase(), "listed": listed.contains(w), "voted": voted.contains(w), "allowed": allow.contains(w), "good": good, "bad": bad})
        })
        .collect();
    Ok(json!({"items": items, "total": ordered.len(), "limit": limit, "offset": offset, "counts": {"listed": listed.len(), "voted": voted.len()}}))
}

/// Beillesztett szavak besorolása a tartós listához: {'new', 'already', 'invalid'} (szóalakok listái). `new`: a szótár
/// most elfogadja, és még nincs a listán; `invalid`: most sem érvényes (nincs mit kizárni). `assume_valid`: a szavak
/// érvényességét nem vizsgáljuk (a szavazatból kizárt szó már nem érvényes).
pub fn classify_words(words: &[String], assume_valid: bool) -> (Vec<String>, Vec<String>, Vec<String>) {
    let listed = dictionary::listed_rejected();
    let already: Vec<String> = words.iter().filter(|w| listed.contains(*w)).cloned().collect();
    let candidates: Vec<String> = words.iter().filter(|w| !listed.contains(*w)).cloned().collect();
    let valid: HashSet<String> = if assume_valid {
        candidates.iter().cloned().collect()
    } else {
        let upper: Vec<String> = candidates.iter().map(|w| w.to_uppercase()).collect();
        dictionary::filter_valid(upper.iter().map(|w| w.as_str())).into_iter().map(|w| w.to_lowercase()).collect()
    };
    let new: Vec<String> = candidates.iter().filter(|w| valid.contains(*w)).cloned().collect();
    let invalid: Vec<String> = candidates.iter().filter(|w| !valid.contains(*w)).cloned().collect();
    (new, already, invalid)
}

fn classification(new: &[String], already: &[String], invalid: &[String], bad: &[String]) -> Value {
    json!({"new": new, "already": already, "invalid": invalid, "bad": bad})
}

pub fn preview_add(text: Option<&Value>) -> AdminResult<Value> {
    let (words, bad) = parse_words(text)?;
    let (new, already, invalid) = classify_words(&words, false);
    Ok(classification(&new, &already, &invalid, &bad))
}

/// Szavak felvétele a tartós listára (csak a most érvényes, még nem listázott szavak). Visszatér: a besorolás.
pub fn add_rejected(app: &App, ctx: &AdminContext, text: Option<&Value>, reason: Option<&Value>, assume_valid: bool) -> AdminResult<Value> {
    let (words, bad) = parse_words(text)?;
    let (new, already, invalid) = classify_words(&words, assume_valid);
    if new.is_empty() {
        return Err(AdminError::field("Nincs felvehető szó (nincs olyan, amely most érvényes és még nincs a listán).", 409, "words"));
    }
    remember_baseline();
    let before = read_list_text();
    let stamp = util::utcnow().format("%Y-%m-%d").to_string();
    let mut sorted_new = new.clone();
    sorted_new.sort();
    let addition = format!(
        "{}# --- admin panel, {stamp} ---\n{}",
        if before.ends_with('\n') || before.is_empty() { "" } else { "\n" },
        sorted_new.iter().map(|w| format!("{w}\n")).collect::<String>()
    );
    let outcome = action(
        &app.db,
        ctx,
        "dict.reject_add",
        Some("word"),
        None,
        reason,
        json!({"count": new.len(), "words": new.iter().take(500).collect::<Vec<_>>(), "skipped_invalid": invalid.len(), "skipped_listed": already.len()}),
        true,
        |_| write_or_fail(&format!("{before}{addition}")),
    );
    if let Err(e) = outcome {
        let _ = write_list_text(&before); // a napló hibája esetén a fájl is visszaáll
        dictionary::reload_rejected(None);
        return Err(e);
    }
    dictionary::reload_rejected(None);
    Ok(classification(&new, &already, &invalid, &bad))
}

/// Szavak eltávolítása a tartós listáról. Visszatér: a ténylegesen eltávolított szavak.
pub fn remove_rejected(app: &App, ctx: &AdminContext, text: Option<&Value>, reason: Option<&Value>) -> AdminResult<Vec<String>> {
    let (words, _) = parse_words(text)?;
    let listed = dictionary::listed_rejected();
    let targets: Vec<String> = words.into_iter().filter(|w| listed.contains(w)).collect();
    if targets.is_empty() {
        return Err(AdminError::field("Egyik szó sincs a tartós listán.", 404, "words"));
    }
    remember_baseline();
    let before = read_list_text();
    let drop: HashSet<&String> = targets.iter().collect();
    let kept: String = before
        .split_inclusive('\n')
        .filter(|line| {
            let word = line.split('#').next().unwrap_or("").trim().to_lowercase();
            word.is_empty() || !drop.contains(&word)
        })
        .collect();
    let outcome = action(
        &app.db,
        ctx,
        "dict.reject_remove",
        Some("word"),
        None,
        reason,
        json!({"count": targets.len(), "words": targets.iter().take(500).collect::<Vec<_>>()}),
        true,
        |_| write_or_fail(&kept),
    );
    if let Err(e) = outcome {
        let _ = write_list_text(&before);
        dictionary::reload_rejected(None);
        return Err(e);
    }
    dictionary::reload_rejected(None);
    Ok(targets)
}

// ===== Felülbírálatok és saját szavak =====

/// Az admin felülbírálatok újratöltése az adatbázisból a szótárba.
pub fn refresh_lists(app: &App) {
    let (allow, reject, additions) = app.db.get_admin_word_lists();
    dictionary::set_admin_lists(allow, reject, additions);
}

pub fn list_overrides(app: &App) -> AdminResult<Vec<Value>> {
    let rows = txn(&app.db, |tx| {
        Ok(fetch_all(
            tx,
            "SELECT o.word, o.verdict, o.reason, o.created_at, u.display_name AS admin_name FROM word_overrides o \
             LEFT JOIN users u ON u.id = o.admin_user_id ORDER BY o.created_at DESC, o.word",
            [],
        )?)
    })?;
    Ok(rows
        .iter()
        .map(|r| json!({"word": r.text("word").to_uppercase(), "verdict": r.text("verdict"), "reason": r.get("reason"), "at": r.text("created_at"), "admin": r.get("admin_name")}))
        .collect())
}

/// Admin felülbírálat: `allow` (a szavazatok / a lista ellenére érvényes — de csak a szótárban szereplő szó) vagy
/// `reject` (érvénytelen).
pub fn set_override(app: &App, ctx: &AdminContext, raw: Option<&Value>, verdict: Option<&Value>, reason: Option<&Value>) -> AdminResult<String> {
    let Some(verdict) = str_of(verdict).filter(|v| *v == "allow" || *v == "reject") else {
        return Err(AdminError::field("Érvénytelen kérés.", 400, "verdict"));
    };
    let word = normalize(raw)?;
    let lower = word.to_lowercase();
    if verdict == "allow" && !dictionary::get_checker().is_some_and(|c| c.check(&lower)) {
        return Err(AdminError::field("A szó nincs a szótárban: az engedélyezés nem teszi érvényessé (ahhoz saját szó kell).", 409, "word"));
    }
    action(&app.db, ctx, "dict.override", Some("word"), Some(lower.clone()), reason, json!({}), true, |act| {
        let previous = fetch_one(act.tx, "SELECT verdict FROM word_overrides WHERE word = ?", [&lower])?;
        act.tx.execute(
            "INSERT INTO word_overrides (word, verdict, admin_user_id, reason) VALUES (?, ?, ?, ?) \
             ON CONFLICT(word) DO UPDATE SET verdict = excluded.verdict, admin_user_id = excluded.admin_user_id, reason = excluded.reason, created_at = datetime('now')",
            rusqlite::params![lower, verdict, ctx.admin_user_id, normalize_reason(reason)?],
        )?;
        act.details.insert("before".into(), json!({"verdict": previous.map(|p| p.text("verdict"))}));
        act.details.insert("after".into(), json!({"verdict": verdict}));
        Ok(())
    })?;
    refresh_lists(app);
    Ok(word)
}

pub fn remove_override(app: &App, ctx: &AdminContext, raw: Option<&Value>, reason: Option<&Value>) -> AdminResult<()> {
    let word = normalize(raw)?;
    let lower = word.to_lowercase();
    action(&app.db, ctx, "dict.override_remove", Some("word"), Some(lower.clone()), reason, json!({}), true, |act| {
        let row = fetch_one(act.tx, "SELECT verdict FROM word_overrides WHERE word = ?", [&lower])?.ok_or_else(|| AdminError::new("Ehhez a szóhoz nincs felülbírálat.", 404))?;
        act.tx.execute("DELETE FROM word_overrides WHERE word = ?", [&lower])?;
        act.details.insert("before".into(), json!({"verdict": row.text("verdict")}));
        Ok(())
    })?;
    refresh_lists(app);
    Ok(())
}

pub fn list_additions(app: &App) -> AdminResult<Vec<Value>> {
    let rows = txn(&app.db, |tx| {
        Ok(fetch_all(
            tx,
            "SELECT a.word, a.reason, a.created_at, u.display_name AS admin_name FROM word_additions a LEFT JOIN users u ON u.id = a.admin_user_id ORDER BY a.word",
            [],
        )?)
    })?;
    Ok(rows.iter().map(|r| json!({"word": r.text("word").to_uppercase(), "reason": r.get("reason"), "at": r.text("created_at"), "admin": r.get("admin_name")})).collect())
}

/// Saját szó: a szótárban nem szereplő, de a játékban elfogadott szó (szavanként indoklással). A robot szókincsébe nem
/// kerül automatikusan.
pub fn add_addition(app: &App, ctx: &AdminContext, raw: Option<&Value>, reason: Option<&Value>) -> AdminResult<String> {
    let word = normalize(raw)?;
    let lower = word.to_lowercase();
    if !VOWEL_RE.is_match(&lower) || tokenize_word(&word).map(|t| t.len()).unwrap_or(0) < 2 {
        return Err(AdminError::field("A saját szó legalább két zsetonos és magánhangzós kell legyen.", 400, "word"));
    }
    action(&app.db, ctx, "dict.addition_add", Some("word"), Some(lower.clone()), reason, json!({}), true, |act| {
        if fetch_one(act.tx, "SELECT 1 AS x FROM word_additions WHERE word = ?", [&lower])?.is_some() {
            return Err(AdminError::field("Ez a szó már szerepel a saját szavak között.", 409, "word"));
        }
        act.tx.execute("INSERT INTO word_additions (word, admin_user_id, reason) VALUES (?, ?, ?)", rusqlite::params![lower, ctx.admin_user_id, normalize_reason(reason)?])?;
        Ok(())
    })?;
    refresh_lists(app);
    Ok(word)
}

pub fn remove_addition(app: &App, ctx: &AdminContext, raw: Option<&Value>, reason: Option<&Value>) -> AdminResult<()> {
    let word = normalize(raw)?;
    let lower = word.to_lowercase();
    action(&app.db, ctx, "dict.addition_remove", Some("word"), Some(lower.clone()), reason, json!({}), true, |act| {
        if act.tx.execute("DELETE FROM word_additions WHERE word = ?", [&lower])? == 0 {
            return Err(AdminError::new("A szó nem szerepel a saját szavak között.", 404));
        }
        Ok(())
    })?;
    refresh_lists(app);
    Ok(())
}

// ===== Szavazatok =====

/// Egy szó összes szavazatának törlése (a kizárás újraszámolódik). Visszatér: a törölt szavazatok száma.
pub fn delete_votes(app: &App, ctx: &AdminContext, raw: Option<&Value>, reason: Option<&Value>) -> AdminResult<usize> {
    let word = normalize(raw)?;
    let lower = word.to_lowercase();
    let count = action(&app.db, ctx, "dict.votes_delete", Some("word"), Some(lower.clone()), reason, json!({}), true, |act| {
        let votes = fetch_all(act.tx, "SELECT user_id, verdict FROM word_reviews WHERE word = ?", [&lower])?;
        if votes.is_empty() {
            return Err(AdminError::new("Ehhez a szóhoz nincs szavazat.", 404));
        }
        act.tx.execute("DELETE FROM word_reviews WHERE word = ?", [&lower])?;
        act.details.insert("count".into(), json!(votes.len()));
        act.details.insert("votes".into(), json!(votes.iter().take(200).map(|v| json!({"user_id": v.get("user_id"), "valid": v.flag("verdict")})).collect::<Vec<_>>()));
        Ok(votes.len())
    })?;
    word_review::refresh(&app.db, &app.settings);
    Ok(count)
}

/// A szavazatokból kizárt szavak átvezetése a tartós listába (a szavazatok megmaradnak). Visszatér: a besorolás.
pub fn export_votes(app: &App, ctx: &AdminContext, reason: Option<&Value>) -> AdminResult<Value> {
    let allow = dictionary::admin_lists().0;
    let mut words: Vec<String> = dictionary::voted_rejected().into_iter().filter(|w| !allow.contains(w)).collect();
    words.sort();
    if words.is_empty() {
        return Err(AdminError::new("Nincs szavazatból kizárt szó.", 409));
    }
    add_rejected(app, ctx, Some(&json!(words.join("\n"))), reason, true)
}

/// Véletlen minta a tartós listáról újraértékelésre: a szó, zsetonjai, és hogy a szótár nélküle elfogadná-e.
pub fn second_opinion(count: Option<&str>) -> AdminResult<Value> {
    let count = match count {
        None => SECOND_OPINION_DEFAULT,
        Some(c) => int_field(Some(&json!(c.trim().parse::<i64>().unwrap_or(-1))), "n", 1, 100)?,
    };
    let mut listed: Vec<String> = dictionary::listed_rejected().into_iter().collect();
    listed.sort();
    let indices = crate::practice::sample_indices(&mut rand::rng(), listed.len(), (count as usize).min(listed.len()));
    let checker = dictionary::get_checker();
    Ok(json!(indices
        .iter()
        .map(|i| {
            let w = &listed[*i];
            let tiles = tokenize_word(&w.to_uppercase()).map(|t| t.iter().map(|x| x.as_str().to_string()).collect::<Vec<_>>());
            json!({"word": w.to_uppercase(), "tiles": tiles, "dictionary_accepts": checker.as_ref().map(|c| c.check(w))})
        })
        .collect::<Vec<_>>()))
}

// ===== Szótár-építő felügyelet =====

fn rows_json(rows: Vec<Row>) -> Value {
    Value::Array(rows.into_iter().map(Value::Object).collect())
}

/// A szótár-építő összesítője: napi döntések, átnézett és kizárt szavak, aktív bírálók.
pub fn review_summary(app: &App, days: Option<&str>) -> AdminResult<Value> {
    let days = match days {
        None => 14,
        Some(d) => int_field(Some(&json!(d.trim().parse::<i64>().unwrap_or(-1))), "days", 1, 365)?,
    };
    let since = util::format_ts(util::utcnow() - chrono::Duration::days(days));
    let week = util::format_ts(util::utcnow() - chrono::Duration::days(7));
    let (daily, totals, active) = txn(&app.db, |tx| {
        let daily = fetch_all(
            tx,
            "SELECT date(created_at) AS day, COUNT(*) AS decisions, COALESCE(SUM(1 - verdict), 0) AS rejections FROM word_reviews WHERE created_at >= ? GROUP BY day ORDER BY day",
            [&since],
        )?;
        let totals = fetch_one(tx, "SELECT COUNT(*) AS decisions, COUNT(DISTINCT word) AS words, COUNT(DISTINCT user_id) AS reviewers FROM word_reviews", [])?.unwrap_or_default();
        let active = fetch_one(tx, "SELECT COUNT(DISTINCT user_id) AS n FROM word_reviews WHERE created_at >= ?", [&week])?.map(|r| r.int("n")).unwrap_or(0);
        Ok((daily, totals, active))
    })?;
    Ok(json!({
        "daily": rows_json(daily), "decisions": totals.int("decisions"), "words_reviewed": totals.int("words"), "reviewers": totals.int("reviewers"),
        "active_reviewers": active, "rejected": dictionary::voted_rejected().len(), "listed": dictionary::listed_rejected().len(),
        "threshold": word_review::threshold(&app.settings),
    }))
}

/// A bírálók: döntések száma, „nem szó” arány, egyezés a többiekkel; a gyanúsak (50 döntés felett > 80% „nem szó”)
/// kiemelve.
pub fn reviewers(app: &App) -> AdminResult<Value> {
    let rows = txn(&app.db, |tx| {
        Ok(fetch_all(
            tx,
            "SELECT r.user_id AS user_id, u.display_name AS display_name, u.review_blocked AS blocked, \
             COUNT(*) AS total, COALESCE(SUM(1 - r.verdict), 0) AS bad, MAX(r.created_at) AS last_at, \
             COALESCE(SUM(CASE WHEN t.n > 1 THEN 1 ELSE 0 END), 0) AS compared, \
             COALESCE(SUM(CASE WHEN t.n > 1 AND ((r.verdict = 1 AND (t.good - 1) >= t.bad) OR (r.verdict = 0 AND (t.bad - 1) >= t.good)) THEN 1 ELSE 0 END), 0) AS agreed \
             FROM word_reviews r JOIN users u ON u.id = r.user_id \
             JOIN (SELECT word, COUNT(*) AS n, SUM(verdict) AS good, SUM(1 - verdict) AS bad FROM word_reviews GROUP BY word) t ON t.word = r.word \
             GROUP BY r.user_id ORDER BY total DESC",
            [],
        )?)
    })?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            let (total, bad, compared, agreed) = (r.int("total"), r.int("bad"), r.int("compared"), r.int("agreed"));
            let share = if total > 0 { round1(bad as f64 / total as f64 * 100.0) } else { 0.0 };
            json!({
                "user_id": r.int("user_id"), "display_name": r.text("display_name"), "blocked": r.flag("blocked"), "total": total, "invalid": bad,
                "invalid_share": share, "agreement": if compared > 0 { json!(round1(agreed as f64 / compared as f64 * 100.0)) } else { Value::Null },
                "compared": compared, "last_at": r.get("last_at"),
                "suspicious": total >= SUSPICIOUS_MIN_DECISIONS && share > SUSPICIOUS_INVALID_SHARE,
            })
        })
        .collect();
    Ok(json!({"items": items}))
}

// ===== Gyorsítótárak =====

/// Egy gyorsítótár ürítése, az előállítás idejének mérésével. Visszatér: {'which', 'seconds', 'detail'}.
pub fn clear_cache(app: &App, ctx: &AdminContext, which: Option<&Value>, reason: Option<&Value>) -> AdminResult<Value> {
    let Some(which) = str_of(which).filter(|w| CACHE_NAMES.contains(w)) else {
        return Err(AdminError::field("Ismeretlen gyorsítótár.", 400, "which"));
    };
    action(&app.db, ctx, "dict.cache_clear", Some("cache"), Some(which.to_string()), reason, json!({}), true, |_| Ok(()))?;
    let started = util::now();
    let detail = match which {
        "valid" => json!(dictionary::clear_valid_cache()),
        "short_words" => {
            crate::practice::clear_short_cache();
            for length in crate::practice::SHORT_LENGTHS {
                crate::practice::short_words(length);
            }
            Value::Null
        }
        "vocabulary" => {
            crate::ai::set_vocabulary(None);
            json!(crate::ai::get_vocabulary().len())
        }
        _ => json!(app.db.with(|tx| tx.execute("DELETE FROM game_analysis", []))?),
    };
    Ok(json!({"which": which, "seconds": ((util::now() - started) * 100.0).round() / 100.0, "detail": detail}))
}

pub fn threshold_info(app: &App) -> Value {
    json!({
        "value": word_review::threshold(&app.settings), "default": app.settings.default_of("word_reject_threshold"),
        "overridden": app.settings.is_overridden("word_reject_threshold"),
    })
}
