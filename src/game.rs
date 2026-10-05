//! Scrabble játékállapot és logika: körök, pontozás, játék vége, szavazás, robotok, mentés.

use crate::ai::{self, Difficulty};
use crate::board::{BOARD_SIZE, Board, FormedWord, Placed};
use crate::challenge::{Challenge, Vote};
use crate::player::{Player, tiles_from_json, tiles_to_json};
use crate::tiles::{Tile, TileBag};
use crate::util;
use serde_json::{Map, Value, json};
use std::collections::HashSet;

pub const HAND_SIZE: usize = 7;
pub const BONUS_ALL_TILES: i32 = 50;
pub const CHALLENGE_TIMEOUT: u64 = 30; // másodperc
/// Ennyi egymást követő pont nélküli kör (passz, csere, elutasított lerakás) után véget ér a játék.
pub const SCORELESS_TURNS_LIMIT: u32 = 6;
/// Levelezős játék: ennyi egymás utáni lejárt határidő után a játékos feladja a játékot.
pub const MAX_TIMEOUTS: u32 = 3;
/// Tippek száma játékonként (0 = kikapcsolva); a szoba létrehozásakor választható.
pub const ALLOWED_HINT_LIMITS: [i64; 5] = [0, 1, 3, 5, 10];
pub const DEFAULT_HINT_LIMIT: i64 = 3;

/// A függő lerakás szavazásának eredménye.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum VoteResult {
    Recorded,
    Accepted,
    Rejected,
}

impl VoteResult {
    /// A Python-oldali név (`vote_recorded` / `vote_accepted` / `vote_rejected`).
    pub fn as_str(self) -> &'static str {
        match self {
            VoteResult::Recorded => "vote_recorded",
            VoteResult::Accepted => "vote_accepted",
            VoteResult::Rejected => "vote_rejected",
        }
    }
}

/// A lépésnapló egy bejegyzése. A tábla-pillanatkép tömören (nem JSON szövegként) él a memóriában.
#[derive(Clone, Debug)]
pub struct MoveLog {
    pub move_number: i64,
    pub player_name: String,
    pub action_type: String,
    pub details: Value,
    pub board: Board,
}

impl MoveLog {
    pub fn details_json(&self) -> String {
        serde_json::to_string(&self.details).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn board_snapshot_json(&self) -> String {
        serde_json::to_string(&self.board.to_json()).unwrap_or_else(|_| "[]".to_string())
    }

    /// A mentett (adatbázisból jövő) lépésből.
    pub fn from_stored(move_number: i64, player_name: &str, action_type: &str, details_json: Option<&str>, snapshot_json: Option<&str>) -> MoveLog {
        let details = details_json.and_then(|t| serde_json::from_str::<Value>(t).ok()).filter(|v| v.is_object()).unwrap_or_else(|| json!({}));
        let board = snapshot_json
            .and_then(|t| serde_json::from_str::<Value>(t).ok())
            .map(|v| Board::from_json(&v))
            .unwrap_or_default();
        MoveLog { move_number, player_name: player_name.to_string(), action_type: action_type.to_string(), details, board }
    }

    pub fn score(&self) -> i64 {
        self.details.get("score").and_then(|v| v.as_i64()).unwrap_or(0)
    }
}

/// Napi feladvány: egy játékos, egyetlen lerakás, utána vége a játéknak.
#[derive(Clone, Debug)]
pub struct Puzzle {
    pub date: String,
    pub score: Option<i32>,
}

#[derive(Clone, Debug)]
pub struct Game {
    pub id: String,
    pub players: Vec<Player>,
    pub board: Board,
    pub bag: TileBag,
    pub current_player_idx: usize,
    pub started: bool,
    pub finished: bool,
    /// döntetlennél több is lehet (a játékos pillanatképe a játék végén)
    pub winners: Vec<Player>,
    pub puzzle: Option<Puzzle>,
    pub async_mode: bool,
    pub turn_hours: i64,
    pub turn_deadline: Option<f64>,
    pub scoreless_turns: u32,
    pub turn_number: i64,
    pub last_action: Option<String>,
    pub challenge_mode: bool,
    pub turn_time_limit: i64,
    pub hint_limit: i64,
    pub hints_used: i64,
    pub pending_challenge: Option<Challenge>,
    pub move_log: Vec<MoveLog>,
    pub last_action_info: Option<Value>,
    /// A szavazással elutasított lerakások az aktuális táblaállásnál: a robot nem rakja le újra.
    pub rejected_placements: HashSet<Vec<Placed>>,
    bot_seq: u32,
}

/// A lerakás kanonikus (rendezett) alakja a `rejected_placements` halmazhoz.
pub fn placement_key(tiles: &[Placed]) -> Vec<Placed> {
    let mut key = tiles.to_vec();
    key.sort();
    key
}

pub type Fallible<T> = Result<T, String>;

impl Game {
    pub fn new(game_id: &str, challenge_mode: bool, turn_time_limit: i64, hint_limit: i64) -> Game {
        Game {
            id: game_id.to_string(),
            players: Vec::new(),
            board: Board::new(),
            bag: TileBag::new(),
            current_player_idx: 0,
            started: false,
            finished: false,
            winners: Vec::new(),
            puzzle: None,
            async_mode: false,
            turn_hours: 0,
            turn_deadline: None,
            scoreless_turns: 0,
            turn_number: 0,
            last_action: None,
            challenge_mode,
            turn_time_limit,
            hint_limit: if ALLOWED_HINT_LIMITS.contains(&hint_limit) { hint_limit } else { DEFAULT_HINT_LIMIT },
            hints_used: 0,
            pending_challenge: None,
            move_log: Vec::new(),
            last_action_info: None,
            rejected_placements: HashSet::new(),
            bot_seq: 0,
        }
    }

    pub fn with_defaults(game_id: &str) -> Game {
        Game::new(game_id, false, 0, DEFAULT_HINT_LIMIT)
    }

    pub fn add_player(&mut self, player_id: &str, name: &str) -> Fallible<&'static str> {
        if self.players.iter().any(|p| p.id == player_id) {
            return Err("Ez a játékos már csatlakozott.".to_string());
        }
        if self.players.len() >= 4 {
            return Err("Maximum 4 játékos lehet.".to_string());
        }
        if self.started {
            return Err("A játék már elkezdődött.".to_string());
        }
        self.players.push(Player::new(player_id, name));
        Ok("Csatlakozás sikeres.")
    }

    /// Számítógépes ellenfelet ad a játékhoz (csak indítás előtt).
    pub fn add_bot(&mut self, name: &str, difficulty: &Value) -> Fallible<&'static str> {
        if self.players.len() >= 4 {
            return Err("Maximum 4 játékos lehet.".to_string());
        }
        if self.started {
            return Err("A játék már elkezdődött.".to_string());
        }
        self.bot_seq += 1;
        let mut bot_id = format!("bot-{}-{}", self.id, self.bot_seq);
        while self.players.iter().any(|p| p.id == bot_id) {
            self.bot_seq += 1;
            bot_id = format!("bot-{}-{}", self.id, self.bot_seq);
        }
        self.players.push(Player::new_bot(&bot_id, name, difficulty));
        Ok("Robot hozzáadva.")
    }

    /// Az egyetlen győztes; döntetlennél (vagy játék közben) None.
    pub fn winner(&self) -> Option<&Player> {
        if self.winners.len() == 1 { self.winners.first() } else { None }
    }

    pub fn hints_left(&self) -> i64 {
        (self.hint_limit - self.hints_used).max(0)
    }

    /// Elhasznál egy tippet. Visszatér: igaz, ha volt még elérhető.
    pub fn use_hint(&mut self) -> bool {
        if self.hints_left() <= 0 {
            return false;
        }
        self.hints_used += 1;
        true
    }

    pub fn human_players(&self) -> Vec<&Player> {
        self.players.iter().filter(|p| !p.is_bot).collect()
    }

    /// A megadott nevű játékosok utolsó `window` körének átlagos pontja (a passz / csere 0 pont).
    ///
    /// Több játékosnál az egyéni átlagok átlaga. Visszatér: (átlag vagy None, a legtöbb kört lépett játékos
    /// körei a figyelembe vett ablakban).
    pub fn recent_stats(&self, names: &[String], window: usize) -> (Option<f64>, usize) {
        let wanted: HashSet<&String> = names.iter().collect();
        let mut scores: std::collections::HashMap<&str, Vec<f64>> = wanted.iter().map(|n| (n.as_str(), Vec::new())).collect();
        for entry in &self.move_log {
            let Some(list) = scores.get_mut(entry.player_name.as_str()) else { continue };
            match entry.action_type.as_str() {
                "place" | "challenge_accept" => list.push(entry.score() as f64),
                "exchange" | "pass" => list.push(0.0),
                _ => {}
            }
        }
        let mut averages = Vec::new();
        let mut turns = 0;
        for history in scores.values() {
            let start = history.len().saturating_sub(window);
            let recent = &history[start..];
            if !recent.is_empty() {
                averages.push(recent.iter().sum::<f64>() / recent.len() as f64);
                turns = turns.max(recent.len());
            }
        }
        let average = if averages.is_empty() { None } else { Some(averages.iter().sum::<f64>() / averages.len() as f64) };
        (average, turns)
    }

    /// A robot ebben a körben használt fokozata (1–10). Az "igazodik hozzám" robot az emberi játékosok (vagy a
    /// `reference_names` játékosok) utolsó köreinek átlagához állítja az erejét.
    pub fn bot_level<R: rand::Rng + ?Sized>(&self, bot: &Player, rng: &mut R, reference_names: Option<&[String]>) -> u8 {
        match bot.difficulty {
            Some(Difficulty::Level(n)) => n,
            Some(Difficulty::Auto) => {
                let names: Vec<String> = match reference_names {
                    Some(names) => names.to_vec(),
                    None => self.human_players().iter().map(|p| p.name.clone()).collect(),
                };
                let (average, turns) = self.recent_stats(&names, ai::ADAPT_WINDOW);
                ai::adaptive_level(average, turns, rng)
            }
            None => ai::DEFAULT_LEVEL,
        }
    }

    /// Van-e még kapcsolódott (nem lecsatlakozott) emberi játékos?
    pub fn has_connected_human(&self) -> bool {
        self.players.iter().any(|p| !p.is_bot && !p.disconnected)
    }

    /// Az utolsó akció szövege + szerkezetes leírása (utóbbit a kliens lokalizálja).
    fn set_last_action(&mut self, text: String, info: Value) {
        self.last_action = Some(text);
        self.last_action_info = match info {
            Value::Object(ref o) if o.is_empty() => None,
            Value::Null => None,
            other => Some(other),
        };
    }

    /// Jelöli a játékost ideiglenesen lecsatlakozottnak.
    pub fn mark_disconnected(&mut self, player_id: &str) -> bool {
        for p in &mut self.players {
            if p.id == player_id {
                p.disconnected = true;
                return true;
            }
        }
        false
    }

    /// Kicseréli a játékos sid-jét újracsatlakozáskor.
    pub fn replace_player_sid(&mut self, old_id: &str, new_id: &str) -> bool {
        for p in &mut self.players {
            if p.id == old_id {
                p.id = new_id.to_string();
                p.disconnected = false;
                if let Some(pc) = &mut self.pending_challenge {
                    pc.update_player_sid(old_id, new_id);
                }
                for w in &mut self.winners {
                    if w.id == old_id {
                        w.id = new_id.to_string();
                        w.disconnected = false;
                    }
                }
                return true;
            }
        }
        false
    }

    /// Eltávolít egy játékost. Visszatér igazzal, ha körtovábbítás szükséges.
    pub fn remove_player(&mut self, player_id: &str) -> bool {
        let Some(removed_idx) = self.players.iter().position(|p| p.id == player_id) else { return false };

        // Ha a távozó játékos éppen challenge fázisban van, töröljük
        if self.pending_challenge.as_ref().is_some_and(|pc| pc.player_idx == removed_idx) {
            self.pending_challenge = None;
        }
        let was_current = removed_idx == self.current_player_idx;
        self.players.remove(removed_idx);

        let mut needs_next_turn = false;
        if !self.players.is_empty() {
            if removed_idx < self.current_player_idx {
                self.current_player_idx -= 1;
            } else if was_current {
                if self.current_player_idx >= self.players.len() {
                    self.current_player_idx = 0;
                }
                if self.started && !self.finished {
                    needs_next_turn = true;
                }
            }
            if let Some(pc) = &mut self.pending_challenge {
                if removed_idx < pc.player_idx {
                    pc.player_idx -= 1;
                }
            }
        } else {
            self.current_player_idx = 0;
        }
        needs_next_turn
    }

    pub fn start(&mut self) -> Fallible<&'static str> {
        if self.players.is_empty() {
            return Err("Legalább 1 játékos kell.".to_string());
        }
        if self.started {
            return Err("A játék már elkezdődött.".to_string());
        }
        self.started = true;
        for player in &mut self.players {
            player.hand = self.bag.draw(HAND_SIZE);
        }
        self.reset_deadline();
        Ok("A játék elkezdődött!")
    }

    pub fn current_player(&self) -> Option<&Player> {
        self.players.get(self.current_player_idx)
    }

    /// Következő játékos. Kihagyja a büntetett és a lecsatlakozott játékosokat.
    fn next_turn(&mut self) {
        for _ in 0..self.players.len() {
            self.current_player_idx = (self.current_player_idx + 1) % self.players.len();
            let idx = self.current_player_idx;

            // Ha le van csatlakozva, csak átlépjük (kivéve ha mindenki le van csatlakozva); a levelezős
            // játékban a távollévő is sorra kerül, neki a határidő szab időt
            if self.players[idx].disconnected && !self.async_mode {
                // Csak akkor lépünk tovább, ha van még nem lecsatlakozott játékos
                if self.players.iter().any(|p| !p.disconnected) {
                    continue;
                } else {
                    break;
                }
            }

            if self.players[idx].skip_next_turn {
                self.players[idx].skip_next_turn = false;
                let name = self.players[idx].name.clone();
                self.set_last_action(
                    format!("{name} kihagy egy kört (sikertelen megtámadás)"),
                    json!({"type": "skip", "player": name}),
                );
            } else {
                break;
            }
        }
        self.turn_number += 1;
        self.reset_deadline();
    }

    /// Levelezős játékban az új kör határideje.
    pub fn reset_deadline(&mut self) {
        self.turn_deadline = if self.async_mode && self.turn_hours != 0 {
            Some(util::now() + (self.turn_hours * 3600) as f64)
        } else {
            None
        };
    }

    /// Ha a soron lévő játékos lecsatlakozott, tovább lépteti a kört.
    ///
    /// Függő megtámadás alatt nem lép: a lerakás lezárása úgyis továbbadja a kört, itt a léptetés dupla
    /// ugrást okozna. Visszatér igazzal, ha a kör tényleg továbblépett.
    pub fn skip_disconnected_current(&mut self) -> bool {
        if !self.started || self.finished || self.pending_challenge.is_some() || self.async_mode {
            return false;
        }
        let Some(current) = self.current_player() else { return false };
        if !current.disconnected {
            return false;
        }
        if self.players.iter().all(|p| p.disconnected) {
            return false;
        }
        self.next_turn();
        true
    }

    // --- Tile placement ---

    /// Megvannak-e a játékosnak a lerakandó betűk.
    fn validate_hand(player: &Player, tiles: &[Placed]) -> Fallible<()> {
        let mut hand = player.hand.clone();
        for p in tiles {
            let target = p.hand_tile();
            match hand.iter().position(|t| *t == target) {
                Some(i) => {
                    hand.remove(i);
                }
                None => {
                    return Err(if p.is_blank {
                        "Nincs üres zsetonod.".to_string()
                    } else {
                        format!("Nincs '{}' betűd.", p.letter.as_str())
                    });
                }
            }
        }
        Ok(())
    }

    /// Eltávolítja a lerakott betűket a kézből. Visszaadja az eltávolított zsetonokat.
    fn remove_tiles_from_hand(player: &mut Player, tiles: &[Placed]) -> Vec<Tile> {
        let mut removed = Vec::new();
        for p in tiles {
            let target = p.hand_tile();
            if let Some(i) = player.hand.iter().position(|t| *t == target) {
                player.hand.remove(i);
            }
            removed.push(target);
        }
        removed
    }

    /// Véglegesíti a lerakást: tábla, pont, húzás, kör.
    fn finalize_placement(&mut self, tiles: &[Placed], total_score: i32, word_strs: &[String], formed: &[FormedWord]) {
        let idx = self.current_player_idx;
        let rack = self.players[idx].hand.clone(); // a lépés előtti kéz (elemzéshez)
        self.board.apply_placement(tiles);
        self.rejected_placements.clear();
        self.players[idx].score += total_score;
        Self::remove_tiles_from_hand(&mut self.players[idx], tiles);
        let new_tiles = self.bag.draw(HAND_SIZE - self.players[idx].hand.len());
        self.players[idx].hand.extend(new_tiles);
        self.scoreless_turns = 0;
        self.players[idx].timeouts = 0;
        let name = self.players[idx].name.clone();
        self.set_last_action(
            format!("{name}: {} ({total_score} pont)", word_strs.join(", ")),
            json!({"type": "place", "player": name, "words": word_strs, "score": total_score}),
        );
        self.record_move(&name, "place", Some(tiles), Some(formed), total_score, Some(&rack));

        if self.puzzle.is_some() {
            self.finish_puzzle(idx, total_score, word_strs);
        } else if self.players[idx].hand.is_empty() && self.bag.is_empty() {
            self.end_game(Some(idx));
        } else {
            self.next_turn();
        }
    }

    /// A napi feladvány vége: a beküldött lépés pontszáma az eredmény (nincs végső elszámolás).
    fn finish_puzzle(&mut self, player_idx: usize, score: i32, words: &[String]) {
        self.finished = true;
        self.winners = vec![self.players[player_idx].clone()];
        if let Some(puzzle) = &mut self.puzzle {
            puzzle.score = Some(score);
        }
        let name = self.players[player_idx].name.clone();
        self.set_last_action(
            format!("{name}: {} ({score} pont)", words.join(", ")),
            json!({"type": "puzzle", "player": name, "words": words, "score": score}),
        );
    }

    /// Él-e a megtámadási (szavazásos) mód a lerakónál? Csak akkor, ha van legalább egy másik emberi játékos,
    /// aki szavazhat — a robotok nem szavaznak, velük szemben a szótár dönt.
    fn challenge_applies(&self, placer_idx: usize) -> bool {
        if !self.challenge_mode {
            return false;
        }
        self.players.iter().enumerate().any(|(i, p)| i != placer_idx && !p.is_bot)
    }

    fn lerakas_total(formed: &[FormedWord], tile_count: usize) -> i32 {
        let mut total: i32 = formed.iter().map(|w| w.score as i32).sum();
        if tile_count == HAND_SIZE {
            total += BONUS_ALL_TILES;
        }
        total
    }

    /// A lerakás kipróbálása véglegesítés nélkül (élő pontszám-előnézet).
    /// Visszatér: {'valid', 'score', 'words': [{'word','score'}], 'message'}
    pub fn preview_placement(&self, player_id: &str, tiles: &[Placed]) -> Value {
        let empty = |message: &str| json!({"valid": false, "score": 0, "words": [], "message": message});
        if self.finished || !self.started || self.pending_challenge.is_some() {
            return empty("");
        }
        let Some(player) = self.current_player() else { return empty("") };
        if player.id != player_id {
            return empty("");
        }
        if let Err(error) = Self::validate_hand(player, tiles) {
            return empty(&error);
        }
        match self.board.validate_placement(tiles, self.challenge_applies(self.current_player_idx)) {
            Err(error) => empty(&error),
            Ok(formed) => {
                let total = Self::lerakas_total(&formed, tiles.len());
                json!({
                    "valid": true,
                    "score": total,
                    "words": formed.iter().map(|w| json!({"word": w.word, "score": w.score})).collect::<Vec<_>>(),
                    "message": "",
                })
            }
        }
    }

    /// Betűk lerakása. Visszatér: (üzenet, pontszám) vagy a hibaüzenet.
    pub fn place_tiles(&mut self, player_id: &str, tiles: &[Placed]) -> Fallible<(String, i32)> {
        if self.finished {
            return Err("A játék véget ért.".to_string());
        }
        if self.pending_challenge.is_some() {
            return Err("Várj a megtámadási fázis végéig.".to_string());
        }
        let idx = self.current_player_idx;
        let Some(player) = self.players.get(idx) else { return Err("Nem te következel.".to_string()) };
        if player.id != player_id {
            return Err("Nem te következel.".to_string());
        }
        Self::validate_hand(player, tiles)?;

        let challenge_active = self.challenge_applies(idx);
        let formed = self.board.validate_placement(tiles, challenge_active)?;
        let total_score = Self::lerakas_total(&formed, tiles.len());
        let word_strs: Vec<String> = formed.iter().map(|w| w.word.clone()).collect();

        if challenge_active {
            let removed = Self::remove_tiles_from_hand(&mut self.players[idx], tiles);
            self.pending_challenge =
                Some(Challenge::new(tiles.to_vec(), formed, word_strs.clone(), total_score, idx, removed));
            let name = self.players[idx].name.clone();
            self.set_last_action(
                format!("{name}: {} ({total_score} pont) — szavazásra vár", word_strs.join(", ")),
                json!({"type": "pending", "player": name, "words": word_strs, "score": total_score}),
            );
            return Ok((format!("Szavak: {} — szavazásra vár!", word_strs.join(", ")), total_score));
        }

        // Normál mód (vagy egyjátékos challenge módban): azonnal véglegesít
        self.finalize_placement(tiles, total_score, &word_strs, &formed);
        Ok((format!("Szavak: {}", word_strs.join(", ")), total_score))
    }

    // --- Challenge system (voting) ---

    /// Szavazásra jogosult játékosok (a lerakón kívül minden emberi játékos).
    pub fn voter_ids(&self) -> HashSet<String> {
        let Some(pc) = &self.pending_challenge else { return HashSet::new() };
        let placer_id = &self.players[pc.player_idx].id;
        self.players.iter().filter(|p| &p.id != placer_id && !p.is_bot).map(|p| p.id.clone()).collect()
    }

    /// Lerakás véglegesítése (elfogadva).
    fn finalize_accept(&mut self) {
        let Some(pc) = self.pending_challenge.take() else { return };
        let idx = pc.player_idx;
        let mut rack = self.players[idx].hand.clone();
        rack.extend(pc.removed_from_hand.iter().copied()); // a lépés előtti kéz (elemzéshez)
        self.board.apply_placement(&pc.tiles_placed);
        self.rejected_placements.clear();
        self.players[idx].score += pc.score;
        let new_tiles = self.bag.draw(HAND_SIZE - self.players[idx].hand.len());
        self.players[idx].hand.extend(new_tiles);
        self.scoreless_turns = 0;

        let name = self.players[idx].name.clone();
        self.set_last_action(
            format!("{name}: {} ({} pont)", pc.word_strs.join(", "), pc.score),
            json!({"type": "place", "player": name, "words": pc.word_strs, "score": pc.score}),
        );
        self.record_move(&name, "challenge_accept", Some(&pc.tiles_placed), Some(&pc.formed_words), pc.score, Some(&rack));

        if self.players[idx].hand.is_empty() && self.bag.is_empty() {
            self.end_game(Some(idx));
        } else {
            self.next_turn();
        }
    }

    /// Lerakás elutasítása. Betűk visszakerülnek, a lerakó újra jön.
    fn finalize_reject(&mut self) {
        let Some(pc) = self.pending_challenge.take() else { return };
        let idx = pc.player_idx;
        self.players[idx].hand.extend(pc.removed_from_hand.iter().copied());
        self.rejected_placements.insert(placement_key(&pc.tiles_placed));

        let name = self.players[idx].name.clone();
        self.set_last_action(
            format!("{name} szavai elutasítva: {}. Betűk visszavéve, újra ő következik.", pc.word_strs.join(", ")),
            json!({"type": "rejected", "player": name, "words": pc.word_strs}),
        );
        self.record_move(&name, "challenge_reject", None, None, 0, None);

        // Az elutasított lerakás pont nélküli körnek számít (különben végtelen próbálkozás lenne)
        if self.register_scoreless_turn() {
            return;
        }
        if self.players[idx].disconnected {
            // A lecsatlakozott lerakó nem tud újra lépni, ne akadjon el a játék.
            self.next_turn();
        }
    }

    /// Szavazás kiértékelése és véglegesítése.
    fn resolve_and_finalize(&mut self) -> VoteResult {
        let voter_ids = self.voter_ids();
        let accepted = self.pending_challenge.as_ref().map(|pc| pc.resolve_votes(&voter_ids)).unwrap_or(true);
        if accepted {
            self.finalize_accept();
            VoteResult::Accepted
        } else {
            self.finalize_reject();
            VoteResult::Rejected
        }
    }

    fn make_vote_message(result: VoteResult) -> String {
        if result == VoteResult::Accepted {
            "A szavak elfogadva szavazással.".to_string()
        } else {
            "A szavak elutasítva szavazással!".to_string()
        }
    }

    /// Játékos szavaz a függő lerakásra. Visszatér: (eredmény, üzenet) vagy a hibaüzenet.
    fn cast_vote(&mut self, player_id: &str, vote: Vote) -> Fallible<(VoteResult, String)> {
        let Some(pc) = &self.pending_challenge else { return Err("Nincs függő lerakás.".to_string()) };
        let placer = &self.players[pc.player_idx];
        if placer.id == player_id {
            return Err(if vote == Vote::Accept {
                "Saját lerakásodat nem fogadhatod el.".to_string()
            } else {
                "Saját lerakásodat nem utasíthatod el.".to_string()
            });
        }
        let voter_ids = self.voter_ids();
        if !voter_ids.contains(player_id) {
            return Err("Nem szavazhatsz.".to_string());
        }
        if pc.votes.contains_key(player_id) {
            return Err("Már szavaztál.".to_string());
        }
        let Some(voter) = self.players.iter().find(|p| p.id == player_id) else {
            return Err("Nem vagy a játék résztvevője.".to_string());
        };
        let voter_name = voter.name.clone();

        self.pending_challenge.as_mut().unwrap().add_vote(player_id, vote);
        if self.pending_challenge.as_ref().unwrap().all_voted(&voter_ids) {
            let result = self.resolve_and_finalize();
            return Ok((result, Self::make_vote_message(result)));
        }
        let text = if vote == Vote::Accept { format!("{voter_name} elfogadta.") } else { format!("{voter_name} elutasította.") };
        self.set_last_action(text.clone(), json!({"type": "vote", "player": voter_name, "vote": vote.as_str()}));
        Ok((VoteResult::Recorded, text))
    }

    /// Játékos elfogadja a függő lerakást (elfogadó szavazat).
    pub fn accept_pending_by_player(&mut self, player_id: &str) -> Fallible<(VoteResult, String)> {
        self.cast_vote(player_id, Vote::Accept)
    }

    /// Játékos elutasítja a függő lerakást (elutasító szavazat).
    pub fn reject_pending_by_player(&mut self, player_id: &str) -> Fallible<(VoteResult, String)> {
        self.cast_vote(player_id, Vote::Reject)
    }

    /// A lerakó visszavonja a még el nem döntött (szavazásra váró) lerakását.
    ///
    /// Csak addig lehet, amíg senki sem szavazott; a betűk visszakerülnek a kezébe, és újra ő következik.
    /// Nem számít körnek (a pont nélküli körök számlálója sem változik).
    pub fn withdraw_pending(&mut self, player_id: &str) -> Fallible<&'static str> {
        let Some(pc) = &self.pending_challenge else { return Err("Nincs függő lerakás.".to_string()) };
        let idx = pc.player_idx;
        if self.players[idx].id != player_id {
            return Err("Csak a lerakó vonhatja vissza a lerakását.".to_string());
        }
        if !pc.votes.is_empty() {
            return Err("Már szavaztak, a lerakás már nem vonható vissza.".to_string());
        }
        let pc = self.pending_challenge.take().unwrap();
        self.players[idx].hand.extend(pc.removed_from_hand.iter().copied());
        let name = self.players[idx].name.clone();
        self.set_last_action(
            format!("{name} visszavonta a lerakását."),
            json!({"type": "withdrawn", "player": name, "words": pc.word_strs}),
        );
        Ok("Lerakás visszavonva.")
    }

    /// Függő lerakás elfogadása (időtúllépés).
    pub fn accept_pending(&mut self) -> Fallible<(VoteResult, String)> {
        if self.pending_challenge.is_none() {
            return Err("Nincs függő lerakás.".to_string());
        }
        let result = self.resolve_and_finalize();
        let message = if result == VoteResult::Accepted {
            "Szavak elfogadva (időtúllépés)."
        } else {
            "Szavak elutasítva (időtúllépés)."
        };
        Ok((result, message.to_string()))
    }

    // --- Other actions ---

    /// Betűcsere. `tile_indices`: a cserélendő betűk indexei a kézben.
    pub fn exchange_tiles(&mut self, player_id: &str, tile_indices: &[i64]) -> Fallible<String> {
        if self.finished {
            return Err("A játék véget ért.".to_string());
        }
        if self.pending_challenge.is_some() {
            return Err("Várj a megtámadási fázis végéig.".to_string());
        }
        if self.puzzle.is_some() {
            return Err("A napi feladványban nem lehet cserélni.".to_string());
        }
        let idx = self.current_player_idx;
        let Some(player) = self.players.get(idx) else { return Err("Nem te következel.".to_string()) };
        if player.id != player_id {
            return Err("Nem te következel.".to_string());
        }
        if self.bag.remaining() < 7 {
            return Err("Nincs elég zseton a zsákban a cseréhez (minimum 7 kell).".to_string());
        }
        if self.bag.remaining() < tile_indices.len() {
            return Err("Nincs elég zseton a zsákban a cseréhez.".to_string());
        }
        if tile_indices.is_empty() {
            return Err("Legalább egy zsetont ki kell választani.".to_string());
        }
        let unique: HashSet<i64> = tile_indices.iter().copied().collect();
        if unique.len() != tile_indices.len() {
            return Err("Duplikált zseton index.".to_string());
        }
        for &i in tile_indices {
            if i < 0 || i as usize >= player.hand.len() {
                return Err("Érvénytelen zseton index.".to_string());
            }
        }

        let rack = player.hand.clone(); // a csere előtti kéz (elemzéshez)
        let mut sorted_desc: Vec<usize> = tile_indices.iter().map(|i| *i as usize).collect();
        sorted_desc.sort_by(|a, b| b.cmp(a));
        let tiles_to_exchange: Vec<Tile> = sorted_desc.iter().map(|i| self.players[idx].hand[*i]).collect();
        for i in &sorted_desc {
            self.players[idx].hand.remove(*i);
        }
        let new_tiles = self.bag.draw(tiles_to_exchange.len());
        self.players[idx].hand.extend(new_tiles);
        self.bag.put_back(&tiles_to_exchange);

        let name = self.players[idx].name.clone();
        let count = tiles_to_exchange.len();
        self.set_last_action(format!("{name} cserélt {count} zsetont"), json!({"type": "exchange", "player": name, "count": count}));
        self.players[idx].timeouts = 0;
        self.record_move(&name, "exchange", None, None, 0, Some(&rack));
        if !self.register_scoreless_turn() {
            self.next_turn();
        }
        Ok(format!("{count} zseton kicserélve."))
    }

    /// Passz. `timeout`: a levelezős játék határideje járt le (automatikus passz).
    pub fn pass_turn(&mut self, player_id: &str, timeout: bool) -> Fallible<&'static str> {
        if self.finished {
            return Err("A játék véget ért.".to_string());
        }
        if self.pending_challenge.is_some() {
            return Err("Várj a megtámadási fázis végéig.".to_string());
        }
        if self.puzzle.is_some() {
            return Err("A napi feladványban nem lehet passzolni.".to_string());
        }
        let idx = self.current_player_idx;
        let Some(player) = self.players.get(idx) else { return Err("Nem te következel.".to_string()) };
        if player.id != player_id {
            return Err("Nem te következel.".to_string());
        }
        let name = player.name.clone();
        if timeout {
            self.players[idx].timeouts += 1;
            self.set_last_action(format!("{name} nem lépett időben (passz)"), json!({"type": "timeout", "player": name}));
        } else {
            self.players[idx].timeouts = 0;
            self.set_last_action(format!("{name} passzolt"), json!({"type": "pass", "player": name}));
        }
        let rack = self.players[idx].hand.clone();
        self.record_move(&name, "pass", None, None, 0, Some(&rack));
        if !self.register_scoreless_turn() {
            self.next_turn();
        }
        Ok("Passz.")
    }

    /// Levelezős játék: a soron lévő nem lépett a határidőig — passz, és ha ez már a `MAX_TIMEOUTS`-adik
    /// egymás utáni lejárt határideje, a játékos feladja a játékot. Visszatér: igaz, ha történt valami.
    pub fn expire_turn(&mut self) -> bool {
        if !self.async_mode || !self.started || self.finished || self.pending_challenge.is_some() {
            return false;
        }
        let Some(player) = self.current_player() else { return false };
        let player_id = player.id.clone();
        let ok = self.pass_turn(&player_id, true).is_ok();
        if ok && !self.finished {
            // a lépés után a játékos azonosító szerinti keresése (a kör már továbbléphetett)
            let timeouts = self.players.iter().find(|p| p.id == player_id).map(|p| p.timeouts).unwrap_or(0);
            if timeouts >= MAX_TIMEOUTS {
                let _ = self.resign(&player_id);
            }
        }
        ok
    }

    /// A játékos feladja a játékot: a játék véget ér, és ő nem lehet győztes.
    pub fn resign(&mut self, player_id: &str) -> Fallible<&'static str> {
        if !self.started {
            return Err("A játék még nem indult el.".to_string());
        }
        if self.finished {
            return Err("A játék véget ért.".to_string());
        }
        let Some(idx) = self.players.iter().position(|p| p.id == player_id) else {
            return Err("Nem vagy a játék résztvevője.".to_string());
        };
        self.players[idx].resigned = true;
        self.pending_challenge = None;
        self.end_game(None);
        let best = self.players.iter().filter(|p| !p.resigned).map(|p| p.score).max();
        if let Some(best) = best {
            self.winners = self.players.iter().filter(|p| !p.resigned && p.score == best).cloned().collect();
        }
        let name = self.players[idx].name.clone();
        self.set_last_action(format!("{name} feladta a játékot."), json!({"type": "resigned", "player": name}));
        Ok("A játék feladva.")
    }

    // --- Admin beavatkozások ---

    /// Admin: a szavazásra váró lerakás elfogadásának vagy elutasításának kikényszerítése.
    pub fn force_resolve_pending(&mut self, accept: bool) -> Fallible<(VoteResult, String)> {
        if self.pending_challenge.is_none() {
            return Err("Nincs függő lerakás.".to_string());
        }
        let result = if accept {
            self.finalize_accept();
            VoteResult::Accepted
        } else {
            self.finalize_reject();
            VoteResult::Rejected
        };
        Ok((result, Self::make_vote_message(result)))
    }

    /// Admin: a játék befejezése a jelenlegi állással (normál végső elszámolás). A szavazásra váró lerakás
    /// betűi előbb visszakerülnek a lerakó kezébe. Visszatér: igaz, ha véget ért a játék.
    pub fn force_end(&mut self) -> bool {
        if !self.started || self.finished {
            return false;
        }
        if let Some(pc) = self.pending_challenge.take() {
            self.players[pc.player_idx].hand.extend(pc.removed_from_hand.iter().copied());
        }
        self.end_game(None);
        true
    }

    // --- Helpers ---

    /// Pont nélküli kör (passz, csere, elutasított lerakás) rögzítése. Igaz, ha ezzel véget ért a játék.
    fn register_scoreless_turn(&mut self) -> bool {
        self.scoreless_turns += 1;
        if self.scoreless_turns >= SCORELESS_TURNS_LIMIT {
            self.end_game(None);
            return true;
        }
        false
    }

    pub fn find_player(&self, player_id: &str) -> Option<&Player> {
        self.players.iter().find(|p| p.id == player_id)
    }

    /// Játék vége, végső pontozás.
    fn end_game(&mut self, finisher: Option<usize>) {
        self.finished = true;
        self.pending_challenge = None;

        let mut remaining_total = 0;
        for player in &mut self.players {
            let hand_value: i32 = player.hand.iter().map(|t| t.value() as i32).sum();
            player.score -= hand_value;
            remaining_total += hand_value;
        }
        if let Some(i) = finisher {
            self.players[i].score += remaining_total;
        }

        // Egyenlő pontnál mindenki nyer (döntetlen), nem a lista első játékosa
        let top_score = self.players.iter().map(|p| p.score).max().unwrap_or(0);
        self.winners = self.players.iter().filter(|p| p.score == top_score).cloned().collect();
        if self.winners.len() == 1 {
            let name = self.winners[0].name.clone();
            self.set_last_action(
                format!("Játék vége! Győztes: {name} ({top_score} pont)"),
                json!({"type": "game_over", "player": name, "score": top_score}),
            );
        } else {
            let names: Vec<String> = self.winners.iter().map(|p| p.name.clone()).collect();
            self.set_last_action(
                format!("Játék vége! Döntetlen: {} ({top_score} pont)", names.join(", ")),
                json!({"type": "game_over_draw", "players": names, "score": top_score}),
            );
        }
    }

    // --- Move logging & persistence ---

    /// Lépés rögzítése a move_log-ba. `rack`: a játékos keze a lépés előtt.
    fn record_move(
        &mut self,
        player_name: &str,
        action_type: &str,
        tiles: Option<&[Placed]>,
        formed: Option<&[FormedWord]>,
        score: i32,
        rack: Option<&[Tile]>,
    ) {
        let mut details = Map::new();
        if let Some(rack) = rack {
            details.insert("rack".into(), tiles_to_json(rack));
        }
        if let Some(tiles) = tiles.filter(|t| !t.is_empty()) {
            details.insert("tiles".into(), Value::Array(tiles.iter().map(|p| p.to_json()).collect()));
        }
        if let Some(formed) = formed.filter(|f| !f.is_empty()) {
            details.insert("words".into(), Value::Array(formed.iter().map(|w| Value::from(w.word.clone())).collect()));
        }
        details.insert("score".into(), Value::from(score));
        self.move_log.push(MoveLog {
            move_number: self.move_log.len() as i64 + 1,
            player_name: player_name.to_string(),
            action_type: action_type.to_string(),
            details: Value::Object(details),
            board: self.board.clone(),
        });
    }

    /// Teljes játékállapot szerializálása mentéshez.
    ///
    /// Függő (szavazásra váró) lerakás nem menthető: a lerakó betűit ilyenkor visszaírjuk a kezébe, így a
    /// mentésből betöltve nem vesznek el zsetonok.
    pub fn to_save_dict(&self) -> Value {
        let mut pending_idx: Option<usize> = None;
        let mut returned_tiles: Vec<Tile> = Vec::new();
        let mut last_action = self.last_action.clone();
        let mut last_action_info = self.last_action_info.clone();
        if let Some(pc) = &self.pending_challenge {
            pending_idx = Some(pc.player_idx);
            returned_tiles = pc.removed_from_hand.clone();
            let placer = &self.players[pc.player_idx];
            last_action = Some(format!("{} lerakása a mentés miatt visszavonva.", placer.name));
            last_action_info = Some(json!({"type": "save_revert", "player": placer.name}));
        }

        let players: Vec<Value> = self
            .players
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let mut hand = p.hand.clone();
                if Some(i) == pending_idx {
                    hand.extend(returned_tiles.iter().copied());
                }
                json!({
                    "id": p.id,
                    "name": p.name,
                    "hand": tiles_to_json(&hand),
                    "score": p.score,
                    "skip_next_turn": p.skip_next_turn,
                    "disconnected": p.disconnected,
                    "timeouts": p.timeouts,
                    "resigned": p.resigned,
                    "is_bot": p.is_bot,
                    "difficulty": p.difficulty.map(|d| d.to_json()).unwrap_or(Value::Null),
                })
            })
            .collect();

        json!({
            "id": self.id,
            "challenge_mode": self.challenge_mode,
            "turn_time_limit": self.turn_time_limit,
            "hint_limit": self.hint_limit,
            "hints_used": self.hints_used,
            "started": self.started,
            "finished": self.finished,
            "current_player_idx": self.current_player_idx,
            "turn_number": self.turn_number,
            "scoreless_turns": self.scoreless_turns,
            "async_mode": self.async_mode,
            "turn_hours": self.turn_hours,
            "turn_deadline": self.turn_deadline,
            "last_action": last_action,
            "last_action_info": last_action_info,
            "board": self.board.to_json(),
            "board_is_empty": self.board.is_empty,
            "bag_tiles": tiles_to_json(&self.bag.tiles),
            "players": players,
            "winner_names": self.winners.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
        })
    }

    /// Játék visszaállítása mentett állapotból.
    pub fn from_save_dict(data: &Value) -> Fallible<Game> {
        let id = data.get("id").and_then(|v| v.as_str()).ok_or("hiányzó játékazonosító")?.to_string();
        let int = |key: &str, default: i64| data.get(key).and_then(util::py_int).unwrap_or(default);
        let mut game = Game::new(
            &id,
            data.get("challenge_mode").map(util::truthy).unwrap_or(false),
            int("turn_time_limit", 0),
            int("hint_limit", DEFAULT_HINT_LIMIT),
        );
        game.hints_used = int("hints_used", 0).max(0);
        game.started = data.get("started").map(util::truthy).unwrap_or(false);
        game.finished = data.get("finished").map(util::truthy).unwrap_or(false);
        game.current_player_idx = int("current_player_idx", 0).max(0) as usize;
        game.turn_number = int("turn_number", 0);

        // Régi mentésben nincs játékszintű számláló: a játékosonkénti passz-sorozat legnagyobbja
        let players_data: Vec<&Value> = data.get("players").and_then(|v| v.as_array()).map(|a| a.iter().collect()).unwrap_or_default();
        let legacy_passes = players_data.iter().map(|pd| pd.get("consecutive_passes").and_then(util::py_int).unwrap_or(0)).max().unwrap_or(0);
        game.scoreless_turns = int("scoreless_turns", legacy_passes).max(0) as u32;
        game.async_mode = data.get("async_mode").map(util::truthy).unwrap_or(false);
        game.turn_hours = int("turn_hours", 0);
        game.turn_deadline = data.get("turn_deadline").and_then(|v| v.as_f64());
        game.last_action = data.get("last_action").and_then(|v| v.as_str()).map(|s| s.to_string());
        game.last_action_info = data.get("last_action_info").filter(|v| !v.is_null()).cloned();

        // Board visszaállítás
        if let Some(board_data) = data.get("board") {
            game.board = Board::from_json(board_data);
        }
        game.board.is_empty = data.get("board_is_empty").map(util::truthy).unwrap_or(true);

        // Bag visszaállítás
        game.bag.tiles = tiles_from_json(data.get("bag_tiles").unwrap_or(&Value::Null));

        // Játékosok visszaállítás
        for pd in players_data {
            let player_id = pd.get("id").and_then(|v| v.as_str()).ok_or("hiányzó játékosazonosító")?;
            let name = pd.get("name").and_then(|v| v.as_str()).ok_or("hiányzó játékosnév")?;
            let is_bot = pd.get("is_bot").map(util::truthy).unwrap_or(false);
            let mut player = if is_bot {
                Player::new_bot(player_id, name, pd.get("difficulty").unwrap_or(&Value::Null))
            } else {
                Player::new(player_id, name)
            };
            player.hand = tiles_from_json(pd.get("hand").unwrap_or(&Value::Null));
            player.score = pd.get("score").and_then(util::py_int).unwrap_or(0) as i32;
            player.skip_next_turn = pd.get("skip_next_turn").map(util::truthy).unwrap_or(false);
            player.timeouts = pd.get("timeouts").and_then(util::py_int).unwrap_or(0).max(0) as u32;
            player.resigned = pd.get("resigned").map(util::truthy).unwrap_or(false);
            player.disconnected = if player.is_bot { false } else { pd.get("disconnected").map(util::truthy).unwrap_or(false) };
            game.players.push(player);
        }

        // Győztes(ek) visszaállítása (régi mentésben egyetlen `winner_name`)
        let winner_names: Vec<String> = match data.get("winner_names") {
            Some(Value::Array(names)) => names.iter().filter_map(|n| n.as_str().map(|s| s.to_string())).collect(),
            _ => match data.get("winner_name").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                Some(name) => vec![name.to_string()],
                None => Vec::new(),
            },
        };
        game.winners = game.players.iter().filter(|p| winner_names.contains(&p.name)).cloned().collect();
        Ok(game)
    }

    // --- Lépéstörténet ---

    /// A lépések rövid, kliensnek szánt listája (a move_log-ból).
    pub fn history(&self) -> Vec<Value> {
        self.move_log
            .iter()
            .map(|m| {
                let tiles: Vec<Value> = m
                    .details
                    .get("tiles")
                    .and_then(|t| t.as_array())
                    .map(|a| a.iter().map(|t| json!({"row": t["row"], "col": t["col"]})).collect())
                    .unwrap_or_default();
                json!({
                    "n": m.move_number,
                    "player": m.player_name,
                    "type": m.action_type,
                    "words": m.details.get("words").cloned().unwrap_or_else(|| json!([])),
                    "score": m.details.get("score").cloned().unwrap_or(json!(0)),
                    "tiles": tiles,
                })
            })
            .collect()
    }

    /// Az utolsó lépés lerakott mezői (kiemeléshez), ha az utolsó lépés lerakás volt.
    fn last_move_tiles(history: &[Value]) -> Value {
        match history.last() {
            Some(last) if matches!(last["type"].as_str(), Some("place") | Some("challenge_accept")) => last["tiles"].clone(),
            _ => json!([]),
        }
    }

    // --- State serialization ---

    /// A játék közös állapota (ami minden játékosnál azonos).
    fn shared_state(&self) -> Map<String, Value> {
        let current = self.current_player();
        let history = self.history();
        let last_move_tiles = Self::last_move_tiles(&history);
        let history_light: Vec<Value> = history
            .into_iter()
            .map(|mut h| {
                if let Some(o) = h.as_object_mut() {
                    o.remove("tiles");
                }
                h
            })
            .collect();
        // a győztes játékos élő adata (ha még a játékban van), különben a játék végi pillanatkép
        let live = |w: &Player| self.players.iter().find(|p| p.name == w.name).cloned().unwrap_or_else(|| w.clone());
        let winners: Vec<Player> = self.winners.iter().map(live).collect();
        let mut state = Map::new();
        let mut put = |key: &str, value: Value| {
            state.insert(key.to_string(), value);
        };
        put("game_id", json!(self.id));
        put("started", json!(self.started));
        put("finished", json!(self.finished));
        put("board", self.board.to_json());
        put("current_player", current.map(|p| json!(p.id)).unwrap_or(Value::Null));
        put("current_player_name", current.map(|p| json!(p.name)).unwrap_or(Value::Null));
        put("turn_number", json!(self.turn_number));
        put("tiles_remaining", json!(self.bag.remaining()));
        put("last_action", self.last_action.clone().map(Value::from).unwrap_or(Value::Null));
        put("last_action_info", self.last_action_info.clone().unwrap_or(Value::Null));
        put("last_move_tiles", last_move_tiles);
        put("history", Value::Array(history_light));
        put("winner", if winners.len() == 1 { winners[0].to_json(false) } else { Value::Null });
        put("winners", Value::Array(winners.iter().map(|p| p.to_json(false)).collect()));
        put(
            "puzzle",
            match &self.puzzle {
                Some(p) => json!({"date": p.date, "score": p.score}),
                None => Value::Null,
            },
        );
        put("challenge_mode", json!(self.challenge_mode));
        put("turn_time_limit", json!(self.turn_time_limit));
        put("async_mode", json!(self.async_mode));
        put("turn_hours", json!(self.turn_hours));
        put("turn_deadline", self.turn_deadline.map(Value::from).unwrap_or(Value::Null));
        put("hint_limit", json!(self.hint_limit));
        put("hints_left", json!(self.hints_left()));
        put(
            "pending_challenge",
            self.pending_challenge.as_ref().map(|pc| pc.to_state(&self.players)).unwrap_or(Value::Null),
        );
        state
    }

    fn state_from_shared(&self, shared: &Map<String, Value>, for_player_id: Option<&str>) -> Value {
        let mut state = shared.clone();
        state.insert(
            "players".to_string(),
            Value::Array(self.players.iter().map(|p| p.to_json(Some(p.id.as_str()) == for_player_id)).collect()),
        );
        Value::Object(state)
    }

    /// A játék állapota JSON formában (a megadott játékos kezével).
    pub fn get_state(&self, for_player_id: Option<&str>) -> Value {
        let shared = self.shared_state();
        self.state_from_shared(&shared, for_player_id)
    }

    /// Az összes (nem robot) játékos állapota egyszerre; a közös részt csak egyszer számítja ki.
    pub fn get_all_states(&self) -> Vec<(String, Value)> {
        let shared = self.shared_state();
        self.players
            .iter()
            .filter(|p| !p.is_bot)
            .map(|p| (p.id.clone(), self.state_from_shared(&shared, Some(&p.id))))
            .collect()
    }

    /// Megfigyelői nézet: ugyanaz, mint a játékosoké, de egyetlen kéz sem látszik.
    pub fn get_spectator_state(&self) -> Value {
        let mut state = self.get_state(None);
        state["spectator"] = json!(true);
        state
    }
}

pub fn board_size() -> usize {
    BOARD_SIZE
}
