//! Kitüntetések (érmék): a befejezett játékokból számolt, tartósan megszerzett jelvények.
//!
//! A játékonkénti jelvényeket a lépésnaplóból és a végeredményből számoljuk (`evaluate_game`), az összesítőket
//! (első játék, győzelmek száma...) a felhasználó statisztikájából (`cumulative_badges`). A megszerzést a
//! `Db::grant_achievements` rögzíti (egyszer, kulcsonként).

use crate::game::Game;
use crate::tiles::tokenize_word;
use std::collections::{HashMap, HashSet};

pub const HAND_SIZE: usize = 7;

/// A megjelenítés sorrendje is ez; a nevek / leírások a kliens i18n `badge.<kulcs>` kulcsaiban vannak.
pub const BADGES: [&str; 11] =
    ["first_game", "first_win", "bingo", "score_100", "long_word", "joker_play", "game_300", "bot_slayer", "wins_10", "games_25", "daily_best"];

pub const BIG_MOVE_SCORE: i64 = 100;
pub const LONG_WORD_TILES: usize = 8;
pub const HIGH_GAME_SCORE: i32 = 300;
pub const STRONG_BOT_LEVEL: u8 = 8;
pub const WINS_FOR_VETERAN: i64 = 10;
pub const GAMES_FOR_REGULAR: i64 = 25;

/// Játékonkénti jelvények. `name_to_user`: {játékosnév: user_id} (regisztrált emberek).
/// Visszatér: {user_id: {jelvénykulcs, ...}}
pub fn evaluate_game(game: &Game, name_to_user: &HashMap<String, i64>) -> HashMap<i64, HashSet<&'static str>> {
    let mut earned: HashMap<i64, HashSet<&'static str>> = name_to_user.values().filter(|u| **u != 0).map(|u| (*u, HashSet::new())).collect();

    for mv in &game.move_log {
        let Some(uid) = name_to_user.get(&mv.player_name).filter(|u| **u != 0) else { continue };
        if mv.action_type != "place" && mv.action_type != "challenge_accept" {
            continue;
        }
        let badges = earned.entry(*uid).or_default();
        let tiles = mv.details.get("tiles").and_then(|t| t.as_array()).cloned().unwrap_or_default();
        if tiles.len() >= HAND_SIZE {
            badges.insert("bingo");
        }
        if mv.score() >= BIG_MOVE_SCORE {
            badges.insert("score_100");
        }
        if tiles.iter().any(|t| t.get("is_blank").is_some_and(crate::util::truthy)) {
            badges.insert("joker_play");
        }
        let words = mv.details.get("words").and_then(|w| w.as_array()).cloned().unwrap_or_default();
        if words.iter().any(|w| w.as_str().and_then(tokenize_word).is_some_and(|t| t.len() >= LONG_WORD_TILES)) {
            badges.insert("long_word");
        }
    }

    // Az "igazodik hozzám" robot ('auto') nem számít erősnek: az ember szintjére áll be
    let strong_bot = game.players.iter().any(|p| p.is_bot && p.difficulty.and_then(|d| d.level()).is_some_and(|l| l >= STRONG_BOT_LEVEL));
    let winners: HashSet<&str> = game.winners.iter().map(|p| p.name.as_str()).collect();
    for player in &game.players {
        let Some(uid) = name_to_user.get(&player.name).filter(|u| **u != 0) else { continue };
        if player.is_bot {
            continue;
        }
        let badges = earned.entry(*uid).or_default();
        if player.score >= HIGH_GAME_SCORE {
            badges.insert("game_300");
        }
        if strong_bot && winners.contains(player.name.as_str()) {
            badges.insert("bot_slayer");
        }
    }
    earned
}

/// A felhasználó összesített statisztikájából járó jelvények.
pub fn cumulative_badges(games_played: i64, games_won: i64) -> HashSet<&'static str> {
    let mut badges = HashSet::new();
    if games_played >= 1 {
        badges.insert("first_game");
    }
    if games_won >= 1 {
        badges.insert("first_win");
    }
    if games_won >= WINS_FOR_VETERAN {
        badges.insert("wins_10");
    }
    if games_played >= GAMES_FOR_REGULAR {
        badges.insert("games_25");
    }
    badges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::Difficulty;
    use crate::board::Board;
    use crate::game::MoveLog;
    use crate::player::Player;
    use serde_json::json;

    fn mv(name: &str, kind: &str, details: serde_json::Value) -> MoveLog {
        MoveLog { move_number: 1, player_name: name.into(), action_type: kind.into(), details, board: Board::new() }
    }

    fn game_with(moves: Vec<MoveLog>) -> Game {
        let mut game = Game::with_defaults("g");
        game.add_player("a", "Alice").unwrap();
        game.add_player("b", "Bob").unwrap();
        game.move_log = moves;
        game
    }

    fn users() -> HashMap<String, i64> {
        HashMap::from([("Alice".to_string(), 1), ("Bob".to_string(), 2)])
    }

    #[test]
    fn move_based_badges() {
        let tiles7: Vec<_> = (0..7).map(|i| json!({"row": 7, "col": i, "letter": "A", "is_blank": false})).collect();
        let game = game_with(vec![
            mv("Alice", "place", json!({"tiles": tiles7, "words": ["ALMA"], "score": 120})),
            mv("Bob", "place", json!({"tiles": [{"row": 7, "col": 7, "letter": "A", "is_blank": true}], "words": ["KÉSZSÉGES"], "score": 12})),
            mv("Bob", "pass", json!({"score": 0})),
        ]);
        let earned = evaluate_game(&game, &users());
        assert_eq!(earned[&1], HashSet::from(["bingo", "score_100"]));
        assert!(earned[&2].contains("joker_play") && earned[&2].contains("long_word"));
        assert!(!earned[&2].contains("bingo"));
    }

    #[test]
    fn game_based_badges() {
        let mut game = game_with(vec![]);
        game.players[0].score = 310;
        game.players.push(Player::new_bot("bot-1", "Robi", &json!(9)));
        game.winners = vec![game.players[0].clone()];
        let earned = evaluate_game(&game, &users());
        assert!(earned[&1].contains("game_300") && earned[&1].contains("bot_slayer"));
        assert!(earned[&2].is_empty());
    }

    #[test]
    fn adaptive_bot_is_not_strong() {
        let mut game = game_with(vec![]);
        let mut bot = Player::new_bot("bot-1", "Robi", &json!("auto"));
        assert_eq!(bot.difficulty, Some(Difficulty::Auto));
        bot.score = 1;
        game.players.push(bot);
        game.winners = vec![game.players[0].clone()];
        assert!(!evaluate_game(&game, &users())[&1].contains("bot_slayer"));
    }

    #[test]
    fn cumulative() {
        assert!(cumulative_badges(0, 0).is_empty());
        assert_eq!(cumulative_badges(1, 0), HashSet::from(["first_game"]));
        assert_eq!(cumulative_badges(25, 10), HashSet::from(["first_game", "first_win", "wins_10", "games_25"]));
    }
}
