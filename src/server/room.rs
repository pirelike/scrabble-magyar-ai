//! Egy szoba állapota: játék, tulajdonos, beállítások, chat, időzítő-azonosítók.

use crate::db::Row;
use crate::game::Game;
use crate::util;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

pub const MAX_CHAT_MESSAGES: usize = 100;
pub const MAX_SPECTATORS: usize = 30;

/// Egy hely gazdája a mentett játékban: ismeretlen (régi mentés), vendég, vagy regisztrált felhasználó.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Identity {
    /// a mentésből nem ismert a hely gazdája (régi mentés): név alapján enged
    Any,
    Guest,
    User(i64),
}

impl Identity {
    pub fn from_user_id(user_id: Option<i64>) -> Identity {
        match user_id {
            Some(id) if id != 0 => Identity::User(id),
            _ => Identity::Guest,
        }
    }
}

/// A chat üzenet kiegészítő adatai (a `chat_messages` elemei változatlanok).
#[derive(Clone, Debug)]
pub struct ChatMeta {
    pub ts: f64,
    pub user_id: Option<i64>,
}

pub struct Room {
    pub id: String,
    pub game: Game,
    /// a tulajdonos kapcsolata (SID); levelezős játékban nincs
    pub owner: Option<String>,
    pub owner_token: Option<String>,
    pub owner_name: String,
    pub name: String,
    pub max_players: usize,
    pub join_code: String,
    pub is_private: bool,
    pub is_restored: bool,
    /// napi feladvány (egyjátékos, nem mentődik)
    pub is_puzzle: bool,
    /// melyik körhöz küldtünk már push értesítést
    pub pushed_turn: i64,
    /// levelezős játék: tartós, a játékosok bármikor visszatérhetnek
    pub is_async: bool,
    /// levelezős játék: melyik körről értesítettük a lobbyban lévő játékost
    pub notified_turn: i64,
    /// levelezős játék: a legutóbb mentett állapot ujjlenyomata
    pub persisted_sig: Option<(usize, bool, i64)>,
    pub chat_messages: Vec<Value>,
    pub chat_meta: Vec<ChatMeta>,
    pub created_at: f64,
    /// az utolsó állapotküldés ideje (elakadás-jelzéshez)
    pub last_activity: f64,
    /// a legutóbbi robotlépés-ütemezés ideje, ha van függő
    pub bot_scheduled_at: Option<f64>,
    /// szüneteltetett körtimer hátralévő ideje (mp)
    pub timer_paused_left: Option<f64>,
    /// az élő szobát figyelő admin kapcsolatok (SID)
    pub admin_watchers: HashSet<String>,
    challenge_timer_id: u64,
    turn_timer_id: u64,
    bot_turn_id: u64,
    /// megfigyelők: {sid: név}
    pub spectators: HashMap<String, String>,
    pub turn_timer_expires_at: Option<f64>,
    /// a szavazásra váró lerakáskor hátralévő köridő (visszavonáshoz)
    pub withdraw_time_left: Option<f64>,
    pub db_game_id: Option<i64>,
    pub last_saved_move_count: usize,
    pub manually_saved: bool,
    /// a végeredmény (statisztika) már rögzítve van
    pub result_saved: bool,
    /// Visszaállított játék: név -> user_id (None = vendég) a mentésből
    pub known_user_ids: HashMap<String, Option<i64>>,
    /// Visszaállított játékból hiányzó játékosok, akik menet közben még csatlakozhatnak
    pub late_join_names: HashMap<String, Identity>,
    /// a visszaállítandó mentés (várakozó szoba)
    pub restore_save_data: Option<Row>,
    pub expected_players: Option<Vec<String>>,
}

impl Room {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: &str,
        game: Game,
        owner_sid: Option<&str>,
        owner_name: &str,
        name: &str,
        max_players: usize,
        join_code: &str,
        is_private: bool,
        owner_token: Option<&str>,
    ) -> Room {
        let now = util::now();
        Room {
            id: id.to_string(),
            game,
            owner: owner_sid.map(|s| s.to_string()),
            owner_token: owner_token.map(|s| s.to_string()),
            owner_name: owner_name.to_string(),
            name: name.to_string(),
            max_players,
            join_code: join_code.to_string(),
            is_private,
            is_restored: false,
            is_puzzle: false,
            pushed_turn: -1,
            is_async: false,
            notified_turn: -1,
            persisted_sig: None,
            chat_messages: Vec::new(),
            chat_meta: Vec::new(),
            created_at: now,
            last_activity: now,
            bot_scheduled_at: None,
            timer_paused_left: None,
            admin_watchers: HashSet::new(),
            challenge_timer_id: 0,
            turn_timer_id: 0,
            bot_turn_id: 0,
            spectators: HashMap::new(),
            turn_timer_expires_at: None,
            withdraw_time_left: None,
            db_game_id: None,
            last_saved_move_count: 0,
            manually_saved: false,
            result_saved: false,
            known_user_ids: HashMap::new(),
            late_join_names: HashMap::new(),
            restore_save_data: None,
            expected_players: None,
        }
    }

    /// Chat üzenet hozzáadása (max 100 darab). `system`: rendszerüzenet (kiemelten jelenik meg).
    pub fn add_chat_message(&mut self, name: &str, message: &str, user_id: Option<i64>, system: bool) {
        let mut entry = json!({"name": name, "message": message});
        if system {
            entry["system"] = json!(true);
        }
        self.chat_messages.push(entry);
        self.chat_meta.push(ChatMeta { ts: util::now(), user_id });
        if self.chat_messages.len() > MAX_CHAT_MESSAGES {
            let excess = self.chat_messages.len() - MAX_CHAT_MESSAGES;
            self.chat_messages.drain(..excess);
            self.chat_meta.drain(..excess);
        }
    }

    /// Az utolsó tevékenység ideje (állapotküldéskor frissül).
    pub fn touch(&mut self) {
        self.last_activity = util::now();
    }

    /// Érvényteleníti az aktuális challenge timert. Visszaadja az új timer id-t.
    pub fn invalidate_challenge_timer(&mut self) -> u64 {
        self.challenge_timer_id += 1;
        self.challenge_timer_id
    }

    pub fn challenge_timer_id(&self) -> u64 {
        self.challenge_timer_id
    }

    /// Érvényteleníti az aktuális kör timert. Visszaadja az új timer id-t.
    pub fn invalidate_turn_timer(&mut self) -> u64 {
        self.turn_timer_id += 1;
        self.turn_timer_expires_at = None;
        self.turn_timer_id
    }

    pub fn turn_timer_id(&self) -> u64 {
        self.turn_timer_id
    }

    /// Érvényteleníti a függőben lévő robotlépést. Visszaadja az új azonosítót.
    pub fn invalidate_bot_turn(&mut self) -> u64 {
        self.bot_turn_id += 1;
        self.bot_turn_id
    }

    pub fn bot_turn_id(&self) -> u64 {
        self.bot_turn_id
    }

    /// Tulajdonjog átadása.
    pub fn transfer_ownership(&mut self, new_owner_sid: &str, new_owner_name: &str, new_owner_token: Option<&str>) {
        self.owner = Some(new_owner_sid.to_string());
        self.owner_name = new_owner_name.to_string();
        self.owner_token = new_owner_token.map(|t| t.to_string());
    }

    /// Megjelenik-e a szoba a nyilvános lobby listában.
    pub fn is_lobby_visible(&self) -> bool {
        !self.is_private && !self.is_restored && !self.game.finished && !self.game.started
    }

    /// Megfigyelhető-e a lobby "Élő játékok" listájából (nyilvános, folyamatban lévő játék).
    pub fn is_live_visible(&self) -> bool {
        !self.is_private && self.game.started && !self.game.finished
    }

    /// Az "Élő játékok" listához szükséges adatok.
    pub fn to_live_dict(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "players": self.game.players.iter().map(|p| json!({"name": p.name, "score": p.score, "is_bot": p.is_bot})).collect::<Vec<_>>(),
            "challenge_mode": self.game.challenge_mode,
            "turn_time_limit": self.game.turn_time_limit,
            "spectators": self.spectators.len(),
            "turn_number": self.game.turn_number,
        })
    }

    /// Lobby listához szükséges adatok (publikus szobákhoz).
    pub fn to_lobby_dict(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "players": self.game.players.len(),
            "max_players": self.max_players,
            "started": self.game.started,
            "finished": self.game.finished,
            "owner": self.owner_name,
            "challenge_mode": self.game.challenge_mode,
            "turn_time_limit": self.game.turn_time_limit,
            "is_restored": self.is_restored,
            "bots": self.game.players.iter().filter(|p| p.is_bot).count(),
        })
    }
}

/// 6 számjegyű egyedi csatlakozási kód generálása.
pub fn generate_join_code(existing_codes: &HashMap<String, String>) -> String {
    loop {
        let code = format!("{:06}", util::random_below(1_000_000));
        if !existing_codes.contains_key(&code) {
            return code;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room() -> Room {
        Room::new("abc", Game::with_defaults("abc"), Some("sid1"), "Alice", "Szoba", 4, "123456", false, Some("tok"))
    }

    #[test]
    fn chat_is_capped_at_100() {
        let mut room = room();
        for i in 0..130 {
            room.add_chat_message("A", &format!("üzenet {i}"), Some(1), false);
        }
        assert_eq!(room.chat_messages.len(), 100);
        assert_eq!(room.chat_meta.len(), 100);
        assert_eq!(room.chat_messages[0]["message"], "üzenet 30");
        room.add_chat_message("Rendszer", "x", None, true);
        assert_eq!(room.chat_messages.last().unwrap()["system"], true);
    }

    #[test]
    fn timer_ids_invalidate() {
        let mut room = room();
        let a = room.invalidate_turn_timer();
        room.turn_timer_expires_at = Some(5.0);
        let b = room.invalidate_turn_timer();
        assert!(b > a);
        assert!(room.turn_timer_expires_at.is_none());
        let c1 = room.invalidate_challenge_timer();
        assert_eq!(room.challenge_timer_id(), c1);
        let bot = room.invalidate_bot_turn();
        assert_eq!(room.bot_turn_id(), bot);
    }

    #[test]
    fn visibility_rules() {
        let mut room = room();
        assert!(room.is_lobby_visible());
        assert!(!room.is_live_visible());
        room.is_private = true;
        assert!(!room.is_lobby_visible());
        room.is_private = false;
        room.game.add_player("sid1", "Alice").unwrap();
        room.game.start().unwrap();
        assert!(!room.is_lobby_visible());
        assert!(room.is_live_visible());
        room.game.finished = true;
        assert!(!room.is_live_visible());
        room.is_restored = true;
        room.game.finished = false;
        room.game.started = false;
        assert!(!room.is_lobby_visible());
    }

    #[test]
    fn join_codes_are_unique_six_digits() {
        let mut existing = HashMap::new();
        for _ in 0..200 {
            let code = generate_join_code(&existing);
            assert_eq!(code.len(), 6);
            assert!(code.chars().all(|c| c.is_ascii_digit()));
            assert!(!existing.contains_key(&code));
            existing.insert(code, "room".to_string());
        }
    }

    #[test]
    fn lobby_and_live_dicts() {
        let mut room = room();
        room.game.add_player("sid1", "Alice").unwrap();
        room.game.add_bot("Robi", &json!(3)).unwrap();
        let lobby = room.to_lobby_dict();
        assert_eq!(lobby["players"], 2);
        assert_eq!(lobby["bots"], 1);
        assert_eq!(lobby["owner"], "Alice");
        room.spectators.insert("s".into(), "N".into());
        assert_eq!(room.to_live_dict()["spectators"], 1);
        assert_eq!(room.to_live_dict()["players"][1]["is_bot"], true);
    }
}
