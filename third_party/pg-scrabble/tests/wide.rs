//! Tests for the widened letter masks (pg-scrabble patch): alphabets of more
//! than 30 letters, multi-character tile labels, blanks, serialisation.
//!
//! The Hungarian tile set below is copied from
//! `scrabble-magyar-ai/src/engine/tiles.rs` (`TILE_DISTRIBUTION`, blank
//! excluded): 38 letters, seven of them digraph tiles (SZ CS GY LY NY TY ZS)
//! that are single tiles worth their own points. Upstream's limit was 30.

mod common;

use common::reference::{self, PlayKey};
use pg_scrabble::bag::Bag;
use pg_scrabble::board::{Board, Direction};
use pg_scrabble::game::{Game, Turn};
use pg_scrabble::lexicon::{Lexicon, LexiconError, MAX_GADDAG_LETTERS};
use pg_scrabble::movegen::MoveGenerator;
use pg_scrabble::rack::Rack;
use pg_scrabble::rng::Rng;
use pg_scrabble::rules::{Alphabet, BoardLayout, GameConfig, TileDistribution};
use pg_scrabble::tile::{Square, Tile, BLANK_CODE, MAX_LETTERS, TILE_CODES};
use std::collections::HashSet;

/// (label, points, count), the blank excluded. Order = letter index.
const HU: [(&str, i32, u8); 38] = [
    ("A", 1, 6),
    ("E", 1, 6),
    ("K", 1, 6),
    ("T", 1, 5),
    ("Á", 1, 4),
    ("L", 1, 4),
    ("N", 1, 4),
    ("R", 1, 4),
    ("I", 1, 3),
    ("M", 1, 3),
    ("O", 1, 3),
    ("S", 1, 3),
    ("B", 2, 3),
    ("D", 2, 3),
    ("G", 2, 3),
    ("Ó", 2, 3),
    ("É", 3, 3),
    ("H", 3, 2),
    ("SZ", 3, 2),
    ("V", 3, 2),
    ("F", 4, 2),
    ("GY", 4, 2),
    ("J", 4, 2),
    ("Ö", 4, 2),
    ("P", 4, 2),
    ("U", 4, 2),
    ("Ü", 4, 2),
    ("Z", 4, 2),
    ("C", 5, 1),
    ("Í", 5, 1),
    ("NY", 5, 1),
    ("CS", 7, 1),
    ("Ő", 7, 1),
    ("Ú", 7, 1),
    ("Ű", 7, 1),
    ("LY", 8, 1),
    ("ZS", 8, 1),
    ("TY", 10, 1),
];

/// Letters 0..=30 fit upstream's old limit; these do not.
const FIRST_WIDE: u8 = 31;

fn hu_alphabet() -> Alphabet {
    Alphabet::new(
        "Hungarian",
        HU.iter().map(|t| t.0.to_string()).collect(),
        HU.iter().map(|t| t.1).collect(),
    )
}

fn hu_distribution() -> TileDistribution {
    let counts: Vec<u8> = HU.iter().map(|t| t.2).collect();
    TileDistribution::new(&counts, 2)
}

fn hu_config(layout: BoardLayout, rack_size: usize) -> GameConfig {
    GameConfig {
        alphabet: hu_alphabet(),
        distribution: hu_distribution(),
        layout,
        rack_size,
        ..GameConfig::standard()
    }
}

fn small_layout() -> BoardLayout {
    BoardLayout::parse(
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
    .expect("the test layout is well-formed")
}

/// Greedy longest-match tokeniser: a two-character label is one tile.
/// (Ambiguous splits such as K É SZ S É G are not used in the word list.)
fn tokenize(alphabet: &Alphabet, word: &str) -> Vec<u8> {
    let chars: Vec<char> = word.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len() {
            let two: String = chars[i..i + 2].iter().collect();
            if let Some(l) = alphabet.index_of(&two) {
                out.push(l);
                i += 2;
                continue;
            }
        }
        let one: String = chars[i..i + 1].iter().collect();
        out.push(
            alphabet
                .index_of(&one)
                .unwrap_or_else(|| panic!("no tile for {one:?} in {word:?}")),
        );
        i += 1;
    }
    out
}

const STEMS: &str = "\
ALMA ASZTAL AKAR ÁLOM ÁRNYÉK BARÁT BÁNYA BESZÉD BOLT CSAK CSÁSZÁR CSEND CSÖK CSÚCS CSŰR \
DOLOG DÖNT ERDŐ ÉJSZAKA ÉLET ÉRTÉK FALU FÉNY FÜST FŰ FÜRDŐ GYERMEK GYŰRŰ GYÓGY HÁZ HÍD HŐ HÚS \
IDŐ ISKOLA KÉSZ KŐ KÖNYV LÁTÓ LÓ LYUK LYUKAS MAGYAR MÉZ NYÁR NYELV NYÚL NYŰG ÓRA ÖSSZE ŐSZ \
PÁLYA PÉNZ RÓKA SZÍV SZÓ SZŰK TÁNC TEHÉN TÉLI TŰZ TÜKÖR TYÚK ÚJ ÚT ÜVEG ŰR VÍZ VŰ ZÁR ZSÁK \
ZSEB ZSÍR ZSŰR LÚD KÉZ KÖZ HOLD VÖLGY ZÖLD ÚSZ ÚSZÓ FÚJ FÜL GYŰL JÓ MÚZSA PUSZTA RÉSZ RÚZS \
BÚZA CÉL CIPŐ CUKOR NÉZ NYIT TYÚKÓL LYÚK ŰZ ŐZ OLÓ ELÓ ÓLOM ÚJÓ SZŐLŐ TŰ ŰZÖTT";

const SUFFIXES: [&str; 5] = ["", "K", "T", "AK", "NAK"];

/// A made-up, Hungarian-looking word list: stems x a few suffixes.
fn hu_words(alphabet: &Alphabet) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    for stem in STEMS.split_whitespace() {
        for suffix in SUFFIXES {
            let w = format!("{stem}{suffix}");
            let t = tokenize(alphabet, &w);
            if (2..=8).contains(&t.len()) {
                out.push(t);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn hu_lexicon(alphabet: &Alphabet) -> Lexicon {
    Lexicon::from_encoded("hu-test", alphabet, hu_words(alphabet)).expect("38 letters must build")
}

fn compare(config: &GameConfig, lexicon: &Lexicon, board: &Board, r: &Rack) -> Vec<PlayKey> {
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
    let describe = |s: &HashSet<&PlayKey>, t: &HashSet<&PlayKey>| -> Vec<String> {
        let mut v: Vec<String> = s.difference(t).map(|p| p.describe(config)).collect();
        v.sort();
        v.truncate(10);
        v
    };
    let missing = describe(&slow_set, &fast_set);
    let spurious = describe(&fast_set, &slow_set);
    assert!(
        missing.is_empty() && spurious.is_empty(),
        "generator and naive reference disagree for rack {} on\n{}\n  missing: {missing:#?}\n  spurious: {spurious:#?}",
        r.to_text(&config.alphabet),
        board.display(&config.layout, &config.alphabet),
    );
    assert_eq!(fast.len(), fast_set.len(), "a play was emitted twice");
    fast
}

/// A rack that is playable: tokens of a random word, random filler tiles and
/// (often) a blank. Biased towards the wide letters on purpose.
fn sample_rack(rng: &mut Rng, words: &[Vec<u8>], letters: usize, size: usize) -> Rack {
    let mut r = Rack::new();
    let word = &words[rng.below(words.len() as u64) as usize];
    for &l in word.iter().take(size.saturating_sub(1)) {
        r.add(Tile::letter(l));
    }
    while r.len() < size {
        if r.blanks() == 0 && rng.below(3) == 0 {
            r.add(Tile::BLANK);
        } else {
            r.add(Tile::letter(rng.below(letters as u64) as u8));
        }
    }
    if rng.below(5) == 0 {
        // two blanks sometimes
        r.remove(Tile::letter(word[0]));
        r.add(Tile::BLANK);
    }
    r
}

#[derive(Default, Debug)]
struct Coverage {
    positions: usize,
    plays: usize,
    positions_with_blank: usize,
    plays_with_blank: usize,
    plays_using_wide_letter: usize,
    plays_using_blank_as_wide_letter: usize,
    plays_through_wide_tile_on_board: usize,
}

fn tally(cov: &mut Coverage, board: &Board, r: &Rack, plays: &[pg_scrabble::movegen::Play]) {
    cov.positions += 1;
    cov.plays += plays.len();
    if r.has_blank() {
        cov.positions_with_blank += 1;
    }
    for p in plays {
        let sq = p.word();
        if p.placements().any(|(_, s)| s.is_blank()) {
            cov.plays_with_blank += 1;
        }
        if p.placements()
            .any(|(_, s)| s.index_unchecked() >= FIRST_WIDE)
        {
            cov.plays_using_wide_letter += 1;
        }
        if p.placements()
            .any(|(_, s)| s.is_blank() && s.index_unchecked() >= FIRST_WIDE)
        {
            cov.plays_using_blank_as_wide_letter += 1;
        }
        let placed: HashSet<(usize, usize)> = p.placements().map(|(c, _)| (c.row, c.col)).collect();
        let mut through = false;
        for (i, s) in sq.iter().enumerate() {
            let c = match p.direction() {
                Direction::Horizontal => (p.coord().row, p.coord().col + i),
                Direction::Vertical => (p.coord().row + i, p.coord().col),
            };
            if !placed.contains(&c) {
                let on_board = board.get(c.0, c.1);
                assert_eq!(
                    on_board.index(),
                    s.index(),
                    "word does not follow the board"
                );
                if s.index_unchecked() >= FIRST_WIDE {
                    through = true;
                }
            }
        }
        if through {
            cov.plays_through_wide_tile_on_board += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// the alphabet itself
// ---------------------------------------------------------------------------

#[test]
fn the_hungarian_tile_set_exceeds_the_old_limits() {
    let a = hu_alphabet();
    assert_eq!(a.len(), 38);
    assert!(a.len() > 30, "upstream 0.1.0 rejected anything above 30");
    assert_eq!(a.full_mask().count_ones(), 38);
    assert_eq!(a.full_mask(), (1u64 << 38) - 1);
    let d = hu_distribution();
    assert_eq!(d.total(), 100);
    assert_eq!(d.count(Tile::BLANK), 2);
    assert_eq!(d.count(Tile::letter(37)), 1, "TY");
    assert_eq!(a.display(18), "SZ");
    assert_eq!(a.score(37), 10);
    // wide letters are used by the word list
    let words = hu_words(&a);
    let used: HashSet<u8> = words.iter().flatten().copied().collect();
    assert_eq!(
        used.len(),
        38,
        "every one of the 38 letters occurs in the test lexicon"
    );
    assert!(words.len() > 300);
}

#[test]
fn an_alphabet_of_62_letters_builds_and_63_is_refused() {
    let letters = |n: usize| -> Vec<String> { (0..n).map(|i| format!("L{i:02}")).collect() };
    assert_eq!(MAX_GADDAG_LETTERS, 62);
    assert_eq!(MAX_LETTERS, 63);
    let a62 = Alphabet::new("x62", letters(62), vec![1; 62]);
    let words: Vec<Vec<u8>> = (0..62u8).map(|l| vec![l, 61 - l / 2, l]).collect();
    let lex = Lexicon::from_encoded("x62", &a62, words.clone()).expect("62 letters is the maximum");
    assert_eq!(lex.separator(), 62, "the GADDAG separator is letter 62");
    assert_eq!(lex.alphabet_len(), 62);
    assert_eq!(lex.letter_mask(), (1u64 << 62) - 1);
    for w in &words {
        assert!(lex.contains(w));
        assert!(!lex.contains(&w[..2]));
    }
    lex.validate().unwrap();

    let a63 = Alphabet::new("x63", letters(63), vec![1; 63]);
    let err = Lexicon::from_encoded("x63", &a63, vec![vec![0, 1]]).unwrap_err();
    assert_eq!(err, LexiconError::AlphabetTooLarge { len: 63, max: 62 });
}

#[test]
#[should_panic(expected = "exceeds MAX_LETTERS")]
fn a_distribution_beyond_the_limit_still_panics() {
    let _ = TileDistribution::new(&[1u8; MAX_LETTERS + 1], 2);
}

// ---------------------------------------------------------------------------
// lexicon over 38 letters
// ---------------------------------------------------------------------------

#[test]
fn a_38_letter_lexicon_answers_membership_exactly() {
    let a = hu_alphabet();
    let words = hu_words(&a);
    let lex = hu_lexicon(&a);
    assert_eq!(lex.alphabet_len(), 38);
    assert_eq!(lex.separator(), 38);
    assert_eq!(lex.word_count(), words.len() as u64);
    assert_eq!(lex.dawg().collect_words(), words);
    lex.validate().unwrap();

    let set: HashSet<&Vec<u8>> = words.iter().collect();
    let mut rng = Rng::seed_from_u64(5);
    for w in &words {
        assert!(lex.contains(w));
        let mut cut = w.clone();
        cut.pop();
        assert_eq!(lex.contains(&cut), set.contains(&cut));
    }
    for _ in 0..4000 {
        let len = 2 + rng.below(4) as usize;
        let w: Vec<u8> = (0..len).map(|_| rng.below(38) as u8).collect();
        assert_eq!(lex.contains(&w), set.contains(&w), "{w:?}");
    }

    // GADDAG holds every split, including those through the wide letters
    for w in words.iter().filter(|w| w.iter().any(|&l| l >= FIRST_WIDE)) {
        for split in 1..=w.len() {
            let mut path: Vec<u8> = w[..split].iter().rev().copied().collect();
            path.push(lex.separator());
            path.extend_from_slice(&w[split..]);
            assert!(
                lex.gaddag().accepts_sequence(path),
                "GADDAG misses {w:?} @ {split}"
            );
        }
    }
    assert_eq!(a.display(tokenize(&a, "ASZTAL")[1]), "SZ");
    assert_eq!(lex.decode(&a, &tokenize(&a, "ASZTAL")), "ASZTAL");
}

#[test]
fn cross_sets_over_38_letters_agree_with_brute_force() {
    let a = hu_alphabet();
    let words = hu_words(&a);
    let lex = hu_lexicon(&a);
    let mut rng = Rng::seed_from_u64(9);
    let mut checked_wide = 0;
    for _ in 0..1500 {
        let w = &words[rng.below(words.len() as u64) as usize];
        let hole = rng.below(w.len() as u64) as usize;
        let (p, s) = (&w[..hole], &w[hole + 1..]);
        let mut want = 0u64;
        for letter in 0..38u8 {
            let mut cand = p.to_vec();
            cand.push(letter);
            cand.extend_from_slice(s);
            if lex.contains(&cand) {
                want |= 1u64 << letter;
            }
        }
        let got = lex.cross_set(p, s);
        assert_eq!(got, want, "{p:?}_{s:?}");
        if got >> FIRST_WIDE != 0 {
            checked_wide += 1;
        }
    }
    assert!(
        checked_wide > 30,
        "the sample must hit wide cross-set bits ({checked_wide})"
    );
    assert_eq!(lex.cross_set(&[], &[]), lex.letter_mask());
}

#[test]
fn a_38_letter_lexicon_survives_serialisation() {
    let a = hu_alphabet();
    let lex = hu_lexicon(&a).with_name("hu-roundtrip");
    let bytes = lex.to_bytes();
    assert_eq!(&bytes[..4], b"SLEX");
    assert_eq!(bytes[4], 38, "alphabet length is stored");
    assert_eq!(bytes[5], 38, "separator is stored");

    let back = Lexicon::from_bytes(&bytes).expect("round trip");
    assert_eq!(back.name(), "hu-roundtrip");
    assert_eq!(back.alphabet_len(), 38);
    assert_eq!(back.separator(), 38);
    assert_eq!(back.word_count(), lex.word_count());
    assert_eq!(back.dawg(), lex.dawg());
    assert_eq!(back.gaddag(), lex.gaddag());
    assert_eq!(back.size_bytes(), lex.size_bytes());
    back.validate().unwrap();
    assert_eq!(back.dawg().collect_words(), lex.dawg().collect_words());
    assert_eq!(back.letter_mask(), lex.letter_mask());

    // a lexicon loaded from bytes must generate exactly the same moves
    let config = hu_config(small_layout(), 5);
    let mut rng = Rng::seed_from_u64(1);
    let words = hu_words(&a);
    let mut g1 = MoveGenerator::new(&config);
    let mut g2 = MoveGenerator::new(&config);
    for _ in 0..30 {
        let r = sample_rack(&mut rng, &words, 38, 5);
        let b = Board::new(&config.layout);
        assert_eq!(
            g1.generate(&b, &r, &lex).to_vec(),
            g2.generate(&b, &r, &back).to_vec()
        );
    }

    let mut cut = bytes.clone();
    cut.truncate(bytes.len() - 3);
    assert!(Lexicon::from_bytes(&cut).is_err());
}

// ---------------------------------------------------------------------------
// fast generator == naive reference on the Hungarian tile set
// ---------------------------------------------------------------------------

#[test]
fn hungarian_digraph_tiles_play_as_single_tiles() {
    let config = hu_config(small_layout(), 5);
    let a = &config.alphabet;
    let lex = hu_lexicon(a);
    let board = Board::new(&config.layout);
    let sz = a.index_of("SZ").unwrap();
    let rack = Rack::from_tiles(
        ["A", "SZ", "T", "A", "L"]
            .iter()
            .map(|l| Tile::letter(a.index_of(l).unwrap())),
    );
    assert_eq!(rack.len(), 5);
    assert!(rack.contains(Tile::letter(sz)));
    let plays = compare(&config, &lex, &board, &rack);
    assert!(!plays.is_empty());

    let mut gen = MoveGenerator::new(&config);
    let asztal: Vec<_> = gen
        .generate(&board, &rack, &lex)
        .iter()
        .filter(|p| p.word_text(a) == "ASZTAL")
        .copied()
        .collect();
    assert!(
        !asztal.is_empty(),
        "ASZTAL (A SZ T A L = 5 tiles) should be playable"
    );
    for p in &asztal {
        assert_eq!(p.word().len(), 5, "the SZ is ONE square");
        assert_eq!(p.tiles_used(), 5);
        assert!(
            p.is_bingo(config.rack_size),
            "5 of 5 is a bingo on this config"
        );
    }
    // A1 SZ3 T1 A1 L1 = 7, doubled somewhere by the premium squares, plus 50
    assert!(asztal.iter().all(|p| p.score() >= 7 + 50));
    assert_eq!(asztal[0].to_text(a).split(' ').nth(1), Some("ASZTAL"));
}

#[test]
fn random_hungarian_games_match_the_naive_generator_at_every_turn() {
    let config = hu_config(small_layout(), 5);
    let a = &config.alphabet;
    let lex = hu_lexicon(a);
    let words = hu_words(a);
    let mut rng = Rng::seed_from_u64(0x4D41_4759);
    let mut gen = MoveGenerator::new(&config);
    let mut cov = Coverage::default();

    for _game in 0..14 {
        let mut board = Board::new(&config.layout);
        for _turn in 0..10 {
            let r = sample_rack(&mut rng, &words, 38, 5);
            let fast = compare(&config, &lex, &board, &r);
            let plays = gen.generate(&board, &r, &lex).to_vec();
            assert_eq!(fast.len(), plays.len());
            tally(&mut cov, &board, &r, &plays);
            if plays.is_empty() {
                continue;
            }
            // prefer plays that put wide letters on the board so later
            // positions have wide tiles to hook onto
            let wide: Vec<_> = plays
                .iter()
                .filter(|p| {
                    p.placements()
                        .any(|(_, s)| s.index_unchecked() >= FIRST_WIDE)
                })
                .collect();
            let pick = if !wide.is_empty() && rng.below(2) == 0 {
                *wide[rng.below(wide.len() as u64) as usize]
            } else {
                plays[rng.below(plays.len() as u64) as usize]
            };
            pick.apply(&mut board);
        }
    }

    eprintln!("coverage: {cov:?}");
    assert!(cov.positions >= 140);
    assert!(
        cov.plays > 3000,
        "meaningful number of plays: {}",
        cov.plays
    );
    assert!(
        cov.positions_with_blank > 40,
        "blanks on racks: {}",
        cov.positions_with_blank
    );
    assert!(
        cov.plays_with_blank > 500,
        "plays using a blank: {}",
        cov.plays_with_blank
    );
    assert!(
        cov.plays_using_wide_letter > 300,
        "plays placing letters >= 31 (CS Ő Ú Ű LY ZS TY): {}",
        cov.plays_using_wide_letter
    );
    assert!(
        cov.plays_using_blank_as_wide_letter > 50,
        "a blank standing for a wide letter: {}",
        cov.plays_using_blank_as_wide_letter
    );
    assert!(
        cov.plays_through_wide_tile_on_board > 30,
        "plays hooking through a wide tile already on the board: {}",
        cov.plays_through_wide_tile_on_board
    );
}

#[test]
fn hungarian_positions_with_blanks_match_the_naive_generator() {
    let config = hu_config(small_layout(), 5);
    let a = &config.alphabet;
    let lex = hu_lexicon(a);
    let ix = |l: &str| a.index_of(l).unwrap();

    // LYUK across row 4 and CSÖK down column 4, sharing the K: wide tiles
    // (LY = 35, CS = 31) already on the board for the plays to hook onto.
    let mut board = Board::new(&config.layout);
    for (i, l) in ["LY", "U", "K"].iter().enumerate() {
        board.set(4, 2 + i, Square::letter(ix(l)));
    }
    for (i, l) in ["CS", "Ö"].iter().enumerate() {
        board.set(2 + i, 4, Square::letter(ix(l)));
    }

    let rack_of = |spec: &[&str]| -> Rack {
        Rack::from_tiles(spec.iter().map(|s| {
            if *s == "?" {
                Tile::BLANK
            } else {
                Tile::letter(ix(s))
            }
        }))
    };
    for spec in [
        vec!["?"],
        vec!["?", "?"],
        vec!["?", "Z", "S"],
        vec!["ZS", "?", "E", "B"],
        vec!["TY", "Ú", "K", "?", "Ő"],
        vec!["?", "?", "CS", "A", "K"],
        vec!["Ő", "Ú", "Ű", "?"],
        vec!["LY", "TY", "ZS", "CS", "?"],
    ] {
        let r = rack_of(&spec);
        let plays = compare(&config, &lex, &board, &r);
        assert!(!plays.is_empty(), "rack {spec:?} should have plays");
    }
    // the empty board too (opening plays, blank as wide letter)
    let empty = Board::new(&config.layout);
    for spec in [
        vec!["?", "A"],
        vec!["?", "Ű", "Z"],
        vec!["?", "?", "S", "Z"],
    ] {
        compare(&config, &lex, &empty, &rack_of(&spec));
    }
}

#[test]
fn a_blank_standing_for_the_widest_letters_scores_zero_and_is_flagged() {
    let config = hu_config(small_layout(), 5);
    let a = &config.alphabet;
    let lex = hu_lexicon(a);
    let board = Board::new(&config.layout);
    let ix = |l: &str| a.index_of(l).unwrap();
    // ?-Ú-T => ÚT with the blank as the Ú or as the T
    let rack = Rack::from_tiles([Tile::BLANK, Tile::letter(ix("T")), Tile::letter(ix("Ú"))]);
    let mut gen = MoveGenerator::new(&config);
    let plays = gen.generate(&board, &rack, &lex).to_vec();
    let blank_as_u_acute: Vec<_> = plays
        .iter()
        .filter(|p| p.word_text(a).eq_ignore_ascii_case("ÚT") || p.word_text(a) == "úT")
        .collect();
    assert!(!blank_as_u_acute.is_empty());
    let blank_as_wide = plays.iter().any(|p| {
        p.placements()
            .any(|(_, s)| s.is_blank() && s.index_unchecked() == ix("Ú"))
    });
    assert!(blank_as_wide, "the blank can stand for Ú (index 33)");
    // a blank never scores its letter: for the same word on the same squares
    // the readings that use the blank score strictly less than the plain one
    let ut = |p: &&pg_scrabble::movegen::Play| p.word_text(a).to_lowercase() == "út";
    let mut compared = 0;
    for plain in plays
        .iter()
        .filter(ut)
        .filter(|p| p.placements().all(|(_, s)| !s.is_blank()))
    {
        for with_blank in plays.iter().filter(ut).filter(|q| {
            q.coord() == plain.coord()
                && q.direction() == plain.direction()
                && q.placements().any(|(_, s)| s.is_blank())
        }) {
            assert!(
                with_blank.score() < plain.score(),
                "blank reading {} should score below {}",
                with_blank.score(),
                plain.score()
            );
            compared += 1;
        }
    }
    assert!(
        compared > 0,
        "ÚT should be playable both with and without the blank"
    );
}

#[test]
fn a_62_letter_alphabet_matches_the_naive_generator_including_blanks() {
    let letters: Vec<String> = (0..62).map(|i| format!("L{i:02}")).collect();
    let mut scores = vec![1; 62];
    for (i, s) in scores.iter_mut().enumerate() {
        *s = 1 + (i % 7) as i32;
    }
    let alphabet = Alphabet::new("x62", letters, scores);
    let dist = TileDistribution::new(&[2u8; 62], 2);
    let config = GameConfig {
        alphabet: alphabet.clone(),
        distribution: dist,
        layout: small_layout(),
        rack_size: 4,
        ..GameConfig::standard()
    };

    // words over the full 0..62 range, favouring the top of it
    let mut rng = Rng::seed_from_u64(62);
    let mut words: Vec<Vec<u8>> = Vec::new();
    for _ in 0..900 {
        let len = 2 + rng.below(3) as usize;
        let w: Vec<u8> = (0..len)
            .map(|_| {
                if rng.below(3) == 0 {
                    50 + rng.below(12) as u8
                } else {
                    rng.below(62) as u8
                }
            })
            .collect();
        words.push(w);
    }
    // every letter appears at least once, including index 61
    for l in 0..62u8 {
        words.push(vec![l, 61 - (l % 5)]);
        words.push(vec![61, l]);
    }
    words.sort();
    words.dedup();
    let lex = Lexicon::from_encoded("x62", &alphabet, words.clone()).unwrap();
    assert_eq!(lex.separator(), 62);

    let mut gen = MoveGenerator::new(&config);
    let mut saw_top = 0;
    let mut saw_blank_top = 0;
    for _game in 0..8 {
        let mut board = Board::new(&config.layout);
        for _turn in 0..8 {
            let r = sample_rack(&mut rng, &words, 62, 4);
            let fast = compare(&config, &lex, &board, &r);
            assert!(fast.len() < 100_000);
            let plays = gen.generate(&board, &r, &lex).to_vec();
            for p in &plays {
                if p.placements().any(|(_, s)| s.index_unchecked() == 61) {
                    saw_top += 1;
                }
                if p.placements()
                    .any(|(_, s)| s.is_blank() && s.index_unchecked() == 61)
                {
                    saw_blank_top += 1;
                }
            }
            if plays.is_empty() {
                continue;
            }
            let pick = plays[rng.below(plays.len() as u64) as usize];
            pick.apply(&mut board);
        }
    }
    assert!(saw_top > 50, "letter 61 was exercised ({saw_top})");
    assert!(
        saw_blank_top > 5,
        "a blank as letter 61 was exercised ({saw_blank_top})"
    );
}

#[cfg(feature = "rayon")]
#[test]
fn parallel_generation_matches_sequential_on_hungarian() {
    let config = hu_config(small_layout(), 5);
    let a = &config.alphabet;
    let lex = hu_lexicon(a);
    let words = hu_words(a);
    let mut rng = Rng::seed_from_u64(0xBEEF);
    let mut gen = MoveGenerator::new(&config);
    for _game in 0..5 {
        let mut board = Board::new(&config.layout);
        for _ in 0..8 {
            let r = sample_rack(&mut rng, &words, 38, 5);
            let sequential = gen.generate(&board, &r, &lex).to_vec();
            let parallel = gen.prepare(&board, &lex).generate_parallel(&r).to_vec();
            assert_eq!(sequential, parallel);
            if sequential.is_empty() {
                break;
            }
            sequential[rng.below(sequential.len() as u64) as usize].apply(&mut board);
        }
    }
}

// ---------------------------------------------------------------------------
// whole games / bag / rack with the blank at code 63
// ---------------------------------------------------------------------------

#[test]
fn the_hungarian_bag_and_rack_keep_every_tile_and_the_blank_code() {
    let dist = hu_distribution();
    assert_eq!(BLANK_CODE as usize, 63);
    assert_eq!(dist.counts().len(), TILE_CODES);
    assert_eq!(dist.counts()[BLANK_CODE as usize], 2);
    let mut bag = Bag::new(&dist, 3);
    assert_eq!(bag.len(), 100);
    let mut rack = Rack::new();
    let mut drawn = Vec::new();
    while let Some(t) = bag.draw() {
        drawn.push(t);
        rack.add(t);
    }
    assert_eq!(rack.counts(), dist.counts());
    assert_eq!(rack.blanks(), 2);
    let mask = rack.mask();
    assert_eq!(
        mask.count_ones(),
        38,
        "all 38 letters present, blank excluded"
    );
    assert_eq!(mask, (1u64 << 38) - 1);
    assert_eq!(drawn.iter().filter(|t| t.is_blank()).count(), 2);
}

fn census(game: &Game) -> [u8; TILE_CODES] {
    let mut counts = *game.bag().counts();
    for player in game.players() {
        for (code, n) in player.rack.counts().iter().enumerate() {
            counts[code] += n;
        }
    }
    for square in game.board().squares() {
        if square.is_occupied() {
            let tile = if square.is_blank() {
                Tile::BLANK
            } else {
                Tile::letter(square.index_unchecked())
            };
            counts[tile.code() as usize] += 1;
        }
    }
    counts
}

#[test]
fn full_hungarian_games_conserve_tiles_and_terminate() {
    let config = hu_config(BoardLayout::standard(), 7);
    let a = config.alphabet.clone();
    let lex = hu_lexicon(&a);
    let start = *config.distribution.counts();
    let mut blanks_played = 0;
    let mut wide_played = 0;
    for seed in 0..6u64 {
        let mut generator = MoveGenerator::new(&config);
        let mut game = Game::new(config.clone(), &["anna", "bela"], seed);
        let mut turns = 0;
        while !game.is_over() && turns < 300 {
            let best = game
                .legal_plays(&mut generator, &lex)
                .iter()
                .max_by_key(|p| p.score())
                .copied();
            let turn = match best {
                Some(play) => {
                    blanks_played += play.placements().filter(|(_, s)| s.is_blank()).count();
                    wide_played += play
                        .placements()
                        .filter(|(_, s)| s.index_unchecked() >= FIRST_WIDE)
                        .count();
                    Turn::Place(play)
                }
                None if game.bag().len() >= game.config().min_exchange_bag => {
                    Turn::Exchange(*game.current_rack())
                }
                None => Turn::Pass,
            };
            game.apply(turn, &lex).expect("the chosen turn is legal");
            turns += 1;
        }
        assert!(game.is_over(), "seed {seed}: game must terminate");
        assert_eq!(census(&game), start, "seed {seed}: tiles lost or invented");
    }
    assert!(wide_played > 0, "wide letters were played ({wide_played})");
    eprintln!("blanks played: {blanks_played}, wide tiles played: {wide_played}");
}

#[cfg(feature = "ai")]
#[test]
fn the_ai_stack_runs_on_the_hungarian_alphabet() {
    use pg_scrabble::endgame::EndgameSolver;
    use pg_scrabble::eval::StaticEvaluator;
    use pg_scrabble::sim::{SimOptions, Simulator};

    let config = hu_config(small_layout(), 5);
    let a = &config.alphabet;
    let lex = hu_lexicon(a);
    let ix = |l: &str| a.index_of(l).unwrap();
    let mut board = Board::new(&config.layout);
    for (i, l) in ["CS", "Ő", "SZ"].iter().enumerate() {
        board.set(4, 2 + i, Square::letter(ix(l)));
    }
    let rack = Rack::from_tiles([
        Tile::letter(ix("Ű")),
        Tile::letter(ix("TY")),
        Tile::letter(ix("Ú")),
        Tile::letter(ix("K")),
        Tile::BLANK,
    ]);

    let mut sim = Simulator::new(&config, SimOptions::fast());
    let results = sim.run(&board, &rack, &lex, 20, 0);
    assert!(!results.is_empty());
    assert!(results.iter().all(|r| r.mean.is_finite()));

    let mut solver = EndgameSolver::new(&config).with_budget(200_000);
    let opp = Rack::from_tiles([Tile::letter(ix("Ú")), Tile::letter(ix("T"))]);
    let mover = Rack::from_tiles([Tile::letter(ix("K")), Tile::letter(ix("Ő"))]);
    let r1 = solver.solve(&board, &mover, &opp, &lex);
    let r2 = EndgameSolver::new(&config)
        .with_budget(200_000)
        .solve(&board, &mover, &opp, &lex);
    assert_eq!(r1, r2, "endgame solving is deterministic");
    assert!(r1.exact);

    let ev = StaticEvaluator::new();
    let mut gen = MoveGenerator::new(&config);
    let plays = gen.generate(&board, &rack, &lex).to_vec();
    assert!(!plays.is_empty());
    let _ = ev;
}

// ---------------------------------------------------------------------------
// serde (feature "serde"): TileDistribution is now 64 codes wide
// ---------------------------------------------------------------------------

#[cfg(feature = "serde")]
mod serde_tests {
    use super::*;

    #[test]
    fn english_distribution_serialises_exactly_as_upstream_0_1_0_did() {
        // upstream: [u8; 32] -> 26 letter counts, zeros, then the blank count
        let json = serde_json::to_string(&TileDistribution::english()).unwrap();
        assert_eq!(
            json,
            r#"{"counts":[9,2,2,4,12,2,3,2,9,1,1,4,2,6,8,2,1,6,4,6,4,2,2,1,2,1,0,0,0,0,0,2]}"#
        );
        let back: TileDistribution = serde_json::from_str(&json).unwrap();
        assert_eq!(back, TileDistribution::english());
    }

    #[test]
    fn hungarian_distribution_round_trips() {
        let d = hu_distribution();
        let json = serde_json::to_string(&d).unwrap();
        let back: TileDistribution = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
        assert_eq!(back.total(), 100);
        assert_eq!(back.count(Tile::letter(37)), 1);
        assert_eq!(back.count(Tile::BLANK), 2);
    }

    #[test]
    fn a_bad_length_is_rejected_and_game_config_round_trips() {
        assert!(serde_json::from_str::<TileDistribution>(r#"{"counts":[1,2,3]}"#).is_err());
        let config = hu_config(small_layout(), 5);
        let json = serde_json::to_string(&config).unwrap();
        let back: GameConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, config);
    }
}
