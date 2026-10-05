//! Challenge (szavazás) állapotgép.
//!
//! Életciklus: létrehozás után minden nem-lerakó emberi játékos szavazhat (elfogad / elutasít); lezáráskor
//! 50% vagy több elfogadás → elfogadva, különben elutasítva.

use crate::board::{FormedWord, Placed};
use crate::player::Player;
use crate::tiles::Tile;
use crate::util;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Vote {
    Accept,
    Reject,
}

impl Vote {
    pub fn as_str(self) -> &'static str {
        match self {
            Vote::Accept => "accept",
            Vote::Reject => "reject",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Challenge {
    pub tiles_placed: Vec<Placed>,
    pub formed_words: Vec<FormedWord>,
    pub word_strs: Vec<String>,
    pub score: i32,
    pub player_idx: usize,
    pub removed_from_hand: Vec<Tile>,
    pub votes: HashMap<String, Vote>,
    pub expires_at: f64,
}

impl Challenge {
    pub fn new(
        tiles_placed: Vec<Placed>,
        formed_words: Vec<FormedWord>,
        word_strs: Vec<String>,
        score: i32,
        player_idx: usize,
        removed_from_hand: Vec<Tile>,
    ) -> Challenge {
        Challenge {
            tiles_placed,
            formed_words,
            word_strs,
            score,
            player_idx,
            removed_from_hand,
            votes: HashMap::new(),
            expires_at: util::now() + 30.0, // CHALLENGE_TIMEOUT
        }
    }

    pub fn add_vote(&mut self, player_id: &str, vote: Vote) {
        self.votes.insert(player_id.to_string(), vote);
    }

    /// Mindenki szavazott-e?
    pub fn all_voted(&self, voter_ids: &HashSet<String>) -> bool {
        voter_ids.iter().all(|id| self.votes.contains_key(id))
    }

    /// Szavazás kiértékelése. Visszatér: igaz, ha elfogadva ('vote_accepted'), hamis, ha elutasítva.
    pub fn resolve_votes(&self, voter_ids: &HashSet<String>) -> bool {
        let total = voter_ids.len();
        if total == 0 {
            return true;
        }
        let accept_count = voter_ids.iter().filter(|id| self.votes.get(*id) != Some(&Vote::Reject)).count();
        accept_count * 2 >= total
    }

    /// Játékos SID frissítése újracsatlakozáskor.
    pub fn update_player_sid(&mut self, old_id: &str, new_id: &str) {
        if let Some(vote) = self.votes.remove(old_id) {
            self.votes.insert(new_id.to_string(), vote);
        }
    }

    /// Szerializálás a kliensnek.
    pub fn to_state(&self, players: &[Player]) -> Value {
        let placer = &players[self.player_idx];
        let votes: serde_json::Map<String, Value> =
            self.votes.iter().map(|(k, v)| (k.clone(), Value::from(v.as_str()))).collect();
        json!({
            "player_id": placer.id,
            "player_name": placer.name,
            "words": self.word_strs,
            "score": self.score,
            "tiles": self.tiles_placed.iter().map(|p| p.to_json()).collect::<Vec<_>>(),
            "votes": votes,
            "player_count": players.len(),
            "expires_at": self.expires_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voters(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    fn challenge() -> Challenge {
        Challenge::new(vec![], vec![], vec!["ALMA".into()], 10, 0, vec![])
    }

    #[test]
    fn no_voters_accepts() {
        assert!(challenge().resolve_votes(&voters(&[])));
    }

    #[test]
    fn half_or_more_accepting_keeps_the_word() {
        let mut c = challenge();
        let v = voters(&["b", "c"]);
        c.add_vote("b", Vote::Accept);
        c.add_vote("c", Vote::Reject);
        assert!(c.resolve_votes(&v)); // 1 elfogad + 1 elutasít = 50% → elfogadva
        c.add_vote("b", Vote::Reject);
        assert!(!c.resolve_votes(&v));
    }

    #[test]
    fn missing_votes_count_as_accepts() {
        let c = challenge();
        assert!(c.resolve_votes(&voters(&["b", "c", "d"])));
        assert!(!c.all_voted(&voters(&["b"])));
    }

    #[test]
    fn single_voter_decides() {
        let mut c = challenge();
        let v = voters(&["b"]);
        c.add_vote("b", Vote::Reject);
        assert!(!c.resolve_votes(&v));
        assert!(c.all_voted(&v));
    }

    #[test]
    fn sid_change_moves_the_vote() {
        let mut c = challenge();
        c.add_vote("old", Vote::Reject);
        c.update_player_sid("old", "new");
        assert_eq!(c.votes.get("new"), Some(&Vote::Reject));
        assert!(!c.votes.contains_key("old"));
    }
}
