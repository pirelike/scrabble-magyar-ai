//! Beállítások, push feliratkozások, szótár-építő szavazatok, napi feladvány, elemzés gyorsítótár,
//! kitüntetések, barátok és felhasználó-keresés.

use super::{Db, Row, RowExt, escape_like, fetch_all, fetch_one};
use crate::util;
use rusqlite::functions::FunctionFlags;
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::HashSet;

/// A napi feladvány tárolt alakja.
#[derive(Clone, Debug)]
pub struct DailyPuzzle {
    pub date: String,
    pub board: Value,
    pub rack: Vec<String>,
    pub best_score: i64,
    pub best: Value,
}

/// Egy felhasználó napi eredménye.
#[derive(Clone, Debug, PartialEq)]
pub struct DailyEntry {
    pub best_score: i64,
    pub attempts: i64,
    pub revealed: bool,
}

impl DailyEntry {
    pub fn to_json(&self) -> Value {
        json!({"best_score": self.best_score, "attempts": self.attempts, "revealed": self.revealed})
    }
}

/// Egy beküldött lépés rögzítésének eredménye.
#[derive(Clone, Debug)]
pub struct DailyRecord {
    pub recorded: bool,
    pub best: i64,
    pub attempts: i64,
    pub improved: bool,
}

const DAILY_ORDER: &str = "ds.best_score DESC, ds.attempts ASC, ds.first_best_at ASC, ds.user_id ASC";

impl Db {
    // --- Beállítások és push feliratkozások ---

    pub fn get_setting(&self, key: &str) -> Option<String> {
        self.with(|tx| tx.query_row("SELECT value FROM app_settings WHERE key = ?", [key], |r| r.get(0)).optional()).ok().flatten()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.with(|tx| {
            tx.execute(
                "INSERT INTO app_settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map(|_| ())
        })
    }

    /// Web Push feliratkozás mentése (egy végpont egyszer szerepelhet; másik felhasználóhoz is átkerülhet, ha
    /// ugyanazon az eszközön másik fiókkal léptek be).
    pub fn save_push_subscription(&self, user_id: i64, endpoint: &str, p256dh: &str, auth_key: &str, lang: &str) -> rusqlite::Result<()> {
        self.with(|tx| {
            tx.execute(
                "INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth, lang) VALUES (?, ?, ?, ?, ?) \
                 ON CONFLICT(endpoint) DO UPDATE SET user_id = excluded.user_id, p256dh = excluded.p256dh, \
                 auth = excluded.auth, lang = excluded.lang",
                params![user_id, endpoint, p256dh, auth_key, lang],
            )
            .map(|_| ())
        })
    }

    /// Feliratkozás törlése (a `user_id` megadásakor csak a sajátja). Visszatér: törölt sorok száma.
    pub fn delete_push_subscription(&self, endpoint: &str, user_id: Option<i64>) -> usize {
        self.with(|tx| match user_id {
            None => tx.execute("DELETE FROM push_subscriptions WHERE endpoint = ?", [endpoint]),
            Some(uid) => tx.execute("DELETE FROM push_subscriptions WHERE endpoint = ? AND user_id = ?", params![endpoint, uid]),
        })
        .unwrap_or(0)
    }

    pub fn get_push_subscriptions(&self, user_id: i64) -> Vec<Row> {
        self.with(|tx| fetch_all(tx, "SELECT endpoint, p256dh, auth, lang FROM push_subscriptions WHERE user_id = ?", [user_id]))
            .unwrap_or_default()
    }

    pub fn count_push_subscriptions(&self, user_id: i64) -> i64 {
        self.with(|tx| tx.query_row("SELECT COUNT(*) FROM push_subscriptions WHERE user_id = ?", [user_id], |r| r.get(0))).unwrap_or(0)
    }

    // --- Szótár-építő: szavak átnézése ---
    //
    // `word_reviews`: felhasználónként és szavanként egy döntés (verdict 1: rendes szó, 0: nem szó). A szavak
    // kisbetűsen tárolódnak.

    /// Egy szó átnézésének mentése (a korábbi döntést felülírja).
    pub fn save_word_review(&self, user_id: i64, word: &str, verdict: bool) -> rusqlite::Result<()> {
        self.with(|tx| {
            tx.execute(
                "INSERT INTO word_reviews (word, user_id, verdict) VALUES (?, ?, ?) \
                 ON CONFLICT(word, user_id) DO UPDATE SET verdict = excluded.verdict, created_at = datetime('now')",
                params![word, user_id, verdict as i64],
            )
            .map(|_| ())
        })
    }

    /// Egy saját döntés törlése (visszavonás). Visszatér: törölt sorok száma.
    pub fn delete_word_review(&self, user_id: i64, word: &str) -> usize {
        self.with(|tx| tx.execute("DELETE FROM word_reviews WHERE word = ? AND user_id = ?", params![word, user_id])).unwrap_or(0)
    }

    /// Egy szó szavazatai: (rendes szó, nem szó) darabszám.
    pub fn get_word_review_votes(&self, word: &str) -> (i64, i64) {
        self.with(|tx| {
            tx.query_row(
                "SELECT COALESCE(SUM(r.verdict), 0), COALESCE(SUM(1 - r.verdict), 0) \
                 FROM word_reviews r JOIN users u ON u.id = r.user_id AND u.review_blocked = 0 WHERE r.word = ?",
                [word],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
        .unwrap_or((0, 0))
    }

    /// Az admin szótári felülbírálatai és saját szavai: (allow, reject, additions) kisbetűs halmazok.
    pub fn get_admin_word_lists(&self) -> (HashSet<String>, HashSet<String>, HashSet<String>) {
        self.with(|tx| {
            let rows = fetch_all(tx, "SELECT word, verdict FROM word_overrides", [])?;
            let additions = fetch_all(tx, "SELECT word FROM word_additions", [])?;
            let allow = rows.iter().filter(|r| r.text("verdict") == "allow").map(|r| r.text("word")).collect();
            let reject = rows.iter().filter(|r| r.text("verdict") == "reject").map(|r| r.text("word")).collect();
            let adds = additions.iter().map(|r| r.text("word")).collect();
            Ok((allow, reject, adds))
        })
        .unwrap_or_default()
    }

    /// Az összes szó, amelyet bárki átnézett (kisbetűs halmaz): a mintavételből kimaradnak.
    pub fn get_reviewed_words(&self) -> HashSet<String> {
        self.with(|tx| {
            let rows = fetch_all(tx, "SELECT DISTINCT word FROM word_reviews", [])?;
            Ok(rows.iter().map(|r| r.text("word")).collect())
        })
        .unwrap_or_default()
    }

    /// Azok a szavak, amelyekre a „nem szó” szavazatok száma legalább `threshold`-dal több, mint a „rendes
    /// szó” szavazatoké.
    pub fn get_voted_rejected_words(&self, threshold: i64) -> HashSet<String> {
        self.with(|tx| {
            let rows = fetch_all(
                tx,
                "SELECT r.word AS word FROM word_reviews r JOIN users u ON u.id = r.user_id AND u.review_blocked = 0 \
                 GROUP BY r.word HAVING SUM(1 - r.verdict) - SUM(r.verdict) >= ?",
                [threshold],
            )?;
            Ok(rows.iter().map(|r| r.text("word")).collect())
        })
        .unwrap_or_default()
    }

    /// A felhasználó átnézéseinek összesítője: összes, rendes szó, nem szó, ma átnézett (UTC nap).
    pub fn get_word_review_stats(&self, user_id: i64) -> Value {
        let (total, good, today): (i64, i64, i64) = self
            .with(|tx| {
                tx.query_row(
                    "SELECT COUNT(*) AS total, COALESCE(SUM(verdict), 0) AS good, \
                     COALESCE(SUM(CASE WHEN date(created_at) = date('now') THEN 1 ELSE 0 END), 0) AS today \
                     FROM word_reviews WHERE user_id = ?",
                    [user_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
            })
            .unwrap_or((0, 0, 0));
        json!({"total": total, "valid": good, "invalid": total - good, "today": today})
    }

    // --- Napi feladvány ---

    /// A nap feladványa, vagy None.
    pub fn get_daily_puzzle(&self, puzzle_date: &str) -> Option<DailyPuzzle> {
        let row = self.with(|tx| fetch_one(tx, "SELECT * FROM daily_puzzles WHERE puzzle_date = ?", [puzzle_date])).ok().flatten()?;
        Some(DailyPuzzle {
            date: row.text("puzzle_date"),
            board: serde_json::from_str(&row.text("board_json")).unwrap_or(Value::Null),
            rack: serde_json::from_str::<Vec<String>>(&row.text("rack_json")).unwrap_or_default(),
            best_score: row.int("best_score"),
            best: serde_json::from_str(&row.text("best_json")).unwrap_or(Value::Null),
        })
    }

    /// A feladvány mentése (ha az adott napra már van, az marad: mindenkinek ugyanaz).
    pub fn save_daily_puzzle(&self, puzzle_date: &str, board: &Value, rack: &[String], best_score: i64, best: &Value) -> rusqlite::Result<()> {
        self.with(|tx| {
            tx.execute(
                "INSERT OR IGNORE INTO daily_puzzles (puzzle_date, board_json, rack_json, best_score, best_json) VALUES (?, ?, ?, ?, ?)",
                params![puzzle_date, board.to_string(), serde_json::to_string(rack).unwrap_or_default(), best_score, best.to_string()],
            )
            .map(|_| ())
        })
    }

    /// Ha valaki a feladvány eddig ismert legjobb lépésénél többet ért el, az lesz az új legjobb.
    pub fn raise_daily_best(&self, puzzle_date: &str, score: i64, best: &Value) {
        let _ = self.with(|tx| {
            tx.execute(
                "UPDATE daily_puzzles SET best_score = ?, best_json = ? WHERE puzzle_date = ? AND best_score < ?",
                params![score, best.to_string(), puzzle_date, score],
            )
        });
    }

    pub fn get_daily_entry(&self, puzzle_date: &str, user_id: i64) -> Option<DailyEntry> {
        self.with(|tx| {
            tx.query_row(
                "SELECT best_score, attempts, revealed FROM daily_scores WHERE puzzle_date = ? AND user_id = ?",
                params![puzzle_date, user_id],
                |r| Ok(DailyEntry { best_score: r.get(0)?, attempts: r.get(1)?, revealed: r.get::<_, i64>(2)? != 0 }),
            )
            .optional()
        })
        .ok()
        .flatten()
    }

    /// Egy beküldött lépés rögzítése. A legjobb pontszám számít; ha valaki a megoldást megnézte, további
    /// próbálkozása már nem kerül a ranglistára.
    pub fn record_daily_score(&self, puzzle_date: &str, user_id: i64, score: i64) -> rusqlite::Result<DailyRecord> {
        let now = util::now_ts();
        self.with(|tx| {
            let row: Option<(i64, i64, i64)> = tx
                .query_row(
                    "SELECT best_score, attempts, revealed FROM daily_scores WHERE puzzle_date = ? AND user_id = ?",
                    params![puzzle_date, user_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            match row {
                Some((best, attempts, revealed)) if revealed != 0 => {
                    Ok(DailyRecord { recorded: false, best, attempts, improved: false })
                }
                None => {
                    tx.execute(
                        "INSERT INTO daily_scores (puzzle_date, user_id, best_score, attempts, first_best_at) VALUES (?, ?, ?, 1, ?)",
                        params![puzzle_date, user_id, score, now],
                    )?;
                    Ok(DailyRecord { recorded: true, best: score, attempts: 1, improved: true })
                }
                Some((best, attempts, _)) => {
                    let improved = score > best;
                    if improved {
                        tx.execute(
                            "UPDATE daily_scores SET best_score = ?, attempts = attempts + 1, first_best_at = ? WHERE puzzle_date = ? AND user_id = ?",
                            params![score, now, puzzle_date, user_id],
                        )?;
                    } else {
                        tx.execute(
                            "UPDATE daily_scores SET attempts = attempts + 1 WHERE puzzle_date = ? AND user_id = ?",
                            params![puzzle_date, user_id],
                        )?;
                    }
                    Ok(DailyRecord { recorded: true, best: best.max(score), attempts: attempts + 1, improved })
                }
            }
        })
    }

    /// Jelzi, hogy a felhasználó megnézte a megoldást (innentől nem javíthat a ranglistán).
    pub fn mark_daily_revealed(&self, puzzle_date: &str, user_id: i64) {
        let _ = self.with(|tx| {
            tx.execute(
                "INSERT INTO daily_scores (puzzle_date, user_id, best_score, attempts, revealed) VALUES (?, ?, 0, 0, 1) \
                 ON CONFLICT(puzzle_date, user_id) DO UPDATE SET revealed = 1",
                params![puzzle_date, user_id],
            )
        });
    }

    /// A nap ranglistája (csak azok, akik legalább egy lépést beküldtek).
    /// Visszatér: (a lista, a megadott felhasználó helyezése — a top listán kívül is).
    pub fn get_daily_leaderboard(&self, puzzle_date: &str, limit: i64, user_id: Option<i64>) -> (Vec<Value>, Option<Value>) {
        let limit = limit.clamp(1, super::LEADERBOARD_MAX_LIMIT);
        let rows = self
            .with(|tx| {
                fetch_all(
                    tx,
                    &format!(
                        "SELECT ds.user_id AS user_id, u.display_name AS display_name, ds.best_score AS best_score, \
                         ds.attempts AS attempts FROM daily_scores ds JOIN users u ON u.id = ds.user_id \
                         WHERE ds.puzzle_date = ? AND ds.attempts > 0 AND u.deleted_at IS NULL ORDER BY {DAILY_ORDER}"
                    ),
                    [puzzle_date],
                )
            })
            .unwrap_or_default();
        let mut entries = Vec::new();
        let mut me = None;
        for (i, row) in rows.iter().enumerate() {
            let rank = i + 1;
            let entry = json!({
                "rank": rank, "user_id": row.int("user_id"), "display_name": row.text("display_name"),
                "best_score": row.int("best_score"), "attempts": row.int("attempts"),
            });
            if rank as i64 <= limit {
                entries.push(entry.clone());
            }
            if user_id.is_some() && Some(row.int("user_id")) == user_id {
                me = Some(entry);
            }
        }
        (entries, me)
    }

    // --- Elemzés, kitüntetések ---

    /// A gyorsítótárazott játékelemzés: (verzió, eredmény JSON) vagy None.
    pub fn get_game_analysis(&self, game_id: i64) -> Option<(i64, String)> {
        self.with(|tx| {
            tx.query_row("SELECT version, result_json FROM game_analysis WHERE game_id = ?", [game_id], |r| Ok((r.get(0)?, r.get(1)?))).optional()
        })
        .ok()
        .flatten()
    }

    pub fn save_game_analysis(&self, game_id: i64, version: i64, result_json: &str) -> rusqlite::Result<()> {
        self.with(|tx| {
            tx.execute(
                "INSERT INTO game_analysis (game_id, version, result_json) VALUES (?, ?, ?) \
                 ON CONFLICT(game_id) DO UPDATE SET version = excluded.version, \
                 result_json = excluded.result_json, created_at = datetime('now')",
                params![game_id, version, result_json],
            )
            .map(|_| ())
        })
    }

    /// Kitüntetések rögzítése (kulcsonként egyszer). Visszatér: az újonnan megszerzettek listája.
    pub fn grant_achievements(&self, user_id: i64, badges: &HashSet<String>, game_id: Option<i64>) -> Vec<String> {
        let mut sorted: Vec<&String> = badges.iter().collect();
        sorted.sort();
        self.with(|tx| {
            let mut new = Vec::new();
            for badge in sorted {
                let inserted = tx.execute(
                    "INSERT OR IGNORE INTO achievements (user_id, badge, game_id) VALUES (?, ?, ?)",
                    params![user_id, badge, game_id],
                )?;
                if inserted > 0 {
                    new.push(badge.clone());
                }
            }
            Ok(new)
        })
        .unwrap_or_default()
    }

    /// A felhasználó kitüntetései, megszerzés sorrendjében.
    pub fn get_user_achievements(&self, user_id: i64) -> Vec<Value> {
        self.with(|tx| {
            let rows = fetch_all(
                tx,
                "SELECT badge, game_id, earned_at FROM achievements WHERE user_id = ? ORDER BY earned_at, rowid",
                [user_id],
            )?;
            Ok(rows.into_iter().map(Value::Object).collect())
        })
        .unwrap_or_default()
    }

    // --- Friendship System ---

    /// Barátkérés küldése. Visszatér: (sikerült-e, üzenet).
    pub fn send_friend_request(&self, user_id: i64, friend_id: i64) -> (bool, String) {
        if user_id == friend_id {
            return (false, "Nem küldhetsz magadnak barátkérést.".to_string());
        }
        let result = self.with(|tx| {
            let exists: Option<i64> = tx.query_row("SELECT id FROM users WHERE id = ?", [friend_id], |r| r.get(0)).optional()?;
            if exists.is_none() {
                return Ok((false, "Felhasználó nem található.".to_string()));
            }
            let existing: Option<(String, i64)> = tx
                .query_row(
                    "SELECT status, user_id FROM friendships WHERE (user_id = ? AND friend_id = ?) OR (user_id = ? AND friend_id = ?)",
                    params![user_id, friend_id, friend_id, user_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((status, from)) = existing {
                return Ok((
                    false,
                    if status == "accepted" {
                        "Már barátok vagytok.".to_string()
                    } else if from == user_id {
                        "Már küldtél barátkérést ennek a felhasználónak.".to_string()
                    } else {
                        "Ez a felhasználó már küldött neked barátkérést. Fogadd el!".to_string()
                    },
                ));
            }
            match tx.execute("INSERT INTO friendships (user_id, friend_id, status) VALUES (?, ?, 'pending')", params![user_id, friend_id]) {
                Ok(_) => Ok((true, "Barátkérés elküldve.".to_string())),
                Err(rusqlite::Error::SqliteFailure(err, _)) if err.code == rusqlite::ErrorCode::ConstraintViolation => {
                    Ok((false, "Hiba történt a barátkérés során.".to_string()))
                }
                Err(e) => Err(e),
            }
        });
        result.unwrap_or_else(|e| (false, format!("Adatbázis-hiba: {e}")))
    }

    /// Barátkérés elfogadása.
    pub fn accept_friend_request(&self, user_id: i64, requester_id: i64) -> (bool, String) {
        let updated = self
            .with(|tx| {
                tx.execute(
                    "UPDATE friendships SET status = 'accepted' WHERE user_id = ? AND friend_id = ? AND status = 'pending'",
                    params![requester_id, user_id],
                )
            })
            .unwrap_or(0);
        if updated > 0 {
            (true, "Barátkérés elfogadva.".to_string())
        } else {
            (false, "Barátkérés nem található vagy már elfogadtad.".to_string())
        }
    }

    /// Barátkérés elutasítása.
    pub fn decline_friend_request(&self, user_id: i64, requester_id: i64) -> (bool, String) {
        let deleted = self
            .with(|tx| {
                tx.execute(
                    "DELETE FROM friendships WHERE user_id = ? AND friend_id = ? AND status = 'pending'",
                    params![requester_id, user_id],
                )
            })
            .unwrap_or(0);
        if deleted > 0 { (true, "Barátkérés elutasítva.".to_string()) } else { (false, "Barátkérés nem található.".to_string()) }
    }

    /// Barát törlése.
    pub fn remove_friend(&self, user_id: i64, friend_id: i64) -> (bool, String) {
        let deleted = self
            .with(|tx| {
                tx.execute(
                    "DELETE FROM friendships WHERE ((user_id = ? AND friend_id = ?) OR (user_id = ? AND friend_id = ?)) AND status = 'accepted'",
                    params![user_id, friend_id, friend_id, user_id],
                )
            })
            .unwrap_or(0);
        if deleted > 0 { (true, "Barát törölve.".to_string()) } else { (false, "Nem vagytok barátok.".to_string()) }
    }

    /// Barátlista: [{id, display_name}] név szerint.
    pub fn get_friends(&self, user_id: i64) -> Vec<Row> {
        self.with(|tx| {
            fetch_all(
                tx,
                "SELECT u.id, u.display_name FROM friendships f \
                 JOIN users u ON (f.user_id = u.id OR f.friend_id = u.id) \
                 WHERE (f.user_id = ? OR f.friend_id = ?) AND f.status = 'accepted' AND u.id != ? \
                 ORDER BY u.display_name",
                params![user_id, user_id, user_id],
            )
        })
        .unwrap_or_default()
    }

    /// Bejövő barátkérések.
    pub fn get_pending_requests(&self, user_id: i64) -> Vec<Row> {
        self.with(|tx| {
            fetch_all(
                tx,
                "SELECT u.id, u.display_name, f.created_at FROM friendships f JOIN users u ON f.user_id = u.id \
                 WHERE f.friend_id = ? AND f.status = 'pending' ORDER BY f.created_at DESC",
                [user_id],
            )
        })
        .unwrap_or_default()
    }

    /// Kimenő barátkérések.
    pub fn get_sent_requests(&self, user_id: i64) -> Vec<Row> {
        self.with(|tx| {
            fetch_all(
                tx,
                "SELECT u.id, u.display_name, f.created_at FROM friendships f JOIN users u ON f.friend_id = u.id \
                 WHERE f.user_id = ? AND f.status = 'pending' ORDER BY f.created_at DESC",
                [user_id],
            )
        })
        .unwrap_or_default()
    }

    /// Felhasználók keresése megjelenítési név részlet (kis/nagybetű- és ékezet-egyező kisbetűsítéssel) vagy
    /// pontos email cím alapján. Az email cím nem részletre illeszkedik: így a keresés nem szivárogtatja ki,
    /// hogy kinek milyen email címe van.
    pub fn search_users(&self, query: &str, exclude_user_id: i64, limit: i64) -> Vec<Row> {
        let query = query.trim();
        if query.chars().count() < 2 {
            return Vec::new();
        }
        let lowered = query.to_lowercase();
        let pattern = format!("%{}%", escape_like(&lowered));
        self.with(|tx| {
            // SQLite lower()/LIKE csak ASCII-ra kis/nagybetű-független: az ékezetekhez Unicode-os kisbetűsítés kell
            tx.create_scalar_function("py_lower", 1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |ctx| {
                let value: Option<String> = ctx.get(0)?;
                Ok(value.map(|v| v.to_lowercase()))
            })?;
            fetch_all(
                tx,
                "SELECT id, display_name FROM users \
                 WHERE (py_lower(display_name) LIKE ? ESCAPE '\\' OR email_lower = ?) \
                 AND id != ? AND deleted_at IS NULL ORDER BY display_name LIMIT ?",
                params![pattern, lowered, exclude_user_id, limit],
            )
        })
        .unwrap_or_default()
    }
}
