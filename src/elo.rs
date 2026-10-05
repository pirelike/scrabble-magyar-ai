//! Élő-értékszám (ELO) 2–4 játékos közötti játékokhoz.
//!
//! Minden játékospárra külön számolunk (a nagyobb pontszám nyer, az egyenlő döntetlen), a változást az
//! ellenfelek számával osztjuk, így a játékos teljes elmozdulása legfeljebb egy K lehet, akárhányan játszanak.
//! A számításhoz a játék előtti értékszámok kellenek.

use std::collections::HashMap;
use std::hash::Hash;

pub const INITIAL_RATING: i64 = 1200;
/// ennyi értékelt játékig nagyobb a K (gyorsabban áll be a valódi szint)
pub const PROVISIONAL_GAMES: i64 = 10;
pub const K_PROVISIONAL: f64 = 32.0;
pub const K_ESTABLISHED: f64 = 20.0;
/// A ranglistára csak ennyi értékelt játék után kerülhet fel valaki.
pub const MIN_RATED_GAMES_FOR_RANKING: i64 = 3;

/// A játékos várható eredménye (0–1) az ellenféllel szemben.
pub fn expected_score(rating: f64, opponent_rating: f64) -> f64 {
    1.0 / (1.0 + 10f64.powf((opponent_rating - rating) / 400.0))
}

pub fn k_factor(rated_games: i64) -> f64 {
    if rated_games < PROVISIONAL_GAMES { K_PROVISIONAL } else { K_ESTABLISHED }
}

/// Értékszám-változások egy játék végén.
///
/// `entries`: (kulcs, értékszám, értékelt játékok száma, pontszám) — csak a rangsorolt (regisztrált, emberi)
/// játékosok. Kevesebb mint két játékosnál nincs változás. Visszatér: kulcsonként az egész változás.
pub fn rating_changes<K: Eq + Hash + Clone>(entries: &[(K, i64, i64, i64)]) -> HashMap<K, i64> {
    if entries.len() < 2 {
        return entries.iter().map(|(key, ..)| (key.clone(), 0)).collect();
    }
    let opponents = (entries.len() - 1) as f64;
    let mut changes = HashMap::new();
    for (key, rating, rated_games, score) in entries {
        let mut total = 0.0;
        for (other_key, other_rating, _other_games, other_score) in entries {
            if other_key == key {
                continue;
            }
            let actual = if score > other_score {
                1.0
            } else if score == other_score {
                0.5
            } else {
                0.0
            };
            total += actual - expected_score(*rating as f64, *other_rating as f64);
        }
        // Python round(): a .5 a páros felé kerekít
        changes.insert(key.clone(), (k_factor(*rated_games) * total / opponents).round_ties_even() as i64);
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_players_split_evenly() {
        let changes = rating_changes(&[("a", 1200, 0, 100), ("b", 1200, 0, 50)]);
        assert_eq!(changes["a"], 16);
        assert_eq!(changes["b"], -16);
    }

    #[test]
    fn draw_between_equals_changes_nothing() {
        let changes = rating_changes(&[("a", 1200, 0, 80), ("b", 1200, 0, 80)]);
        assert_eq!(changes["a"], 0);
        assert_eq!(changes["b"], 0);
    }

    #[test]
    fn single_player_has_no_change() {
        let changes = rating_changes(&[("a", 1200, 0, 100)]);
        assert_eq!(changes["a"], 0);
    }

    #[test]
    fn established_players_move_less() {
        let provisional = rating_changes(&[("a", 1200, 0, 100), ("b", 1200, 0, 50)]);
        let established = rating_changes(&[("a", 1200, 20, 100), ("b", 1200, 20, 50)]);
        assert!(provisional["a"] > established["a"]);
        assert_eq!(established["a"], 10);
    }

    #[test]
    fn upset_gives_more_than_expected_win() {
        let upset = rating_changes(&[("weak", 1000, 20, 100), ("strong", 1400, 20, 50)]);
        let expected = rating_changes(&[("strong", 1400, 20, 100), ("weak", 1000, 20, 50)]);
        assert!(upset["weak"] > expected["strong"]);
    }

    #[test]
    fn four_players_total_move_is_bounded_by_k() {
        let entries: Vec<(i32, i64, i64, i64)> = vec![(1, 1200, 0, 300), (2, 1200, 0, 200), (3, 1200, 0, 100), (4, 1200, 0, 0)];
        let changes = rating_changes(&entries);
        assert!(changes[&1].abs() <= 32);
        assert_eq!(changes.values().sum::<i64>(), 0);
    }
}
