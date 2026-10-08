//! Robotok a játékmodellben, szerkezetes utolsó akció, lépéstörténet, előnézet (a Python `test_bots.py` játékmodell
//! része).

use scrabble::ai::Difficulty;
use scrabble::board::Placed;
use scrabble::game::Game;
use scrabble::tiles::Tile;
use serde_json::{Value, json};

fn t(letter: &str) -> Tile {
    Tile::from_str(letter).unwrap_or_else(|| panic!("ismeretlen zseton: {letter}"))
}

fn set_hand(g: &mut Game, idx: usize, letters: &[&str]) {
    g.players[idx].hand = letters.iter().map(|l| t(l)).collect();
}

fn word_at(row: i32, col: i32, letters: &[&str]) -> Vec<Placed> {
    letters.iter().enumerate().map(|(i, l)| Placed::new(row, col + i as i32, t(l), false)).collect()
}

const ALMAKOR: [&str; 7] = ["A", "L", "M", "A", "K", "Ö", "R"];
const ALMA: [&str; 4] = ["A", "L", "M", "A"];

fn with_human(challenge: bool) -> Game {
    let mut g = Game::new("g", challenge, 0, 3);
    g.add_player("H", "Human").unwrap();
    g
}

fn level(g: &Game, idx: usize) -> Option<Difficulty> {
    g.players[idx].difficulty
}

// ---------------------------------------------------------------------------------------------------
// játékmodell
// ---------------------------------------------------------------------------------------------------

#[test]
fn adding_bots() {
    let mut g = with_human(false);
    g.add_bot("Robi", &json!("easy")).unwrap();
    assert!(g.players[1].is_bot);
    assert_eq!(level(&g, 1), Some(Difficulty::Level(3)), "a régi 'easy' a 3. fokozat");
    assert!(g.players[1].id.starts_with("bot-"));
    let mut g = with_human(false);
    g.add_bot("Robi", &json!(8)).unwrap();
    assert_eq!(level(&g, 1), Some(Difficulty::Level(8)));
    // azonosítók egyediek
    let mut g = with_human(false);
    g.add_bot("A", &json!("easy")).unwrap();
    g.add_bot("B", &json!("hard")).unwrap();
    let ids: std::collections::HashSet<&String> = g.players.iter().map(|p| &p.id).collect();
    assert_eq!(ids.len(), 3);
}

#[test]
fn bot_limits() {
    let mut g = with_human(false);
    for i in 0..3 {
        assert!(g.add_bot(&format!("B{i}"), &json!("easy")).is_ok());
    }
    assert!(g.add_bot("Extra", &json!("easy")).unwrap_err().contains("Maximum"));
    let mut g = with_human(false);
    g.start().unwrap();
    assert!(g.add_bot("Late", &json!("easy")).is_err());
}

#[test]
fn human_helpers_and_state() {
    let mut g = with_human(false);
    g.add_bot("Robi", &json!("hard")).unwrap();
    assert_eq!(g.human_players().iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Human"]);
    assert!(g.has_connected_human());
    g.mark_disconnected("H");
    assert!(!g.has_connected_human());
    let mut g = with_human(false);
    g.add_bot("Robi", &json!("hard")).unwrap();
    g.start().unwrap();
    let states = g.get_all_states();
    assert_eq!(states.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), vec!["H"], "robotnak nem készül állapot");
    let bot = states[0].1["players"].as_array().unwrap().iter().find(|p| p["is_bot"] == true).unwrap().clone();
    assert_eq!(bot["difficulty"], 10);
    assert!(bot.get("hand").is_none());
    assert_eq!(bot["hand_count"], 7);
}

#[test]
fn bots_do_not_vote() {
    let mut g = Game::new("g", true, 0, 3);
    g.add_player("A", "A").unwrap();
    g.add_player("B", "B").unwrap();
    g.add_bot("Robi", &json!("easy")).unwrap();
    g.start().unwrap();
    set_hand(&mut g, 0, &ALMAKOR);
    g.place_tiles("A", &word_at(7, 7, &ALMA)).unwrap();
    assert!(g.pending_challenge.is_some());
    assert_eq!(g.voter_ids(), ["B".to_string()].into_iter().collect());
}

#[test]
fn no_vote_against_bots_only_the_dictionary_decides() {
    let mut g = Game::new("g", true, 0, 3);
    g.add_player("A", "A").unwrap();
    g.add_bot("Robi", &json!("easy")).unwrap();
    g.start().unwrap();
    set_hand(&mut g, 0, &["Ü", "Ö", "Ó", "Ú", "A", "L", "M"]);
    let msg = g.place_tiles("A", &word_at(7, 7, &["Ü", "Ö"])).unwrap_err();
    assert!(msg.contains("Érvénytelen szó"), "{msg}");
    assert!(g.pending_challenge.is_none());
    // érvényes szó azonnal lekerül, és a robot jön
    set_hand(&mut g, 0, &ALMAKOR);
    let (_, score) = g.place_tiles("A", &word_at(7, 7, &ALMA)).unwrap();
    assert!(score > 0);
    assert!(g.pending_challenge.is_none());
    assert!(g.current_player().unwrap().is_bot);
}

#[test]
fn bots_survive_save_and_restore() {
    let mut g = with_human(false);
    g.add_bot("Rezső", &json!("hard")).unwrap();
    g.start().unwrap();
    let data: Value = serde_json::from_str(&g.to_save_dict().to_string()).unwrap();
    let restored = Game::from_save_dict(&data).unwrap();
    let bot = &restored.players[1];
    assert!(bot.is_bot && bot.name == "Rezső");
    assert_eq!(bot.difficulty, Some(Difficulty::Level(10)));
    assert!(!bot.disconnected);
    // minden fokozat megmarad
    let mut g = with_human(false);
    for level in [1, 5, 8] {
        g.add_bot(&format!("B{level}"), &json!(level)).unwrap();
    }
    g.start().unwrap();
    let restored = Game::from_save_dict(&g.to_save_dict()).unwrap();
    assert_eq!(
        restored.players[1..].iter().map(|p| p.difficulty).collect::<Vec<_>>(),
        vec![Some(Difficulty::Level(1)), Some(Difficulty::Level(5)), Some(Difficulty::Level(8))]
    );
}

#[test]
fn old_saves_load_on_the_new_scale() {
    let mut g = with_human(false);
    for (name, level) in [("A", "easy"), ("B", "medium"), ("C", "hard")] {
        g.add_bot(name, &json!(level)).unwrap();
    }
    g.start().unwrap();
    let mut data = g.to_save_dict();
    for (pd, old) in data["players"].as_array_mut().unwrap().iter_mut().skip(1).zip(["easy", "medium", "hard"]) {
        pd["difficulty"] = json!(old);
    }
    let restored = Game::from_save_dict(&data).unwrap();
    assert_eq!(
        restored.players[1..].iter().map(|p| p.difficulty).collect::<Vec<_>>(),
        vec![Some(Difficulty::Level(3)), Some(Difficulty::Level(6)), Some(Difficulty::Level(10))]
    );
    // szint nélkül az alapérték (6), a robot sosem lecsatlakozott
    let mut g = with_human(false);
    g.add_bot("Robi", &json!("hard")).unwrap();
    g.start().unwrap();
    let mut data = g.to_save_dict();
    data["players"][1].as_object_mut().unwrap().remove("difficulty");
    assert_eq!(Game::from_save_dict(&data).unwrap().players[1].difficulty, Some(Difficulty::Level(6)));
    let mut data = g.to_save_dict();
    data["players"][1]["disconnected"] = json!(true);
    assert!(!Game::from_save_dict(&data).unwrap().players[1].disconnected);
    // robot-mezők nélküli régi mentés
    let mut g = with_human(false);
    g.start().unwrap();
    let mut data = g.to_save_dict();
    for pd in data["players"].as_array_mut().unwrap() {
        pd.as_object_mut().unwrap().remove("is_bot");
        pd.as_object_mut().unwrap().remove("difficulty");
    }
    data.as_object_mut().unwrap().remove("last_action_info");
    assert!(!Game::from_save_dict(&data).unwrap().players[0].is_bot);
}

// ---------------------------------------------------------------------------------------------------
// szerkezetes utolsó akció, történet
// ---------------------------------------------------------------------------------------------------

fn pair() -> Game {
    let mut g = Game::with_defaults("g");
    g.add_player("A", "Anna").unwrap();
    g.add_player("B", "Béla").unwrap();
    g.start().unwrap();
    g
}

#[test]
fn structured_last_action() {
    let mut g = pair();
    set_hand(&mut g, 0, &ALMAKOR);
    g.place_tiles("A", &word_at(7, 7, &ALMA)).unwrap();
    let info = g.last_action_info.clone().unwrap();
    assert_eq!(info["type"], "place");
    assert_eq!(info["player"], "Anna");
    assert_eq!(info["words"], json!(["ALMA"]));
    assert!(info["score"].as_i64().unwrap() > 0);
    assert!(g.last_action.clone().unwrap().starts_with("Anna: ALMA"));
    let mut g = pair();
    g.pass_turn("A", false).unwrap();
    assert_eq!(g.last_action_info.clone().unwrap(), json!({"type": "pass", "player": "Anna"}));
    g.exchange_tiles("B", &[0, 1]).unwrap();
    assert_eq!(g.last_action_info.clone().unwrap(), json!({"type": "exchange", "player": "Béla", "count": 2}));
    assert_eq!(g.get_state(Some("A"))["last_action_info"]["type"], "exchange");
}

#[test]
fn history_listing() {
    let mut g = pair();
    set_hand(&mut g, 0, &ALMAKOR);
    g.place_tiles("A", &word_at(7, 7, &ALMA)).unwrap();
    g.pass_turn("B", false).unwrap();
    let history = g.history();
    assert_eq!(history.iter().map(|h| h["type"].as_str().unwrap()).collect::<Vec<_>>(), vec!["place", "pass"]);
    assert_eq!(history[0]["words"], json!(["ALMA"]));
    assert_eq!(history[0]["player"], "Anna");
    assert!(history[0]["score"].as_i64().unwrap() > 0);
    assert_eq!(history[0]["tiles"].as_array().unwrap().len(), 4);
    // a kliensnek küldött állapotban nincs zseton-pozíció
    assert!(g.get_state(Some("A"))["history"][0].get("tiles").is_none());
    // az utolsó lépés zsetonjai csak lerakás után
    let mut g = pair();
    set_hand(&mut g, 0, &ALMAKOR);
    g.place_tiles("A", &word_at(7, 7, &ALMA)).unwrap();
    let cells: std::collections::HashSet<(i64, i64)> =
        g.get_state(Some("A"))["last_move_tiles"].as_array().unwrap().iter().map(|t| (t["row"].as_i64().unwrap(), t["col"].as_i64().unwrap())).collect();
    assert_eq!(cells, [(7, 7), (7, 8), (7, 9), (7, 10)].into_iter().collect());
    g.pass_turn("B", false).unwrap();
    assert_eq!(g.get_state(Some("A"))["last_move_tiles"], json!([]));
    // a gyorsítótár követi az új lépéseket
    let mut g = pair();
    assert!(g.history().is_empty());
    g.pass_turn("A", false).unwrap();
    assert_eq!(g.history().len(), 1);
    g.pass_turn("B", false).unwrap();
    assert_eq!(g.history().len(), 2);
}

// ---------------------------------------------------------------------------------------------------
// előnézet
// ---------------------------------------------------------------------------------------------------

#[test]
fn preview_matches_the_real_move() {
    let mut g = pair();
    set_hand(&mut g, 0, &ALMAKOR);
    let tiles = word_at(7, 7, &ALMA);
    let hand_before = g.players[0].hand.clone();
    let preview = g.preview_placement("A", &tiles);
    assert_eq!(preview["valid"], true);
    assert_eq!(preview["words"][0]["word"], "ALMA");
    // az előnézet nem változtat semmit
    assert_eq!(g.players[0].hand, hand_before);
    assert!(g.board.is_empty);
    assert_eq!(g.current_player().unwrap().id, "A");
    let (_, score) = g.place_tiles("A", &tiles).unwrap();
    assert_eq!(preview["score"].as_i64().unwrap(), score as i64);
}

#[test]
fn preview_error_cases() {
    let mut g = pair();
    set_hand(&mut g, 0, &["Ü", "Ö", "Ó", "Ú", "A", "L", "M"]);
    let preview = g.preview_placement("A", &word_at(7, 7, &["Ü", "Ö"]));
    assert_eq!(preview["valid"], false);
    assert!(preview["message"].as_str().unwrap().contains("Érvénytelen szó"));
    set_hand(&mut g, 0, &ALMAKOR);
    assert_eq!(g.preview_placement("B", &word_at(7, 7, &["A", "L"]))["valid"], false, "nem a saját kör");
    let preview = g.preview_placement("A", &word_at(7, 7, &["Z", "L"]));
    assert_eq!(preview["valid"], false);
    assert!(preview["message"].as_str().unwrap().contains("Nincs"));
    // hét zseton: bónusz (ha a szó érvényes)
    set_hand(&mut g, 0, &["A", "L", "M", "A", "F", "Á", "K"]);
    let preview = g.preview_placement("A", &word_at(7, 4, &["A", "L", "M", "A", "F", "Á", "K"]));
    if preview["valid"] == true {
        assert!(preview["score"].as_i64().unwrap() >= 50);
    }
}
