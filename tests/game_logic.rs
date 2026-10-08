//! Játéklogika: zsák, tábla, játékos, játék, megtámadás, mentés, játék vége (a Python `test_game_logic.py`
//! és a `test_regressions.py` játéklogikai részének megfelelője).
//!
//! A Python tesztek a szótár-ellenőrzést kikapcsolták (`patch('board.check_words')`); itt a többjátékos megtámadásos
//! játék eleve szótár nélküli (a szavazás dönt), a többinél valódi szavak (ALMA) szerepelnek.

use scrabble::board::{BOARD_SIZE, Board, Placed, Premium, premium_at};
use scrabble::game::{BONUS_ALL_TILES, Game, HAND_SIZE, VoteResult};
use scrabble::player::Player;
use scrabble::tiles::{TILE_DISTRIBUTION, Tile, TileBag};
use serde_json::{Value, json};

fn t(letter: &str) -> Tile {
    Tile::from_str(letter).unwrap_or_else(|| panic!("ismeretlen zseton: {letter}"))
}

fn pl(row: i32, col: i32, letter: &str) -> Placed {
    Placed::new(row, col, t(letter), false)
}

fn blank(row: i32, col: i32, letter: &str) -> Placed {
    Placed::new(row, col, t(letter), true)
}

const NAMES: [&str; 4] = ["Alice", "Bob", "Charlie", "Diana"];

/// `n` játékos (p1..pn), elindítva.
fn started(n: usize, challenge: bool) -> Game {
    let mut g = Game::new("test", challenge, 0, 3);
    for i in 0..n {
        g.add_player(&format!("p{}", i + 1), NAMES[i]).unwrap();
    }
    g.start().unwrap();
    g
}

fn set_hand(g: &mut Game, idx: usize, letters: &[&str]) {
    g.players[idx].hand = letters.iter().map(|l| t(l)).collect();
}

fn hand(p: &Player) -> Vec<String> {
    let mut h: Vec<String> = p.hand.iter().map(|t| t.as_str().to_string()).collect();
    h.sort();
    h
}

fn sorted(letters: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = letters.iter().map(|s| s.to_string()).collect();
    v.sort();
    v
}

const HAND7: [&str; 7] = ["A", "B", "C", "D", "E", "F", "G"];
const ALMA_HAND: [&str; 7] = ["A", "L", "M", "A", "K", "E", "T"];

fn ab() -> Vec<Placed> {
    vec![pl(7, 6, "A"), pl(7, 7, "B")]
}

fn alma() -> Vec<Placed> {
    vec![pl(7, 5, "A"), pl(7, 6, "L"), pl(7, 7, "M"), pl(7, 8, "A")]
}

fn total_tiles(g: &Game) -> usize {
    let board = g.board.cells.iter().flatten().filter(|c| c.is_some()).count();
    board + g.players.iter().map(|p| p.hand.len()).sum::<usize>() + g.bag.remaining()
}

const TOTAL: usize = 100;

// ===================================================================================================
// TileBag
// ===================================================================================================

#[test]
fn tile_distribution_and_values() {
    assert_eq!(TILE_DISTRIBUTION.iter().map(|(_, _, c)| *c as usize).sum::<usize>(), TOTAL);
    assert_eq!(t("A").value(), 1);
    assert_eq!(t("TY").value(), 10);
    assert_eq!(t("").value(), 0);
    assert_eq!(t("CS").value(), 7);
    assert_eq!(t("SZ").value(), 3);
}

#[test]
fn bag_drawing() {
    let mut bag = TileBag::new();
    assert_eq!(bag.remaining(), 100);
    assert!(!bag.is_empty());
    let drawn = bag.draw(7);
    assert_eq!(drawn.len(), 7);
    assert_eq!(bag.remaining(), 93);
    bag.put_back(&drawn);
    assert_eq!(bag.remaining(), 100);
    assert!(bag.draw(0).is_empty());
    assert_eq!(bag.remaining(), 100);
    bag.tiles = vec![t("A"), t("B")];
    assert_eq!(bag.draw(5).len(), 2);
    assert_eq!(bag.remaining(), 0);
    assert!(bag.draw(3).is_empty());
    assert!(bag.is_empty());
}

// ===================================================================================================
// Board
// ===================================================================================================

#[test]
fn board_basics() {
    let mut b = Board::new();
    assert!(b.is_empty);
    for r in 0..BOARD_SIZE as i32 {
        for c in 0..BOARD_SIZE as i32 {
            assert!(b.get(r, c).is_none());
        }
    }
    for (r, c) in [(-1, 0), (0, -1), (15, 0), (0, 15)] {
        assert!(b.get(r, c).is_none());
    }
    b.set(7, 7, t("A"), false);
    let cell = b.get(7, 7).unwrap();
    assert_eq!((cell.letter, cell.is_blank), (t("A"), false));
    let json = b.to_json();
    assert_eq!(json.as_array().unwrap().len(), 15);
    assert_eq!(json[7][7], json!({"letter": "A", "is_blank": false}));
    assert_eq!(json[0][0], Value::Null);
    let mut b2 = Board::new();
    b2.apply_placement(&[pl(7, 7, "A"), pl(7, 8, "B")]);
    assert!(!b2.is_empty);
    assert_eq!(b2.get(7, 8).unwrap().letter, t("B"));
}

#[test]
fn premium_layout() {
    assert_eq!(premium_at(0, 0), Premium::Tw);
    assert_eq!(premium_at(7, 7), Premium::Star);
    assert_eq!(premium_at(1, 1), Premium::Dw);
    assert_eq!(premium_at(0, 3), Premium::Dl);
    assert_eq!(premium_at(1, 5), Premium::Tl);
    for (r, c) in [(0, 0), (0, 14), (14, 0), (14, 14)] {
        assert_eq!(premium_at(r, c), Premium::Tw);
    }
}

fn validate(b: &Board, tiles: &[Placed]) -> Result<Vec<scrabble::board::FormedWord>, String> {
    b.validate_placement(tiles, true)
}

#[test]
fn first_move_rules() {
    let b = Board::new();
    assert!(validate(&b, &[pl(0, 0, "A"), pl(0, 1, "B")]).unwrap_err().contains("középső"));
    assert!(validate(&b, &[pl(7, 7, "A")]).unwrap_err().contains("2 betű"));
    let words = validate(&b, &[pl(7, 6, "A"), pl(7, 7, "B"), pl(7, 8, "C")]).unwrap();
    assert_eq!(words[0].word, "ABC");
    assert!(validate(&b, &[pl(6, 7, "A"), pl(7, 7, "B")]).is_ok());
    assert!(validate(&b, &[pl(7, 6, "A"), pl(7, 7, "B"), pl(8, 8, "C")]).unwrap_err().contains("egy sorban"));
    assert!(validate(&b, &[pl(7, 5, "A"), pl(7, 7, "B")]).is_err(), "rés");
    assert!(validate(&b, &[]).is_err());
    assert!(validate(&b, &[pl(15, 7, "A")]).is_err());
}

#[test]
fn later_moves_must_connect() {
    let mut b = Board::new();
    b.apply_placement(&[pl(7, 7, "A"), pl(7, 8, "B")]);
    assert!(validate(&b, &[pl(0, 0, "C"), pl(0, 1, "D")]).unwrap_err().contains("csatlakoznia"));
    assert!(validate(&b, &[pl(7, 9, "C"), pl(7, 10, "D")]).is_ok());
    assert!(validate(&b, &[pl(7, 7, "B")]).unwrap_err().contains("foglalt"));
}

#[test]
fn board_scoring() {
    let b = Board::new();
    // A(1) + B(2) a középső (dupla szó) mezőn: (1 + 2) * 2
    assert_eq!(validate(&b, &[pl(7, 6, "A"), pl(7, 7, "B")]).unwrap()[0].score, 6);
    // a joker 0 pontot ér: (0 + 2) * 2
    let words = validate(&b, &[blank(7, 7, "A"), pl(7, 8, "B")]).unwrap();
    assert_eq!((words[0].word.as_str(), words[0].score), ("AB", 4));
    let words = validate(&b, &[blank(7, 6, "A"), pl(7, 7, "B")]).unwrap();
    assert_eq!(words[0].word, "AB");
    // dupla betű a (7,3) mezőn
    assert!(validate(&b, &[pl(7, 3, "A"), pl(7, 4, "B"), pl(7, 5, "C"), pl(7, 6, "D"), pl(7, 7, "E")]).is_ok());
}

#[test]
fn cross_words_are_formed() {
    let mut b = Board::new();
    b.apply_placement(&[pl(7, 7, "A"), pl(7, 8, "B")]);
    let words = validate(&b, &[pl(6, 7, "C"), pl(8, 7, "D")]).unwrap();
    assert!(!words.is_empty());
    let single = validate(&b, &[pl(8, 7, "C")]).unwrap();
    assert!(single.iter().any(|w| w.word == "AC"));
    b.apply_placement(&[pl(8, 7, "C")]);
    let words: Vec<String> = validate(&b, &[pl(8, 8, "D")]).unwrap().into_iter().map(|w| w.word).collect();
    assert!(words.contains(&"CD".to_string()) && words.contains(&"BD".to_string()));
    // hosszú szó a tripla szó mezőn át
    let mut b = Board::new();
    b.apply_placement(&[pl(7, 7, "A"), pl(7, 8, "B")]);
    let tiles: Vec<Placed> = (0..7).map(|r| Placed::new(r, 7, t(&((b'A' + r as u8) as char).to_string()), false)).collect();
    assert!(validate(&b, &tiles).is_ok());
}

#[test]
fn duplicate_positions_are_rejected() {
    let b = Board::new();
    let err = validate(&b, &[pl(7, 7, "A"), pl(7, 7, "L"), pl(7, 8, "M")]).unwrap_err();
    assert!(err.contains("több zseton"));
    let mut g = started(2, false);
    set_hand(&mut g, 0, &ALMA_HAND);
    let before = hand(&g.players[0]);
    assert!(g.place_tiles("p1", &[pl(7, 7, "A"), pl(7, 7, "L"), pl(7, 8, "M")]).is_err());
    assert_eq!(hand(&g.players[0]), before);
    assert!(g.board.is_empty);
}

// ===================================================================================================
// Player
// ===================================================================================================

#[test]
fn player_json_hides_the_hand() {
    let mut p = Player::new("id1", "Alice");
    assert_eq!((p.id.as_str(), p.name.as_str(), p.score), ("id1", "Alice", 0));
    assert!(p.hand.is_empty());
    p.hand = vec![t("A"), t("B"), t("C")];
    p.score = 42;
    let hidden = p.to_json(false);
    assert!(hidden.get("hand").is_none());
    assert_eq!(hidden["hand_count"], 3);
    assert_eq!(hidden["score"], 42);
    assert_eq!(hidden["name"], "Alice");
    assert_eq!(p.to_json(true)["hand"], json!(["A", "B", "C"]));
}

// ===================================================================================================
// Game: alapok
// ===================================================================================================

#[test]
fn players_join_and_leave() {
    let mut g = Game::with_defaults("test-room");
    assert_eq!(g.id, "test-room");
    assert!(!g.started && !g.finished && g.players.is_empty());
    assert!(g.current_player().is_none());
    for i in 0..4 {
        g.add_player(&format!("p{i}"), &format!("Player{i}")).unwrap();
    }
    assert!(g.add_player("p5", "Player5").unwrap_err().contains("4 játékos"));
    g.remove_player("p0");
    assert_eq!(g.players.len(), 3);
    assert_eq!(g.players[0].name, "Player1");
    let mut g = Game::with_defaults("t");
    g.add_player("p1", "Alice").unwrap();
    g.start().unwrap();
    assert!(g.add_player("p2", "Bob").is_err());
}

#[test]
fn starting_the_game() {
    let mut g = Game::with_defaults("t");
    assert!(g.start().is_err());
    g.add_player("p1", "Alice").unwrap();
    g.add_player("p2", "Bob").unwrap();
    g.start().unwrap();
    assert!(g.started);
    assert_eq!(g.players[0].hand.len(), 7);
    assert_eq!(g.players[1].hand.len(), 7);
    assert_eq!(g.bag.remaining(), 100 - 14);
    assert!(g.start().is_err());
}

#[test]
fn passing_and_the_scoreless_limit() {
    let mut g = started(2, false);
    g.pass_turn("p1", false).unwrap();
    assert_eq!(g.current_player().unwrap().id, "p2");
    let mut g = started(2, false);
    assert!(g.pass_turn("p2", false).unwrap_err().contains("Nem te"));
    let mut g = started(2, false);
    for i in 0..5 {
        g.pass_turn(if i % 2 == 0 { "p1" } else { "p2" }, false).unwrap();
        assert!(!g.finished);
    }
    g.pass_turn("p2", false).unwrap();
    assert!(g.finished);
    assert!(g.pass_turn("p1", false).unwrap_err().contains("véget ért"));
    // három játékos kétszer körbepasszol
    let mut g = started(3, false);
    for _ in 0..2 {
        for id in ["p1", "p2", "p3"] {
            g.pass_turn(id, false).unwrap();
        }
    }
    assert!(g.finished);
}

#[test]
fn exchanging_tiles() {
    let mut g = started(1, false);
    g.exchange_tiles("p1", &[0, 1]).unwrap();
    assert_eq!(g.players[0].hand.len(), 7);
    assert!(g.exchange_tiles("p1", &[10]).is_err());
    assert!(g.exchange_tiles("p1", &[]).is_err());
    assert!(g.exchange_tiles("p1", &[0, 0]).unwrap_err().contains("Duplikált"));
    assert!(g.exchange_tiles("p1", &[-1]).unwrap_err().contains("Érvénytelen"));
    let mut g = started(2, false);
    assert!(g.exchange_tiles("p2", &[0]).is_err());
    let mut g = started(1, false);
    g.bag.tiles = vec![t("Z")];
    assert!(g.exchange_tiles("p1", &[0, 1, 2]).unwrap_err().contains("Nincs elég"));
    let mut g = started(1, false);
    g.finished = true;
    assert!(g.exchange_tiles("p1", &[0]).unwrap_err().contains("véget ért"));
    // függő szavazás alatt nem lehet cserélni
    let mut g = started(2, true);
    set_hand(&mut g, 0, &HAND7);
    g.place_tiles("p1", &ab()).unwrap();
    assert!(g.exchange_tiles("p2", &[0]).unwrap_err().contains("Várj"));
    // a csere pont nélküli kör
    let mut g = started(2, false);
    g.pass_turn("p1", false).unwrap();
    assert_eq!(g.scoreless_turns, 1);
    g.pass_turn("p2", false).unwrap();
    g.exchange_tiles("p1", &[0]).unwrap();
    assert_eq!(g.scoreless_turns, 3);
}

#[test]
fn game_state_json() {
    let mut g = Game::with_defaults("t");
    g.add_player("p1", "Alice").unwrap();
    g.start().unwrap();
    let state = g.get_state(Some("p1"));
    assert_eq!(state["started"], true);
    assert_eq!(state["finished"], false);
    assert_eq!(state["players"].as_array().unwrap().len(), 1);
    assert!(state["players"][0].get("hand").is_some());
    assert_eq!(state["tiles_remaining"], 93);
    assert_eq!(state["current_player"], "p1");
    let mut g = started(2, false);
    let state = g.get_state(Some("p1"));
    let find = |id: &str| state["players"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap().clone();
    assert!(find("p1").get("hand").is_some() && find("p2").get("hand").is_none());
    let all: std::collections::HashMap<String, Value> = g.get_all_states().into_iter().collect();
    let p2_view = &all["p2"]["players"];
    assert!(p2_view.as_array().unwrap().iter().find(|p| p["id"] == "p1").unwrap().get("hand").is_none());
    assert!(p2_view.as_array().unwrap().iter().find(|p| p["id"] == "p2").unwrap().get("hand").is_some());
    assert_eq!(all["p1"]["board"], all["p2"]["board"]);
    g.finished = true;
    assert!(g.get_state(None)["finished"] == true);
}

// ===================================================================================================
// Megtámadásos (szavazásos) mód
// ===================================================================================================

#[test]
fn challenge_mode_flags() {
    let g = Game::new("t", true, 0, 3);
    assert!(g.challenge_mode && g.pending_challenge.is_none());
    assert!(!Game::with_defaults("t").challenge_mode);
    let mut g = Game::new("t", true, 0, 3);
    g.add_player("p1", "Alice").unwrap();
    g.start().unwrap();
    let state = g.get_state(None);
    assert_eq!(state["challenge_mode"], true);
    assert_eq!(state["pending_challenge"], Value::Null);
}

#[test]
fn a_single_player_challenge_game_has_no_vote() {
    let mut g = started(1, true);
    set_hand(&mut g, 0, &ALMA_HAND);
    g.place_tiles("p1", &alma()).unwrap();
    assert!(g.pending_challenge.is_none(), "nincs, aki megtámadja");
}

#[test]
fn multiplayer_challenge_creates_a_pending_vote() {
    let mut g = started(2, true);
    set_hand(&mut g, 0, &HAND7);
    let (msg, _) = g.place_tiles("p1", &ab()).unwrap();
    assert!(msg.contains("szavazásra vár"));
    assert!(g.pending_challenge.is_some());
    assert!(!hand(&g.players[0]).contains(&"A".to_string()) || g.players[0].hand.len() == 7);
    assert!(g.pending_challenge.as_ref().unwrap().votes.is_empty());
    let pc = g.get_state(None)["pending_challenge"].clone();
    assert_eq!(pc["player_name"], "Alice");
    assert_eq!(pc["tiles"].as_array().unwrap().len(), 2);
    assert_eq!(pc["player_count"], 2);
    // közben nem lehet lerakni / passzolni
    set_hand(&mut g, 1, &["C", "D", "E", "F", "G", "H", "I"]);
    assert!(g.place_tiles("p2", &[pl(8, 7, "C"), pl(9, 7, "D")]).unwrap_err().contains("Várj"));
    assert!(g.pass_turn("p2", false).is_err());
}

#[test]
fn two_player_accept_and_reject() {
    let mut g = started(2, true);
    set_hand(&mut g, 0, &HAND7);
    g.place_tiles("p1", &ab()).unwrap();
    let old = g.players[0].score;
    assert!(g.accept_pending_by_player("p1").is_err(), "a lerakó nem fogadhatja el");
    assert!(g.reject_pending_by_player("p1").is_err(), "a lerakó nem utasíthatja el");
    let (result, _) = g.accept_pending_by_player("p2").unwrap();
    assert_eq!(result.as_str(), "vote_accepted");
    assert!(g.pending_challenge.is_none());
    assert!(g.players[0].score > old);
    assert_eq!(g.board.get(7, 6).unwrap().letter, t("A"));
    assert_eq!(g.current_player().unwrap().id, "p2", "elfogadás után a másik jön");

    let mut g = started(2, true);
    set_hand(&mut g, 0, &HAND7);
    g.place_tiles("p1", &ab()).unwrap();
    assert_eq!(g.players[0].hand.len(), 5);
    let old = g.players[0].score;
    let (result, _) = g.reject_pending_by_player("p2").unwrap();
    assert_eq!(result.as_str(), "vote_rejected");
    assert!(g.pending_challenge.is_none());
    assert_eq!(g.players[0].score, old);
    assert!(g.board.get(7, 6).is_none() && g.board.get(7, 7).is_none());
    assert_eq!(g.players[0].hand.len(), 7, "a betűk visszakerülnek");
    assert_eq!(g.current_player().unwrap().id, "p1", "elutasítás után a lerakó újra jön");
}

#[test]
fn votes_without_a_pending_placement_fail() {
    let mut g = started(2, true);
    assert!(g.reject_pending_by_player("p2").is_err());
    assert!(g.accept_pending_by_player("p2").is_err());
    let mut g = started(1, true);
    assert!(g.accept_pending().is_err());
}

#[test]
fn timeout_accepts_for_non_voters() {
    let mut g = started(2, true);
    set_hand(&mut g, 0, &HAND7);
    g.place_tiles("p1", &ab()).unwrap();
    let old = g.players[0].score;
    let (result, _) = g.accept_pending().unwrap();
    assert_eq!(result.as_str(), "vote_accepted");
    assert!(g.pending_challenge.is_none() && g.players[0].score > old);
    let mut g = started(3, true);
    set_hand(&mut g, 0, &HAND7);
    g.place_tiles("p1", &ab()).unwrap();
    let (result, _) = g.accept_pending().unwrap();
    assert_eq!(result, VoteResult::Accepted);
    assert_eq!(g.board.get(7, 6).unwrap().letter, t("A"));
}

#[test]
fn three_and_four_player_votes() {
    // 3 játékos: mindketten elfogadnak
    let mut g = started(3, true);
    set_hand(&mut g, 0, &HAND7);
    g.place_tiles("p1", &ab()).unwrap();
    assert!(g.accept_pending_by_player("p1").is_err());
    g.accept_pending_by_player("p2").unwrap();
    let (result, _) = g.accept_pending_by_player("p3").unwrap();
    assert_eq!(result.as_str(), "vote_accepted");
    assert!(g.pending_challenge.is_none());
    assert!(g.players.iter().all(|p| !p.skip_next_turn), "nincs kör kihagyás büntetés");
    // 3 játékos: mindketten elutasítanak
    let mut g = started(3, true);
    set_hand(&mut g, 0, &HAND7);
    g.place_tiles("p1", &ab()).unwrap();
    g.reject_pending_by_player("p2").unwrap();
    let (result, _) = g.reject_pending_by_player("p3").unwrap();
    assert_eq!(result.as_str(), "vote_rejected");
    assert!(g.board.get(7, 6).is_none());
    assert!(hand(&g.players[0]).contains(&"A".to_string()) && hand(&g.players[0]).contains(&"B".to_string()));
    assert_eq!(g.current_player().unwrap().id, "p1");
    // 4 játékos: 1 elfogad, 2 elutasít → elutasítva
    let mut g = started(4, true);
    set_hand(&mut g, 0, &HAND7);
    g.place_tiles("p1", &ab()).unwrap();
    g.accept_pending_by_player("p2").unwrap();
    g.reject_pending_by_player("p3").unwrap();
    let (result, _) = g.reject_pending_by_player("p4").unwrap();
    assert_eq!(result.as_str(), "vote_rejected");
    // 4 játékos: mindenki elutasít
    let mut g = started(4, true);
    set_hand(&mut g, 0, &HAND7);
    g.place_tiles("p1", &ab()).unwrap();
    for id in ["p2", "p3", "p4"] {
        g.reject_pending_by_player(id).unwrap();
    }
    assert!(g.board.get(7, 6).is_none());
    assert_eq!(g.current_player().unwrap().id, "p1");
    // duplikált szavazat
    let mut g = started(4, true);
    set_hand(&mut g, 0, &HAND7);
    g.place_tiles("p1", &ab()).unwrap();
    g.accept_pending_by_player("p3").unwrap();
    assert!(g.reject_pending_by_player("p3").unwrap_err().contains("Már szavaztál"));
}

#[test]
fn removing_the_placer_clears_the_pending_vote() {
    let mut g = started(2, true);
    set_hand(&mut g, 0, &HAND7);
    g.place_tiles("p1", &ab()).unwrap();
    assert!(g.pending_challenge.is_some());
    g.remove_player("p1");
    assert!(g.pending_challenge.is_none());
    // a szavazó játékos indexe igazodik
    let mut g = started(3, true);
    set_hand(&mut g, 1, &HAND7);
    g.current_player_idx = 1;
    g.place_tiles("p2", &ab()).unwrap();
    assert_eq!(g.pending_challenge.as_ref().unwrap().player_idx, 1);
    g.remove_player("p1");
    assert_eq!(g.pending_challenge.as_ref().unwrap().player_idx, 0);
}

#[test]
fn reconnect_helpers() {
    let mut g = started(2, false);
    assert!(!g.players[0].disconnected);
    g.mark_disconnected("p1");
    assert!(g.players[0].disconnected && !g.players[1].disconnected);
    assert!(g.replace_player_sid("p1", "p1_new"));
    assert_eq!(g.players[0].id, "p1_new");
    assert!(!g.players[0].disconnected);
    let mut g = started(3, true);
    set_hand(&mut g, 0, &HAND7);
    g.place_tiles("p1", &ab()).unwrap();
    g.accept_pending_by_player("p2").unwrap();
    g.mark_disconnected("p2");
    g.replace_player_sid("p2", "p2_new");
    assert_eq!(g.players[1].id, "p2_new");
    assert_eq!(g.pending_challenge.as_ref().unwrap().votes.len(), 1, "a szavazat átkerül az új azonosítóra");
}

// ===================================================================================================
// Mentés / visszatöltés
// ===================================================================================================

#[test]
fn save_restore_roundtrip() {
    let mut g = Game::new("test-room", true, 0, 3);
    g.add_player("p1", "Alice").unwrap();
    g.add_player("p2", "Bob").unwrap();
    g.start().unwrap();
    set_hand(&mut g, 0, &HAND7);
    g.place_tiles("p1", &ab()).unwrap();
    g.accept_pending_by_player("p2").unwrap();
    let restored = Game::from_save_dict(&g.to_save_dict()).unwrap();
    assert_eq!(restored.id, "test-room");
    assert!(restored.challenge_mode && restored.started);
    assert_eq!(restored.current_player_idx, g.current_player_idx);
    assert_eq!(restored.turn_number, g.turn_number);
    assert_eq!(restored.players.len(), 2);
    assert_eq!(restored.players[0].name, "Alice");
    assert_eq!(restored.players[1].name, "Bob");
    assert_eq!(restored.players[0].score, g.players[0].score);
    assert_eq!(restored.board.get(7, 6).unwrap().letter, t("A"));
    assert_eq!(restored.board.get(7, 7).unwrap().letter, t("B"));
}

#[test]
fn save_restore_details() {
    let mut g = started(1, false);
    g.players[0].score = 42;
    g.players[0].skip_next_turn = true;
    let original: Vec<Tile> = g.players[0].hand.clone();
    let remaining = g.bag.remaining();
    let r = Game::from_save_dict(&g.to_save_dict()).unwrap();
    assert!(r.started && !r.finished && r.board.is_empty);
    assert_eq!(r.players[0].hand, original);
    assert_eq!(r.bag.remaining(), remaining);
    assert_eq!((r.players[0].score, r.players[0].skip_next_turn), (42, true));
    // a lejátszott játék nyertese
    let mut g = started(2, false);
    g.finished = true;
    g.winners = vec![g.players[0].clone()];
    let r = Game::from_save_dict(&g.to_save_dict()).unwrap();
    assert_eq!(r.winner().unwrap().name, "Alice");
    // pont nélküli körök
    let mut g = started(2, false);
    g.pass_turn("p1", false).unwrap();
    g.pass_turn("p2", false).unwrap();
    assert_eq!(Game::from_save_dict(&g.to_save_dict()).unwrap().scoreless_turns, 2);
    // régi mentés a játékosonkénti passz-sorozatból
    let mut data = started(2, false).to_save_dict();
    data.as_object_mut().unwrap().remove("scoreless_turns");
    data["players"][0]["consecutive_passes"] = json!(2);
    data["players"][1]["consecutive_passes"] = json!(1);
    assert_eq!(Game::from_save_dict(&data).unwrap().scoreless_turns, 2);
}

#[test]
fn save_during_a_pending_vote_returns_the_tiles() {
    let mut g = started(3, true);
    set_hand(&mut g, 0, &ALMA_HAND);
    g.place_tiles("p1", &[pl(7, 7, "A"), pl(7, 8, "L")]).unwrap();
    assert_eq!(g.players[0].hand.len(), 5);
    let saved = g.to_save_dict();
    assert_eq!(saved["players"][0]["hand"].as_array().unwrap().len(), 7);
    let restored = Game::from_save_dict(&saved).unwrap();
    assert_eq!(hand(&restored.players[0]), sorted(&ALMA_HAND));
    assert!(restored.board.is_empty);
    assert_eq!(restored.current_player().unwrap().id, "p1");
    assert_eq!(total_tiles(&restored), TOTAL);
    // az élő játékot a mentés nem változtatja
    assert_eq!(g.players[0].hand.len(), 5);
    assert!(g.pending_challenge.is_some());
    assert!(started(3, false).to_save_dict()["players"].as_array().unwrap().iter().all(|p| p["hand"].as_array().unwrap().len() == HAND_SIZE));
}

#[test]
fn turn_time_limit_is_kept() {
    let mut g = Game::new("t", false, 90, 3);
    g.add_player("p1", "Alice").unwrap();
    g.start().unwrap();
    assert_eq!(g.get_state(Some("p1"))["turn_time_limit"], 90);
    assert_eq!(Game::with_defaults("t").turn_time_limit, 0);
    let mut g = Game::new("t", false, 180, 3);
    g.add_player("p1", "Alice").unwrap();
    g.start().unwrap();
    let mut saved = g.to_save_dict();
    assert_eq!(saved["turn_time_limit"], 180);
    assert_eq!(Game::from_save_dict(&saved).unwrap().turn_time_limit, 180);
    saved.as_object_mut().unwrap().remove("turn_time_limit");
    assert_eq!(Game::from_save_dict(&saved).unwrap().turn_time_limit, 0);
}

// ===================================================================================================
// Játék vége
// ===================================================================================================

#[test]
fn end_of_game_hand_penalties() {
    let mut g = started(2, false);
    g.players[0].hand = vec![];
    g.players[0].score = 100;
    set_hand(&mut g, 1, &["A", "B"]);
    g.players[1].score = 80;
    g.bag.tiles.clear();
    g.end_game(Some(0));
    assert!(g.finished);
    assert_eq!(g.players[1].score, 77);
    assert_eq!(g.players[0].score, 103);
    assert_eq!(g.winner().unwrap().name, "Alice");
    // senki nem fejezte be: mindenki veszít, bónusz nincs
    let mut g = started(2, false);
    set_hand(&mut g, 0, &["A"]);
    g.players[0].score = 50;
    set_hand(&mut g, 1, &["B"]);
    g.players[1].score = 60;
    g.end_game(None);
    assert_eq!((g.players[0].score, g.players[1].score), (49, 58));
    assert_eq!(g.winner().unwrap().name, "Bob");
    // a joker nem von le
    let mut g = started(1, false);
    set_hand(&mut g, 0, &[""]);
    g.players[0].score = 50;
    g.end_game(None);
    assert_eq!(g.players[0].score, 50);
}

fn tied(names: usize, scores: &[i32]) -> Game {
    let mut g = started(names, false);
    for (p, s) in g.players.iter_mut().zip(scores) {
        p.score = *s;
        p.hand.clear();
    }
    g.end_game(None);
    g
}

#[test]
fn draws_make_every_top_scorer_a_winner() {
    let g = tied(2, &[50, 50]);
    assert_eq!(g.winners.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Alice", "Bob"]);
    assert!(g.winner().is_none());
    assert_eq!(g.last_action_info.clone().unwrap(), json!({"type": "game_over_draw", "players": ["Alice", "Bob"], "score": 50}));
    assert!(g.last_action.clone().unwrap().contains("Döntetlen"));
    let g = tied(3, &[40, 70, 70]);
    assert_eq!(g.winners.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Bob", "Charlie"]);
    let g = tied(2, &[10, 30]);
    assert_eq!(g.winners.len(), 1);
    assert_eq!(g.winner().unwrap().name, "Bob");
    assert_eq!(g.last_action_info.clone().unwrap()["type"], "game_over");
    // a levonások utáni pontszám dönt
    let mut g = started(2, false);
    g.players[0].score = 51;
    set_hand(&mut g, 0, &["A"]);
    g.players[1].score = 50;
    g.players[1].hand.clear();
    g.end_game(None);
    assert_eq!(g.winners.len(), 2);
    // az állapot és a mentés őrzi a döntetlent
    let g = tied(2, &[20, 20]);
    let state = g.get_state(None);
    assert_eq!(state["winner"], Value::Null);
    assert_eq!(state["winners"].as_array().unwrap().iter().map(|w| w["name"].as_str().unwrap()).collect::<Vec<_>>(), vec!["Alice", "Bob"]);
    let restored = Game::from_save_dict(&g.to_save_dict()).unwrap();
    assert_eq!(restored.winners.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Alice", "Bob"]);
    // régi mentés egyetlen győztes névvel
    let g = tied(2, &[5, 9]);
    let mut data = g.to_save_dict();
    data.as_object_mut().unwrap().remove("winner_names");
    data["winner_name"] = json!("Bob");
    assert_eq!(Game::from_save_dict(&data).unwrap().winner().unwrap().name, "Bob");
}

#[test]
fn scoreless_turns_end_the_game() {
    // csere is pont nélküli kör
    let mut g = started(2, false);
    for _ in 0..6 {
        assert!(!g.finished);
        let id = g.current_player().unwrap().id.clone();
        g.exchange_tiles(&id, &[0]).unwrap();
    }
    assert!(g.finished);
    assert_eq!(g.scoreless_turns, 6);
    let id = g.current_player().unwrap().id.clone();
    assert!(g.exchange_tiles(&id, &[0]).unwrap_err().contains("véget ért"));
    // vegyes passz és csere
    let mut g = started(2, false);
    for action in ["pass", "exchange", "pass", "exchange", "pass"] {
        let id = g.current_player().unwrap().id.clone();
        if action == "pass" {
            g.pass_turn(&id, false).unwrap();
        } else {
            g.exchange_tiles(&id, &[0]).unwrap();
        }
    }
    assert!(!g.finished);
    let id = g.current_player().unwrap().id.clone();
    g.pass_turn(&id, false).unwrap();
    assert!(g.finished);
    // a pontot érő lerakás nullázza a sorozatot
    let mut g = started(2, false);
    for _ in 0..5 {
        let id = g.current_player().unwrap().id.clone();
        g.pass_turn(&id, false).unwrap();
    }
    assert_eq!(g.scoreless_turns, 5);
    let idx = g.current_player_idx;
    set_hand(&mut g, idx, &ALMA_HAND);
    let id = g.current_player().unwrap().id.clone();
    g.place_tiles(&id, &alma()).unwrap();
    assert_eq!(g.scoreless_turns, 0);
    assert!(!g.finished);
}

#[test]
fn rejected_placements_count_as_scoreless_turns() {
    let mut g = started(2, true);
    for i in 1..=6 {
        assert!(!g.finished);
        assert_eq!(g.current_player().unwrap().id, "p1", "elutasításnál a lerakó újra jön");
        set_hand(&mut g, 0, &HAND7);
        g.place_tiles("p1", &ab()).unwrap();
        g.rejected_placements.clear();
        let (result, _) = g.reject_pending_by_player("p2").unwrap();
        assert_eq!(result, VoteResult::Rejected);
        assert_eq!(g.scoreless_turns, i);
    }
    assert!(g.finished);
    assert!(g.pending_challenge.is_none());
    // a játékot lezáró elutasítás után a visszakapott betűk a kézben maradnak
    let mut g = started(2, true);
    g.scoreless_turns = 5;
    set_hand(&mut g, 0, &HAND7);
    g.place_tiles("p1", &ab()).unwrap();
    g.reject_pending_by_player("p2").unwrap();
    assert!(g.finished);
    assert_eq!(hand(&g.players[0]), sorted(&HAND7));
}

// ===================================================================================================
// place_tiles: szélső esetek
// ===================================================================================================

#[test]
fn place_tiles_edge_cases() {
    let mut g = started(1, false);
    g.finished = true;
    assert!(g.place_tiles("p1", &[pl(7, 7, "A")]).unwrap_err().contains("véget ért"));
    let mut g = started(2, false);
    assert!(g.place_tiles("p2", &[pl(7, 7, "A")]).unwrap_err().contains("Nem te"));
    let mut g = started(1, false);
    set_hand(&mut g, 0, &HAND7);
    assert!(g.place_tiles("p1", &[pl(7, 6, "Z"), pl(7, 7, "A")]).unwrap_err().contains("Nincs"));
    // joker: üres zseton kell a kézben
    let mut g = started(1, false);
    set_hand(&mut g, 0, &["", "L", "M", "A", "K", "E", "T"]);
    assert!(g.place_tiles("p1", &[blank(7, 5, "A"), pl(7, 6, "L"), pl(7, 7, "M"), pl(7, 8, "A")]).is_ok());
    let mut g = started(1, false);
    set_hand(&mut g, 0, &ALMA_HAND);
    assert!(g.place_tiles("p1", &[blank(7, 6, "Z"), pl(7, 7, "A")]).unwrap_err().contains("üres"));
}

#[test]
fn placing_all_seven_tiles_gives_the_bonus() {
    let mut g = started(1, false);
    set_hand(&mut g, 0, &["A", "L", "M", "A", "K", "E", "T"]);
    // KALAMAT nem szó; megtámadásos több játékosos játékban a szótár nem dönt
    let mut g2 = started(2, true);
    set_hand(&mut g2, 0, &HAND7);
    let tiles: Vec<Placed> = (4..11).map(|c| pl(7, c, HAND7[(c - 4) as usize])).collect();
    let (_, score) = g2.place_tiles("p1", &tiles).unwrap();
    assert!(score >= BONUS_ALL_TILES, "{score}");
    let _ = &mut g;
}

#[test]
fn placing_draws_new_tiles_and_resets_the_streak() {
    let mut g = started(2, false);
    g.pass_turn("p1", false).unwrap();
    assert_eq!(g.scoreless_turns, 1);
    g.pass_turn("p2", false).unwrap();
    set_hand(&mut g, 0, &ALMA_HAND);
    let remaining = g.bag.remaining();
    g.place_tiles("p1", &alma()).unwrap();
    assert_eq!(g.scoreless_turns, 0);
    assert_eq!(g.players[0].hand.len(), 7);
    assert_eq!(g.bag.remaining(), remaining - 4);
}

#[test]
fn the_move_log() {
    let mut g = started(1, false);
    set_hand(&mut g, 0, &ALMA_HAND);
    g.place_tiles("p1", &alma()).unwrap();
    assert_eq!(g.move_log.len(), 1);
    assert_eq!((g.move_log[0].player_name.as_str(), g.move_log[0].action_type.as_str(), g.move_log[0].move_number), ("Alice", "place", 1));
    let snapshot: Value = serde_json::from_str(&g.move_log[0].board_snapshot_json()).unwrap();
    assert_eq!(snapshot.as_array().unwrap().len(), 15);
    let mut g = started(1, false);
    g.pass_turn("p1", false).unwrap();
    assert_eq!(g.move_log[0].action_type, "pass");
    let mut g = started(1, false);
    g.exchange_tiles("p1", &[0]).unwrap();
    assert_eq!(g.move_log[0].action_type, "exchange");
    let mut g = started(2, true);
    set_hand(&mut g, 0, &HAND7);
    g.place_tiles("p1", &ab()).unwrap();
    g.accept_pending_by_player("p2").unwrap();
    assert!(g.move_log.iter().any(|m| m.action_type == "challenge_accept"));
    let mut g = started(2, true);
    set_hand(&mut g, 0, &HAND7);
    g.place_tiles("p1", &ab()).unwrap();
    g.reject_pending_by_player("p2").unwrap();
    assert!(g.move_log.iter().any(|m| m.action_type == "challenge_reject"));
}

// ===================================================================================================
// játékosok eltávolítása, kihagyás
// ===================================================================================================

#[test]
fn removing_players_adjusts_the_turn_index() {
    let mut g = started(3, false);
    g.remove_player("p1");
    assert!(g.current_player_idx < g.players.len());
    let mut g = started(3, false);
    g.current_player_idx = 2;
    g.remove_player("p1");
    assert_eq!(g.current_player_idx, 1);
    assert_eq!(g.current_player().unwrap().name, "Charlie");
    let mut g = started(3, false);
    g.remove_player("p3");
    assert_eq!(g.current_player_idx, 0);
    assert_eq!(g.current_player().unwrap().name, "Alice");
    let mut g = started(1, false);
    g.remove_player("nincs");
    assert_eq!(g.players.len(), 1);
    g.remove_player("p1");
    assert!(g.players.is_empty());
    assert_eq!(g.current_player_idx, 0);
    let g = started(1, false);
    assert!(g.find_player("p1").is_some() && g.find_player("p_none").is_none());
}

#[test]
fn skip_next_turn_is_honoured() {
    let mut g = started(3, false);
    g.players[1].skip_next_turn = true;
    g.pass_turn("p1", false).unwrap();
    assert_eq!(g.current_player().unwrap().id, "p3");
    assert!(!g.players[1].skip_next_turn);
}

#[test]
fn disconnected_players_and_passes() {
    let mut g = started(3, false);
    g.mark_disconnected("p3");
    for _ in 0..10 {
        if g.finished {
            break;
        }
        let id = g.current_player().unwrap().id.clone();
        g.pass_turn(&id, false).unwrap();
    }
    assert!(g.finished, "egy lecsatlakozott játékossal is véget ér hat pont nélküli kör után");
    let mut g = started(3, false);
    g.mark_disconnected("p3");
    g.pass_turn("p1", false).unwrap();
    g.pass_turn("p2", false).unwrap();
    assert!(!g.finished);
}

#[test]
fn skipping_a_disconnected_current_player() {
    let mut g = started(3, false);
    g.current_player_idx = 1;
    g.mark_disconnected("p2");
    assert!(g.skip_disconnected_current());
    assert_eq!(g.current_player().unwrap().id, "p3");
    let mut g = started(3, false);
    assert!(!g.skip_disconnected_current());
    assert_eq!(g.current_player().unwrap().id, "p1");
    let mut g = started(3, false);
    for id in ["p1", "p2", "p3"] {
        g.mark_disconnected(id);
    }
    assert!(!g.skip_disconnected_current());
    // szavazás közben a lezárás adja tovább a kört
    let mut g = started(3, true);
    set_hand(&mut g, 0, &HAND7);
    g.place_tiles("p1", &[pl(7, 7, "A"), pl(7, 8, "B")]).unwrap();
    g.mark_disconnected("p1");
    assert!(!g.skip_disconnected_current());
    assert_eq!(g.current_player().unwrap().id, "p1");
    g.accept_pending_by_player("p2").unwrap();
    g.accept_pending_by_player("p3").unwrap();
    assert_eq!(g.current_player().unwrap().id, "p2", "nem ugrik át egy játékost");
}

#[test]
fn rejecting_a_disconnected_placers_word() {
    let mut g = started(3, true);
    set_hand(&mut g, 0, &ALMA_HAND);
    g.place_tiles("p1", &[pl(7, 7, "A"), pl(7, 8, "L")]).unwrap();
    g.mark_disconnected("p1");
    g.reject_pending_by_player("p2").unwrap();
    g.reject_pending_by_player("p3").unwrap();
    assert!(g.pending_challenge.is_none());
    assert_eq!(hand(&g.players[0]), sorted(&ALMA_HAND));
    assert_eq!(g.current_player().unwrap().id, "p2", "a lecsatlakozott lerakó helyett a következő jön");
    let mut g = started(3, true);
    set_hand(&mut g, 0, &ALMA_HAND);
    g.place_tiles("p1", &[pl(7, 7, "A"), pl(7, 8, "L")]).unwrap();
    g.reject_pending_by_player("p2").unwrap();
    g.reject_pending_by_player("p3").unwrap();
    assert_eq!(g.current_player().unwrap().id, "p1");
}

// ===================================================================================================
// Visszavonás
// ===================================================================================================

fn pending(players: usize) -> (Game, String) {
    let mut g = started(players, true);
    let idx = g.current_player_idx;
    set_hand(&mut g, idx, &HAND7);
    let id = g.current_player().unwrap().id.clone();
    g.place_tiles(&id, &ab()).unwrap();
    assert!(g.pending_challenge.is_some());
    (g, id)
}

#[test]
fn withdrawing_a_pending_placement() {
    let (mut g, id) = pending(2);
    let idx = g.current_player_idx;
    assert_eq!(hand(&g.players[idx]), sorted(&["C", "D", "E", "F", "G"]));
    g.withdraw_pending(&id).unwrap();
    assert!(g.pending_challenge.is_none());
    assert_eq!(hand(&g.players[idx]), sorted(&HAND7));
    assert_eq!(g.current_player().unwrap().id, id);
    assert!(g.board.is_empty);
    assert_eq!(g.last_action_info.clone().unwrap()["type"], "withdrawn");
    // nem pont nélküli kör, nincs a naplóban
    let (mut g, id) = pending(2);
    g.scoreless_turns = 3;
    g.withdraw_pending(&id).unwrap();
    assert_eq!(g.scoreless_turns, 3);
    assert!(g.move_log.is_empty());
    // utána újra lerakhat
    let (mut g, id) = pending(2);
    g.withdraw_pending(&id).unwrap();
    assert!(g.place_tiles(&id, &[pl(7, 6, "C"), pl(7, 7, "D")]).is_ok());
    assert!(g.pending_challenge.is_some());
}

#[test]
fn only_the_placer_may_withdraw_and_not_after_a_vote() {
    let (mut g, id) = pending(2);
    let other = g.players.iter().find(|p| p.id != id).unwrap().id.clone();
    assert!(g.withdraw_pending(&other).unwrap_err().contains("lerakó"));
    assert!(g.pending_challenge.is_some());
    let (mut g, id) = pending(3);
    let voter = g.players.iter().find(|p| p.id != id).unwrap().id.clone();
    g.accept_pending_by_player(&voter).unwrap();
    assert!(g.withdraw_pending(&id).unwrap_err().contains("szavaztak"));
    let mut g = started(2, true);
    assert!(g.withdraw_pending("p1").unwrap_err().contains("Nincs"));
}
