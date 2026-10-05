//! 15×15 Scrabble tábla: premium mezők, szó elhelyezés validáció és pontozás.

use crate::dictionary;
use crate::tiles::{Tile, forms_digraph};
use serde_json::{Value, json};

pub const BOARD_SIZE: usize = 15;
pub const CENTER: i32 = 7;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Premium {
    None,
    Dl,
    Tl,
    Dw,
    Tw,
    Star,
}

// Standard Scrabble tábla premium mező elrendezés (egy negyed)
const PREMIUM_QUARTER: [(usize, usize, Premium); 18] = [
    (0, 0, Premium::Tw),
    (0, 3, Premium::Dl),
    (0, 7, Premium::Tw),
    (1, 1, Premium::Dw),
    (1, 5, Premium::Tl),
    (2, 2, Premium::Dw),
    (2, 6, Premium::Dl),
    (3, 0, Premium::Dl),
    (3, 3, Premium::Dw),
    (3, 7, Premium::Dl),
    (4, 4, Premium::Dw),
    (5, 1, Premium::Tl),
    (5, 5, Premium::Tl),
    (6, 2, Premium::Dl),
    (6, 6, Premium::Dl),
    (7, 0, Premium::Tw),
    (7, 3, Premium::Dl),
    (7, 7, Premium::Star),
];

fn premium_map() -> &'static [[Premium; BOARD_SIZE]; BOARD_SIZE] {
    static MAP: std::sync::OnceLock<[[Premium; BOARD_SIZE]; BOARD_SIZE]> = std::sync::OnceLock::new();
    MAP.get_or_init(|| {
        let mut map = [[Premium::None; BOARD_SIZE]; BOARD_SIZE];
        for (r, c, kind) in PREMIUM_QUARTER {
            for (rr, cc) in [(r, c), (r, 14 - c), (14 - r, c), (14 - r, 14 - c)] {
                map[rr][cc] = kind;
            }
        }
        map
    })
}

pub fn premium_at(row: usize, col: usize) -> Premium {
    if row < BOARD_SIZE && col < BOARD_SIZE { premium_map()[row][col] } else { Premium::None }
}

/// Egy mező tartalma: a betű és az, hogy joker-e (a joker a választott betűjével szerepel).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Cell {
    pub letter: Tile,
    pub is_blank: bool,
}

/// Egy lerakott zseton: (sor, oszlop, betű, joker-e). A koordináták előjelesek, hogy a határon kívüli
/// érték is ellenőrizhető legyen.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Placed {
    pub row: i32,
    pub col: i32,
    pub letter: Tile,
    pub is_blank: bool,
}

impl Placed {
    pub fn new(row: i32, col: i32, letter: Tile, is_blank: bool) -> Placed {
        Placed { row, col, letter, is_blank }
    }

    pub fn to_json(&self) -> Value {
        json!({"row": self.row, "col": self.col, "letter": self.letter.as_str(), "is_blank": self.is_blank})
    }

    /// A kézből elvett zseton: joker esetén az üres zseton, különben a betű.
    pub fn hand_tile(&self) -> Tile {
        if self.is_blank { Tile::BLANK } else { self.letter }
    }
}

/// Egy képzett szó: a szöveg, a mezői és a pontszáma.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FormedWord {
    pub word: String,
    pub positions: Vec<(usize, usize)>,
    pub score: u32,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Board {
    pub cells: [[Option<Cell>; BOARD_SIZE]; BOARD_SIZE],
    pub is_empty: bool,
}

impl Default for Board {
    fn default() -> Self {
        Board::new()
    }
}

type Mask = [[bool; BOARD_SIZE]; BOARD_SIZE];

impl Board {
    pub fn new() -> Board {
        Board { cells: [[None; BOARD_SIZE]; BOARD_SIZE], is_empty: true }
    }

    pub fn get(&self, row: i32, col: i32) -> Option<Cell> {
        if (0..BOARD_SIZE as i32).contains(&row) && (0..BOARD_SIZE as i32).contains(&col) {
            self.cells[row as usize][col as usize]
        } else {
            None
        }
    }

    pub fn set(&mut self, row: usize, col: usize, letter: Tile, is_blank: bool) {
        self.cells[row][col] = Some(Cell { letter, is_blank });
    }

    fn has_letter(&self, row: i32, col: i32) -> bool {
        self.get(row, col).is_some()
    }

    /// A `to_dict` által készített pillanatképből építi vissza a táblát.
    pub fn from_json(rows: &Value) -> Board {
        let mut board = Board::new();
        let Some(rows) = rows.as_array() else { return board };
        for (r, row) in rows.iter().take(BOARD_SIZE).enumerate() {
            let Some(row) = row.as_array() else { continue };
            for (c, cell) in row.iter().take(BOARD_SIZE).enumerate() {
                if cell.is_null() {
                    continue;
                }
                let letter = cell.get("letter").and_then(|l| l.as_str()).and_then(Tile::from_str);
                if let Some(letter) = letter {
                    let blank = cell.get("is_blank").map(crate::util::truthy).unwrap_or(false);
                    board.cells[r][c] = Some(Cell { letter, is_blank: blank });
                    board.is_empty = false;
                }
            }
        }
        board
    }

    /// Szerializálja a táblát JSON-kompatibilis formátumba.
    pub fn to_json(&self) -> Value {
        Value::Array(
            self.cells
                .iter()
                .map(|row| {
                    Value::Array(
                        row.iter()
                            .map(|cell| match cell {
                                None => Value::Null,
                                Some(c) => json!({"letter": c.letter.as_str(), "is_blank": c.is_blank}),
                            })
                            .collect(),
                    )
                })
                .collect(),
        )
    }

    // --- Szavak kinyerése ---

    /// Kiterjeszti a szó határait a meglévő betűkkel.
    fn find_word_bounds(&self, fixed: usize, mut start: usize, mut end: usize, horizontal: bool) -> (usize, usize) {
        let at = |i: usize| if horizontal { self.cells[fixed][i] } else { self.cells[i][fixed] };
        while start > 0 && at(start - 1).is_some() {
            start -= 1;
        }
        while end < BOARD_SIZE - 1 && at(end + 1).is_some() {
            end += 1;
        }
        (start, end)
    }

    /// Kinyeri a szót és kiszámítja a pontszámát.
    fn extract_word(&self, fixed: usize, start: usize, end: usize, horizontal: bool, new_positions: &Mask) -> FormedWord {
        let mut word = String::new();
        let mut positions = Vec::new();
        let mut letter_score = 0;
        let mut word_multiplier = 1;
        for i in start..=end {
            let (r, c) = if horizontal { (fixed, i) } else { (i, fixed) };
            let cell = self.cells[r][c].expect("a szó minden mezője foglalt");
            word.push_str(cell.letter.as_str());
            positions.push((r, c));
            let mut tile_value = if cell.is_blank { 0 } else { cell.letter.value() };
            if new_positions[r][c] {
                match premium_at(r, c) {
                    Premium::Dl => tile_value *= 2,
                    Premium::Tl => tile_value *= 3,
                    Premium::Dw | Premium::Star => word_multiplier *= 2,
                    Premium::Tw => word_multiplier *= 3,
                    Premium::None => {}
                }
            }
            letter_score += tile_value;
        }
        FormedWord { word, positions, score: letter_score * word_multiplier }
    }

    // --- Validáció fázisai ---

    fn validate_positions(&self, placed: &[Placed]) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        for p in placed {
            if !(0..BOARD_SIZE as i32).contains(&p.row) || !(0..BOARD_SIZE as i32).contains(&p.col) {
                return Err(format!("A ({},{}) pozíció a táblán kívül van.", p.row, p.col));
            }
            if self.cells[p.row as usize][p.col as usize].is_some() {
                return Err(format!("A ({},{}) mező már foglalt.", p.row, p.col));
            }
            if !seen.insert((p.row, p.col)) {
                return Err(format!("A ({},{}) mezőre több zseton került.", p.row, p.col));
            }
        }
        Ok(())
    }

    /// Egy sorban vagy oszlopban vannak-e a betűk. Visszatér: horizontal.
    fn validate_alignment(positions: &[(usize, usize)]) -> Result<bool, String> {
        let rows: std::collections::HashSet<usize> = positions.iter().map(|p| p.0).collect();
        let cols: std::collections::HashSet<usize> = positions.iter().map(|p| p.1).collect();
        if rows.len() > 1 && cols.len() > 1 {
            return Err("A betűknek egy sorban vagy egy oszlopban kell lenniük.".to_string());
        }
        Ok(rows.len() == 1 || positions.len() == 1)
    }

    /// Egy betűnél meghatározza az irányt a szomszédok alapján. Visszatér: (horizontal, has_neighbor)
    fn determine_direction_single(&self, r: i32, c: i32) -> (bool, bool) {
        let has_h = self.has_letter(r, c - 1) || self.has_letter(r, c + 1);
        let has_v = self.has_letter(r - 1, c) || self.has_letter(r + 1, c);
        if !self.is_empty && !has_h && !has_v {
            return (true, false); // nincs szomszéd — hiba lesz
        }
        (has_h || !has_v, true)
    }

    /// A fő irány fix tengelye és határai.
    fn get_main_bounds(positions: &[(usize, usize)], horizontal: bool) -> (usize, usize, usize) {
        if horizontal {
            let rows: std::collections::HashSet<usize> = positions.iter().map(|p| p.0).collect();
            let fixed = if rows.len() == 1 { *rows.iter().next().unwrap() } else { positions[0].0 };
            let start = positions.iter().map(|p| p.1).min().unwrap();
            let end = positions.iter().map(|p| p.1).max().unwrap();
            (fixed, start, end)
        } else {
            let cols: std::collections::HashSet<usize> = positions.iter().map(|p| p.1).collect();
            let fixed = if cols.len() == 1 { *cols.iter().next().unwrap() } else { positions[0].1 };
            let start = positions.iter().map(|p| p.0).min().unwrap();
            let end = positions.iter().map(|p| p.0).max().unwrap();
            (fixed, start, end)
        }
    }

    /// Folytonos-e a sor/oszlop (nincs üres rés).
    fn check_continuity(&self, fixed: usize, start: usize, end: usize, horizontal: bool) -> bool {
        (start..=end).all(|i| {
            let (r, c) = if horizontal { (fixed, i) } else { (i, fixed) };
            self.cells[r][c].is_some()
        })
    }

    /// Csatlakoznak-e a lerakott betűk meglévőkhöz.
    fn check_adjacency(&self, positions: &[(usize, usize)], new_positions: &Mask) -> bool {
        for &(r, c) in positions {
            for (dr, dc) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                let (nr, nc) = (r as i32 + dr, c as i32 + dc);
                let in_board = (0..BOARD_SIZE as i32).contains(&nr) && (0..BOARD_SIZE as i32).contains(&nc);
                if in_board && !new_positions[nr as usize][nc as usize] && self.has_letter(nr, nc) {
                    return true;
                }
            }
        }
        false
    }

    /// Összegyűjti az összes képzett szót (fő + mellékszavak).
    fn collect_words(
        &self,
        positions: &[(usize, usize)],
        new_positions: &Mask,
        horizontal: bool,
        fixed: usize,
        start: usize,
        end: usize,
    ) -> Vec<FormedWord> {
        let mut formed = Vec::new();
        if end > start {
            formed.push(self.extract_word(fixed, start, end, horizontal, new_positions));
        }
        for &(r, c) in positions {
            if horizontal {
                let (cs, ce) = self.find_word_bounds(c, r, r, false);
                if ce > cs {
                    formed.push(self.extract_word(c, cs, ce, false, new_positions));
                }
            } else {
                let (cs, ce) = self.find_word_bounds(r, c, c, true);
                if ce > cs {
                    formed.push(self.extract_word(r, cs, ce, true, new_positions));
                }
            }
        }
        formed
    }

    /// Kétjegyű betű két külön zsetonból (pl. S + Z a SZ helyett): az első ilyen betű (SZ, CS...), vagy None.
    ///
    /// Csak az újonnan lerakott zsetont érintő párokat nézi, így a régi, még megengedőbb szabállyal indult
    /// állások folytathatók maradnak.
    fn find_split_digraph(&self, formed: &[FormedWord], new_positions: &Mask) -> Option<String> {
        for word in formed {
            for pair in word.positions.windows(2) {
                let (r1, c1) = pair[0];
                let (r2, c2) = pair[1];
                if !new_positions[r1][c1] && !new_positions[r2][c2] {
                    continue;
                }
                let first = self.cells[r1][c1].unwrap().letter.as_str();
                let second = self.cells[r2][c2].unwrap().letter.as_str();
                if forms_digraph(first, second) {
                    return Some(format!("{first}{second}"));
                }
            }
        }
        None
    }

    fn validate_first_move(&self, placed: &[Placed], new_positions: &Mask) -> Option<String> {
        if !new_positions[CENTER as usize][CENTER as usize] {
            return Some("Az első szónak a középső mezőt (csillagot) kell fednie.".to_string());
        }
        if placed.len() < 2 {
            return Some("Az első szónak legalább 2 betűből kell állnia.".to_string());
        }
        None
    }

    fn validate_words(
        &self,
        positions: &[(usize, usize)],
        new_positions: &Mask,
        horizontal: bool,
        skip_dictionary: bool,
    ) -> Result<Vec<FormedWord>, String> {
        let (fixed, start, end) = Self::get_main_bounds(positions, horizontal);
        let (start, end) = self.find_word_bounds(fixed, start, end, horizontal);

        if !self.check_continuity(fixed, start, end, horizontal) {
            return Err("A betűknek folytonos sort kell alkotniuk.".to_string());
        }
        if !self.is_empty && !self.check_adjacency(positions, new_positions) {
            return Err("A szónak csatlakoznia kell meglévő betűkhöz.".to_string());
        }
        let formed = self.collect_words(positions, new_positions, horizontal, fixed, start, end);
        if formed.is_empty() {
            return Err("Legalább egy szót kell alkotni.".to_string());
        }
        if let Some(digraph) = self.find_split_digraph(&formed, new_positions) {
            let mut letters = digraph.chars();
            let a = letters.next().unwrap();
            let b = letters.next().unwrap();
            return Err(format!(
                "Kétjegyű betű ({digraph}) csak a saját zsetonjával rakható ki, {a} + {b} külön zsetonnal nem."
            ));
        }
        if !skip_dictionary {
            let words: Vec<&str> = formed.iter().map(|w| w.word.as_str()).collect();
            let (all_valid, invalid) = dictionary::check_words(&words);
            if !all_valid {
                return Err(format!("Érvénytelen szó(k): {}", invalid.join(", ")));
            }
        }
        Ok(formed)
    }

    /// Ellenőrzi a lerakott zsetonokat. Visszatér: a képzett szavak, vagy a hibaüzenet.
    pub fn validate_placement(&self, placed: &[Placed], skip_dictionary: bool) -> Result<Vec<FormedWord>, String> {
        if placed.is_empty() {
            return Err("Legalább egy zsetont le kell rakni.".to_string());
        }
        self.validate_positions(placed)?;

        let positions: Vec<(usize, usize)> = placed.iter().map(|p| (p.row as usize, p.col as usize)).collect();
        let mut new_positions: Mask = [[false; BOARD_SIZE]; BOARD_SIZE];
        for &(r, c) in &positions {
            new_positions[r][c] = true;
        }
        let mut horizontal = Self::validate_alignment(&positions)?;

        // Ideiglenes elhelyezés egy másolaton (a valódi tábla érintetlen marad)
        let mut temp = self.clone();
        for p in placed {
            temp.cells[p.row as usize][p.col as usize] = Some(Cell { letter: p.letter, is_blank: p.is_blank });
        }
        if self.is_empty {
            if let Some(error) = temp.validate_first_move(placed, &new_positions) {
                return Err(error);
            }
        }
        if placed.len() == 1 {
            let (h, has_neighbor) = temp.determine_direction_single(placed[0].row, placed[0].col);
            horizontal = h;
            if !self.is_empty && !has_neighbor {
                return Err("A betűnek csatlakoznia kell meglévő szóhoz.".to_string());
            }
        }
        temp.validate_words(&positions, &new_positions, horizontal, skip_dictionary)
    }

    /// Véglegesen lerakja a betűket.
    pub fn apply_placement(&mut self, placed: &[Placed]) {
        for p in placed {
            self.cells[p.row as usize][p.col as usize] = Some(Cell { letter: p.letter, is_blank: p.is_blank });
        }
        self.is_empty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(row: i32, col: i32, letter: &str) -> Placed {
        Placed::new(row, col, Tile::from_str(letter).unwrap(), false)
    }

    fn place_word(row: i32, col: i32, word: &[&str]) -> Vec<Placed> {
        word.iter().enumerate().map(|(i, l)| p(row, col + i as i32, l)).collect()
    }

    #[test]
    fn premium_layout_is_symmetric() {
        assert_eq!(premium_at(0, 0), Premium::Tw);
        assert_eq!(premium_at(0, 14), Premium::Tw);
        assert_eq!(premium_at(14, 14), Premium::Tw);
        assert_eq!(premium_at(7, 7), Premium::Star);
        assert_eq!(premium_at(7, 0), Premium::Tw);
        assert_eq!(premium_at(1, 5), Premium::Tl);
        assert_eq!(premium_at(1, 9), Premium::Tl);
        assert_eq!(premium_at(3, 7), Premium::Dl);
    }

    #[test]
    fn first_word_must_cover_center_and_scores() {
        let board = Board::new();
        // ALMA a (7,5)-től: A(1) L(1) M(1) A(1) — a (7,7) csillag DW
        let tiles = place_word(7, 5, &["A", "L", "M", "A"]);
        let words = board.validate_placement(&tiles, true).unwrap();
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].word, "ALMA");
        // (7,7) a harmadik betű: M a csillagon, DW → (1+1+1+1)*2 = 8, a (7,3) DL nem érintett
        assert_eq!(words[0].score, 8);

        let off_center = place_word(0, 0, &["A", "L", "M", "A"]);
        assert!(board.validate_placement(&off_center, true).unwrap_err().contains("középső mezőt"));
        let single = vec![p(7, 7, "A")];
        assert!(board.validate_placement(&single, true).unwrap_err().contains("legalább 2 betű"));
    }

    #[test]
    fn rejects_bad_geometry() {
        let board = Board::new();
        let diagonal = vec![p(7, 7, "A"), p(8, 8, "L")];
        assert!(board.validate_placement(&diagonal, true).unwrap_err().contains("egy sorban vagy egy oszlopban"));
        let outside = vec![Placed::new(15, 0, Tile::from_str("A").unwrap(), false)];
        assert!(board.validate_placement(&outside, true).unwrap_err().contains("táblán kívül"));
        let dup = vec![p(7, 7, "A"), p(7, 7, "L")];
        assert!(board.validate_placement(&dup, true).unwrap_err().contains("több zseton"));
        assert!(board.validate_placement(&[], true).unwrap_err().contains("Legalább egy zsetont"));
    }

    #[test]
    fn gap_and_connection_checks() {
        let mut board = Board::new();
        board.apply_placement(&place_word(7, 5, &["A", "L", "M", "A"]));
        // rés a sorban
        let gap = vec![p(9, 5, "K"), p(9, 7, "A")];
        assert!(board.validate_placement(&gap, true).is_err());
        // nem csatlakozik
        let loose = place_word(0, 0, &["K", "A"]);
        assert!(board.validate_placement(&loose, true).unwrap_err().contains("csatlakoznia"));
        // keresztszó: az ALMA 'L' betűje (7,6) alatt (8,6)
        let down = vec![p(8, 6, "A"), p(9, 6, "K")];
        let words = board.validate_placement(&down, true).unwrap();
        assert_eq!(words[0].word, "LAK");
    }

    #[test]
    fn blank_scores_zero_and_split_digraph_is_rejected() {
        let mut board = Board::new();
        let tiles = vec![
            Placed::new(7, 6, Tile::from_str("A").unwrap(), true),
            Placed::new(7, 7, Tile::from_str("L").unwrap(), false),
        ];
        let words = board.validate_placement(&tiles, true).unwrap();
        assert_eq!(words[0].score, 2); // joker 0 + L 1, a csillagon DW → 1*2
        board.apply_placement(&tiles);

        let mut board = Board::new();
        let split = vec![p(7, 6, "S"), p(7, 7, "Z")];
        let error = board.validate_placement(&split, true).unwrap_err();
        assert!(error.contains("Kétjegyű betű (SZ)"), "{error}");
        let sz = vec![p(7, 6, "SZ"), p(7, 7, "Z")];
        assert!(board.validate_placement(&sz, true).is_ok());
        board.apply_placement(&sz);
        assert!(!board.is_empty);
    }

    #[test]
    fn json_roundtrip() {
        let mut board = Board::new();
        board.apply_placement(&place_word(7, 5, &["A", "L", "M", "A"]));
        let restored = Board::from_json(&board.to_json());
        assert_eq!(restored, board);
    }
}
