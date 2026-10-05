//! Admin panel: moderáció — tiltott szavak, chat napló, bejelentések, nevek átnézése.
//!
//! A játékszerver is használja: a tiltott szavak szűrése (`contains_banned`, `filter_chat`), a chat tartós
//! naplózása (`log_chat`) és a játékosok bejelentései (`create_report`) ide futnak be.

use crate::admin::fold;
use crate::db::Db;
use crate::room::Room;
use parking_lot::Mutex;
use serde_json::{Value, json};

pub const MAX_REPORT_REASON: usize = 300;
pub const MAX_REPORT_MESSAGE: usize = 200;
pub const SNAPSHOT_CHAT_LINES: usize = 15;
pub const REPORT_KINDS: [&str; 2] = ["player", "chat"];
pub const REPORT_STATUSES: [&str; 3] = ["new", "handled", "rejected"];

/// A tiltott szavak gyorsítótára (kis-nagybetű és ékezet nélküli alakban).
#[derive(Default)]
pub struct BannedWords {
    cache: Mutex<Option<Vec<String>>>,
}

impl BannedWords {
    pub fn invalidate(&self) {
        *self.cache.lock() = None;
    }

    pub fn words(&self, db: &Db) -> Vec<String> {
        let mut cache = self.cache.lock();
        if let Some(words) = cache.as_ref() {
            return words.clone();
        }
        let words: Vec<String> = db
            .with(|tx| {
                let rows = crate::db::fetch_all(tx, "SELECT word FROM banned_words ORDER BY word", [])?;
                Ok(rows.iter().map(|r| r.get("word").and_then(|w| w.as_str()).unwrap_or("").to_string()).collect())
            })
            .unwrap_or_default();
        *cache = Some(words.clone());
        words
    }
}

/// A szöveg ékezet- és kisbetű-független alakja + minden karakterének eredeti helye.
fn fold_with_map(text: &str) -> (Vec<char>, Vec<usize>) {
    let mut chars = Vec::new();
    let mut index = Vec::new();
    for (i, ch) in text.chars().enumerate() {
        for folded in fold(&ch.to_string()).chars() {
            chars.push(folded);
            index.push(i);
        }
    }
    (chars, index)
}

/// A szövegben talált tiltott szavak helyei: [(kezdet, vég)] az eredeti szövegben (átfedés nélkül).
pub fn find_banned(words: &[String], text: &str) -> Vec<(usize, usize)> {
    if words.is_empty() {
        return Vec::new();
    }
    let (folded, index) = fold_with_map(text);
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for word in words {
        let needle: Vec<char> = word.chars().collect();
        if needle.is_empty() || needle.len() > folded.len() {
            continue;
        }
        for start in 0..=(folded.len() - needle.len()) {
            if folded[start..start + needle.len()] == needle[..] {
                spans.push((index[start], index[start + needle.len() - 1] + 1));
            }
        }
    }
    spans.sort();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in spans {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}

pub fn contains_banned(words: &[String], text: &str) -> bool {
    !find_banned(words, text).is_empty()
}

/// A tiltott szavak kicsillagozva. Visszatér: (szöveg, talált-e).
pub fn mask_banned(words: &[String], text: &str) -> (String, bool) {
    let spans = find_banned(words, text);
    if spans.is_empty() {
        return (text.to_string(), false);
    }
    let mut chars: Vec<char> = text.chars().collect();
    for (start, end) in spans {
        for ch in chars.iter_mut().take(end).skip(start) {
            *ch = '*';
        }
    }
    (chars.into_iter().collect(), true)
}

/// A chat üzenet szűrése a beállított kezelés szerint. Visszatér: (üzenet vagy None = eldobva, jelölt-e).
pub fn filter_chat(words: &[String], drop_instead_of_mask: bool, message: &str) -> (Option<String>, bool) {
    let (masked, flagged) = mask_banned(words, message);
    if !flagged {
        return (Some(message.to_string()), false);
    }
    if drop_instead_of_mask { (None, true) } else { (Some(masked), true) }
}

/// A chat üzenet tartós naplózása (csak ha a beállítás be van kapcsolva; a hiba nem akadályozza a chatet).
pub fn log_chat(db: &Db, enabled: bool, room: &Room, user_id: Option<i64>, name: &str, message: &str) {
    if !enabled {
        return;
    }
    let result = db.with(|tx| {
        tx.execute(
            "INSERT INTO chat_log (room_id, room_name, user_id, name, message) VALUES (?, ?, ?, ?, ?)",
            rusqlite::params![room.id, room.name, user_id, name, message],
        )
        .map(|_| ())
    });
    if let Err(e) = result {
        println!("[chat] A chat napló nem írható: {e}");
    }
}

/// Egy bejelentés rögzítése a bejelentett tartalom pillanatképével. Visszatér: az azonosító, vagy None, ha a
/// bejelentés érvénytelen.
#[allow(clippy::too_many_arguments)]
pub fn create_report(
    db: &Db,
    reporter_id: i64,
    reporter_name: &str,
    reported_user_id: Option<i64>,
    reported_name: &str,
    kind: &Value,
    message: &Value,
    reason: &Value,
    room: &Room,
) -> Option<i64> {
    let kind = kind.as_str().filter(|k| REPORT_KINDS.contains(k))?;
    let reason = reason.as_str().map(|r| r.trim()).unwrap_or("");
    if reason.chars().count() > MAX_REPORT_REASON {
        return None;
    }
    let mut text = message.as_str().map(|m| m.trim().to_string()).unwrap_or_default();
    if kind == "chat" && (text.is_empty() || text.chars().count() > MAX_REPORT_MESSAGE) {
        return None;
    }
    if kind == "player" {
        text.clear();
    }
    let chat_len = room.chat_messages.len();
    let snapshot = json!({
        "players": room.game.players.iter().map(|p| json!({"name": p.name, "is_bot": p.is_bot, "score": p.score})).collect::<Vec<_>>(),
        "chat": &room.chat_messages[chat_len.saturating_sub(SNAPSHOT_CHAT_LINES)..],
        "turn": room.game.turn_number,
    });
    db.with(|tx| {
        tx.execute(
            "INSERT INTO reports (reporter_user_id, reporter_name, reported_user_id, reported_name, kind, message, \
             reason, room_id, room_name, snapshot_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![reporter_id, reporter_name, reported_user_id, reported_name, kind, text, reason, room.id, room.name, snapshot.to_string()],
        )?;
        Ok(tx.last_insert_rowid())
    })
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn banned_words_ignore_case_and_accents() {
        let w = words(&["rossz", "csunya"]);
        assert!(contains_banned(&w, "Te ROSSZ vagy"));
        assert!(contains_banned(&w, "csúnya"));
        assert!(!contains_banned(&w, "szép"));
        assert!(!contains_banned(&[], "rossz"));
    }

    #[test]
    fn masking_preserves_the_rest() {
        let w = words(&["rossz", "csunya"]);
        assert_eq!(mask_banned(&w, "Te ROSSZ és csúnya"), ("Te ***** és ******".to_string(), true));
        assert_eq!(mask_banned(&w, "minden rendben"), ("minden rendben".to_string(), false));
        assert_eq!(mask_banned(&words(&["ab", "bc"]), "xabcx").0, "x***x", "az átfedő találatok összeolvadnak");
    }

    #[test]
    fn chat_filter_modes() {
        let w = words(&["rossz"]);
        assert_eq!(filter_chat(&w, false, "rossz"), (Some("*****".to_string()), true));
        assert_eq!(filter_chat(&w, true, "rossz"), (None, true));
        assert_eq!(filter_chat(&w, true, "jó"), (Some("jó".to_string()), false));
    }
}

/// Push értesítés az adminoknak egy új bejelentésről (háttérben; push híján nem történik semmi).
pub fn notify_admins_of_report(app: &std::sync::Arc<crate::app::App>, room_name: &str) {
    let mut emails: Vec<&String> = app.config.admin_emails.iter().collect();
    emails.sort();
    for email in emails {
        if let Ok(Some(user)) = app.db.get_user_by_email(email) {
            app.push.notify_background(user.id, "report", room_name);
        }
    }
}
