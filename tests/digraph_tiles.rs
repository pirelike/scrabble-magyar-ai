//! Kétjegyű betűk (SZ, CS, GY, LY, NY, TY, ZS): csak a saját zsetonjukkal rakhatók ki, külön S + Z-ből nem (a Python
//! `test_digraph_tiles.py` megfelelője).

mod common;
use common::*;
use scrabble::ai::{Vocabulary, generate_moves};
use scrabble::board::{Board, CENTER, Placed};
use scrabble::game::Game;
use scrabble::tiles::{Tile, forms_digraph};
use serde_json::json;

const DIGRAPHS: [&str; 7] = ["CS", "GY", "LY", "NY", "SZ", "TY", "ZS"];

fn t(letter: &str) -> Tile {
    Tile::from_str(letter).unwrap_or_else(|| panic!("ismeretlen zseton: {letter}"))
}

/// Egy sor zseton: a `letters` elemei egy-egy zseton (a kétjegyű betű egy elem).
fn row(letters: &[&str]) -> Vec<Placed> {
    letters.iter().enumerate().map(|(i, l)| Placed::new(CENTER, CENTER + i as i32, t(l), false)).collect()
}

fn splits() -> Vec<(String, String)> {
    DIGRAPHS.iter().map(|d| (d[..1].to_string(), d[1..].to_string())).collect()
}

// ===================================================================================================
// forms_digraph
// ===================================================================================================

#[test]
fn the_seven_digraphs() {
    let mut pairs = splits();
    pairs.sort();
    let expected: Vec<(String, String)> =
        [("C", "S"), ("G", "Y"), ("L", "Y"), ("N", "Y"), ("S", "Z"), ("T", "Y"), ("Z", "S")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    let mut expected = expected;
    expected.sort();
    assert_eq!(pairs, expected);
    for (first, second) in &pairs {
        assert!(forms_digraph(first, second), "{first}{second}");
    }
}

#[test]
fn ordinary_pairs_and_digraph_tiles_do_not_count() {
    assert!(!forms_digraph("A", "S"));
    assert!(!forms_digraph("S", "A"));
    assert!(!forms_digraph("Z", "Z"));
    assert!(!forms_digraph("S", "SZ")); // a második egy kétjegyű zseton
    assert!(!forms_digraph("Z", "SZ"));
    assert!(!forms_digraph("SZ", "Z"));
    assert!(!forms_digraph("", "Z")); // a szó elején nincs előző zseton
}

// ===================================================================================================
// A tábla
// ===================================================================================================

#[test]
fn two_new_single_tiles_are_rejected() {
    // csak a SZ, CS, ZS pár fordulhat elő valóban: önálló Y zseton nincs (a GY, LY, NY, TY második betűje nem külön zseton)
    for (first, second) in splits().into_iter().filter(|(a, b)| Tile::from_str(a).is_some() && Tile::from_str(b).is_some()) {
        let error = Board::new().validate_placement(&row(&["A", &first, &second, "A"]), true).expect_err(&format!("{first}{second}"));
        assert!(error.contains(&format!("{first}{second}")) && error.contains("saját zsetonjával"), "{error}");
    }
}

#[test]
fn the_digraph_tile_itself_is_fine() {
    for digraph in DIGRAPHS {
        let words = Board::new().validate_placement(&row(&["A", digraph, "A"]), true).expect(digraph);
        assert_eq!(words[0].word, format!("A{digraph}A"));
    }
}

fn board_with(cells: &[(i32, i32, &str)]) -> Board {
    let mut board = Board::new();
    for (r, c, l) in cells {
        board.set(*r as usize, *c as usize, t(l), false);
    }
    board.is_empty = false;
    board
}

#[test]
fn a_new_tile_next_to_an_old_one() {
    let board = board_with(&[(CENTER, CENTER, "S")]);
    let error = board.validate_placement(&[Placed::new(CENTER, CENTER + 1, t("Z"), false)], true).unwrap_err();
    assert!(error.contains("SZ"), "{error}");
}

#[test]
fn cross_word_pairs_are_checked_too() {
    let board = board_with(&[(CENTER, CENTER, "S")]);
    // a függőleges lerakás egyetlen betűje (a vízszintes keresztszóban) Z-vel zárul az S után
    let placed = [Placed::new(CENTER - 1, CENTER + 1, t("A"), false), Placed::new(CENTER, CENTER + 1, t("Z"), false)];
    let error = board.validate_placement(&placed, true).unwrap_err();
    assert!(error.contains("SZ"), "{error}");
}

#[test]
fn a_blank_tile_counts_as_its_letter() {
    let placed = [Placed::new(CENTER, CENTER, t("S"), true), Placed::new(CENTER, CENTER + 1, t("Z"), false)];
    let error = Board::new().validate_placement(&placed, true).unwrap_err();
    assert!(error.contains("SZ"), "{error}");
}

#[test]
fn a_single_tile_beside_a_digraph_tile_is_allowed() {
    // vízszint: Z + SZ, asszony: S + SZ — az egyik zseton kétjegyű, tehát nem két külön zseton alkotja
    for letters in [vec!["Z", "SZ", "I"], vec!["A", "S", "SZ", "O", "NY"], vec!["ZS", "Z", "A"]] {
        Board::new().validate_placement(&row(&letters), true).unwrap_or_else(|e| panic!("{letters:?}: {e}"));
    }
}

#[test]
fn old_pairs_do_not_block_the_game() {
    // a régi (megengedőbb) szabállyal lerakott S + Z pár az állásban marad, a lerakás folytatható
    let board = board_with(&[(CENTER, CENTER, "S"), (CENTER, CENTER + 1, "Z")]);
    let words = board.validate_placement(&[Placed::new(CENTER, CENTER + 2, t("A"), false)], true).unwrap();
    assert_eq!(words[0].word, "SZA");
}

#[test]
fn the_dictionary_still_decides_the_rest() {
    // az S + Z elutasítása megelőzi a szótárat; a szótár-ellenőrzés is lefut, ha nincs ilyen pár
    scrabble::dictionary::warm_up();
    let error = Board::new().validate_placement(&row(&["K", "K", "K"]), false).unwrap_err();
    assert!(error.contains("Érvénytelen szó"), "{error}");
}

// ===================================================================================================
// A játék
// ===================================================================================================

fn game_with(hand: &[&str], challenge: bool) -> Game {
    let mut game = Game::new("t", challenge, 0, 3);
    game.add_player("a", "A").unwrap();
    game.add_player("b", "B").unwrap();
    game.start().unwrap();
    let idx = game.current_player_idx;
    game.players[idx].hand = hand.iter().map(|l| t(l)).collect();
    game
}

fn current_id(game: &Game) -> String {
    game.current_player().unwrap().id.clone()
}

#[test]
fn place_tiles_refuses_a_split_digraph() {
    scrabble::dictionary::warm_up();
    let mut game = game_with(&["S", "Z", "Ó", "K", "A", "L", "M"], false);
    let id = current_id(&game);
    let error = game.place_tiles(&id, &row(&["S", "Z", "Ó"])).unwrap_err();
    assert!(error.contains("SZ"), "{error}");
    assert!(game.board.is_empty);
    assert_eq!(game.current_player().unwrap().hand.len(), 7);
}

#[test]
fn place_tiles_accepts_the_digraph_tile() {
    scrabble::dictionary::warm_up();
    let mut game = game_with(&["SZ", "Ó", "K", "A", "L", "M", "T"], false);
    let id = current_id(&game);
    let (_, score) = game.place_tiles(&id, &row(&["SZ", "Ó"])).unwrap();
    assert_eq!(score, (3 + 2) * 2); // a középső csillag dupla szó
}

#[test]
fn the_preview_explains_why() {
    scrabble::dictionary::warm_up();
    let game = game_with(&["S", "Z", "Ó", "K", "A", "L", "M"], false);
    let preview = game.preview_placement(&current_id(&game), &row(&["S", "Z", "Ó"]));
    assert_eq!(preview["valid"], false);
    assert!(preview["message"].as_str().unwrap().contains("SZ"), "{preview}");
}

#[test]
fn challenge_mode_applies_the_rule_without_a_dictionary() {
    let mut game = game_with(&["S", "Z", "Ó", "K", "A", "L", "M"], true);
    let id = current_id(&game);
    let error = game.place_tiles(&id, &row(&["S", "Z", "Ó"])).unwrap_err();
    assert!(error.contains("SZ"), "{error}");
    assert!(game.pending_challenge.is_none());
}

// ===================================================================================================
// A robot
// ===================================================================================================

fn vocab(words: &[&str]) -> Vocabulary {
    Vocabulary::new(words.iter().map(|w| w.to_string()))
}

fn rack(letters: &[&str]) -> Vec<Tile> {
    letters.iter().map(|l| if l.is_empty() { Tile::BLANK } else { t(l) }).collect()
}

#[test]
fn no_split_digraph_move_is_generated() {
    let moves = generate_moves(&Board::new(), &rack(&["S", "Z", "Ó", "K"]), &vocab(&["SZÓ", "ÓZ", "KÓ"]), false, 7, 5.0);
    assert!(!moves.iter().any(|m| m.words == vec!["SZÓ".to_string()]));
    assert!(moves.iter().any(|m| m.words == vec!["KÓ".to_string()]));
}

#[test]
fn the_digraph_tile_move_is_still_found() {
    let moves = generate_moves(&Board::new(), &rack(&["SZ", "Ó", "S", "Z"]), &vocab(&["SZÓ", "ÓZ"]), false, 7, 5.0);
    let szo: Vec<_> = moves.iter().filter(|m| m.words == vec!["SZÓ".to_string()]).collect();
    assert!(!szo.is_empty());
    assert!(szo.iter().all(|m| m.tiles.len() == 2 && ["SZ", "Ó"].contains(&m.tiles[0].letter.as_str())));
}

#[test]
fn a_blank_cannot_complete_a_digraph_either() {
    let moves = generate_moves(&Board::new(), &rack(&["S", "", "Ó"]), &vocab(&["SZÓ"]), true, 7, 5.0);
    // a joker SZ-ként (egy zseton) rendben van, Z-ként az S mellett nem
    let szo: Vec<_> = moves.iter().filter(|m| m.words == vec!["SZÓ".to_string()]).collect();
    assert!(!szo.is_empty());
    assert!(szo.iter().all(|m| m.tiles.len() == 2));
}

// ===================================================================================================
// Socket.IO: a lerakás és az előnézet is ugyanazt az üzenetet adja
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn place_and_preview_refuse_a_split_digraph_and_keep_the_hand() {
    let server = TestServer::start().await;
    let (a, b) = started_pair(&server, json!({"name": "Room", "max_players": 4})).await;
    let cur = current(&[&a, &b]);
    let name = format!("Jatekos{}", cur.name_n);
    let hand = ["S", "Z", "Ó", "K", "A", "L", "M"];
    set_hand(&server, &name, &hand);
    let tiles = json!([
        {"row": 7, "col": 7, "letter": "S", "is_blank": false},
        {"row": 7, "col": 8, "letter": "Z", "is_blank": false},
        {"row": 7, "col": 9, "letter": "Ó", "is_blank": false},
    ]);
    let events = cur.call("preview_move", json!({"tiles": tiles})).await;
    let preview = find(&events, "move_preview").expect("előnézet");
    assert_eq!(preview["valid"], false);
    assert!(preview["message"].as_str().unwrap().contains("saját zsetonjával"), "{preview}");

    let events = cur.call("place_tiles", json!({"tiles": tiles})).await;
    let result = find(&events, "action_result").expect("eredmény");
    assert_eq!(result["success"], false);
    assert!(result["message"].as_str().unwrap().contains("SZ"), "{result}");
    assert_eq!(hand_of(&server, &name), hand.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert!(with_room(&server, |room| room.game.board.is_empty));
}
