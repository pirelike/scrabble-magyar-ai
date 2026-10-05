//! Levelezős (aszinkron) játékmód: a játékosok órák vagy napok alatt lépnek.
//!
//! A játék egy kész, elindított `Game`, amelynek minden résztvevője (a létrehozón kívül) lecsatlakozottként várja
//! az első belépését; a lecsatlakozottat nem ugorjuk át, a soron lévőnek `turn_hours` órája van, utána
//! automatikus passz (három egymás utáni lejárt határidő után feladja). A játék tartós otthona az adatbázis:
//! minden lépés után mentődik, és a szerver újraindulásakor a szobák visszaépülnek.

use crate::db::Row;
use crate::game::Game;
use rand::{Rng, RngExt};
use serde_json::{Value, json};
use std::collections::HashMap;

pub const ALLOWED_TURN_HOURS: [i64; 4] = [24, 48, 72, 168];
pub const DEFAULT_TURN_HOURS: i64 = 48;
/// a létrehozón kívül legfeljebb ennyi játékos
pub const MAX_FRIENDS: usize = 3;

/// A névből a már foglaltaktól (kis-nagybetűtől függetlenül) különböző név: Anna, Anna (2)...
pub fn unique_name<'a>(name: &str, taken: impl IntoIterator<Item = &'a String>) -> String {
    let taken: Vec<String> = taken.into_iter().map(|t| t.to_lowercase()).collect();
    if !taken.contains(&name.to_lowercase()) {
        return name.to_string();
    }
    let mut n = 2;
    while taken.contains(&format!("{name} ({n})").to_lowercase()) {
        n += 1;
    }
    format!("{name} ({n})")
}

/// A még be nem lépett játékos helyettesítő azonosítója (belépéskor a SID váltja fel).
pub fn placeholder_id(room_id: &str, user_id: i64) -> String {
    format!("async-{room_id}-{user_id}")
}

/// A levelezős játék: a létrehozó (kapcsolódva) és a meghívott barátok (lecsatlakozottként).
///
/// `friends`: (user_id, megjelenítési név). Visszatér: (játék, {játékosnév: user_id}).
pub fn build_game<R: Rng + ?Sized>(
    room_id: &str,
    creator_sid: &str,
    creator_name: &str,
    creator_user_id: i64,
    friends: &[(i64, String)],
    turn_hours: i64,
    rng: &mut R,
) -> (Game, HashMap<String, i64>) {
    let mut game = Game::new(room_id, false, 0, 0);
    game.async_mode = true;
    game.turn_hours = turn_hours;
    let mut known: HashMap<String, i64> = HashMap::new();
    let _ = game.add_player(creator_sid, creator_name);
    known.insert(creator_name.to_string(), creator_user_id);
    for (id, friend_name) in friends {
        let name = unique_name(friend_name, known.keys());
        let _ = game.add_player(&placeholder_id(room_id, *id), &name);
        if let Some(last) = game.players.last_mut() {
            last.disconnected = true;
        }
        known.insert(name, *id);
    }
    let _ = game.start();
    game.current_player_idx = rng.random_range(0..game.players.len()); // véletlen kezdő játékos
    game.reset_deadline();
    (game, known)
}

/// Egy `get_user_async_games` sor a lobby listájához (a mentett állapotból).
pub fn summarize(row: &Row) -> Option<Value> {
    use crate::db::RowExt;
    let state: Value = serde_json::from_str(&row.text("state_json")).ok()?;
    let players = state.get("players").and_then(|p| p.as_array()).cloned().unwrap_or_default();
    let idx = state.get("current_player_idx").and_then(|i| i.as_i64()).unwrap_or(0);
    let current = if idx >= 0 { players.get(idx as usize).and_then(|p| p.get("name")).and_then(|n| n.as_str()) } else { None };
    let my_name = row.text("my_name");
    let finished = state.get("finished").is_some_and(crate::util::truthy);
    Some(json!({
        "game_id": row.int("game_id"),
        "room_name": row.text("room_name"),
        "my_name": my_name,
        "players": players.iter().map(|p| json!({"name": p["name"], "score": p.get("score").cloned().unwrap_or(json!(0))})).collect::<Vec<_>>(),
        "current_player": current,
        "my_turn": current == Some(my_name.as_str()) && !finished,
        "turn_deadline": state.get("turn_deadline").cloned().unwrap_or(Value::Null),
        "turn_hours": state.get("turn_hours").cloned().unwrap_or(json!(0)),
        "updated_at": row.text("updated_at"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_names_get_a_counter() {
        let taken = ["Anna".to_string(), "Anna (2)".to_string()];
        assert_eq!(unique_name("Béla", taken.iter()), "Béla");
        assert_eq!(unique_name("anna", taken.iter()), "anna (3)");
        assert_eq!(unique_name("Anna", Vec::<String>::new().iter()), "Anna");
    }

    #[test]
    fn built_game_has_creator_online_and_friends_away() {
        let mut rng = rand::rng();
        let friends = vec![(2, "Béla".to_string()), (3, "Anna".to_string())];
        let (game, known) = build_game("room1", "sid1", "Anna", 1, &friends, 48, &mut rng);
        assert!(game.started && game.async_mode);
        assert_eq!(game.players.len(), 3);
        assert!(!game.players[0].disconnected);
        assert!(game.players[1].disconnected && game.players[2].disconnected);
        assert_eq!(game.players[1].id, "async-room1-2");
        assert_eq!(game.players[2].name, "Anna (2)");
        assert_eq!(known["Anna (2)"], 3);
        assert_eq!(game.hint_limit, 0, "levelezős játékban nincs tipp");
        assert!(game.turn_deadline.is_some());
        assert!(game.players.iter().all(|p| p.hand.len() == 7));
    }

    #[test]
    fn summary_marks_my_turn() {
        use crate::db::Row;
        let mut row = Row::new();
        row.insert("game_id".into(), json!(5));
        row.insert("room_name".into(), json!("Szoba"));
        row.insert("my_name".into(), json!("Béla"));
        row.insert("updated_at".into(), json!("2026-10-05 10:00:00"));
        row.insert(
            "state_json".into(),
            json!(json!({"players": [{"name": "Anna", "score": 3}, {"name": "Béla", "score": 5}], "current_player_idx": 1, "finished": false, "turn_deadline": 5.0, "turn_hours": 48}).to_string()),
        );
        let summary = summarize(&row).unwrap();
        assert_eq!(summary["my_turn"], true);
        assert_eq!(summary["current_player"], "Béla");
        assert_eq!(summary["players"][1]["score"], 5);
        row.insert("my_name".into(), json!("Anna"));
        assert_eq!(summarize(&row).unwrap()["my_turn"], false);
        row.insert("state_json".into(), json!("nem json"));
        assert!(summarize(&row).is_none());
    }
}
