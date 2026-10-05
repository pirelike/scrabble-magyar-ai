mod common;

use common::reference::{self, PlayKey};
use pg_scrabble::board::{Board, Coord, Direction};
use pg_scrabble::lexicon::Lexicon;
use pg_scrabble::movegen::MoveGenerator;
use pg_scrabble::rack::Rack;
use pg_scrabble::rng::Rng;
use pg_scrabble::rules::{Alphabet, BoardLayout, GameConfig};
use pg_scrabble::tile::Square;
use std::collections::HashSet;

fn standard() -> GameConfig {
    GameConfig::standard()
}

fn small_config() -> GameConfig {
    let layout = BoardLayout::parse(
        "Small 9x9",
        "\
T..d.d..T
.D..t..D.
..d...d..
d..D.D..d
.t..D..t.
d..D.D..d
..d...d..
.D..t..D.
T..d.d..T",
    )
    .expect("the test layout is well-formed");
    GameConfig {
        layout,
        ..GameConfig::standard()
    }
}

fn lexicon(config: &GameConfig) -> Lexicon {
    reference::lexicon(config)
}

fn rack(config: &GameConfig, s: &str) -> Rack {
    Rack::parse(&config.alphabet, s).expect("test racks use A-Z and ?")
}

fn compare(config: &GameConfig, lexicon: &Lexicon, board: &Board, r: &Rack) {
    let mut gen = MoveGenerator::new(config);
    let fast: Vec<PlayKey> = {
        let mut v: Vec<PlayKey> = gen
            .generate(board, r, lexicon)
            .iter()
            .map(PlayKey::of)
            .collect();
        v.sort();
        v
    };
    let slow = reference::generate(board, r, lexicon, config);

    let fast_set: HashSet<&PlayKey> = fast.iter().collect();
    let slow_set: HashSet<&PlayKey> = slow.iter().collect();

    let missing: Vec<String> = slow_set
        .difference(&fast_set)
        .map(|p| p.describe(config))
        .collect();
    let spurious: Vec<String> = fast_set
        .difference(&slow_set)
        .map(|p| p.describe(config))
        .collect();

    let show = |v: &[String]| -> String {
        let mut v = v.to_vec();
        v.sort();
        v.truncate(12);
        v.join("\n    ")
    };

    assert!(
        missing.is_empty(),
        "generator missed {} legal plays for rack {} on\n{}\n  missing:\n    {}",
        missing.len(),
        r.to_text(&config.alphabet),
        board.display(&config.layout, &config.alphabet),
        show(&missing),
    );
    assert!(
        spurious.is_empty(),
        "generator produced {} plays the rules forbid, for rack {} on\n{}\n  spurious:\n    {}",
        spurious.len(),
        r.to_text(&config.alphabet),
        board.display(&config.layout, &config.alphabet),
        show(&spurious),
    );

    assert_eq!(
        fast.len(),
        fast_set.len(),
        "generator emitted the same play more than once"
    );
}

fn board_of(config: &GameConfig, text: &str) -> Board {
    Board::parse(&config.layout, &config.alphabet, text).expect("test board should parse")
}

#[test]
fn opening_plays_match_the_reference() {
    let config = standard();
    let lex = lexicon(&config);
    let board = Board::new(&config.layout);
    for r in ["CAT", "STONE", "AB", "QIZ", "AE?"] {
        compare(&config, &lex, &board, &rack(&config, r));
    }
}

#[test]
fn every_opening_play_covers_the_star() {
    let config = standard();
    let lex = lexicon(&config);
    let board = Board::new(&config.layout);
    let mut gen = MoveGenerator::new(&config);
    let plays = gen.generate(&board, &rack(&config, "STONE"), &lex);
    assert!(!plays.is_empty());
    let (sr, sc) = config.layout.start();
    for p in plays {
        assert!(
            p.placements().any(|(c, _)| (c.row, c.col) == (sr, sc)),
            "opening play {p:?} does not cover the star"
        );
    }
}

#[test]
fn a_single_tile_hook_is_found_exactly_once() {
    let config = small_config();
    let lex = lexicon(&config);

    let board = board_of(
        &config,
        "\
.........
.........
.........
..CAT....
.........
.........
.........
.........
.........",
    );
    let r = rack(&config, "S");
    compare(&config, &lex, &board, &r);

    let mut gen = MoveGenerator::new(&config);
    let plays: Vec<_> = gen.generate(&board, &r, &lex).to_vec();
    let cats: Vec<_> = plays
        .iter()
        .filter(|p| p.word_text(&config.alphabet) == "CATS")
        .collect();
    assert_eq!(cats.len(), 1, "CATS should appear once, got {cats:?}");
}

#[test]
fn plays_extending_both_sides_of_a_word_are_found() {
    let config = small_config();
    let lex = lexicon(&config);
    let board = board_of(
        &config,
        "\
.........
.........
.........
...EAT...
.........
.........
.........
.........
.........",
    );

    let r = rack(&config, "SS");
    compare(&config, &lex, &board, &r);

    let mut gen = MoveGenerator::new(&config);
    let words: HashSet<String> = gen
        .generate(&board, &r, &lex)
        .iter()
        .map(|p| p.word_text(&config.alphabet))
        .collect();
    assert!(words.contains("SEATS"), "expected SEATS among {words:?}");
}

#[test]
fn parallel_plays_forming_many_cross_words_match() {
    let config = small_config();
    let lex = lexicon(&config);
    let board = board_of(
        &config,
        "\
.........
.........
...CAT...
.........
.........
.........
.........
.........
.........",
    );

    for r in ["AT", "ATE", "SO", "AB?"] {
        compare(&config, &lex, &board, &rack(&config, r));
    }
}

#[test]
fn blanks_generate_both_readings() {
    let config = small_config();
    let lex = lexicon(&config);
    let board = Board::new(&config.layout);

    let r = rack(&config, "AT?");
    compare(&config, &lex, &board, &r);

    let mut gen = MoveGenerator::new(&config);
    let ats: Vec<_> = gen
        .generate(&board, &r, &lex)
        .iter()
        .filter(|p| {
            p.word_text(&config.alphabet).eq_ignore_ascii_case("at")
                && p.direction() == Direction::Horizontal
                && p.coord() == Coord::new(4, 4)
        })
        .copied()
        .collect();
    assert_eq!(
        ats.len(),
        3,
        "expected all three readings of AT, got {ats:?}"
    );
    assert_eq!(
        ats.iter()
            .filter(|p| p.word().iter().any(|s| s.is_blank()))
            .count(),
        2,
        "two of the three readings use the blank"
    );

    let best = ats.iter().map(|p| p.score()).max().unwrap();
    let worst = ats.iter().map(|p| p.score()).min().unwrap();
    assert!(best > worst, "a blank must cost points");
}

#[test]
fn a_crowded_board_matches_the_reference() {
    let config = small_config();
    let lex = lexicon(&config);
    let board = board_of(
        &config,
        "\
..STONE..
..T...A..
..O...T..
..NOTE...
..E...N..
......O..
..CAT.T..
....O....
....W....",
    );
    for r in ["AEI", "RS", "D?", "BAT"] {
        compare(&config, &lex, &board, &rack(&config, r));
    }
}

#[test]
fn a_full_rack_on_a_real_board_matches_the_reference() {
    let config = standard();
    let lex = lexicon(&config);
    let mut board = Board::new(&config.layout);
    for (i, c) in "STONE".chars().enumerate() {
        let (letter, _) = config.alphabet.parse_char(c).unwrap();
        board.set(7, 5 + i, Square::letter(letter));
    }
    compare(&config, &lex, &board, &rack(&config, "AEINRST"));
}

#[test]
fn generated_plays_are_scored_correctly() {
    let config = standard();
    let lex = lexicon(&config);
    let board = Board::new(&config.layout);
    let mut gen = MoveGenerator::new(&config);

    let plays: Vec<_> = gen.generate(&board, &rack(&config, "CAT"), &lex).to_vec();
    let cat = plays
        .iter()
        .find(|p| {
            p.word_text(&config.alphabet) == "CAT"
                && p.direction() == Direction::Horizontal
                && p.coord() == Coord::new(7, 7)
        })
        .expect("CAT starting on the star should be generated");

    assert_eq!(cat.score(), 10);

    let plays: Vec<_> = gen.generate(&board, &rack(&config, "STONE"), &lex).to_vec();
    let stone = plays
        .iter()
        .find(|p| {
            p.word_text(&config.alphabet) == "STONE"
                && p.direction() == Direction::Horizontal
                && p.coord() == Coord::new(7, 3)
        })
        .expect("STONE spanning the star should be generated");
    assert_eq!(stone.score(), 12);
}

#[test]
fn a_bingo_gets_its_bonus() {
    let config = standard();
    let lex = lexicon(&config);
    let board = Board::new(&config.layout);
    let mut gen = MoveGenerator::new(&config);
    let plays: Vec<_> = gen
        .generate(&board, &rack(&config, "RETINA"), &lex)
        .to_vec();
    let six = plays
        .iter()
        .find(|p| p.word_text(&config.alphabet) == "RETINA")
        .expect("RETINA is in the test lexicon");
    assert_eq!(six.tiles_used(), 6);
    assert!(
        !six.is_bingo(config.rack_size),
        "six tiles from a six-tile rack is not a bingo"
    );
    assert!(
        six.score() < 50,
        "a six-tile play must not collect the bingo bonus, got {}",
        six.score()
    );
}

#[test]
fn an_empty_rack_generates_nothing() {
    let config = standard();
    let lex = lexicon(&config);
    let board = Board::new(&config.layout);
    let mut gen = MoveGenerator::new(&config);
    assert!(gen.generate(&board, &Rack::new(), &lex).is_empty());
}

#[test]
fn an_unplayable_rack_generates_nothing() {
    let config = standard();
    let lex = lexicon(&config);
    let board = Board::new(&config.layout);
    let mut gen = MoveGenerator::new(&config);

    assert!(gen.generate(&board, &rack(&config, "VV"), &lex).is_empty());
}

#[test]
fn random_games_match_the_reference_at_every_turn() {
    let config = small_config();
    let lex = lexicon(&config);
    let mut rng = Rng::seed_from_u64(0xC0FFEE);
    let mut gen = MoveGenerator::new(&config);

    let mut positions_checked = 0;
    for game in 0..8u64 {
        let mut board = Board::new(&config.layout);
        let mut bag = pg_scrabble::bag::Bag::new(&config.distribution, game);
        let mut r = Rack::new();
        bag.refill(&mut r, 5);

        for _turn in 0..14 {
            compare(&config, &lex, &board, &r);
            positions_checked += 1;

            let plays: Vec<_> = gen.generate(&board, &r, &lex).to_vec();
            if plays.is_empty() {
                break;
            }

            let pick = plays[rng.below(plays.len() as u64) as usize];
            pick.apply(&mut board);
            r = pick.leave(&r).expect("a generated play must fit its rack");
            bag.refill(&mut r, 5);
            if r.is_empty() {
                break;
            }
        }
    }
    assert!(
        positions_checked > 40,
        "expected a meaningful sample of positions"
    );
}

#[test]
fn generation_is_deterministic() {
    let config = small_config();
    let lex = lexicon(&config);
    let board = board_of(
        &config,
        "\
.........
.........
...CAT...
.........
.........
.........
.........
.........
.........",
    );
    let r = rack(&config, "AES?");
    let mut gen = MoveGenerator::new(&config);
    let first: Vec<_> = gen.generate(&board, &r, &lex).to_vec();
    let second: Vec<_> = gen.generate(&board, &r, &lex).to_vec();
    assert_eq!(
        first, second,
        "repeated generation must give identical output"
    );

    let mut fresh = MoveGenerator::new(&config);
    assert_eq!(first, fresh.generate(&board, &r, &lex).to_vec());
}

#[test]
fn the_generator_handles_a_board_with_no_room_left() {
    let config = small_config();
    let lex = lexicon(&config);
    let alphabet = Alphabet::english();
    let mut board = Board::new(&config.layout);

    for row in 0..config.height() {
        for col in 0..config.width() {
            board.set(row, col, Square::letter(0));
        }
    }
    let mut gen = MoveGenerator::new(&config);
    let r = Rack::parse(&alphabet, "AEIOU").unwrap();
    assert!(gen.generate(&board, &r, &lex).is_empty());
}

#[cfg(feature = "rayon")]
#[test]
fn parallel_generation_matches_sequential_exactly() {
    let config = small_config();
    let lex = lexicon(&config);
    let mut rng = Rng::seed_from_u64(0xBEEF);
    let mut gen = MoveGenerator::new(&config);

    for game in 0..6u64 {
        let mut board = Board::new(&config.layout);
        let mut bag = pg_scrabble::bag::Bag::new(&config.distribution, game);
        let mut r = Rack::new();
        bag.refill(&mut r, 6);

        for _ in 0..10 {
            let sequential: Vec<_> = gen.generate(&board, &r, &lex).to_vec();
            let parallel: Vec<_> = gen.prepare(&board, &lex).generate_parallel(&r).to_vec();
            assert_eq!(
                sequential, parallel,
                "parallel generation must be order-identical to sequential"
            );

            if sequential.is_empty() {
                break;
            }
            let pick = sequential[rng.below(sequential.len() as u64) as usize];
            pick.apply(&mut board);
            r = pick.leave(&r).expect("a generated play fits its rack");
            bag.refill(&mut r, 6);
            if r.is_empty() {
                break;
            }
        }
    }
}
