//! Magyar betűkészlet (100 zseton), zsák, szó → zsetonok felbontása.

use rand::seq::SliceRandom;

/// (betű, pontérték, darabszám); a 0. elem az üres zseton (joker).
pub const TILE_DISTRIBUTION: [(&str, u8, u8); 39] = [
    ("", 0, 2),
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

/// Egy zseton: az index a `TILE_DISTRIBUTION`-ben (0 = üres zseton / joker).
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Tile(pub u8);

impl Tile {
    pub const BLANK: Tile = Tile(0);

    pub fn from_str(text: &str) -> Option<Tile> {
        TILE_DISTRIBUTION.iter().position(|(letter, _, _)| *letter == text).map(|i| Tile(i as u8))
    }

    pub fn as_str(self) -> &'static str {
        TILE_DISTRIBUTION[self.0 as usize].0
    }

    pub fn value(self) -> u32 {
        TILE_DISTRIBUTION[self.0 as usize].1 as u32
    }

    pub fn is_blank(self) -> bool {
        self.0 == 0
    }

    pub fn is_vowel(self) -> bool {
        self.as_str().chars().next().map(|c| VOWELS.contains(c)).unwrap_or(false)
    }

    pub fn is_digraph(self) -> bool {
        self.as_str().chars().count() == 2
    }
}

pub const VOWELS: &str = "AÁEÉIÍOÓÖŐUÚÜŰ";

/// A táblán használható betűk (üres zseton nélkül).
pub fn letters() -> impl Iterator<Item = Tile> {
    (1..TILE_DISTRIBUTION.len() as u8).map(Tile)
}

/// Pontérték egy betűhöz (a joker 0).
pub fn tile_value(letter: &str) -> u32 {
    Tile::from_str(letter).map(|t| t.value()).unwrap_or(0)
}

/// Két szomszédos zseton egy-egy betűje kétjegyű betűt adna-e (S + Z = SZ)? Ez nem megengedett: a kétjegyű
/// betűt (SZ, CS, GY, LY, NY, TY, ZS) csak a saját zsetonjával lehet kirakni.
pub fn forms_digraph(first: &str, second: &str) -> bool {
    if first.chars().count() != 1 || second.chars().count() != 1 {
        return false;
    }
    let combined = format!("{first}{second}");
    TILE_DISTRIBUTION.iter().any(|(letter, _, _)| *letter == combined && letter.chars().count() == 2)
}

/// Szó felbontása zsetonokra (a kétkarakteres betűk: SZ, CS, ... egy zseton).
///
/// A felbontás nem egyértelmű (pl. S+Z vagy SZ): a kevesebb zsetont használót adja (a magyar helyesírásban
/// a kétjegyű betű egy betű), egyenlőség esetén a több pontot érőt.
/// Visszatér: a zsetonok, vagy None, ha a szó olyan karaktert tartalmaz, amihez nincs zseton.
pub fn tokenize_word(word: &str) -> Option<Vec<Tile>> {
    let upper: Vec<char> = word.to_uppercase().chars().collect();
    let n = upper.len();
    // best[i] = (zsetonok száma, -pontszám, felbontás) a word[:i]-re
    let mut best: Vec<Option<(i32, i32, Vec<Tile>)>> = vec![None; n + 1];
    best[0] = Some((0, 0, Vec::new()));
    for i in 0..n {
        let Some((count, neg_score, parts)) = best[i].clone() else { continue };
        for size in 1..=2usize {
            if i + size > n {
                continue;
            }
            let piece: String = upper[i..i + size].iter().collect();
            let Some(tile) = Tile::from_str(&piece) else { continue };
            if tile.is_blank() {
                continue;
            }
            let cand = (count + 1, neg_score - tile.value() as i32);
            let better = match &best[i + size] {
                None => true,
                Some((c, s, _)) => cand < (*c, *s),
            };
            if better {
                let mut new_parts = parts.clone();
                new_parts.push(tile);
                best[i + size] = Some((cand.0, cand.1, new_parts));
            }
        }
    }
    best[n].take().map(|(_, _, parts)| parts)
}

/// A szó betűértékeinek összege prémium mezők nélkül (None, ha nem rakható ki).
pub fn word_base_score(word: &str) -> Option<u32> {
    tokenize_word(word).map(|tokens| tokens.iter().map(|t| t.value()).sum())
}

/// Betűzseton zsák.
#[derive(Clone, Debug)]
pub struct TileBag {
    pub tiles: Vec<Tile>,
}

impl TileBag {
    pub fn new() -> TileBag {
        let mut tiles = Vec::with_capacity(100);
        for (i, (_, _, count)) in TILE_DISTRIBUTION.iter().enumerate() {
            for _ in 0..*count {
                tiles.push(Tile(i as u8));
            }
        }
        tiles.shuffle(&mut rand::rng());
        TileBag { tiles }
    }

    /// Húz `count` darab zsetont a zsákból.
    pub fn draw(&mut self, count: usize) -> Vec<Tile> {
        let n = count.min(self.tiles.len());
        if n == 0 {
            return Vec::new();
        }
        let at = self.tiles.len() - n;
        self.tiles.split_off(at)
    }

    /// Visszateszi a zsetonokat és újrakeveri.
    pub fn put_back(&mut self, tiles: &[Tile]) {
        self.tiles.extend_from_slice(tiles);
        self.tiles.shuffle(&mut rand::rng());
    }

    pub fn remaining(&self) -> usize {
        self.tiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }
}

impl Default for TileBag {
    fn default() -> Self {
        TileBag::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(tiles: Vec<Tile>) -> Vec<&'static str> {
        tiles.into_iter().map(|t| t.as_str()).collect()
    }

    #[test]
    fn distribution_is_100_tiles() {
        let total: u32 = TILE_DISTRIBUTION.iter().map(|(_, _, c)| *c as u32).sum();
        assert_eq!(total, 100);
        assert_eq!(TileBag::new().remaining(), 100);
    }

    #[test]
    fn tokenize_prefers_digraphs() {
        // döntetlen darabszámnál a több pontot érő felbontás nyer: KÉSZSÉG = K É S ZS É G
        assert_eq!(strs(tokenize_word("készség").unwrap()), vec!["K", "É", "S", "ZS", "É", "G"]);
        assert_eq!(strs(tokenize_word("KÉSZSÉG").unwrap()), vec!["K", "É", "S", "ZS", "É", "G"]);
        assert_eq!(strs(tokenize_word("SZÉK").unwrap()), vec!["SZ", "É", "K"]);
        assert_eq!(strs(tokenize_word("ZSEB").unwrap()), vec!["ZS", "E", "B"]);
        assert_eq!(strs(tokenize_word("GYÓGYÍT").unwrap()), vec!["GY", "Ó", "GY", "Í", "T"]);
    }

    #[test]
    fn tokenize_rejects_foreign_letters() {
        assert!(tokenize_word("ADYAS").is_none());
        assert!(tokenize_word("QUIZ").is_none());
    }

    #[test]
    fn base_score_sums_tile_values() {
        assert_eq!(word_base_score("ALMA"), Some(4));
        assert_eq!(word_base_score("TYÚK"), Some(10 + 7 + 1));
    }

    #[test]
    fn digraph_pairs() {
        assert!(forms_digraph("S", "Z"));
        assert!(forms_digraph("C", "S"));
        assert!(forms_digraph("Z", "S"));
        assert!(!forms_digraph("SZ", "S"));
        assert!(!forms_digraph("A", "S"));
    }

    #[test]
    fn draw_never_overdraws() {
        let mut bag = TileBag::new();
        assert!(bag.draw(0).is_empty());
        assert_eq!(bag.remaining(), 100);
        assert_eq!(bag.draw(7).len(), 7);
        assert_eq!(bag.draw(500).len(), 93);
        assert!(bag.is_empty());
    }
}
