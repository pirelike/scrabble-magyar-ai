//! Napi feladvány: előállítás, játék, ranglista, Socket.IO és HTTP (a Python `test_daily.py` megfelelője).

mod common;
use common::*;
use scrabble::ai;
use scrabble::board::{Board, Placed};
use scrabble::daily;
use scrabble::db::{DailyPuzzle, Db};
use scrabble::game::Game;
use scrabble::server::core;
use scrabble::tiles::{TILE_DISTRIBUTION, Tile};
use serde_json::{Value, json};
use std::sync::{Arc, OnceLock};

const FIXED_DATE: &str = "2026-01-05";

/// Egyszer előállított feladvány (az előállítás másodpercekig tart): a tesztek ezt írják be az aznapi adatbázisba.
fn generated() -> &'static (Value, Vec<String>, i64, Value) {
    static CACHE: OnceLock<(Value, Vec<String>, i64, Value)> = OnceLock::new();
    CACHE.get_or_init(|| {
        scrabble::dictionary::warm_up();
        let vocab = ai::get_vocabulary();
        let g = daily::generate_puzzle(FIXED_DATE, &vocab).expect("feladvány");
        (g.board, g.rack, g.best_score, g.best)
    })
}

fn puzzle_for(date: &str) -> DailyPuzzle {
    let (board, rack, best_score, best) = generated();
    DailyPuzzle { date: date.to_string(), board: board.clone(), rack: rack.clone(), best_score: *best_score, best: best.clone() }
}

/// A mai feladvány az adatbázisban.
fn store_today(db: &Db) -> DailyPuzzle {
    let (board, rack, best_score, best) = generated();
    let today = daily::today_str();
    db.save_daily_puzzle(&today, board, rack, *best_score, best).unwrap();
    db.get_daily_puzzle(&today).unwrap()
}

fn temp_db() -> (Arc<Db>, std::path::PathBuf) {
    let dir = temp_dir("daily");
    let db = Db::open(dir.join("t.db").to_str().unwrap()).unwrap();
    db.init().unwrap();
    (Arc::new(db), dir)
}

fn placed(tiles: &Value) -> Vec<Placed> {
    tiles
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            Placed::new(
                t["row"].as_i64().unwrap() as i32,
                t["col"].as_i64().unwrap() as i32,
                Tile::from_str(t["letter"].as_str().unwrap()).unwrap(),
                t["is_blank"].as_bool().unwrap_or(false),
            )
        })
        .collect()
}

fn best_tiles(puzzle: &DailyPuzzle) -> Vec<Placed> {
    placed(&puzzle.best["tiles"])
}

fn total_tiles() -> usize {
    TILE_DISTRIBUTION.iter().map(|(_, _, c)| *c as usize).sum()
}

// ===================================================================================================
// Dátumok és előállítás
// ===================================================================================================

#[test]
fn today_has_a_valid_format() {
    assert!(daily::is_valid_date(&daily::today_str()));
}

#[test]
fn previous_date_crosses_month_and_year() {
    assert_eq!(daily::previous_date("2026-03-01"), "2026-02-28");
    assert_eq!(daily::previous_date("2026-01-01"), "2025-12-31");
}

#[test]
fn date_validation() {
    for (text, valid) in [("2026-01-05", true), ("2026-13-01", false), ("2026-1-5", false), ("nem dátum", false), ("", false), ("2026-02-30", false)] {
        assert_eq!(daily::is_valid_date(text), valid, "{text}");
    }
}

#[test]
fn the_puzzle_has_a_rack_a_board_and_a_best_move() {
    let (board, rack, best_score, best) = generated();
    assert_eq!(rack.len(), 7);
    assert!((daily::MIN_BEST_SCORE as i64..=daily::MAX_BEST_SCORE as i64).contains(best_score));
    assert!(!Board::from_json(board).is_empty);
    assert_eq!(best["score"].as_i64(), Some(*best_score));
    assert!(!best["words"].as_array().unwrap().is_empty());
    assert!(!best["tiles"].as_array().unwrap().is_empty());
}

#[test]
fn the_puzzle_is_deterministic_for_a_date() {
    let again = daily::generate_puzzle(FIXED_DATE, &ai::get_vocabulary()).unwrap();
    let (board, rack, best_score, best) = generated();
    assert_eq!((&again.board, &again.rack, again.best_score, &again.best), (board, rack, *best_score, best));
}

#[test]
fn different_dates_give_different_puzzles() {
    let other = daily::generate_puzzle("2026-01-06", &ai::get_vocabulary()).unwrap();
    let (board, rack, _, _) = generated();
    assert!((&other.rack, &other.board) != (rack, board));
}

#[test]
fn the_best_move_is_playable_and_scores_as_stated() {
    let puzzle = puzzle_for(FIXED_DATE);
    let mut game = daily::build_game("r", "sid", "Anna", &puzzle);
    let (_, score) = game.place_tiles("sid", &best_tiles(&puzzle)).expect("a legjobb lépés lerakható");
    assert_eq!(score as i64, puzzle.best_score);
}

#[test]
fn ensure_puzzle_generates_saves_once_and_reuses() {
    let (db, dir) = temp_db();
    let vocab = ai::get_vocabulary();
    let first = daily::ensure_puzzle(&db, Some("2026-02-02"), &vocab).unwrap();
    let second = daily::ensure_puzzle(&db, Some("2026-02-02"), &vocab).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.date, "2026-02-02");
    let direct = daily::generate_puzzle("2026-02-02", &vocab).unwrap();
    assert_eq!(first.best_score, direct.best_score);
    assert_eq!(first.rack, direct.rack);
    // a mentett feladvány változatlanul marad: egy másik tartalmú, azonos napra mentés nem írja felül
    db.save_daily_puzzle("2026-02-02", &json!([]), &vec!["A".to_string(); 7], 99, &json!({"words": ["B"]})).unwrap();
    assert_eq!(daily::ensure_puzzle(&db, Some("2026-02-02"), &vocab).unwrap(), first);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn saving_does_not_overwrite_an_existing_puzzle() {
    let (db, dir) = temp_db();
    let (board, rack, _, _) = generated();
    db.save_daily_puzzle("2026-02-03", board, rack, 40, &json!({"words": ["A"]})).unwrap();
    db.save_daily_puzzle("2026-02-03", board, &vec!["A".to_string(); 7], 99, &json!({"words": ["B"]})).unwrap();
    let stored = db.get_daily_puzzle("2026-02-03").unwrap();
    assert_eq!(stored.best_score, 40);
    assert_eq!(&stored.rack, rack);
    let _ = std::fs::remove_dir_all(dir);
}

// ===================================================================================================
// A feladvány-játék
// ===================================================================================================

#[test]
fn the_puzzle_game_starts_from_the_stored_position() {
    let puzzle = puzzle_for(FIXED_DATE);
    let game = daily::build_game("r", "sid", "Anna", &puzzle);
    assert!(game.started && !game.finished);
    let hand: Vec<String> = game.players[0].hand.iter().map(|t| t.as_str().to_string()).collect();
    assert_eq!(hand, puzzle.rack);
    assert_eq!(game.board.to_json(), puzzle.board);
    // a zsákban a táblán és a kézben nem látható zsetonok vannak
    let on_board = game.board.cells.iter().flatten().filter(|c| c.is_some()).count();
    assert_eq!(game.bag.remaining(), total_tiles() - on_board - puzzle.rack.len());
    let state = game.get_state(Some("sid"));
    assert_eq!(state["puzzle"], json!({"date": puzzle.date, "score": null}));
    assert!(!state["puzzle"].to_string().contains("best_score"));
}

#[test]
fn the_bag_of_the_puzzle_depends_only_on_the_date() {
    let puzzle = puzzle_for(FIXED_DATE);
    let a = daily::build_game("r1", "s1", "Anna", &puzzle);
    let b = daily::build_game("r2", "s2", "Bela", &puzzle);
    assert_eq!(a.bag.tiles, b.bag.tiles);
}

#[test]
fn one_placement_ends_the_game_with_the_move_score() {
    let puzzle = puzzle_for(FIXED_DATE);
    let mut game = daily::build_game("r", "sid", "Anna", &puzzle);
    let (_, score) = game.place_tiles("sid", &best_tiles(&puzzle)).unwrap();
    assert!(game.finished);
    assert_eq!(game.players[0].score, score); // nincs végső elszámolás
    assert_eq!(game.puzzle.as_ref().unwrap().score, Some(score));
    assert_eq!(game.winners.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Anna"]);
    assert_eq!(game.last_action_info.as_ref().unwrap()["type"], "puzzle");
}

#[test]
fn an_invalid_placement_keeps_the_puzzle_open() {
    let puzzle = puzzle_for(FIXED_DATE);
    let mut game = daily::build_game("r", "sid", "Anna", &puzzle);
    let first = game.players[0].hand[0];
    assert!(game.place_tiles("sid", &[Placed::new(0, 0, first, false)]).is_err());
    assert!(!game.finished);
    assert_eq!(game.puzzle.as_ref().unwrap().score, None);
}

#[test]
fn pass_and_exchange_are_refused() {
    let puzzle = puzzle_for(FIXED_DATE);
    let mut game = daily::build_game("r", "sid", "Anna", &puzzle);
    let err = game.pass_turn("sid", false).unwrap_err();
    assert!(err.contains("feladvány"), "{err}");
    let err = game.exchange_tiles("sid", &[0]).unwrap_err();
    assert!(err.contains("feladvány"), "{err}");
}

#[test]
fn reset_restores_the_initial_position() {
    let puzzle = puzzle_for(FIXED_DATE);
    let mut game = daily::build_game("r", "sid", "Anna", &puzzle);
    game.place_tiles("sid", &best_tiles(&puzzle)).unwrap();
    daily::reset_game(&mut game, &puzzle);
    assert!(!game.finished && game.winners.is_empty() && game.move_log.is_empty());
    let hand: Vec<String> = game.players[0].hand.iter().map(|t| t.as_str().to_string()).collect();
    assert_eq!(hand, puzzle.rack);
    assert_eq!(game.players[0].score, 0);
    assert_eq!(game.board.to_json(), puzzle.board);
    assert_eq!(game.puzzle.as_ref().unwrap().date, puzzle.date);
    assert_eq!(game.puzzle.as_ref().unwrap().score, None);
}

#[test]
fn normal_games_have_no_puzzle() {
    let mut game = Game::with_defaults("x");
    game.add_player("a", "A").unwrap();
    game.start().unwrap();
    assert!(game.puzzle.is_none());
    assert!(game.get_state(Some("a"))["puzzle"].is_null());
}

// ===================================================================================================
// Eredmények és ranglista
// ===================================================================================================

fn users(db: &Db, n: usize) -> Vec<i64> {
    (0..n).map(|i| db.create_user(&format!("u{i}@example.com"), &format!("U{i}"), "pass123").unwrap()).collect()
}

fn record(db: &Db, puzzle: &DailyPuzzle, user: Option<i64>, score: i64) -> Value {
    daily::record_result(db, puzzle, user, score, &json!([]), &json!(["AL"]))
}

#[test]
fn the_first_attempt_is_recorded() {
    let (db, dir) = temp_db();
    let puzzle = store_today(&db);
    let u = users(&db, 1)[0];
    let result = record(&db, &puzzle, Some(u), 20);
    assert_eq!(result["recorded"], true);
    assert_eq!(result["attempts"], 1);
    assert_eq!(result["my_best"], 20);
    assert_eq!(result["rank"], 1);
    assert_eq!(result["is_best"], false);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_best_attempt_counts_and_attempts_accumulate() {
    let (db, dir) = temp_db();
    let puzzle = store_today(&db);
    let u = users(&db, 1)[0];
    record(&db, &puzzle, Some(u), 20);
    let worse = record(&db, &puzzle, Some(u), 10);
    let better = record(&db, &puzzle, Some(u), 25);
    assert_eq!((worse["my_best"].clone(), worse["attempts"].clone()), (json!(20), json!(2)));
    assert_eq!((better["my_best"].clone(), better["attempts"].clone()), (json!(25), json!(3)));
    assert_eq!(db.get_daily_entry(&puzzle.date, u).unwrap().best_score, 25);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn ranking_order_with_ties() {
    let (db, dir) = temp_db();
    let puzzle = store_today(&db);
    let ids = users(&db, 3);
    let (a, b, c) = (ids[0], ids[1], ids[2]);
    record(&db, &puzzle, Some(a), 30);
    record(&db, &puzzle, Some(b), 40);
    record(&db, &puzzle, Some(c), 30);
    record(&db, &puzzle, Some(c), 5); // c-nek több próbálkozása van
    let (entries, me) = db.get_daily_leaderboard(&puzzle.date, 10, Some(c));
    assert_eq!(entries.iter().map(|e| e["user_id"].as_i64().unwrap()).collect::<Vec<_>>(), vec![b, a, c]);
    assert_eq!(me.unwrap()["rank"], 3);
    assert_eq!(entries[0]["best_score"], 40);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_score_above_the_known_best_raises_it() {
    let (db, dir) = temp_db();
    let puzzle = store_today(&db);
    let u = users(&db, 1)[0];
    let higher = puzzle.best_score + 7;
    let tiles = json!([{"row": 7, "col": 7, "letter": "A", "is_blank": false}]);
    let result = daily::record_result(&db, &puzzle, Some(u), higher, &tiles, &json!(["ÚJ"]));
    assert_eq!(result["is_best"], true);
    assert_eq!(result["best_score"], higher);
    let stored = db.get_daily_puzzle(&puzzle.date).unwrap();
    assert_eq!(stored.best_score, higher);
    assert_eq!(stored.best["words"], json!(["ÚJ"]));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_guest_result_is_not_recorded() {
    let (db, dir) = temp_db();
    let puzzle = store_today(&db);
    let result = record(&db, &puzzle, None, 12);
    assert_eq!(result["recorded"], false);
    assert!(result["rank"].is_null() && result["attempts"].is_null());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_revealed_solution_blocks_ranking() {
    let (db, dir) = temp_db();
    let puzzle = store_today(&db);
    let u = users(&db, 1)[0];
    db.mark_daily_revealed(&puzzle.date, u);
    let result = record(&db, &puzzle, Some(u), 99);
    assert_eq!(result["recorded"], false);
    assert!(db.get_daily_leaderboard(&puzzle.date, 10, None).0.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn revealing_after_scoring_keeps_the_score_but_blocks_improvement() {
    let (db, dir) = temp_db();
    let puzzle = store_today(&db);
    let u = users(&db, 1)[0];
    record(&db, &puzzle, Some(u), 20);
    db.mark_daily_revealed(&puzzle.date, u);
    assert_eq!(record(&db, &puzzle, Some(u), 99)["recorded"], false);
    let entries = db.get_daily_leaderboard(&puzzle.date, 10, None).0;
    assert_eq!(entries[0]["best_score"], 20);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn revealing_without_attempts_stays_off_the_leaderboard() {
    let (db, dir) = temp_db();
    let puzzle = store_today(&db);
    let u = users(&db, 1)[0];
    db.mark_daily_revealed(&puzzle.date, u);
    assert!(db.get_daily_leaderboard(&puzzle.date, 10, None).0.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn days_are_independent() {
    let (db, dir) = temp_db();
    let puzzle = store_today(&db);
    let u = users(&db, 1)[0];
    record(&db, &puzzle, Some(u), 20);
    assert!(db.get_daily_leaderboard("2000-01-01", 10, None).0.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

// ===================================================================================================
// Socket.IO
// ===================================================================================================

async fn start(sio: &Sio) -> Vec<(String, Value)> {
    sio.emit0("start_daily");
    sio.settle_long().await;
    sio.take()
}

fn names(events: &[(String, Value)]) -> Vec<&str> {
    events.iter().map(|(n, _)| n.as_str()).collect()
}

fn all(events: &[(String, Value)], name: &str) -> Vec<Value> {
    events.iter().filter(|(n, _)| n == name).map(|(_, d)| d.clone()).collect()
}

fn error_messages(events: &[(String, Value)]) -> Vec<String> {
    all(events, "error").iter().filter_map(|d| d["message"].as_str().map(|s| s.to_string())).collect()
}

async fn submit_best(sio: &Sio, puzzle: &DailyPuzzle) -> Vec<(String, Value)> {
    sio.emit("place_tiles", json!({"tiles": puzzle.best["tiles"]}));
    sio.settle_long().await;
    sio.take()
}

#[tokio::test(flavor = "multi_thread")]
async fn starting_creates_a_private_puzzle_room() {
    let server = TestServer::start().await;
    let puzzle = store_today(&server.app.db);
    let anna = server.player(1).await;
    let got = start(&anna).await;
    for expected in ["room_joined", "game_state", "game_started", "daily_started"] {
        assert!(names(&got).contains(&expected), "hiányzik: {expected} ({:?})", names(&got));
    }
    assert_eq!(find(&got, "daily_started").unwrap(), json!({"date": puzzle.date, "registered": true, "my": null}));
    {
        let st = server.app.state.lock();
        let room = st.rooms.values().next().unwrap();
        assert!(room.is_puzzle && room.is_private);
        assert_eq!(room.max_players, 1);
        let hand: Vec<String> = room.game.players[0].hand.iter().map(|t| t.as_str().to_string()).collect();
        assert_eq!(hand, puzzle.rack);
        assert!(st.get_rooms_list().is_empty());
        assert!(st.get_live_games().is_empty());
    }
    assert_eq!(find(&got, "game_state").unwrap()["puzzle"]["date"], json!(puzzle.date));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_best_move_is_recorded_and_earns_the_badge() {
    let server = TestServer::start().await;
    let puzzle = store_today(&server.app.db);
    let anna = server.player(1).await;
    start(&anna).await;
    let uid = server.user_id("u1@example.com");
    let got = submit_best(&anna, &puzzle).await;
    let result = all(&got, "daily_result").remove(0);
    assert_eq!(result["score"], puzzle.best_score);
    assert_eq!(result["is_best"], true);
    assert_eq!(result["recorded"], true);
    assert_eq!(result["rank"], 1);
    assert_eq!(result["attempts"], 1);
    assert_eq!(all(&got, "game_state").last().unwrap()["finished"], true);
    assert_eq!(all(&got, "achievements_earned")[0]["badges"], json!(["daily_best"]));
    assert!(server.app.db.get_user_achievements(uid).iter().any(|a| a["badge"] == "daily_best"));
    assert_eq!(server.app.db.get_daily_leaderboard(&puzzle.date, 10, None).0[0]["user_id"], uid);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_puzzle_game_is_never_saved_to_the_history() {
    let server = TestServer::start().await;
    let puzzle = store_today(&server.app.db);
    let anna = server.player(1).await;
    start(&anna).await;
    submit_best(&anna, &puzzle).await;
    let uid = server.user_id("u1@example.com");
    let id = room_id_of(&server);
    let (ok, _) = {
        let mut st = server.app.state.lock();
        core::save_game_to_db(&server.app, &mut st, &id)
    };
    assert!(ok);
    assert!(server.app.db.load_active_games().unwrap().is_empty());
    assert!(server.app.db.get_user_game_history(uid, 20).unwrap().is_empty());
    assert_eq!(server.app.db.get_user_by_id(uid).unwrap().unwrap().games_played, 0);
}

fn room_id_of(server: &TestServer) -> String {
    server.app.state.lock().rooms.keys().next().cloned().expect("nincs szoba")
}

#[tokio::test(flavor = "multi_thread")]
async fn retry_resets_the_game_and_keeps_the_best() {
    let server = TestServer::start().await;
    let puzzle = store_today(&server.app.db);
    let anna = server.player(1).await;
    start(&anna).await;
    submit_best(&anna, &puzzle).await;
    assert!(with_room(&server, |room| room.game.finished));
    anna.emit0("retry_daily");
    anna.settle_long().await;
    let got = anna.take();
    assert!(with_room(&server, |room| !room.game.finished));
    let hand: Vec<String> = with_room(&server, |room| room.game.players[0].hand.iter().map(|t| t.as_str().to_string()).collect());
    assert_eq!(hand, puzzle.rack);
    assert_eq!(all(&got, "game_state").last().unwrap()["finished"], false);
    assert_eq!(find(&got, "daily_started").unwrap()["my"]["best_score"], puzzle.best_score);
    let second = all(&submit_best(&anna, &puzzle).await, "daily_result").remove(0);
    assert_eq!(second["attempts"], 2);
    assert_eq!(second["my_best"], puzzle.best_score);
}

#[tokio::test(flavor = "multi_thread")]
async fn retry_is_ignored_before_a_submission() {
    let server = TestServer::start().await;
    store_today(&server.app.db);
    let anna = server.player(1).await;
    start(&anna).await;
    set_hand(&server, "Jatekos1", &["A", "A", "A", "A", "A", "A", "A"]);
    anna.emit0("retry_daily");
    anna.settle().await;
    assert_eq!(hand_of(&server, "Jatekos1"), vec!["A"; 7]);
}

#[tokio::test(flavor = "multi_thread")]
async fn revealing_marks_the_user_and_blocks_ranking() {
    let server = TestServer::start().await;
    let puzzle = store_today(&server.app.db);
    let anna = server.player(1).await;
    start(&anna).await;
    let uid = server.user_id("u1@example.com");
    anna.emit0("reveal_daily");
    anna.settle().await;
    let got = anna.take();
    let solution = find(&got, "daily_solution").unwrap();
    assert_eq!(solution["tiles"], puzzle.best["tiles"]);
    assert_eq!(solution["ranked"], false);
    assert!(server.app.db.get_daily_entry(&puzzle.date, uid).unwrap().revealed);
    let result = all(&submit_best(&anna, &puzzle).await, "daily_result").remove(0);
    assert_eq!(result["recorded"], false);
    assert!(server.app.db.get_daily_leaderboard(&puzzle.date, 10, None).0.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guest_can_play_but_is_not_ranked() {
    let server = TestServer::start().await;
    let puzzle = store_today(&server.app.db);
    let guest = server.guest("Vendég").await;
    let got = start(&guest).await;
    assert_eq!(find(&got, "daily_started").unwrap()["registered"], false);
    let result = all(&submit_best(&guest, &puzzle).await, "daily_result").remove(0);
    assert_eq!(result["score"], puzzle.best_score);
    assert_eq!(result["recorded"], false);
    assert!(server.app.db.get_daily_leaderboard(&puzzle.date, 10, None).0.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn pass_and_exchange_events_are_refused() {
    let server = TestServer::start().await;
    store_today(&server.app.db);
    let anna = server.player(1).await;
    start(&anna).await;
    let got = anna.call("pass_turn", Value::Null).await;
    assert_eq!(find(&got, "action_result").unwrap()["success"], false);
    let got = anna.call("exchange_tiles", json!({"indices": [0]})).await;
    assert_eq!(find(&got, "action_result").unwrap()["success"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn cannot_start_while_in_a_room() {
    let server = TestServer::start().await;
    store_today(&server.app.db);
    let anna = server.player(1).await;
    start(&anna).await;
    let got = anna.call("start_daily", Value::Null).await;
    assert!(error_messages(&got).iter().any(|m| m.contains("szobában")), "{got:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_start_right_after_the_first_leaves_no_orphan_room() {
    let server = TestServer::start().await;
    store_today(&server.app.db);
    let anna = server.player(1).await;
    anna.emit0("start_daily");
    anna.emit0("start_daily");
    anna.settle_long().await;
    anna.settle_long().await;
    assert_eq!(room_count(&server), 1);
    let st = server.app.state.lock();
    let room_of_player = st.player_rooms.values().next().cloned().unwrap();
    assert!(st.rooms.contains_key(&room_of_player));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unavailable_puzzle_is_reported() {
    let server = TestServer::start().await;
    // nincs mentett feladvány, és a mentés sem lehetséges: az előállítás hibával végződik
    server.app.db.with(|tx| tx.execute("DROP TABLE daily_puzzles", []).map(|_| ())).unwrap();
    let anna = server.player(1).await;
    anna.emit0("start_daily");
    let got = anna.wait("error", 60).await;
    assert!(got.is_some_and(|d| d["message"].as_str().unwrap_or("").contains("nem érhető el")));
    assert_eq!(room_count(&server), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn leaving_removes_the_room() {
    let server = TestServer::start().await;
    store_today(&server.app.db);
    let anna = server.player(1).await;
    start(&anna).await;
    anna.call("leave_room", Value::Null).await;
    assert_eq!(room_count(&server), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn starting_is_rate_limited() {
    let server = TestServer::start().await;
    store_today(&server.app.db);
    let anna = server.player(1).await;
    for _ in 0..3 {
        start(&anna).await;
        anna.call("leave_room", Value::Null).await;
    }
    let got = anna.call("start_daily", Value::Null).await;
    assert!(error_messages(&got).iter().any(|m| m.contains("Túl sok")), "{got:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_feature_switch_blocks_the_puzzle() {
    let server = TestServer::start().await;
    store_today(&server.app.db);
    server
        .app
        .db
        .with(|tx| server.app.settings.store(tx, "feature_daily", &json!(false), None).map(|_| ()).map_err(|_| rusqlite::Error::InvalidQuery))
        .unwrap();
    let anna = server.player(1).await;
    let got = start(&anna).await;
    assert!(!error_messages(&got).is_empty(), "{got:?}");
    assert_eq!(room_count(&server), 0);
}

// ===================================================================================================
// HTTP
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn info_hides_the_best_score_until_you_played() {
    let server = TestServer::start().await;
    let puzzle = store_today(&server.app.db);
    let uid = server.ensure_user(1);
    let http = server.http_as("u1@example.com").await;
    let data = http.get("/api/daily").await.json();
    assert_eq!(data["success"], true);
    assert_eq!(data["date"], json!(puzzle.date));
    assert_eq!(data["ready"], true);
    assert!(data["best_score"].is_null() && data["my"].is_null());
    record(&server.app.db, &puzzle, Some(uid), 21);
    let data = http.get("/api/daily").await.json();
    assert_eq!(data["best_score"], puzzle.best_score);
    assert_eq!(data["my"]["best_score"], 21);
    assert_eq!(data["leaderboard"][0]["is_me"], true);
    assert_eq!(data["me"]["rank"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn anonymous_info() {
    let server = TestServer::start().await;
    store_today(&server.app.db);
    let data = server.http().get("/api/daily").await.json();
    assert!(data["my"].is_null() && data["best_score"].is_null());
}

#[tokio::test(flavor = "multi_thread")]
async fn not_ready_before_the_first_request_of_the_day() {
    let server = TestServer::start().await;
    assert_eq!(server.http().get("/api/daily").await.json()["ready"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn yesterdays_solution_is_shown() {
    let server = TestServer::start().await;
    let (board, rack, best_score, best) = generated();
    let yesterday = daily::previous_date(&daily::today_str());
    server.app.db.save_daily_puzzle(&yesterday, board, rack, *best_score, best).unwrap();
    let uid = server.ensure_user(1);
    server.app.db.record_daily_score(&yesterday, uid, 33).unwrap();
    let data = server.http().get("/api/daily").await.json();
    assert_eq!(data["yesterday"]["best_words"], best["words"]);
    assert_eq!(data["yesterday"]["top"][0]["best_score"], 33);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_leaderboard_endpoint() {
    let server = TestServer::start().await;
    let puzzle = store_today(&server.app.db);
    let uid = server.ensure_user(1);
    record(&server.app.db, &puzzle, Some(uid), 30);
    let http = server.http();
    let data = http.get("/api/daily/leaderboard").await.json();
    assert_eq!(data["entries"].as_array().unwrap().iter().map(|e| e["display_name"].as_str().unwrap()).collect::<Vec<_>>(), vec!["Jatekos1"]);
    assert!(!http.get(&format!("/api/daily/leaderboard?date={}", puzzle.date)).await.json()["entries"].as_array().unwrap().is_empty());
    assert!(http.get("/api/daily/leaderboard?date=2000-01-01").await.json()["entries"].as_array().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_leaderboard_rejects_bad_and_future_dates() {
    let server = TestServer::start().await;
    let http = server.http();
    assert_eq!(http.get("/api/daily/leaderboard?date=nem-datum").await.status, 400);
    assert_eq!(http.get("/api/daily/leaderboard?date=2999-01-01").await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_api_is_rate_limited() {
    let server = TestServer::start().await;
    store_today(&server.app.db);
    restore_default_limits(&server.app);
    let http = server.http();
    let mut last = 0;
    for _ in 0..61 {
        last = http.get("/api/daily").await.status;
    }
    assert_eq!(last, 429);
}
