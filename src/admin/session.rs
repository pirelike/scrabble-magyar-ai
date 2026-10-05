//! Admin munkamenet: tétlenség és sudo mód (a Python `admin.py` munkamenet-része).

use super::{AdminContext, AdminResult, TOUCH_INTERVAL_SECONDS, record};
use crate::app::App;
use crate::util;
use chrono::{Duration, NaiveDateTime};
use serde_json::{Value, json};

/// Az admin munkamenet állapota (a panel fejlécéhez és az őrhöz).
#[derive(Clone, Debug)]
pub struct SessionStatus {
    pub idle_minutes: i64,
    /// mp; 0 → lejárt, jelszó kell
    pub idle_remaining: i64,
    pub sudo_minutes: i64,
    /// mp; 0 → nincs érvényes sudo
    pub sudo_remaining: i64,
    /// az utolsó admin-kérés ideje
    pub seen_at: Option<NaiveDateTime>,
}

impl SessionStatus {
    pub fn idle_expired(&self) -> bool {
        self.idle_remaining <= 0
    }

    pub fn as_json(&self) -> Value {
        json!({
            "idle_minutes": self.idle_minutes,
            "idle_remaining": self.idle_remaining,
            "idle_expired": self.idle_expired(),
            "sudo_minutes": self.sudo_minutes,
            "sudo_remaining": self.sudo_remaining,
        })
    }
}

fn seconds_until(stamp: Option<&str>, now: NaiveDateTime) -> i64 {
    match stamp.and_then(util::parse_ts) {
        Some(moment) => (moment - now).num_seconds().max(0),
        None => 0,
    }
}

/// A munkamenet admin állapota; ha nincs ilyen session, None. A tétlenség az utolsó admin-kérésből számolódik; ha
/// még nem volt ilyen (nincs időbélyeg), lejártnak számít.
pub fn session_status(app: &App, token: &str) -> Option<SessionStatus> {
    let (seen_at, sudo_until) = app.db.get_admin_session(Some(token))?;
    let now = util::utcnow();
    let seen = seen_at.as_deref().and_then(util::parse_ts);
    let idle_limit = app.config.admin_session_idle_minutes * 60;
    let idle_remaining = match seen {
        None => 0,
        Some(seen) => (idle_limit - (now - seen).num_seconds()).max(0),
    };
    Some(SessionStatus {
        idle_minutes: app.config.admin_session_idle_minutes,
        idle_remaining,
        sudo_minutes: app.config.admin_sudo_minutes,
        // Tétlenség után a sudo sem él (az újraigazolás is lezárja)
        sudo_remaining: if idle_remaining <= 0 { 0 } else { seconds_until(sudo_until.as_deref(), now) },
        seen_at: seen,
    })
}

/// Tétlenségi időzítő frissítése egy sikeres admin kérés után (ritkított írással).
pub fn touch(app: &App, token: &str, status: Option<&SessionStatus>) {
    let owned;
    let status = match status {
        Some(status) => status,
        None => match session_status(app, token) {
            Some(status) => {
                owned = status;
                &owned
            }
            None => return,
        },
    };
    if let Some(seen) = status.seen_at
        && (util::utcnow() - seen).num_seconds() < TOUCH_INTERVAL_SECONDS
    {
        return;
    }
    app.db.touch_admin_session(token);
}

/// Igaz, ha a munkamenetnek érvényes (nem lejárt) sudo megerősítése van.
pub fn sudo_active(app: &App, token: &str) -> bool {
    session_status(app, token).is_some_and(|s| s.sudo_remaining > 0)
}

/// Jelszavas újraigazolás tétlenség után: az időzítő újraindul, a sudo lezárul. Naplózva.
pub fn reauth(app: &App, ctx: &AdminContext, token: &str) -> AdminResult<()> {
    app.db.with(|tx| {
        tx.execute("UPDATE sessions SET admin_seen_at = ? WHERE token = ?", rusqlite::params![util::now_ts(), token])?;
        tx.execute("UPDATE sessions SET sudo_until = NULL WHERE token = ?", [token])?;
        record(tx, ctx, "admin.reauth", None, None, &Value::Null)?;
        Ok(())
    })?;
    Ok(())
}

/// Sudo mód megnyitása `ADMIN_SUDO_MINUTES` percre. Naplózva, egy tranzakcióban.
pub fn grant_sudo(app: &App, ctx: &AdminContext, token: &str) -> AdminResult<()> {
    let minutes = app.config.admin_sudo_minutes;
    let until = util::utcnow() + Duration::minutes(minutes);
    app.db.with(|tx| {
        tx.execute("UPDATE sessions SET sudo_until = ?, admin_seen_at = ? WHERE token = ?", rusqlite::params![util::format_ts(until), util::now_ts(), token])?;
        record(tx, ctx, "admin.sudo", None, None, &json!({"minutes": minutes}))?;
        Ok(())
    })?;
    Ok(())
}

/// Sudo mód lezárása. Naplózva.
pub fn end_sudo(app: &App, ctx: &AdminContext, token: &str) -> AdminResult<()> {
    app.db.with(|tx| {
        tx.execute("UPDATE sessions SET sudo_until = NULL, admin_seen_at = ? WHERE token = ?", rusqlite::params![util::now_ts(), token])?;
        record(tx, ctx, "admin.sudo_end", None, None, &Value::Null)?;
        Ok(())
    })?;
    Ok(())
}

/// Rossz jelszó megadása (`admin.sudo_failed` / `admin.reauth_failed`): csak napló, nincs más hatása.
pub fn record_failed_password(app: &App, ctx: &AdminContext, name: &str) -> AdminResult<()> {
    app.db.with(|tx| record(tx, ctx, name, None, None, &Value::Null).map(|_| ()))?;
    Ok(())
}
