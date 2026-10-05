//! Felhasználók, munkamenetek, verifikációs kódok, kitiltás / némítás, belépési napló, admin munkamenet.

use super::{Db, Row, RowExt, fetch_one};
use crate::config::{EMAIL_VERIFIED_WINDOW_MINUTES, SESSION_MAX_AGE_DAYS, VERIFICATION_CODE_EXPIRY_MINUTES, VERIFICATION_MAX_ATTEMPTS};
use crate::password::{check_password_hash, generate_password_hash};
use crate::util::{self, format_ts, parse_ts, token_urlsafe, utcnow};
use chrono::Duration;
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

/// Végleges kitiltás: ennyi a lejárat.
pub const PERMANENT_UNTIL: &str = "9999-12-31 23:59:59";
/// A munkamenet „utoljára látva” ideje ennyi másodpercenként frissül.
const SESSION_SEEN_INTERVAL: i64 = 300;

/// Egy felhasználó (a `users` tábla egy sora).
#[derive(Clone, Debug)]
pub struct User {
    pub id: i64,
    pub email: String,
    pub email_lower: String,
    pub display_name: String,
    pub password_hash: String,
    pub created_at: String,
    pub games_played: i64,
    pub games_won: i64,
    pub total_score: i64,
    pub reconnect_token: Option<String>,
    pub rating: i64,
    pub rated_games: i64,
    pub banned_until: Option<String>,
    pub ban_reason: Option<String>,
    pub chat_muted_until: Option<String>,
    pub review_blocked: bool,
    pub last_login_at: Option<String>,
    pub last_login_ip: Option<String>,
    pub deleted_at: Option<String>,
}

impl User {
    pub fn from_row(row: &Row) -> User {
        User {
            id: row.int("id"),
            email: row.text("email"),
            email_lower: row.text("email_lower"),
            display_name: row.text("display_name"),
            password_hash: row.text("password_hash"),
            created_at: row.text("created_at"),
            games_played: row.int("games_played"),
            games_won: row.int("games_won"),
            total_score: row.int("total_score"),
            reconnect_token: row.opt_text("reconnect_token"),
            rating: row.opt_int("rating").unwrap_or(crate::elo::INITIAL_RATING),
            rated_games: row.int("rated_games"),
            banned_until: row.opt_text("banned_until"),
            ban_reason: row.opt_text("ban_reason"),
            chat_muted_until: row.opt_text("chat_muted_until"),
            review_blocked: row.flag("review_blocked"),
            last_login_at: row.opt_text("last_login_at"),
            last_login_ip: row.opt_text("last_login_ip"),
            deleted_at: row.opt_text("deleted_at"),
        }
    }

    pub fn ban(&self) -> Option<Ban> {
        ban_from_parts(self.banned_until.as_deref(), self.ban_reason.as_deref())
    }
}

/// Egy aktív kitiltás.
#[derive(Clone, Debug, PartialEq)]
pub struct Ban {
    pub reason: String,
    /// None = végleges
    pub until: Option<String>,
    pub permanent: bool,
}

impl Ban {
    pub fn to_json(&self) -> Value {
        json!({"reason": self.reason, "until": self.until, "permanent": self.permanent})
    }
}

/// Igaz, ha a lejárati időbélyeg még a jövőben van (a hiányzó érték nem aktív).
pub fn until_active(stamp: Option<&str>) -> bool {
    stamp.and_then(parse_ts).is_some_and(|moment| moment > utcnow())
}

/// Az aktív kitiltás a (banned_until, ban_reason) párból, vagy None.
pub fn ban_from_parts(banned_until: Option<&str>, ban_reason: Option<&str>) -> Option<Ban> {
    if !until_active(banned_until) {
        return None;
    }
    let permanent = banned_until == Some(PERMANENT_UNTIL);
    Some(Ban { reason: ban_reason.unwrap_or("").to_string(), until: if permanent { None } else { banned_until.map(|s| s.to_string()) }, permanent })
}

/// A munkamenethez tartozó felhasználó (a `validate_session` eredménye).
#[derive(Clone, Debug)]
pub struct SessionUser {
    pub id: i64,
    pub email: String,
    pub display_name: String,
    pub games_played: i64,
    pub games_won: i64,
    pub total_score: i64,
    pub reconnect_token: Option<String>,
    pub rating: i64,
    pub rated_games: i64,
}

fn expiry_after(minutes: i64) -> String {
    format_ts(utcnow() + Duration::minutes(minutes))
}

impl Db {
    // --- User CRUD ---

    pub fn get_user_by_email(&self, email: &str) -> rusqlite::Result<Option<User>> {
        let key = email.to_lowercase();
        let key = key.trim();
        self.with(|tx| Ok(fetch_one(tx, "SELECT * FROM users WHERE email_lower = ?", [key])?.map(|r| User::from_row(&r))))
    }

    pub fn get_user_by_id(&self, user_id: i64) -> rusqlite::Result<Option<User>> {
        self.with(|tx| Ok(fetch_one(tx, "SELECT * FROM users WHERE id = ?", [user_id])?.map(|r| User::from_row(&r))))
    }

    /// Új felhasználó létrehozása. Visszatér: a felhasználó azonosítója vagy a hibaüzenet.
    pub fn create_user(&self, email: &str, display_name: &str, password: &str) -> Result<i64, String> {
        let email_lower = email.to_lowercase();
        let email_lower = email_lower.trim().to_string();
        const EXISTS: &str = "Ez az email cím már regisztrálva van.";
        if self.get_user_by_email(&email_lower).map_err(|e| e.to_string())?.is_some() {
            return Err(EXISTS.to_string());
        }
        let password_hash = generate_password_hash(password);
        let result = self.with(|tx| {
            tx.execute(
                "INSERT INTO users (email, email_lower, display_name, password_hash) VALUES (?, ?, ?, ?)",
                params![email.trim(), email_lower, display_name.trim(), password_hash],
            )?;
            Ok(tx.last_insert_rowid())
        });
        match result {
            Ok(id) => Ok(id),
            Err(rusqlite::Error::SqliteFailure(err, _)) if err.code == rusqlite::ErrorCode::ConstraintViolation => Err(EXISTS.to_string()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Jelszó ellenőrzés. Visszatér: a felhasználó vagy a hibaüzenet.
    pub fn verify_password(&self, email: &str, password: &str) -> Result<User, String> {
        const BAD: &str = "Hibás email cím vagy jelszó.";
        let user = self.get_user_by_email(email).map_err(|e| e.to_string())?;
        let Some(user) = user else { return Err(BAD.to_string()) };
        if user.deleted_at.is_some() || user.password_hash.is_empty() {
            return Err(BAD.to_string());
        }
        if !check_password_hash(&user.password_hash, password) {
            return Err(BAD.to_string());
        }
        Ok(user)
    }

    /// Használati számláló növelése (napi bontásban): a statisztika a gyakorló módok használatát ebből mutatja.
    /// A hibája sosem akadályozza a kérést.
    pub fn bump_counter(&self, key: &str, amount: i64) {
        let day = utcnow().format("%Y-%m-%d").to_string();
        let _ = self.with(|tx| {
            tx.execute(
                "INSERT INTO usage_counters (day, key, count) VALUES (?, ?, ?) \
                 ON CONFLICT(day, key) DO UPDATE SET count = count + excluded.count",
                params![day, key, amount],
            )
        });
    }

    // --- Kitiltás, némítás, belépési napló ---

    pub fn get_ban(&self, user_id: i64) -> Option<Ban> {
        if user_id == 0 {
            return None;
        }
        self.with(|tx| {
            tx.query_row("SELECT banned_until, ban_reason FROM users WHERE id = ?", [user_id], |r| {
                Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?))
            })
            .optional()
        })
        .ok()
        .flatten()
        .and_then(|(until, reason)| ban_from_parts(until.as_deref(), reason.as_deref()))
    }

    /// Igaz, ha a felhasználó chat némítása még érvényben van.
    pub fn is_chat_muted(&self, user_id: i64) -> bool {
        if user_id == 0 {
            return false;
        }
        let until: Option<Option<String>> =
            self.with(|tx| tx.query_row("SELECT chat_muted_until FROM users WHERE id = ?", [user_id], |r| r.get(0)).optional()).ok().flatten();
        until.is_some_and(|u| until_active(u.as_deref()))
    }

    /// Igaz, ha a felhasználó nem szavazhat a szótár-építőben.
    pub fn is_review_blocked(&self, user_id: i64) -> bool {
        if user_id == 0 {
            return false;
        }
        self.with(|tx| tx.query_row("SELECT review_blocked FROM users WHERE id = ?", [user_id], |r| r.get::<_, i64>(0)).optional())
            .ok()
            .flatten()
            .is_some_and(|v| v != 0)
    }

    /// Belépési kísérlet naplózása (sikeres és sikertelen is). A napló hibája sosem akadályozza a belépést.
    pub fn record_login_event(&self, email: &str, success: bool, ip: Option<&str>, user_agent: Option<&str>, user_id: Option<i64>, reason: &str) {
        let email: String = email.chars().take(254).collect();
        let agent: Option<String> = user_agent.map(|a| a.chars().take(300).collect::<String>()).filter(|a| !a.is_empty());
        let result = self.with(|tx| {
            tx.execute(
                "INSERT INTO login_events (user_id, email, ip, user_agent, success, reason) VALUES (?, ?, ?, ?, ?, ?)",
                params![user_id, email, ip, agent, success as i64, reason],
            )?;
            if let (true, Some(uid)) = (success, user_id) {
                tx.execute("UPDATE users SET last_login_at = ?, last_login_ip = ? WHERE id = ?", params![util::now_ts(), ip, uid])?;
            }
            Ok(())
        });
        if let Err(error) = result {
            println!("[auth] A belépési napló nem írható: {error}");
        }
    }

    // --- Verification codes ---

    /// 6 számjegyű verifikációs kód generálása. Visszaadja a kódot.
    pub fn create_verification_code(&self, email: &str) -> rusqlite::Result<String> {
        let code = format!("{:06}", util::random_below(1_000_000));
        let expires_at = expiry_after(VERIFICATION_CODE_EXPIRY_MINUTES);
        let email_lower = email.to_lowercase();
        let email_lower = email_lower.trim().to_string();
        self.with(|tx| {
            tx.execute("UPDATE verification_codes SET used = 1 WHERE email = ? AND used = 0", [&email_lower])?;
            tx.execute("INSERT INTO verification_codes (email, code, expires_at) VALUES (?, ?, ?)", params![email_lower, code, expires_at])?;
            Ok(())
        })?;
        Ok(code)
    }

    /// Verifikációs kód ellenőrzés. Visszatér: (sikerült-e, üzenet).
    pub fn verify_code(&self, email: &str, code: &str) -> (bool, String) {
        let email_lower = email.to_lowercase();
        let email_lower = email_lower.trim().to_string();
        let result = self.with(|tx| {
            let Some(row) = fetch_one(tx, "SELECT * FROM verification_codes WHERE email = ? AND used = 0 ORDER BY created_at DESC LIMIT 1", [&email_lower])?
            else {
                return Ok((false, "Nincs érvényes verifikációs kód. Kérj újat.".to_string()));
            };
            let id = row.int("id");
            let attempts = row.int("attempts");
            let expired = parse_ts(&row.text("expires_at")).is_none_or(|e| utcnow() > e);
            if expired {
                tx.execute("UPDATE verification_codes SET used = 1 WHERE id = ?", [id])?;
                return Ok((false, "A kód lejárt. Kérj újat.".to_string()));
            }
            if attempts >= VERIFICATION_MAX_ATTEMPTS {
                tx.execute("UPDATE verification_codes SET used = 1 WHERE id = ?", [id])?;
                return Ok((false, "Túl sok próbálkozás. Kérj új kódot.".to_string()));
            }
            if row.text("code") != code {
                // Atomi attempts növelés: UPDATE csak ha attempts < MAX
                let updated =
                    tx.execute("UPDATE verification_codes SET attempts = attempts + 1 WHERE id = ? AND attempts < ?", params![id, VERIFICATION_MAX_ATTEMPTS])?;
                if updated == 0 {
                    tx.execute("UPDATE verification_codes SET used = 1 WHERE id = ?", [id])?;
                    return Ok((false, "Túl sok próbálkozás. Kérj új kódot.".to_string()));
                }
                let remaining = VERIFICATION_MAX_ATTEMPTS - attempts - 1;
                return Ok((false, format!("Hibás kód. Még {remaining} próbálkozásod van.")));
            }
            tx.execute("UPDATE verification_codes SET used = 1 WHERE id = ?", [id])?;
            let verified_until = expiry_after(EMAIL_VERIFIED_WINDOW_MINUTES);
            tx.execute("INSERT OR REPLACE INTO verified_emails (email, expires_at) VALUES (?, ?)", params![email_lower, verified_until])?;
            Ok((true, "Kód elfogadva.".to_string()))
        });
        result.unwrap_or_else(|e| (false, format!("Adatbázis-hiba: {e}")))
    }

    /// Igaz, ha az email címet a közelmúltban sikeresen megerősítették kóddal.
    pub fn is_email_verified(&self, email: &str) -> bool {
        let email_lower = email.to_lowercase();
        let email_lower = email_lower.trim().to_string();
        self.with(|tx| {
            let expires: Option<String> = tx.query_row("SELECT expires_at FROM verified_emails WHERE email = ?", [&email_lower], |r| r.get(0)).optional()?;
            let Some(expires) = expires else { return Ok(false) };
            if parse_ts(&expires).is_none_or(|e| utcnow() > e) {
                tx.execute("DELETE FROM verified_emails WHERE email = ?", [&email_lower])?;
                return Ok(false);
            }
            Ok(true)
        })
        .unwrap_or(false)
    }

    /// A megerősítés felhasználása (regisztráció után egyszer használatos).
    pub fn clear_email_verification(&self, email: &str) {
        let email_lower = email.to_lowercase();
        let _ = self.with(|tx| tx.execute("DELETE FROM verified_emails WHERE email = ?", [email_lower.trim()]));
    }

    // --- Sessions ---

    /// Új session token létrehozása. Visszaadja a tokent.
    pub fn create_session(&self, user_id: i64, ip: Option<&str>, user_agent: Option<&str>) -> rusqlite::Result<String> {
        let token = token_urlsafe(48);
        let expires_at = format_ts(utcnow() + Duration::days(SESSION_MAX_AGE_DAYS));
        let now = util::now_ts();
        let agent: Option<String> = user_agent.map(|a| a.chars().take(300).collect::<String>()).filter(|a| !a.is_empty());
        self.with(|tx| {
            tx.execute(
                "INSERT INTO sessions (user_id, token, expires_at, ip, user_agent, last_seen) VALUES (?, ?, ?, ?, ?, ?)",
                params![user_id, token, expires_at, ip, agent, now],
            )?;
            Ok(())
        })?;
        Ok(token)
    }

    /// Session token ellenőrzés. Kitiltott vagy törölt (anonimizált) fiók munkamenete sem érvényes.
    pub fn validate_session(&self, token: Option<&str>) -> Option<SessionUser> {
        let token = token.filter(|t| !t.is_empty())?;
        self.with(|tx| {
            let Some(row) = fetch_one(
                tx,
                "SELECT s.id AS sid, s.expires_at AS expires_at, s.last_seen AS last_seen, u.id AS uid, u.email, u.display_name, \
                 u.games_played, u.games_won, u.total_score, u.reconnect_token, u.rating, u.rated_games, \
                 u.banned_until, u.ban_reason, u.deleted_at \
                 FROM sessions s JOIN users u ON s.user_id = u.id WHERE s.token = ?",
                [token],
            )?
            else {
                return Ok(None);
            };
            let sid = row.int("sid");
            if parse_ts(&row.text("expires_at")).is_none_or(|e| utcnow() > e) {
                tx.execute("DELETE FROM sessions WHERE id = ?", [sid])?;
                return Ok(None);
            }
            if row.opt_text("deleted_at").is_some() || ban_from_parts(row.opt_text("banned_until").as_deref(), row.opt_text("ban_reason").as_deref()).is_some()
            {
                return Ok(None);
            }
            // Az „utoljára látva” idő ritkított frissítése (a legtöbb kérésnél nincs írás)
            let stale = row.opt_text("last_seen").as_deref().and_then(parse_ts).is_none_or(|seen| (utcnow() - seen).num_seconds() > SESSION_SEEN_INTERVAL);
            if stale {
                tx.execute("UPDATE sessions SET last_seen = ? WHERE id = ?", params![util::now_ts(), sid])?;
            }
            Ok(Some(SessionUser {
                id: row.int("uid"),
                email: row.text("email"),
                display_name: row.text("display_name"),
                games_played: row.int("games_played"),
                games_won: row.int("games_won"),
                total_score: row.int("total_score"),
                reconnect_token: row.opt_text("reconnect_token"),
                rating: row.opt_int("rating").unwrap_or(crate::elo::INITIAL_RATING),
                rated_games: row.int("rated_games"),
            }))
        })
        .ok()
        .flatten()
    }

    /// Session törlése (logout).
    pub fn delete_session(&self, token: Option<&str>) {
        let Some(token) = token.filter(|t| !t.is_empty()) else { return };
        let _ = self.with(|tx| tx.execute("DELETE FROM sessions WHERE token = ?", [token]));
    }

    // --- Admin munkamenet ---

    /// Az admin munkamenet állapota: (admin_seen_at, sudo_until). Nincs ilyen session → None.
    pub fn get_admin_session(&self, token: Option<&str>) -> Option<(Option<String>, Option<String>)> {
        let token = token.filter(|t| !t.is_empty())?;
        self.with(|tx| {
            tx.query_row("SELECT admin_seen_at, sudo_until FROM sessions WHERE token = ?", [token], |r| {
                Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?))
            })
            .optional()
        })
        .ok()
        .flatten()
    }

    /// Az utolsó admin-kérés idejének frissítése (a tétlenségi időkorlát innen számolódik).
    pub fn touch_admin_session(&self, token: &str) {
        let _ = self.with(|tx| tx.execute("UPDATE sessions SET admin_seen_at = ? WHERE token = ?", params![util::now_ts(), token]));
    }

    /// A sudo mód lejáratának beállítása (None → lezárás). Frissíti a tétlenségi időzítőt is.
    pub fn set_admin_sudo(&self, token: &str, until: Option<chrono::NaiveDateTime>) {
        let sudo_until = until.map(format_ts);
        let _ = self.with(|tx| tx.execute("UPDATE sessions SET sudo_until = ?, admin_seen_at = ? WHERE token = ?", params![sudo_until, util::now_ts(), token]));
    }

    /// Visszaadja a felhasználó reconnect tokenjét, vagy generál egyet (kriptográfiailag erős).
    pub fn get_or_create_user_reconnect_token(&self, user_id: i64) -> rusqlite::Result<String> {
        self.with(|tx| {
            let existing: Option<Option<String>> = tx.query_row("SELECT reconnect_token FROM users WHERE id = ?", [user_id], |r| r.get(0)).optional()?;
            if let Some(Some(token)) = existing.clone().filter(|t| t.as_ref().is_some_and(|s| !s.is_empty())) {
                return Ok(token);
            }
            loop {
                let token = token_urlsafe(16);
                let taken: Option<i64> = tx.query_row("SELECT id FROM users WHERE reconnect_token = ?", [&token], |r| r.get(0)).optional()?;
                if taken.is_none() {
                    tx.execute("UPDATE users SET reconnect_token = ? WHERE id = ?", params![token, user_id])?;
                    return Ok(token);
                }
            }
        })
    }

    /// Lejárt sessionök és verifikációs kódok törlése.
    pub fn cleanup_expired(&self) {
        let now = util::now_ts();
        let _ = self.with(|tx| {
            tx.execute("DELETE FROM sessions WHERE expires_at < ?", [&now])?;
            tx.execute("DELETE FROM verification_codes WHERE expires_at < ?", [&now])?;
            tx.execute("DELETE FROM verified_emails WHERE expires_at < ?", [&now])?;
            Ok(())
        });
    }
}

/// Igaz, ha az e-mail cím admin: szerepel a szerveren beállított `ADMIN_EMAILS`-ben. Szándékosan nem
/// adatbázis-oszlop és nem a megjelenítési névhez kötött; vendég (nincs felhasználó) soha nem admin.
pub fn is_admin_email(email: &str, admin_emails: &std::collections::HashSet<String>) -> bool {
    !admin_emails.is_empty() && admin_emails.contains(email.trim().to_lowercase().as_str())
}
