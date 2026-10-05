/// Maximum number of distinct letters (tile types, blank excluded) an alphabet
/// may hold. Letter sets are 64-bit masks; bit 63 of a graph header is the
/// "accepts" flag, so letters (and the GADDAG separator) use bits `0..=62`.
///
/// Upstream `scrabble` 0.1.0 used 32-bit masks and a limit of 31 (30 for a
/// lexicon, whose GADDAG needs one extra symbol for the separator).
pub const MAX_LETTERS: usize = 63;

/// The letter limit of upstream `scrabble` 0.1.0. Kept only so that values that
/// were derived from the old numbering (rack keys, endgame Zobrist keys) stay
/// bit-for-bit identical for alphabets that fit the old limit.
pub(crate) const LEGACY_MAX_LETTERS: usize = 31;

// A `Square` stores `letter | BLANK_FLAG` and reserves 0xFF for "empty", so a
// letter index must stay below the flag bit.
const _: () = assert!(MAX_LETTERS < BLANK_FLAG as usize);
const _: () = assert!(MAX_LETTERS < 64, "letter masks are 64 bits wide");

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Tile(u8);

impl Tile {
    pub const BLANK: Tile = Tile(BLANK_CODE);

    #[inline]
    pub const fn letter(letter: u8) -> Tile {
        assert!((letter as usize) < MAX_LETTERS, "letter index out of range");
        Tile(letter)
    }

    #[inline]
    pub const fn try_letter(letter: u8) -> Option<Tile> {
        if (letter as usize) < MAX_LETTERS {
            Some(Tile(letter))
        } else {
            None
        }
    }

    #[inline]
    pub const fn is_blank(self) -> bool {
        self.0 == BLANK_CODE
    }

    #[inline]
    pub const fn index(self) -> Option<u8> {
        if self.is_blank() {
            None
        } else {
            Some(self.0)
        }
    }

    #[inline]
    pub const fn code(self) -> u8 {
        self.0
    }

    #[inline]
    pub const fn from_code(code: u8) -> Tile {
        assert!(
            (code as usize) < MAX_LETTERS || code == BLANK_CODE,
            "tile code out of range"
        );
        Tile(code)
    }

    #[inline]
    pub const fn place_as(self, letter: u8) -> Square {
        if self.is_blank() {
            Square::blank_letter(letter)
        } else {
            Square::letter(letter)
        }
    }
}

pub const BLANK_CODE: u8 = MAX_LETTERS as u8;

pub const TILE_CODES: usize = MAX_LETTERS + 1;

const BLANK_FLAG: u8 = 0x80;
const EMPTY_CODE: u8 = 0xFF;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Square(u8);

impl Default for Square {
    #[inline]
    fn default() -> Self {
        Square::EMPTY
    }
}

impl Square {
    pub const EMPTY: Square = Square(EMPTY_CODE);

    #[inline]
    pub const fn letter(letter: u8) -> Square {
        assert!((letter as usize) < MAX_LETTERS, "letter index out of range");
        Square(letter)
    }

    #[inline]
    pub const fn blank_letter(letter: u8) -> Square {
        assert!((letter as usize) < MAX_LETTERS, "letter index out of range");
        Square(letter | BLANK_FLAG)
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == EMPTY_CODE
    }

    #[inline]
    pub const fn is_occupied(self) -> bool {
        self.0 != EMPTY_CODE
    }

    #[inline]
    pub const fn is_blank(self) -> bool {
        self.0 != EMPTY_CODE && (self.0 & BLANK_FLAG) != 0
    }

    #[inline]
    pub const fn index(self) -> Option<u8> {
        if self.is_empty() {
            None
        } else {
            Some(self.0 & !BLANK_FLAG)
        }
    }

    #[inline]
    pub const fn index_unchecked(self) -> u8 {
        self.0 & !BLANK_FLAG
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_and_letters_are_distinguishable() {
        assert!(Tile::BLANK.is_blank());
        assert_eq!(Tile::BLANK.index(), None);
        let a = Tile::letter(0);
        assert!(!a.is_blank());
        assert_eq!(a.index(), Some(0));
    }

    #[test]
    fn tile_code_round_trips() {
        for code in 0..MAX_LETTERS as u8 {
            assert_eq!(Tile::from_code(code), Tile::letter(code));
        }
        assert_eq!(Tile::from_code(BLANK_CODE), Tile::BLANK);
    }

    #[test]
    fn empty_square_is_not_a_letter() {
        assert!(Square::EMPTY.is_empty());
        assert!(!Square::EMPTY.is_occupied());
        assert_eq!(Square::EMPTY.index(), None);
        assert!(!Square::EMPTY.is_blank());
    }

    #[test]
    fn placing_a_blank_keeps_the_blank_flag() {
        let sq = Tile::BLANK.place_as(4);
        assert!(sq.is_occupied());
        assert!(sq.is_blank());
        assert_eq!(sq.index(), Some(4));

        let sq = Tile::letter(4).place_as(4);
        assert!(sq.is_occupied());
        assert!(!sq.is_blank());
        assert_eq!(sq.index(), Some(4));
    }

    #[test]
    fn the_widest_letter_and_the_blank_are_distinct_everywhere() {
        let top = (MAX_LETTERS - 1) as u8;
        assert_eq!(Tile::letter(top).index(), Some(top));
        assert_ne!(Tile::letter(top), Tile::BLANK);
        assert!(Tile::try_letter(MAX_LETTERS as u8).is_none());
        assert_eq!(BLANK_CODE as usize, MAX_LETTERS);
        assert_eq!(TILE_CODES, MAX_LETTERS + 1);

        let plain = Square::letter(top);
        let blank = Square::blank_letter(top);
        assert!(plain.is_occupied() && !plain.is_blank());
        assert!(blank.is_occupied() && blank.is_blank());
        assert_eq!(plain.index(), Some(top));
        assert_eq!(blank.index(), Some(top));
        assert_eq!(blank.index_unchecked(), top);
        assert_ne!(blank, Square::EMPTY);
        assert_ne!(plain, blank);
    }

    #[test]
    fn types_are_one_byte() {
        assert_eq!(core::mem::size_of::<Tile>(), 1);
        assert_eq!(core::mem::size_of::<Square>(), 1);
    }
}
