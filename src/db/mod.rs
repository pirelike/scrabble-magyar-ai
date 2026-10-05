//! SQLite adatbázis-réteg (az `auth.py` portja): séma, migrációk és a felhasználók, munkamenetek, mentett
//! játékok, ranglista, barátok és egyéb táblák kezelése. A séma a Python változattal azonos, így a meglévő
//! `scrabble.db` közvetlenül használható.

use parking_lot::{Mutex, MutexGuard};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OptionalExtension, Params, Transaction};
use serde_json::{Map, Value};
use std::time::Duration;

mod games;
mod misc;
mod users;

pub use games::*;
pub use misc::*;
pub use users::*;

/// Egy adatbázissor oszlopnév → érték szerint (mint a Python `dict(row)`).
pub type Row = Map<String, Value>;

pub const SCHEMA: &str = include_str!("schema.sql");

pub struct Db {
    pub path: String,
    conn: Mutex<Connection>,
}

/// Az adatbázis egy kapcsolata, zárolva (a hosszabb, többlépéses műveletekhez).
pub type ConnGuard<'a> = MutexGuard<'a, Connection>;

fn open_connection(path: &str) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(10))?;
    // a WAL üzemmód sorban visszaad egy értéket
    let _mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    Ok(conn)
}

impl Db {
    pub fn open(path: &str) -> rusqlite::Result<Db> {
        Ok(Db { path: path.to_string(), conn: Mutex::new(open_connection(path)?) })
    }

    /// Ideiglenes (tesztekhez használt) adatbázis egyedi fájlban.
    pub fn open_temp() -> rusqlite::Result<Db> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("scrabble-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("db-{}.sqlite", COUNTER.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_file(&path);
        let db = Db::open(path.to_str().expect("utf-8 útvonal"))?;
        db.init()?;
        Ok(db)
    }

    /// Egy tranzakció: a végén commit, hiba esetén visszagörgetés (mint a Python `_db()`).
    pub fn with<T>(&self, f: impl FnOnce(&Transaction) -> rusqlite::Result<T>) -> rusqlite::Result<T> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let result = f(&tx)?;
        tx.commit()?;
        Ok(result)
    }

    /// Közvetlen (tranzakció nélküli) kapcsolat: pl. VACUUM, mentés.
    pub fn raw(&self) -> ConnGuard<'_> {
        self.conn.lock()
    }

    /// Adatbázis séma létrehozása, ha nem létezik (és a migrációk).
    pub fn init(&self) -> rusqlite::Result<()> {
        let conn = self.conn.lock();
        conn.execute_batch(SCHEMA)?;
        // Régi adatbázisok bővítése (migrációk)
        ensure_column(&conn, "saved_games", "owner_name", "TEXT NOT NULL DEFAULT ''")?;
        ensure_column(&conn, "saved_games", "owner_token", "TEXT")?;
        ensure_column(&conn, "users", "reconnect_token", "TEXT")?;
        // Robotos játékok nem számítanak bele a ranglistába
        ensure_column(&conn, "saved_games", "has_bots", "INTEGER NOT NULL DEFAULT 0")?;
        // Megosztható visszajátszás-link (nyilvános, a játékosok kérésére jön létre)
        ensure_column(&conn, "saved_games", "share_token", "TEXT")?;
        // Levelezős játék: a játékosok órák / napok alatt lépnek, a mentés a játék tartós otthona
        ensure_column(&conn, "saved_games", "is_async", "INTEGER NOT NULL DEFAULT 0")?;
        // Élő-értékszám (ELO) és az értékelt játékok száma
        ensure_column(&conn, "users", "rating", &format!("INTEGER NOT NULL DEFAULT {}", crate::elo::INITIAL_RATING))?;
        ensure_column(&conn, "users", "rated_games", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_column(&conn, "game_players", "rating_before", "INTEGER")?;
        ensure_column(&conn, "game_players", "rating_after", "INTEGER")?;
        // Admin panel: az utolsó admin-kérés ideje (tétlenségi időkorlát) és a sudo mód lejárata
        ensure_column(&conn, "sessions", "admin_seen_at", "TEXT")?;
        ensure_column(&conn, "sessions", "sudo_until", "TEXT")?;
        // Admin panel: kitiltás, némítás, szótár-építő tiltás, utolsó belépés, törlés (anonimizálás) jelzése
        for (column, ddl) in [
            ("banned_until", "TEXT"),
            ("ban_reason", "TEXT"),
            ("chat_muted_until", "TEXT"),
            ("review_blocked", "INTEGER NOT NULL DEFAULT 0"),
            ("last_login_at", "TEXT"),
            ("last_login_ip", "TEXT"),
            ("deleted_at", "TEXT"),
        ] {
            ensure_column(&conn, "users", column, ddl)?;
        }
        // Admin panel: a munkamenet létrehozásának IP-je és böngészője, az utolsó használat ideje
        for column in ["ip", "user_agent", "last_seen"] {
            ensure_column(&conn, "sessions", column, "TEXT")?;
        }
        conn.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_saved_games_share_token \
             ON saved_games(share_token) WHERE share_token IS NOT NULL",
        )?;
        Ok(())
    }
}

/// Oszlop hozzáadása migrációval, ha még nincs (a nevek rögzített, kódból jövő értékek).
fn ensure_column(conn: &Connection, table: &str, column: &str, ddl: &str) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let existing: Vec<String> = stmt.query_map([], |row| row.get::<_, String>(1))?.collect::<Result<_, _>>()?;
    if !existing.iter().any(|c| c == column) {
        conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} {ddl}"), [])?;
    }
    Ok(())
}

// --- Általános sor-segédek ---

pub fn row_to_map(row: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    let mut map = Map::new();
    for (i, name) in row.as_ref().column_names().iter().enumerate() {
        let value = match row.get_ref(i)? {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(n) => Value::from(n),
            ValueRef::Real(f) => Value::from(f),
            ValueRef::Text(t) => Value::from(String::from_utf8_lossy(t).into_owned()),
            ValueRef::Blob(_) => Value::Null,
        };
        map.insert((*name).to_string(), value);
    }
    Ok(map)
}

/// Az összes találat sorai oszlopnév szerint.
pub fn fetch_all(conn: &Connection, sql: &str, params: impl Params) -> rusqlite::Result<Vec<Row>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, row_to_map)?;
    rows.collect()
}

/// Az első találat (vagy None).
pub fn fetch_one(conn: &Connection, sql: &str, params: impl Params) -> rusqlite::Result<Option<Row>> {
    let mut stmt = conn.prepare(sql)?;
    stmt.query_row(params, row_to_map).optional()
}

/// Típusos mezőelérés egy sorból.
pub trait RowExt {
    fn int(&self, key: &str) -> i64;
    fn opt_int(&self, key: &str) -> Option<i64>;
    fn text(&self, key: &str) -> String;
    fn opt_text(&self, key: &str) -> Option<String>;
    fn flag(&self, key: &str) -> bool;
}

impl RowExt for Row {
    fn int(&self, key: &str) -> i64 {
        self.get(key).and_then(|v| v.as_i64()).unwrap_or(0)
    }
    fn opt_int(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(|v| v.as_i64())
    }
    fn text(&self, key: &str) -> String {
        self.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string()
    }
    fn opt_text(&self, key: &str) -> Option<String> {
        self.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
    }
    fn flag(&self, key: &str) -> bool {
        self.get(key).map(crate::util::truthy).unwrap_or(false)
    }
}

/// A `LIKE` mintában a joker-karakterek menekítése (`ESCAPE '\'`).
pub fn escape_like(text: &str) -> String {
    text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// `?,?,?` helyőrzők.
pub fn placeholders(n: usize) -> String {
    vec!["?"; n].join(",")
}
