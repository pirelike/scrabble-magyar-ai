//! A robot és a külső motor párharcának biztonsági hálója: a híd veszteségmentes, a szabályok és a szótár azonosak, a két
//! független lépésgenerátor ugyanazt a lépéshalmazt adja, a játékvezető nem enged csendes hibát, a futás megismételhető.
//! (A `--features engine-duel` kapcsolóval fordul.)

use pg_scrabble::eval::EvalContext;
use pg_scrabble::movegen::{MoveGenerator, Play};
use pg_scrabble::prelude::Premium as EPremium;
use pg_scrabble::sim::SimOptions;
use scrabble::ai::{self, BONUS_ALL_TILES, HAND_SIZE};
use scrabble::board::{Board, CENTER, Placed, Premium, premium_at};
use scrabble::engine_duel::bridge;
use scrabble::engine_duel::crosscheck;
use scrabble::engine_duel::leaves::{self, LinearLeaves, TrainOptions};
use scrabble::engine_duel::lexicon::legal_tilings;
use scrabble::engine_duel::referee::{End, GameRecord, play_game, seeded_bag};
use scrabble::engine_duel::report;
use scrabble::engine_duel::runner::{self, DuelOptions};
use scrabble::engine_duel::sides::{Action, BotMode, BotSide, DEFAULT_ENDGAME_BUDGET, Decision, EngineSide, Eval, Search, Side, View};
use scrabble::engine_duel::spec::Env;
use scrabble::engine_duel::{EVAL_SEED_LIMIT, TRAIN_SEED_START};
use scrabble::game::Game;
use scrabble::tiles::{TILE_DISTRIBUTION, Tile, TileBag, forms_digraph, tokenize_word};
use std::sync::{Arc, Mutex, OnceLock};

fn env() -> &'static Env {
    static ENV: OnceLock<Env> = OnceLock::new();
    ENV.get_or_init(|| Env::load(None).expect("a szótár és a szókincs betöltése"))
}

fn t(letter: &str) -> Tile {
    Tile::from_str(letter).unwrap_or_else(|| panic!("ismeretlen zseton: {letter}"))
}

/// Tetszőleges döntési függvényből oldal (tesztekhez).
struct Scripted {
    name: &'static str,
    f: Box<dyn FnMut(&View<'_>, u64) -> Result<Decision, String> + Send>,
}

impl Side for Scripted {
    fn label(&self) -> String {
        self.name.to_string()
    }

    fn choose(&mut self, view: &View<'_>, seed: u64) -> Result<Decision, String> {
        (self.f)(view, seed)
    }
}

fn greedy_bot() -> BotSide {
    BotSide::new(10, BotMode::Greedy, env().vocab.clone())
}

fn greedy_engine() -> EngineSide {
    let e = env();
    EngineSide::new("eng:greedy".into(), e.config.clone(), e.lexicon.clone(), Arc::new(Eval::Greedy), Search::Static, None)
}

fn turns_summary(g: &GameRecord) -> Vec<(u8, char, i32, u8, &'static str)> {
    g.turns.iter().map(|t| (t.seat, t.kind, t.score, t.tiles, t.tag)).collect()
}

// ===================================================================================================
// Szabályok és szótár
// ===================================================================================================

#[test]
fn the_tile_tables_are_identical() {
    let config = &env().config;
    assert_eq!(config.alphabet.len(), 38);
    for (i, (label, value, count)) in TILE_DISTRIBUTION.iter().enumerate().skip(1) {
        assert_eq!(config.alphabet.display(i as u8 - 1), *label);
        assert_eq!(config.alphabet.score(i as u8 - 1), *value as i32, "{label}");
        assert_eq!(config.distribution.counts()[i - 1], *count, "{label}");
    }
    assert_eq!(config.distribution.counts()[pg_scrabble::tile::BLANK_CODE as usize], TILE_DISTRIBUTION[0].2);
    assert_eq!(config.distribution.total(), 100);
    assert_eq!(TILE_DISTRIBUTION.iter().map(|(_, _, c)| *c as usize).sum::<usize>(), 100);
}

#[test]
fn the_tile_mapping_round_trips() {
    for i in 0..39u8 {
        assert_eq!(bridge::from_engine_tile(bridge::to_engine_tile(Tile(i))), Tile(i));
    }
    assert!(bridge::to_engine_tile(Tile::BLANK).is_blank());
    assert_eq!(bridge::to_engine_tile(t("A")).index(), Some(0));
    assert_eq!(bridge::to_engine_tile(t("TY")).index(), Some(37));
}

#[test]
fn the_board_layout_is_the_same_square_by_square() {
    let config = &env().config;
    assert_eq!((config.layout.start(), config.layout.start_required()), ((7, 7), true));
    assert_eq!((config.width(), config.height()), (15, 15));
    for r in 0..15 {
        for c in 0..15 {
            let ours = match premium_at(r, c) {
                Premium::None => EPremium::Normal,
                Premium::Dl => EPremium::DoubleLetter,
                Premium::Tl => EPremium::TripleLetter,
                Premium::Dw | Premium::Star => EPremium::DoubleWord,
                Premium::Tw => EPremium::TripleWord,
            };
            assert_eq!(config.layout.premium(r, c), ours, "({r}, {c})");
        }
    }
    assert_eq!(CENTER, 7);
}

#[test]
fn the_rule_constants_match() {
    let config = &env().config;
    assert_eq!(config.rack_size, HAND_SIZE);
    assert_eq!(config.bingo_bonus, BONUS_ALL_TILES);
    assert_eq!(config.min_exchange_bag, 7);
    assert_eq!(config.max_consecutive_zeros, scrabble::game::SCORELESS_TURNS_LIMIT);
    assert!(config.double_out_adjustment);
}

#[test]
fn the_exchange_gate_is_seven_in_both_worlds() {
    // a mi játékunk: 7 zseton a zsákban még cserélhet, 6 már nem
    for (bag, ok) in [(7usize, true), (8, true), (6, false), (0, false)] {
        let mut game = Game::with_defaults("gate");
        game.add_player("a", "A").unwrap();
        game.add_player("b", "B").unwrap();
        game.bag.tiles = seeded_bag(5);
        game.start().unwrap();
        game.bag.tiles.truncate(bag);
        assert_eq!(game.exchange_tiles("a", &[0]).is_ok(), ok, "zsák: {bag}");
    }
}

#[test]
fn the_engine_lexicon_holds_every_legal_tiling_and_no_split_digraph() {
    let e = env();
    let mut extra = 0usize;
    for (i, word) in e.vocab.iter().enumerate() {
        if i % 7 != 0 {
            continue;
        }
        let tilings = legal_tilings(word);
        assert!(!tilings.is_empty(), "a(z) {word} nem bontható zsetonokra");
        let canonical = tokenize_word(word).expect("zsetonokra bontható");
        assert!(tilings.contains(&canonical), "a kanonikus bontás is jogos: {word}");
        for tiling in &tilings {
            for pair in tiling.windows(2) {
                assert!(!forms_digraph(pair[0].as_str(), pair[1].as_str()), "hasított kétjegyű betű a bontásban: {word}");
            }
            let encoded: Vec<u8> = tiling.iter().map(|x| x.0 - 1).collect();
            assert!(e.lexicon.contains(&encoded), "hiányzik a motor szótárából: {word}");
        }
        extra += tilings.len() - 1;
    }
    // a kétértelmű szavak ritkák (a teljes szókincsben néhány száz)
    assert!(extra < 200, "{extra}");
    assert!(e.sequences >= e.vocab.len());
    assert!(e.sequences - e.vocab.len() < 1000, "{} extra bontás", e.sequences - e.vocab.len());
}

#[test]
fn an_ambiguous_word_has_exactly_the_two_legal_tilings() {
    let tilings = legal_tilings("EGÉSZSÉG");
    let texts: Vec<String> = tilings.iter().map(|t| t.iter().map(|x| x.as_str()).collect::<Vec<_>>().join(" ")).collect();
    assert_eq!(texts.len(), 2, "{texts:?}");
    assert!(texts.contains(&"E G É SZ S É G".to_string()));
    assert!(texts.contains(&"E G É S ZS É G".to_string()));
    // a hasított S + Z nélküli egyetlen bontás
    assert_eq!(legal_tilings("ASZTAL").len(), 1);
}

#[test]
fn the_vocabulary_is_a_subset_of_the_referee_dictionary() {
    let e = env();
    let sample: Vec<&str> = e.vocab.iter().step_by(11).collect();
    let valid = scrabble::dictionary::filter_valid(sample.iter().copied());
    let rejected: Vec<&&str> = sample.iter().filter(|w| !valid.contains(**w)).take(5).collect();
    assert!(rejected.is_empty(), "a játékvezető elutasítja a szókincs szavait: {rejected:?}");
}

// ===================================================================================================
// Híd
// ===================================================================================================

#[test]
fn boards_and_racks_round_trip_with_blanks_and_digraphs() {
    let config = &env().config;
    let mut board = Board::new();
    board.set(7, 7, t("SZ"), false);
    board.set(7, 8, t("A"), false);
    board.set(7, 9, t("GY"), true); // joker kétjegyű betűként
    board.set(8, 9, t("Ő"), false);
    board.is_empty = false;
    let engine = bridge::board_to_engine(&board, config);
    assert_eq!(bridge::board_from_engine(&engine), board);
    let hand = vec![t("A"), t("A"), Tile::BLANK, t("ZS"), t("TY"), Tile::BLANK, t("Ű")];
    let mut sorted = hand.clone();
    sorted.sort();
    assert_eq!(bridge::rack_from_engine(&bridge::rack_to_engine(&hand)), sorted);
}

#[test]
fn the_honest_unseen_pool_matches_the_truth_and_the_engine() {
    let e = env();
    let mut game = Game::with_defaults("unseen");
    game.add_player("a", "A").unwrap();
    game.add_player("b", "B").unwrap();
    game.bag.tiles = seeded_bag(9);
    game.start().unwrap();
    let hand = game.players[0].hand.clone();
    let honest = bridge::honest_unseen(&game.board, &hand);
    let mut truth = [0u8; 39];
    for tile in game.bag.tiles.iter().chain(game.players[1].hand.iter()) {
        truth[tile.0 as usize] += 1;
    }
    assert_eq!(honest, truth);
    let ebrd = bridge::board_to_engine(&game.board, &e.config);
    let unseen = pg_scrabble::eval::Unseen::new(&e.config, &ebrd, &bridge::rack_to_engine(&hand));
    assert_eq!(bridge::unseen_counts_from_engine(&unseen), honest);
    assert_eq!(unseen.len(), game.bag.remaining() + game.players[1].hand.len());
}

#[test]
fn opening_plays_are_canonicalised_to_the_horizontal_direction() {
    let e = env();
    let board = Board::new();
    let ebrd = bridge::board_to_engine(&board, &e.config);
    let hand = vec![t("A"), t("T"), t("E"), t("K"), t("I"), t("L"), Tile::BLANK];
    let mut generator = MoveGenerator::new(&e.config);
    let plays = generator.generate(&ebrd, &bridge::rack_to_engine(&hand), &e.lexicon).to_vec();
    assert!(!plays.is_empty());
    let vertical = plays.iter().filter(|p| p.direction() == pg_scrabble::board::Direction::Vertical).count();
    assert!(vertical > 0, "a motor mindkét irányú nyitólépést adja");
    for play in &plays {
        let placed = bridge::placed_from_play(play, true);
        assert!(placed.iter().all(|p| p.row == CENTER), "a nyitólépés vízszintes");
        board.validate_placement(&placed, false).unwrap_or_else(|err| panic!("a játékvezető elutasítja: {err}"));
    }
}

// ===================================================================================================
// A két független lépésgenerátor
// ===================================================================================================

#[test]
fn the_two_move_generators_agree_on_self_play_positions() {
    let report = crosscheck::run(env(), 3, 1).unwrap();
    assert!(report.positions >= 60, "{report:?}");
    assert!(report.clean(), "{:#?}", report);
    assert_eq!(report.identical_positions, report.positions);
    assert!(report.blank_positions > 0, "legyen jokeres állás is");
    // a dokumentált aszimmetria: a robot (éles működésében) a teljes szótárból is vehet keresztszót, a motor nem
    assert!(report.bot_nonvocab_moves > 0, "a szókincsen kívüli keresztszavas lépések léteznek, ezért kell a robotot szűrni");
    assert!(report.engine_moves > 5_000);
}

#[test]
fn targeted_positions_agree_with_the_referee() {
    let e = env();
    let mut generator = MoveGenerator::new(&e.config);
    // az EGÉSZSÉG kétféle jogos bontása: ...SZ S... játszható és a játékvezető elfogadja
    let hand: Vec<Tile> = ["E", "G", "É", "SZ", "S", "É", "G"].iter().map(|s| t(s)).collect();
    let diff = crosscheck::compare_position(e, &mut generator, &Board::new(), &hand);
    assert!(diff.identical() && diff.engine_rejected.is_empty(), "{diff:?}");
    let board = Board::new();
    let placed: Vec<Placed> = hand.iter().enumerate().map(|(i, x)| Placed::new(7, 4 + i as i32, *x, false)).collect();
    assert!(board.validate_placement(&placed, false).is_ok());
    // a hasított S + Z változatot a játékvezető elutasítja (és a motor sosem javasol ilyet)
    let split: Vec<Tile> = ["E", "G", "É", "S", "Z", "S", "É", "G"].iter().map(|s| t(s)).collect();
    let split_placed: Vec<Placed> = split.iter().enumerate().map(|(i, x)| Placed::new(7, 3 + i as i32, *x, false)).collect();
    assert!(board.validate_placement(&split_placed, false).unwrap_err().contains("Kétjegyű"));
    // joker Z-ként az S után is hasított kétjegyű betű
    let mut with_blank = split_placed.clone();
    with_blank[4] = Placed::new(7, 7, t("Z"), true);
    assert!(board.validate_placement(&with_blank, false).unwrap_err().contains("Kétjegyű"));
}

#[test]
fn engine_plays_never_form_a_split_digraph_even_with_blanks() {
    let e = env();
    let mut generator = MoveGenerator::new(&e.config);
    let board = Board::new();
    let ebrd = bridge::board_to_engine(&board, &e.config);
    // S, Z és két joker: a játékvezető szerint az S+Z külön zsetonnal nem rakható ki
    let hand = vec![t("S"), t("Z"), t("A"), t("L"), Tile::BLANK, Tile::BLANK, t("T")];
    let plays = generator.generate(&ebrd, &bridge::rack_to_engine(&hand), &e.lexicon).to_vec();
    assert!(plays.len() > 100);
    for play in &plays {
        let placed = bridge::placed_from_play(play, true);
        assert!(board.validate_placement(&placed, false).is_ok(), "{placed:?}");
    }
}

// ===================================================================================================
// Játékvezető
// ===================================================================================================

#[test]
fn the_seeded_bag_is_deterministic_and_complete() {
    let a = seeded_bag(7);
    assert_eq!(a, seeded_bag(7));
    assert_ne!(a, seeded_bag(8));
    let mut sorted = a.clone();
    sorted.sort();
    let mut reference = TileBag::new().tiles;
    reference.sort();
    assert_eq!(sorted, reference);
    assert_eq!(a.len(), 100);
}

#[test]
fn a_mirrored_pair_deals_the_same_tiles_to_the_first_seat() {
    let first_hands = Arc::new(Mutex::new(Vec::<(String, Vec<Tile>)>::new()));
    let make = |name: &'static str, hands: Arc<Mutex<Vec<(String, Vec<Tile>)>>>| {
        let mut bot = greedy_bot();
        Scripted {
            name,
            f: Box::new(move |view, seed| {
                if view.ply < 2 {
                    let mut h = view.hand.to_vec();
                    h.sort();
                    hands.lock().unwrap().push((format!("{name}@{}", view.ply), h));
                }
                bot.choose(view, seed)
            }),
        }
    };
    let (mut a, mut b) = (make("A", first_hands.clone()), make("B", first_hands.clone()));
    let g0 = play_game(&mut a, &mut b, 21, 0).unwrap();
    let g1 = play_game(&mut a, &mut b, 21, 1).unwrap();
    assert_eq!((g0.seat_a, g1.seat_a), (0, 1));
    let hands = first_hands.lock().unwrap();
    let get = |k: &str, n: usize| hands.iter().filter(|(l, _)| l == k).nth(n).unwrap().1.clone();
    // 1. fél: A kezd (A@0), B a 2. (B@1); 2. fél: B kezd (B@0), A a 2. (A@1) — ugyanazokat a zsetonokat kapja a kezdő ülés
    assert_eq!(get("A@0", 0), get("B@0", 0), "a kezdő ülés mindkét félben ugyanazt a kezet kapja");
    assert_eq!(get("B@1", 0), get("A@1", 0), "a második ülés is");
}

#[test]
fn a_game_is_exactly_repeatable_including_exchanges() {
    // olyan oldal, amely a játék elején többször cserél: a zsák újrakeverése magból történik
    let build = || {
        let mut bot = greedy_bot();
        let mut exchanges = 0;
        Scripted {
            name: "csereló",
            f: Box::new(move |view, seed| {
                if view.bag_remaining >= 7 && exchanges < 3 {
                    exchanges += 1;
                    return Ok(Decision { action: Action::Exchange(vec![0, 1, 2]), tag: "exchange" });
                }
                bot.choose(view, seed)
            }),
        }
    };
    let run = || {
        let (mut a, mut b) = (build(), greedy_engine());
        (play_game(&mut a, &mut b, 33, 0).unwrap(), play_game(&mut a, &mut b, 33, 1).unwrap())
    };
    let (x0, x1) = run();
    let (y0, y1) = run();
    assert_eq!(turns_summary(&x0), turns_summary(&y0));
    assert_eq!(turns_summary(&x1), turns_summary(&y1));
    assert_eq!((x0.scores, x1.scores), (y0.scores, y1.scores));
    assert!(x0.turns.iter().any(|t| t.kind == 'X'), "volt csere");
}

#[test]
fn a_greedy_game_is_repeatable_across_runs() {
    let run = |seed: u64| {
        let (mut a, mut b) = (greedy_bot(), greedy_engine());
        play_game(&mut a, &mut b, seed, 0).unwrap()
    };
    for seed in [1, 2, 3] {
        let (x, y) = (run(seed), run(seed));
        assert_eq!(turns_summary(&x), turns_summary(&y));
        assert_eq!(x.scores, y.scores);
        assert!(x.turns.len() > 10);
    }
}

#[test]
fn the_referee_aborts_instead_of_hiding_errors() {
    let mut good = greedy_engine();
    // illegális lerakás: egyetlen zseton a tábla sarkában
    let mut bad =
        Scripted { name: "illegális", f: Box::new(|view, _| Ok(Decision { action: Action::Place(vec![Placed::new(0, 0, view.hand[0], false)]), tag: "x" })) };
    let err = play_game(&mut bad, &mut good, 1, 0).unwrap_err();
    assert!(err.contains("elutasította"), "{err}");
    // pánik
    let mut panics = Scripted { name: "pánik", f: Box::new(|_, _| panic!("szándékos")) };
    let err = play_game(&mut panics, &mut good, 1, 0).unwrap_err();
    assert!(err.contains("pánikolt"), "{err}");
    // hibaüzenet
    let mut broken = Scripted { name: "hibás", f: Box::new(|_, _| Err("elromlott".into())) };
    let err = play_game(&mut broken, &mut good, 1, 0).unwrap_err();
    assert!(err.contains("elromlott"), "{err}");
    // olyan cserélendő index, amely nincs a kézben
    let mut wrong_index = Scripted { name: "rossz index", f: Box::new(|_, _| Ok(Decision { action: Action::Exchange(vec![9]), tag: "x" })) };
    assert!(play_game(&mut wrong_index, &mut good, 1, 0).is_err());
}

#[test]
fn six_scoreless_turns_end_the_game_and_subtract_the_racks() {
    let hands = Arc::new(Mutex::new(Vec::<Vec<Tile>>::new()));
    let make = |hands: Arc<Mutex<Vec<Vec<Tile>>>>| Scripted {
        name: "passzoló",
        f: Box::new(move |view, _| {
            if view.ply < 2 {
                hands.lock().unwrap().push(view.hand.to_vec());
            }
            Ok(Decision { action: Action::Pass, tag: "pass" })
        }),
    };
    let (mut a, mut b) = (make(hands.clone()), make(hands.clone()));
    let g = play_game(&mut a, &mut b, 4, 0).unwrap();
    assert_eq!(g.end, End::Scoreless);
    assert_eq!(g.turns.len(), 6);
    let face = |h: &Vec<Tile>| h.iter().map(|t| t.value() as i32).sum::<i32>();
    let hands = hands.lock().unwrap();
    assert_eq!(g.scores, [-face(&hands[0]), -face(&hands[1])]);
}

#[test]
fn the_engine_side_refuses_an_inconsistent_view() {
    let board = Board::new();
    let hand: Vec<Tile> = ["A", "T", "E", "K", "I", "L", "N"].iter().map(|s| t(s)).collect();
    let view = |bag: usize| View { board: &board, hand: &hand, own_score: 0, opp_score: 0, bag_remaining: bag, opp_hand_len: 7, scoreless_turns: 0, ply: 0 };
    let mut side = greedy_engine();
    assert!(side.choose(&view(86), 1).is_ok(), "100 − 7 (saját kéz) = 93 = 86 + 7");
    let err = side.choose(&view(80), 1).unwrap_err();
    assert!(err.contains("nem látott"), "{err}");
}

// ===================================================================================================
// Az oldalak
// ===================================================================================================

#[test]
fn the_strict_level_ten_bot_picks_the_best_equity_among_vocabulary_moves() {
    let e = env();
    // a robot egy teljes játékán át: a választott lépés egyenlősége = a szókincsre korlátozott legjobb
    let checks = Arc::new(Mutex::new(0usize));
    let counters = checks.clone();
    let vocab = e.vocab.clone();
    let mut strict = BotSide::new(10, BotMode::Strict, vocab.clone());
    let mut watcher = Scripted {
        name: "bot:10",
        f: Box::new(move |view, seed| {
            let decision = strict.choose(view, seed)?;
            let moves = ai::generate_moves(view.board, view.hand, &vocab, true, HAND_SIZE, 60.0);
            let mut vocab_only: Vec<ai::Move> = moves.into_iter().filter(|m| m.words.iter().all(|w| vocab.contains(w))).collect();
            ai::rate(&mut vocab_only, view.hand, view.bag_remaining);
            let best_strict = vocab_only.iter().map(|m| m.equity).fold(f64::NEG_INFINITY, f64::max);
            if let Action::Place(placed) = &decision.action {
                let chosen = vocab_only.iter().find(|m| &m.tiles == placed).ok_or("a választott lépés nincs a szókincses lépések között")?;
                assert!((chosen.equity - best_strict).abs() < 1e-9, "a robot nem a legjobb értékelésűt választotta");
                *counters.lock().unwrap() += 1;
            }
            Ok(decision)
        }),
    };
    let mut engine = greedy_engine();
    play_game(&mut watcher, &mut engine, 5, 0).unwrap();
    let total = *checks.lock().unwrap();
    assert!(total > 8, "{total}");
}

#[test]
fn the_production_bot_is_never_weaker_in_equity_than_the_strict_bot() {
    // éles működés: a keresztszavak a teljes szótárból is jöhetnek; az így választott lépés értékelése nem lehet rosszabb
    let e = env();
    let mut production = BotSide::new(10, BotMode::Production, e.vocab.clone());
    let mut strict = BotSide::new(10, BotMode::Strict, e.vocab.clone());
    let mut game = Game::with_defaults("parity");
    game.add_player("a", "A").unwrap();
    game.add_player("b", "B").unwrap();
    game.bag.tiles = seeded_bag(12);
    game.start().unwrap();
    let mut compared = 0;
    for ply in 0..16 {
        let idx = game.current_player_idx;
        let hand = game.players[idx].hand.clone();
        let view = View {
            board: &game.board,
            hand: &hand,
            own_score: game.players[idx].score,
            opp_score: game.players[1 - idx].score,
            bag_remaining: game.bag.remaining(),
            opp_hand_len: game.players[1 - idx].hand.len(),
            scoreless_turns: 0,
            ply,
        };
        let (p, s) = (production.choose(&view, 1).unwrap(), strict.choose(&view, 1).unwrap());
        let equity = |d: &Decision| -> f64 {
            let Action::Place(placed) = &d.action else { return f64::NEG_INFINITY };
            let mut moves = ai::generate_moves(view.board, view.hand, &e.vocab, true, HAND_SIZE, 60.0);
            ai::rate(&mut moves, view.hand, view.bag_remaining);
            moves.iter().find(|m| &m.tiles == placed).map(|m| m.equity).unwrap_or(f64::NEG_INFINITY)
        };
        let (ep, es) = (equity(&p), equity(&s));
        assert!(ep >= es - 1e-9, "éles {ep} < szűrt {es}");
        compared += 1;
        let Action::Place(placed) = s.action else { break };
        game.place_tiles(&game.players[idx].id.clone(), &placed).unwrap();
        if game.finished {
            break;
        }
    }
    assert!(compared >= 8);
}

#[test]
fn spec_parsing() {
    let e = env();
    for ok in [
        "bot:10",
        "bot:9",
        "bot:2",
        "bot:greedy",
        "bot:10:full",
        "eng:greedy",
        "eng:stock",
        "eng:greedy:sim-fast",
        "eng:stock:sim+eg",
        "eng:stock:sim-deep+eg=500000",
        "eng:stock+eg",
    ] {
        let side = e.build_side(ok).unwrap_or_else(|err| panic!("{ok}: {err}"));
        assert!(!side.label().is_empty());
    }
    for bad in ["bot:11", "bot:0", "bot", "eng", "eng:magic", "eng:greedy:warp", "bot:greedy+eg", "xyz:1", "eng:greedy+eg=abc", "eng:leaves"] {
        assert!(e.build_side(bad).is_err(), "{bad}");
    }
}

#[test]
fn simulation_and_endgame_sides_play_complete_legal_games() {
    let e = env();
    let leaves = LinearLeaves::zero();
    let mut engine = EngineSide::new(
        "eng:test".into(),
        e.config.clone(),
        e.lexicon.clone(),
        Arc::new(Eval::Leaves(leaves)),
        Search::Sim(SimOptions::fast()),
        Some(DEFAULT_ENDGAME_BUDGET),
    );
    let mut bot = greedy_bot();
    let mut calls = 0;
    for half in 0..2 {
        let g = play_game(&mut engine, &mut bot, 3, half).unwrap();
        assert!(g.turns.len() > 10);
        assert!(g.turns.iter().any(|t| t.tag == "sim"), "a szimuláció döntött");
        calls += g.turns.iter().filter(|t| t.tag == "endgame").count();
    }
    let (made, exact) = engine.counters();
    assert!(exact <= made);
    assert_eq!(calls as u64, exact, "minden pontos végjáték-eredményből lépés lett");
    assert!(made > 0, "a zsák kiürülése után a végjáték-kereső hívódott");
}

// ===================================================================================================
// Futtató, napló, jelentés
// ===================================================================================================

fn temp_file(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("engine-duel-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("duel.jsonl")
}

#[test]
fn the_runner_logs_resumes_and_reports() {
    let e = env();
    let out = temp_file("runner");
    let opts = DuelOptions { a: "bot:greedy".into(), b: "eng:greedy".into(), first_pair: 1, pairs: 3, jobs: 2, out: out.clone(), log_moves: true };
    let first = runner::run_duel(e, &opts, false).unwrap();
    assert_eq!((first.played, first.skipped), (3, 0));
    let text = std::fs::read_to_string(&out).unwrap();
    assert_eq!(text.lines().count(), 6);
    let again = runner::run_duel(e, &opts, false).unwrap();
    assert_eq!((again.played, again.skipped), (0, 3), "a kész párokat kihagyja");
    assert_eq!(std::fs::read_to_string(&out).unwrap(), text);
    // bővítés: még két pár
    let more = DuelOptions { pairs: 5, ..opts };
    assert_eq!(runner::run_duel(e, &more, false).unwrap().played, 2);
    let matchups = report::load(std::slice::from_ref(&out)).unwrap();
    assert_eq!(matchups.len(), 1);
    let analysis = report::analyze(&matchups[0]).unwrap();
    assert_eq!(analysis.pairs, 5);
    assert_eq!(analysis.outcomes.iter().sum::<u64>(), 5);
    let rendered = report::render(&[analysis]);
    assert!(rendered.contains("bot:greedy") && rendered.contains("Előjelpróba"));
    // minden játéksor feldolgozható, a lépéslista benne van
    for line in text.lines() {
        let (a, b, rec) = report::parse_line(line).unwrap();
        assert_eq!((a.as_str(), b.as_str()), ("bot:greedy", "eng:greedy"));
        assert!(rec.plies > 5);
        assert!(serde_json::from_str::<serde_json::Value>(line).unwrap()["moves"].as_array().unwrap().len() as u64 == rec.plies);
    }
}

#[test]
fn the_runner_rejects_bad_seeds_and_specs() {
    let e = env();
    let out = temp_file("bad");
    let base =
        DuelOptions { a: "bot:greedy".into(), b: "eng:greedy".into(), first_pair: EVAL_SEED_LIMIT - 1, pairs: 2, jobs: 1, out: out.clone(), log_moves: false };
    assert!(runner::run_duel(e, &base, false).unwrap_err().contains("pármagok"));
    let bad_spec = DuelOptions { a: "bot:99".into(), first_pair: 1, ..base };
    assert!(runner::run_duel(e, &bad_spec, false).is_err());
    assert!(!out.exists() || std::fs::read_to_string(&out).unwrap().is_empty(), "hibás kérésnél nem íródik napló");
}

// ===================================================================================================
// Tanítás
// ===================================================================================================

#[test]
fn training_runs_and_keeps_the_seed_ranges_apart() {
    let e = env();
    let policy = |ctx: &EvalContext<'_>, play: &Play| ctx.config.alphabet.len() as f64 * 0.0 + play.score() as f64;
    let opts = TrainOptions { games: 6, jobs: 2, seed_start: TRAIN_SEED_START, epsilon: 0.1, lambda: 100.0, min_bag: 7, holdout_every: 0 };
    let report = leaves::train(&e.config, &e.lexicon, &policy, &opts, false).unwrap();
    assert_eq!(report.games, 6);
    assert!(report.samples > 50);
    assert!(report.leaves.weights.iter().all(|w| w.is_finite()));
    let low = TrainOptions { seed_start: EVAL_SEED_LIMIT - 1, ..opts };
    assert!(leaves::train(&e.config, &e.lexicon, &policy, &low, false).unwrap_err().contains("elkülönítve"));
}

#[test]
fn training_is_reproducible() {
    let e = env();
    let policy = |ctx: &EvalContext<'_>, play: &Play| {
        let _ = ctx;
        play.score() as f64
    };
    let run = |jobs| {
        let opts = TrainOptions { games: 8, jobs, seed_start: TRAIN_SEED_START + 77, epsilon: 0.2, lambda: 50.0, min_bag: 7, holdout_every: 0 };
        leaves::train(&e.config, &e.lexicon, &policy, &opts, false).unwrap()
    };
    let (a, b) = (run(1), run(3));
    assert_eq!(a.samples, b.samples);
    for (x, y) in a.leaves.weights.iter().zip(&b.leaves.weights) {
        assert!((x - y).abs() < 1e-6, "a szálak száma nem változtathatja az eredményt");
    }
}

#[test]
fn leaves_files_round_trip_through_the_loader() {
    let mut leaves = LinearLeaves::zero();
    leaves.weights[3] = 4.5;
    let path = temp_file("leaves").with_file_name("leaves.json");
    std::fs::write(&path, leaves.to_json(serde_json::json!({"games": 1})).to_string()).unwrap();
    let loaded = LinearLeaves::load(&path).unwrap();
    assert_eq!(loaded.weights, leaves.weights);
    std::fs::write(&path, "{\"weights\": [1, 2]}").unwrap();
    assert!(LinearLeaves::load(&path).is_err());
    assert!(LinearLeaves::load(&path.with_file_name("nincs.json")).is_err());
}
