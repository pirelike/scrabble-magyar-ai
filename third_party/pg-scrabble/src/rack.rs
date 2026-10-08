use crate::rules::Alphabet;
use crate::tile::{Tile, BLANK_CODE, LEGACY_MAX_LETTERS, TILE_CODES};
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Rack {
    counts: [u8; TILE_CODES],
    mask: u64,
    len: u8,
}

// `[u8; N]: Default` only exists for N <= 32, so with 64 tile codes the derive
// is no longer available.
impl Default for Rack {
    #[inline]
    fn default() -> Rack {
        Rack::new()
    }
}

impl Rack {
    #[inline]
    pub const fn new() -> Rack {
        Rack {
            counts: [0; TILE_CODES],
            mask: 0,
            len: 0,
        }
    }

    pub fn from_tiles(tiles: impl IntoIterator<Item = Tile>) -> Rack {
        let mut r = Rack::new();
        for t in tiles {
            r.add(t);
        }
        r
    }

    pub fn parse(alphabet: &Alphabet, s: &str) -> Result<Rack, char> {
        let mut r = Rack::new();
        for c in s.chars() {
            if c.is_whitespace() {
                continue;
            }
            if c == '?' || c == '.' || c == '_' {
                r.add(Tile::BLANK);
                continue;
            }
            let (letter, _) = alphabet.parse_char(c).ok_or(c)?;
            r.add(Tile::letter(letter));
        }
        Ok(r)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn count(&self, tile: Tile) -> u8 {
        self.counts[tile.code() as usize]
    }

    #[inline]
    pub fn blanks(&self) -> u8 {
        self.counts[BLANK_CODE as usize]
    }

    #[inline]
    pub fn has_blank(&self) -> bool {
        self.counts[BLANK_CODE as usize] > 0
    }

    #[inline]
    pub fn mask(&self) -> u64 {
        self.mask
    }

    #[inline]
    pub fn playable_mask(&self, allowed: u64) -> u64 {
        if self.has_blank() {
            allowed
        } else {
            self.mask & allowed
        }
    }

    #[inline]
    pub fn contains(&self, tile: Tile) -> bool {
        self.count(tile) > 0
    }

    #[inline]
    pub fn add(&mut self, tile: Tile) {
        let code = tile.code() as usize;
        self.counts[code] = self.counts[code]
            .checked_add(1)
            .expect("rack tile count overflowed");
        self.len += 1;
        if let Some(letter) = tile.index() {
            self.mask |= 1u64 << letter;
        }
    }

    #[inline]
    pub fn remove(&mut self, tile: Tile) -> bool {
        let code = tile.code() as usize;
        if self.counts[code] == 0 {
            return false;
        }
        self.counts[code] -= 1;
        self.len -= 1;
        if self.counts[code] == 0 {
            if let Some(letter) = tile.index() {
                self.mask &= !(1u64 << letter);
            }
        }
        true
    }

    #[inline]
    pub(crate) fn remove_unchecked(&mut self, tile: Tile) {
        let code = tile.code() as usize;
        debug_assert!(
            self.counts[code] > 0,
            "removed a tile the rack does not hold"
        );
        self.counts[code] -= 1;
        self.len -= 1;
        if self.counts[code] == 0 {
            if let Some(letter) = tile.index() {
                self.mask &= !(1u64 << letter);
            }
        }
    }

    #[inline]
    pub(crate) fn add_unchecked(&mut self, tile: Tile) {
        let code = tile.code() as usize;
        self.counts[code] += 1;
        self.len += 1;
        if let Some(letter) = tile.index() {
            self.mask |= 1u64 << letter;
        }
    }

    pub fn contains_all(&self, other: &Rack) -> bool {
        self.counts
            .iter()
            .zip(other.counts.iter())
            .all(|(mine, theirs)| mine >= theirs)
    }

    pub fn remove_all(&mut self, other: &Rack) -> bool {
        if !self.contains_all(other) {
            return false;
        }
        for code in 0..TILE_CODES {
            let n = other.counts[code];
            if n > 0 {
                self.counts[code] -= n;
                self.len -= n;
                if self.counts[code] == 0 && code != BLANK_CODE as usize {
                    self.mask &= !(1u64 << code);
                }
            }
        }
        true
    }

    #[inline]
    pub fn counts(&self) -> &[u8; TILE_CODES] {
        &self.counts
    }

    pub fn tiles(&self) -> Vec<Tile> {
        let mut out = Vec::with_capacity(self.len());
        for code in 0..TILE_CODES {
            for _ in 0..self.counts[code] {
                out.push(Tile::from_code(code as u8));
            }
        }
        out
    }

    #[inline]
    pub fn clear(&mut self) {
        *self = Rack::new();
    }

    pub fn value(&self, alphabet: &Alphabet) -> i32 {
        (0..alphabet.len())
            .map(|l| self.counts[l] as i32 * alphabet.score(l as u8))
            .sum()
    }

    pub fn to_text(&self, alphabet: &Alphabet) -> String {
        let mut s = String::with_capacity(self.len());
        for l in 0..alphabet.len() {
            for _ in 0..self.counts[l] {
                s.push_str(alphabet.display(l as u8));
            }
        }
        for _ in 0..self.blanks() {
            s.push('?');
        }
        s
    }

    pub fn key(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for (code, &n) in self.counts.iter().enumerate() {
            if n != 0 {
                h ^= (legacy_key_code(code) as u64) << 8 | n as u64;
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
        }
        h
    }
}

/// The code folded into [`Rack::key`]. Upstream `scrabble` 0.1.0 numbered the
/// blank 31; keeping that number (and shifting the letters 31.. that did not
/// exist back then up by one) keeps keys of racks over the old alphabets
/// identical, so leave tables trained with upstream (`SLV1`) still match.
#[inline]
fn legacy_key_code(code: usize) -> usize {
    if code == BLANK_CODE as usize {
        LEGACY_MAX_LETTERS
    } else if code >= LEGACY_MAX_LETTERS {
        code + 1
    } else {
        code
    }
}

impl fmt::Display for Rack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for l in 0..26usize {
            for _ in 0..self.counts[l] {
                write!(f, "{}", (b'A' + l as u8) as char)?;
            }
        }
        for _ in 0..self.blanks() {
            write!(f, "?")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha() -> Alphabet {
        Alphabet::english()
    }

    #[test]
    fn parse_reads_letters_and_blanks() {
        let r = Rack::parse(&alpha(), "CAT?").unwrap();
        assert_eq!(r.len(), 4);
        assert_eq!(r.blanks(), 1);
        assert!(r.contains(Tile::letter(2)));
        assert_eq!(r.to_text(&alpha()), "ACT?");
    }

    #[test]
    fn parse_rejects_letters_outside_the_alphabet() {
        assert_eq!(Rack::parse(&alpha(), "CA1"), Err('1'));
    }

    #[test]
    fn mask_tracks_presence_exactly() {
        let mut r = Rack::new();
        assert_eq!(r.mask(), 0);
        r.add(Tile::letter(0));
        r.add(Tile::letter(0));
        assert_eq!(r.mask(), 1);
        assert!(r.remove(Tile::letter(0)));
        assert_eq!(r.mask(), 1, "one A remains, so the bit stays set");
        assert!(r.remove(Tile::letter(0)));
        assert_eq!(r.mask(), 0);
        assert!(!r.remove(Tile::letter(0)));
    }

    #[test]
    fn blanks_do_not_appear_in_the_mask() {
        let mut r = Rack::new();
        r.add(Tile::BLANK);
        assert_eq!(r.mask(), 0);
        assert!(r.has_blank());
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn playable_mask_opens_everything_with_a_blank() {
        let alpha = alpha();
        let mut r = Rack::parse(&alpha, "AB").unwrap();
        let cross = 0b1111u64;
        assert_eq!(r.playable_mask(cross), 0b0011);
        r.add(Tile::BLANK);
        assert_eq!(r.playable_mask(cross), cross);
    }

    #[test]
    fn remove_all_is_atomic() {
        let alpha = alpha();
        let mut r = Rack::parse(&alpha, "CATS").unwrap();
        let too_many = Rack::parse(&alpha, "CATT").unwrap();
        assert!(!r.remove_all(&too_many));
        assert_eq!(r.len(), 4, "a failed removal must not mutate the rack");

        let some = Rack::parse(&alpha, "AT").unwrap();
        assert!(r.remove_all(&some));
        assert_eq!(r.to_text(&alpha), "CS");
    }

    #[test]
    fn value_ignores_blanks() {
        let alpha = alpha();
        let r = Rack::parse(&alpha, "QZ?").unwrap();
        assert_eq!(r.value(&alpha), 20);
    }

    #[test]
    fn key_is_order_independent() {
        let alpha = alpha();
        let a = Rack::parse(&alpha, "AEINRST").unwrap();
        let b = Rack::parse(&alpha, "TSRNIEA").unwrap();
        assert_eq!(a.key(), b.key());
        let c = Rack::parse(&alpha, "AEINRSU").unwrap();
        assert_ne!(a.key(), c.key());
    }

    #[test]
    fn key_matches_the_values_upstream_0_1_0_produced() {
        // computed with the original code numbering (blank = 31)
        let alpha = alpha();
        let r = Rack::parse(&alpha, "AEINRST?").unwrap();
        assert_eq!(r.key(), 0x59bc55061a3ff14d);
        let r = Rack::parse(&alpha, "AAB??").unwrap();
        assert_eq!(r.key(), 0xe753a418732a6b4a);
    }

    #[test]
    fn wide_letters_get_keys_distinct_from_the_blank_and_each_other() {
        let mut keys = alloc::collections::BTreeSet::new();
        for code in 0..=BLANK_CODE {
            let mut r = Rack::new();
            r.add(Tile::from_code(code));
            assert!(keys.insert(r.key()), "code {code} collides");
        }
        assert_eq!(keys.len(), TILE_CODES);
    }

    #[test]
    fn mask_covers_letters_above_the_old_limit() {
        let mut r = Rack::new();
        r.add(Tile::letter(31));
        r.add(Tile::letter(62));
        r.add(Tile::BLANK);
        assert_eq!(r.mask(), (1u64 << 31) | (1u64 << 62));
        assert!(r.remove(Tile::letter(62)));
        assert_eq!(r.mask(), 1u64 << 31);
        assert_eq!(r.len(), 2);
        assert_eq!(r.tiles(), [Tile::letter(31), Tile::BLANK]);
        assert_eq!(Rack::default(), Rack::new());
    }

    #[test]
    fn rack_size_is_pinned() {
        // upstream 0.1.0: [u8; 32] + u32 + u8 = 40 bytes; now [u8; 64] + u64 + u8
        assert_eq!(core::mem::size_of::<Rack>(), 80);
    }

    #[test]
    fn tiles_expands_multiplicity() {
        let alpha = alpha();
        let r = Rack::parse(&alpha, "AAB?").unwrap();
        assert_eq!(
            r.tiles(),
            [
                Tile::letter(0),
                Tile::letter(0),
                Tile::letter(1),
                Tile::BLANK
            ]
        );
    }

    #[test]
    fn unchecked_ops_round_trip() {
        let mut r = Rack::parse(&alpha(), "AB").unwrap();
        let before = r;
        r.remove_unchecked(Tile::letter(0));
        r.add_unchecked(Tile::letter(0));
        assert_eq!(r, before);
    }
}
