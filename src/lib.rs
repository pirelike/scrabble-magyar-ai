//! Magyar Scrabble klón — Rust backend (a korábbi Flask + Socket.IO szerver átírása).
//!
//! A forrás témakörönként van csoportosítva:
//!
//! | Mappa | Tartalom |
//! |---|---|
//! | `engine/` | játékszabályok: zsetonok, tábla, játékmenet, játékosok, megtámadás |
//! | `robot/` | robot ellenfél (lépéskeresés, fokozatok), napi feladvány, játékelemzés |
//! | `words/` | szóellenőrző (hu_HU), szótár, gyakorló módok, szótár-építő |
//! | `accounts/` | jelszó-hash, socket-token, értékszám (ELO), kitüntetések |
//! | `services/` | levelezés, Web Push, tunnel, forgalomkorlát |
//! | `db/` | SQLite réteg (séma: `db/schema.sql`) |
//! | `server/` | HTTP útvonalak és Socket.IO események, szobák és szerverállapot |
//! | `admin/` | admin panel (logika és HTTP réteg) |
//! | `bin/` | karbantartó eszközök (`bot_arena`, `word_review`, `build_attested`) |
//!
//! A moduloknak rövid, lapos elérési útjuk is van (`crate::tiles` = `crate::engine::tiles`): az alábbi
//! újraexportálások miatt a kód és a tesztek nem a mappaszerkezethez kötődnek.

pub mod accounts;
pub mod admin;
pub mod app;
pub mod async_games;
pub mod config;
pub mod db;
pub mod engine;
pub mod robot;
pub mod server;
pub mod services;
pub mod settings;
pub mod util;
pub mod words;

pub use accounts::{achievements, elo, password, socket_auth};
pub use engine::{board, challenge, game, player, tiles};
pub use robot::{ai, analysis, daily};
pub use server::{room, state};
pub use services::{mail, push, ratelimit, tunnel};
pub use words::{affix, dictionary, practice, word_review};
