use crate::tile::{Tile, MAX_LETTERS, TILE_CODES};
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Premium {
    #[default]
    Normal,
    DoubleLetter,
    TripleLetter,
    QuadLetter,
    DoubleWord,
    TripleWord,
    QuadWord,
}

impl Premium {
    #[inline]
    pub const fn letter_multiplier(self) -> i32 {
        match self {
            Premium::DoubleLetter => 2,
            Premium::TripleLetter => 3,
            Premium::QuadLetter => 4,
            _ => 1,
        }
    }

    #[inline]
    pub const fn word_multiplier(self) -> i32 {
        match self {
            Premium::DoubleWord => 2,
            Premium::TripleWord => 3,
            Premium::QuadWord => 4,
            _ => 1,
        }
    }

    #[inline]
    pub const fn as_char(self) -> char {
        match self {
            Premium::Normal => '.',
            Premium::DoubleLetter => 'd',
            Premium::TripleLetter => 't',
            Premium::QuadLetter => 'q',
            Premium::DoubleWord => 'D',
            Premium::TripleWord => 'T',
            Premium::QuadWord => 'Q',
        }
    }

    #[inline]
    pub const fn from_char(c: char) -> Option<Premium> {
        Some(match c {
            '.' | '-' | ' ' => Premium::Normal,
            'd' => Premium::DoubleLetter,
            't' => Premium::TripleLetter,
            'q' => Premium::QuadLetter,
            'D' => Premium::DoubleWord,
            'T' => Premium::TripleWord,
            'Q' => Premium::QuadWord,
            _ => return None,
        })
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Alphabet {
    name: String,
    letters: Vec<String>,
    scores: Vec<i32>,
}

impl Alphabet {
    pub fn new(name: impl Into<String>, letters: Vec<String>, scores: Vec<i32>) -> Alphabet {
        assert_eq!(
            letters.len(),
            scores.len(),
            "alphabet letters and scores must be the same length"
        );
        assert!(
            !letters.is_empty(),
            "alphabet must have at least one letter"
        );
        assert!(
            letters.len() <= MAX_LETTERS,
            "alphabet exceeds MAX_LETTERS ({MAX_LETTERS})"
        );
        Alphabet {
            name: name.into(),
            letters,
            scores,
        }
    }

    pub fn english() -> Alphabet {
        const SCORES: [i32; 26] = [
            1, 3, 3, 2, 1, 4, 2, 4, 1, 8, 5, 1, 3, 1, 1, 3, 10, 1, 1, 1, 1, 4, 4, 8, 4, 10,
        ];

        let letters = (b'A'..=b'Z')
            .map(|b| String::from(b as char))
            .collect::<Vec<_>>();
        Alphabet::new("English", letters, SCORES.to_vec())
    }

    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.letters.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.letters.is_empty()
    }

    #[inline]
    pub fn full_mask(&self) -> u64 {
        if self.len() >= 64 {
            u64::MAX
        } else {
            (1u64 << self.len()) - 1
        }
    }

    #[inline]
    pub fn score(&self, letter: u8) -> i32 {
        self.scores[letter as usize]
    }

    #[inline]
    pub fn tile_score(&self, tile: Tile) -> i32 {
        match tile.index() {
            Some(letter) => self.score(letter),
            None => 0,
        }
    }

    #[inline]
    pub fn display(&self, letter: u8) -> &str {
        &self.letters[letter as usize]
    }

    pub fn index_of(&self, s: &str) -> Option<u8> {
        self.letters.iter().position(|l| l == s).map(|i| i as u8)
    }

    pub fn parse_char(&self, c: char) -> Option<(u8, bool)> {
        let upper = c.to_ascii_uppercase();
        let is_blank = c.is_ascii_lowercase();
        let mut buf = [0u8; 4];
        let s = upper.encode_utf8(&mut buf);
        self.index_of(s).map(|i| (i, is_blank))
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TileDistribution {
    #[cfg_attr(feature = "serde", serde(with = "counts_serde"))]
    counts: [u8; TILE_CODES],
}

impl TileDistribution {
    pub fn new(letter_counts: &[u8], blanks: u8) -> TileDistribution {
        assert!(
            letter_counts.len() <= MAX_LETTERS,
            "distribution exceeds MAX_LETTERS ({MAX_LETTERS})"
        );
        let mut counts = [0u8; TILE_CODES];
        counts[..letter_counts.len()].copy_from_slice(letter_counts);
        counts[crate::tile::BLANK_CODE as usize] = blanks;
        TileDistribution { counts }
    }

    pub fn english() -> TileDistribution {
        const COUNTS: [u8; 26] = [
            9, 2, 2, 4, 12, 2, 3, 2, 9, 1, 1, 4, 2, 6, 8, 2, 1, 6, 4, 6, 4, 2, 2, 1, 2, 1,
        ];
        TileDistribution::new(&COUNTS, 2)
    }

    #[inline]
    pub fn count(&self, tile: Tile) -> u8 {
        self.counts[tile.code() as usize]
    }

    #[inline]
    pub fn counts(&self) -> &[u8; TILE_CODES] {
        &self.counts
    }

    #[inline]
    pub fn total(&self) -> usize {
        self.counts.iter().map(|&c| c as usize).sum()
    }
}

/// serde cannot derive for arrays longer than 32, and `TILE_CODES` is 64 now.
///
/// The wire format is a sequence of counts in the numbering of upstream
/// `scrabble` 0.1.0: letters `0..=30` followed by the blank (32 entries). When
/// letters above that exist (alphabets of 31..=62 letters) their counts follow
/// as 32 more entries. So distributions of alphabets that fit the old limit
/// serialise exactly as before, and old data still deserialises.
#[cfg(feature = "serde")]
mod counts_serde {
    use crate::tile::{BLANK_CODE, LEGACY_MAX_LETTERS, MAX_LETTERS, TILE_CODES};
    use alloc::vec::Vec;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    const WIDE: usize = MAX_LETTERS - LEGACY_MAX_LETTERS;

    pub fn serialize<S: Serializer>(
        counts: &[u8; TILE_CODES],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut seq: Vec<u8> = counts[..LEGACY_MAX_LETTERS].to_vec();
        seq.push(counts[BLANK_CODE as usize]);
        let wide = &counts[LEGACY_MAX_LETTERS..MAX_LETTERS];
        if wide.iter().any(|&c| c != 0) {
            seq.extend_from_slice(wide);
        }
        seq.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<[u8; TILE_CODES], D::Error> {
        let seq = Vec::<u8>::deserialize(deserializer)?;
        let legacy = LEGACY_MAX_LETTERS + 1;
        if seq.len() != legacy && seq.len() != legacy + WIDE {
            return Err(serde::de::Error::invalid_length(
                seq.len(),
                &"32 (letters 0..=30 and the blank) or 64 (plus letters 31..=62) counts",
            ));
        }
        let mut counts = [0u8; TILE_CODES];
        counts[..LEGACY_MAX_LETTERS].copy_from_slice(&seq[..LEGACY_MAX_LETTERS]);
        counts[BLANK_CODE as usize] = seq[LEGACY_MAX_LETTERS];
        if seq.len() > legacy {
            counts[LEGACY_MAX_LETTERS..MAX_LETTERS].copy_from_slice(&seq[legacy..]);
        }
        Ok(counts)
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LayoutError {
    Empty,
    RaggedRow {
        row: usize,
        expected: usize,
        found: usize,
    },
    BadChar {
        row: usize,
        col: usize,
        ch: char,
    },
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayoutError::Empty => write!(f, "layout has no rows"),
            LayoutError::RaggedRow {
                row,
                expected,
                found,
            } => write!(
                f,
                "row {row} has {found} cells, expected {expected} to match the first row"
            ),
            LayoutError::BadChar { row, col, ch } => {
                write!(
                    f,
                    "unrecognised premium character {ch:?} at row {row}, column {col}"
                )
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for LayoutError {}

#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BoardLayout {
    name: String,
    width: usize,
    height: usize,
    premiums: Vec<Premium>,
    transposed: Vec<Premium>,
    start: (usize, usize),
    start_required: bool,
}

pub const STANDARD_LAYOUT: &str = "\
T..d...T...d..T
.D...t...t...D.
..D...d.d...D..
d..D...d...D..d
....D.....D....
.t...t...t...t.
..d...d.d...d..
T..d...D...d..T
..d...d.d...d..
.t...t...t...t.
....D.....D....
d..D...d...D..d
..D...d.d...D..
.D...t...t...D.
T..d...T...d..T";

impl BoardLayout {
    pub fn parse(name: impl Into<String>, s: &str) -> Result<BoardLayout, LayoutError> {
        let rows: Vec<&str> = s
            .lines()
            .map(str::trim_end)
            .filter(|l| !l.is_empty())
            .collect();
        if rows.is_empty() {
            return Err(LayoutError::Empty);
        }
        let width = rows[0].chars().count();
        let height = rows.len();

        let mut premiums = Vec::with_capacity(width * height);
        for (r, row) in rows.iter().enumerate() {
            let mut seen = 0usize;
            for (c, ch) in row.chars().enumerate() {
                let p =
                    Premium::from_char(ch).ok_or(LayoutError::BadChar { row: r, col: c, ch })?;
                premiums.push(p);
                seen += 1;
            }
            if seen != width {
                return Err(LayoutError::RaggedRow {
                    row: r,
                    expected: width,
                    found: seen,
                });
            }
        }

        let start = (height / 2, width / 2);
        Ok(BoardLayout::from_parts(
            name, width, height, premiums, start, true,
        ))
    }

    fn from_parts(
        name: impl Into<String>,
        width: usize,
        height: usize,
        premiums: Vec<Premium>,
        start: (usize, usize),
        start_required: bool,
    ) -> BoardLayout {
        let mut transposed = Vec::with_capacity(width * height);
        for c in 0..width {
            for r in 0..height {
                transposed.push(premiums[r * width + c]);
            }
        }
        BoardLayout {
            name: name.into(),
            width,
            height,
            premiums,
            transposed,
            start,
            start_required,
        }
    }

    pub fn standard() -> BoardLayout {
        BoardLayout::parse("Standard 15x15", STANDARD_LAYOUT)
            .expect("the built-in standard layout is well-formed")
    }

    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[inline]
    pub fn width(&self) -> usize {
        self.width
    }

    #[inline]
    pub fn height(&self) -> usize {
        self.height
    }

    #[inline]
    pub fn cells(&self) -> usize {
        self.width * self.height
    }

    #[inline]
    pub fn start(&self) -> (usize, usize) {
        self.start
    }

    #[inline]
    pub fn start_required(&self) -> bool {
        self.start_required
    }

    pub fn with_start(mut self, start: (usize, usize), required: bool) -> BoardLayout {
        self.start = start;
        self.start_required = required;
        self
    }

    #[inline]
    pub fn premium(&self, row: usize, col: usize) -> Premium {
        self.premiums[row * self.width + col]
    }

    #[inline]
    pub fn premiums(&self) -> &[Premium] {
        &self.premiums
    }

    #[inline]
    pub fn transposed_premiums(&self) -> &[Premium] {
        &self.transposed
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ChallengeRule {
    #[default]
    Void,
    SingleFree,
    SingleLose,
    FivePoint,
    TenPoint,
}

#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GameConfig {
    pub alphabet: Alphabet,
    pub distribution: TileDistribution,
    pub layout: BoardLayout,
    pub rack_size: usize,
    pub bingo_bonus: i32,
    pub min_exchange_bag: usize,
    pub max_consecutive_zeros: u32,
    pub challenge_rule: ChallengeRule,
    pub double_out_adjustment: bool,
}

impl GameConfig {
    pub fn standard() -> GameConfig {
        GameConfig {
            alphabet: Alphabet::english(),
            distribution: TileDistribution::english(),
            layout: BoardLayout::standard(),
            rack_size: 7,
            bingo_bonus: 50,
            min_exchange_bag: 7,
            max_consecutive_zeros: 6,
            challenge_rule: ChallengeRule::Void,
            double_out_adjustment: true,
        }
    }

    #[inline]
    pub fn width(&self) -> usize {
        self.layout.width()
    }

    #[inline]
    pub fn height(&self) -> usize {
        self.layout.height()
    }
}

impl Default for GameConfig {
    fn default() -> Self {
        GameConfig::standard()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_alphabet_has_tournament_values() {
        let a = Alphabet::english();
        assert_eq!(a.len(), 26);
        assert_eq!(a.score(0), 1, "A");
        assert_eq!(a.score(9), 8, "J");
        assert_eq!(a.score(16), 10, "Q");
        assert_eq!(a.score(20), 1, "U");
        assert_eq!(a.score(23), 8, "X");
        assert_eq!(a.score(25), 10, "Z");
        assert_eq!(a.display(4), "E");
    }

    #[test]
    fn english_bag_has_one_hundred_tiles() {
        let d = TileDistribution::english();
        assert_eq!(d.total(), 100);
        assert_eq!(d.count(Tile::BLANK), 2);
        assert_eq!(d.count(Tile::letter(4)), 12, "E");
        assert_eq!(d.count(Tile::letter(16)), 1, "Q");
    }

    #[test]
    fn standard_layout_has_the_right_premium_counts() {
        let l = BoardLayout::standard();
        assert_eq!(l.width(), 15);
        assert_eq!(l.height(), 15);
        assert_eq!(l.start(), (7, 7));

        let count = |p: Premium| l.premiums().iter().filter(|&&x| x == p).count();
        assert_eq!(count(Premium::TripleWord), 8);

        assert_eq!(count(Premium::DoubleWord), 17);
        assert_eq!(count(Premium::TripleLetter), 12);
        assert_eq!(count(Premium::DoubleLetter), 24);
        assert_eq!(l.premium(7, 7), Premium::DoubleWord);
        assert_eq!(l.premium(0, 0), Premium::TripleWord);
    }

    #[test]
    fn transposed_premiums_mirror_the_board() {
        let l = BoardLayout::standard();
        for r in 0..l.height() {
            for c in 0..l.width() {
                assert_eq!(
                    l.transposed_premiums()[c * l.height() + r],
                    l.premium(r, c),
                    "mismatch at ({r}, {c})"
                );
            }
        }
    }

    #[test]
    fn layout_parse_rejects_ragged_and_unknown() {
        assert!(matches!(
            BoardLayout::parse("bad", "..\n..."),
            Err(LayoutError::RaggedRow { .. })
        ));
        assert!(matches!(
            BoardLayout::parse("bad", ".x\n.."),
            Err(LayoutError::BadChar { ch: 'x', .. })
        ));
        assert!(matches!(
            BoardLayout::parse("bad", ""),
            Err(LayoutError::Empty)
        ));
    }

    #[test]
    fn full_mask_covers_every_letter() {
        let a = Alphabet::english();
        assert_eq!(a.full_mask().count_ones(), 26);
    }

    #[test]
    fn parse_char_marks_lowercase_as_blank() {
        let a = Alphabet::english();
        assert_eq!(a.parse_char('E'), Some((4, false)));
        assert_eq!(a.parse_char('e'), Some((4, true)));
        assert_eq!(a.parse_char('?'), None);
    }
}
