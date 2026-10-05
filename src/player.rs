//! Egy játékos állapota.

use crate::ai::{Difficulty, normalize_difficulty};
use crate::tiles::Tile;
use serde_json::{Value, json};

#[derive(Clone, Debug)]
pub struct Player {
    pub id: String,
    pub name: String,
    pub hand: Vec<Tile>,
    pub score: i32,
    pub skip_next_turn: bool,
    pub disconnected: bool,
    /// levelezős játék: egymás utáni lejárt határidők
    pub timeouts: u32,
    pub resigned: bool,
    pub is_bot: bool,
    /// A robot fokozata (1–10) vagy "igazodik hozzám"; embernél None.
    pub difficulty: Option<Difficulty>,
}

impl Player {
    pub fn new(id: &str, name: &str) -> Player {
        Player {
            id: id.to_string(),
            name: name.to_string(),
            hand: Vec::new(),
            score: 0,
            skip_next_turn: false,
            disconnected: false,
            timeouts: 0,
            resigned: false,
            is_bot: false,
            difficulty: None,
        }
    }

    pub fn new_bot(id: &str, name: &str, difficulty: &Value) -> Player {
        let mut player = Player::new(id, name);
        player.is_bot = true;
        player.difficulty = Some(normalize_difficulty(difficulty));
        player
    }

    pub fn hand_json(&self) -> Value {
        Value::Array(self.hand.iter().map(|t| Value::from(t.as_str())).collect())
    }

    pub fn to_json(&self, reveal_hand: bool) -> Value {
        let mut data = json!({
            "id": self.id,
            "name": self.name,
            "score": self.score,
            "hand_count": self.hand.len(),
            "skip_next_turn": self.skip_next_turn,
            "disconnected": self.disconnected,
            "resigned": self.resigned,
            "is_bot": self.is_bot,
            "difficulty": self.difficulty.map(|d| d.to_json()).unwrap_or(Value::Null),
        });
        if reveal_hand {
            data["hand"] = self.hand_json();
        }
        data
    }
}

/// Zsetonok JSON-ban ('' = üres zseton).
pub fn tiles_to_json(tiles: &[Tile]) -> Value {
    Value::Array(tiles.iter().map(|t| Value::from(t.as_str())).collect())
}

/// Zsetonok JSON-ból (ismeretlen betűket kihagy).
pub fn tiles_from_json(value: &Value) -> Vec<Tile> {
    value
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().and_then(Tile::from_str)).collect())
        .unwrap_or_default()
}
