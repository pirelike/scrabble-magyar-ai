use crate::rules::{BoardLayout, Premium};
use crate::tile::Square;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Direction {
    Horizontal,
    Vertical,
}

impl Direction {
    #[inline]
    pub const fn flip(self) -> Direction {
        match self {
            Direction::Horizontal => Direction::Vertical,
            Direction::Vertical => Direction::Horizontal,
        }
    }

    pub const ALL: [Direction; 2] = [Direction::Horizontal, Direction::Vertical];
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Coord {
    pub row: usize,
    pub col: usize,
}

impl Coord {
    #[inline]
    pub const fn new(row: usize, col: usize) -> Coord {
        Coord { row, col }
    }

    pub fn parse(s: &str) -> Option<(Coord, Direction)> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let leading_digit = s.as_bytes()[0].is_ascii_digit();

        let (digits, letters): (String, String) = if leading_digit {
            let split = s.find(|c: char| c.is_ascii_alphabetic())?;
            (s[..split].into(), s[split..].into())
        } else {
            let split = s.find(|c: char| c.is_ascii_digit())?;
            (s[split..].into(), s[..split].into())
        };

        if digits.is_empty() || letters.is_empty() {
            return None;
        }
        if !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        if !letters.bytes().all(|b| b.is_ascii_alphabetic()) {
            return None;
        }

        let row = digits.parse::<usize>().ok()?.checked_sub(1)?;

        let mut col: usize = 0;
        for b in letters.bytes() {
            let v = (b.to_ascii_uppercase() - b'A') as usize + 1;
            col = col.checked_mul(26)?.checked_add(v)?;
        }
        let col = col.checked_sub(1)?;

        let dir = if leading_digit {
            Direction::Horizontal
        } else {
            Direction::Vertical
        };
        Some((Coord::new(row, col), dir))
    }

    pub fn format(self, dir: Direction) -> String {
        let mut letters = String::new();
        let mut n = self.col + 1;
        while n > 0 {
            let rem = (n - 1) % 26;
            letters.insert(0, (b'A' + rem as u8) as char);
            n = (n - 1) / 26;
        }
        let row = self.row + 1;
        match dir {
            Direction::Horizontal => alloc::format!("{row}{letters}"),
            Direction::Vertical => alloc::format!("{letters}{row}"),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Board {
    width: usize,
    height: usize,
    squares: Vec<Square>,
    transposed: Vec<Square>,
    occupied: usize,
}

impl Board {
    pub fn new(layout: &BoardLayout) -> Board {
        let (width, height) = (layout.width(), layout.height());
        Board {
            width,
            height,
            squares: vec![Square::EMPTY; width * height],
            transposed: vec![Square::EMPTY; width * height],
            occupied: 0,
        }
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
    pub fn is_empty(&self) -> bool {
        self.occupied == 0
    }

    #[inline]
    pub fn tiles_played(&self) -> usize {
        self.occupied
    }

    #[inline]
    pub fn contains(&self, row: usize, col: usize) -> bool {
        row < self.height && col < self.width
    }

    #[inline]
    pub fn get(&self, row: usize, col: usize) -> Square {
        self.squares[row * self.width + col]
    }

    #[inline]
    pub fn try_get(&self, row: isize, col: isize) -> Option<Square> {
        if row < 0 || col < 0 {
            return None;
        }
        let (row, col) = (row as usize, col as usize);
        if self.contains(row, col) {
            Some(self.get(row, col))
        } else {
            None
        }
    }

    pub fn set(&mut self, row: usize, col: usize, square: Square) {
        let idx = row * self.width + col;
        let was = self.squares[idx];
        if was.is_occupied() && square.is_empty() {
            self.occupied -= 1;
        } else if was.is_empty() && square.is_occupied() {
            self.occupied += 1;
        }
        self.squares[idx] = square;
        self.transposed[col * self.height + row] = square;
    }

    #[inline]
    pub fn clear(&mut self, row: usize, col: usize) {
        self.set(row, col, Square::EMPTY);
    }

    #[inline]
    pub fn squares(&self) -> &[Square] {
        &self.squares
    }

    #[inline]
    pub fn lane_count(&self, dir: Direction) -> usize {
        match dir {
            Direction::Horizontal => self.height,
            Direction::Vertical => self.width,
        }
    }

    #[inline]
    pub fn lane_len(&self, dir: Direction) -> usize {
        match dir {
            Direction::Horizontal => self.width,
            Direction::Vertical => self.height,
        }
    }

    #[inline]
    pub fn lane(&self, dir: Direction, index: usize) -> &[Square] {
        let len = self.lane_len(dir);
        let start = index * len;
        match dir {
            Direction::Horizontal => &self.squares[start..start + len],
            Direction::Vertical => &self.transposed[start..start + len],
        }
    }

    #[inline]
    pub fn from_lane(dir: Direction, lane: usize, offset: usize) -> Coord {
        match dir {
            Direction::Horizontal => Coord::new(lane, offset),
            Direction::Vertical => Coord::new(offset, lane),
        }
    }

    #[inline]
    pub fn to_lane(dir: Direction, coord: Coord) -> (usize, usize) {
        match dir {
            Direction::Horizontal => (coord.row, coord.col),
            Direction::Vertical => (coord.col, coord.row),
        }
    }

    pub fn has_neighbor(&self, row: usize, col: usize) -> bool {
        let (r, c) = (row as isize, col as isize);
        [(r - 1, c), (r + 1, c), (r, c - 1), (r, c + 1)]
            .iter()
            .any(|&(rr, cc)| self.try_get(rr, cc).is_some_and(|s| s.is_occupied()))
    }

    pub fn run_start(&self, dir: Direction, lane: usize, offset: usize) -> usize {
        let cells = self.lane(dir, lane);
        let mut start = offset;
        while start > 0 && cells[start - 1].is_occupied() {
            start -= 1;
        }
        start
    }

    pub fn run_end(&self, dir: Direction, lane: usize, offset: usize) -> usize {
        let cells = self.lane(dir, lane);
        let mut end = offset;
        while end < cells.len() && cells[end].is_occupied() {
            end += 1;
        }
        end
    }

    pub fn to_text(&self, alphabet: &crate::rules::Alphabet) -> String {
        let mut out = String::with_capacity((self.width + 1) * self.height);
        for row in 0..self.height {
            for col in 0..self.width {
                let sq = self.get(row, col);
                match sq.index() {
                    None => out.push('.'),
                    Some(letter) => {
                        let s = alphabet.display(letter);
                        if sq.is_blank() {
                            out.extend(s.chars().map(|c| c.to_ascii_lowercase()));
                        } else {
                            out.push_str(s);
                        }
                    }
                }
            }
            if row + 1 < self.height {
                out.push('\n');
            }
        }
        out
    }

    pub fn parse(
        layout: &BoardLayout,
        alphabet: &crate::rules::Alphabet,
        text: &str,
    ) -> Result<Board, BoardParseError> {
        let mut board = Board::new(layout);
        let rows: Vec<&str> = text
            .lines()
            .map(str::trim_end)
            .filter(|l| !l.is_empty())
            .collect();
        if rows.len() != layout.height() {
            return Err(BoardParseError::WrongHeight {
                expected: layout.height(),
                found: rows.len(),
            });
        }
        for (r, line) in rows.iter().enumerate() {
            let chars: Vec<char> = line.chars().collect();
            if chars.len() != layout.width() {
                return Err(BoardParseError::WrongWidth {
                    row: r,
                    expected: layout.width(),
                    found: chars.len(),
                });
            }
            for (c, &ch) in chars.iter().enumerate() {
                if ch == '.' || ch == ' ' {
                    continue;
                }
                let (letter, blank) = alphabet.parse_char(ch).ok_or(BoardParseError::BadChar {
                    row: r,
                    col: c,
                    ch,
                })?;
                let sq = if blank {
                    Square::blank_letter(letter)
                } else {
                    Square::letter(letter)
                };
                board.set(r, c, sq);
            }
        }
        Ok(board)
    }

    pub fn display<'a>(
        &'a self,
        layout: &'a BoardLayout,
        alphabet: &'a crate::rules::Alphabet,
    ) -> BoardDisplay<'a> {
        BoardDisplay {
            board: self,
            layout,
            alphabet,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BoardParseError {
    WrongHeight {
        expected: usize,
        found: usize,
    },
    WrongWidth {
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

impl fmt::Display for BoardParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BoardParseError::WrongHeight { expected, found } => {
                write!(f, "board has {found} rows, expected {expected}")
            }
            BoardParseError::WrongWidth {
                row,
                expected,
                found,
            } => write!(f, "row {row} has {found} cells, expected {expected}"),
            BoardParseError::BadChar { row, col, ch } => {
                write!(
                    f,
                    "character {ch:?} at row {row}, column {col} is not in the alphabet"
                )
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for BoardParseError {}

pub struct BoardDisplay<'a> {
    board: &'a Board,
    layout: &'a BoardLayout,
    alphabet: &'a crate::rules::Alphabet,
}

impl fmt::Display for BoardDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = self.board;
        write!(f, "   ")?;
        for col in 0..b.width() {
            write!(f, "{} ", (b'A' + (col % 26) as u8) as char)?;
        }
        writeln!(f)?;
        for row in 0..b.height() {
            write!(f, "{:>2} ", row + 1)?;
            for col in 0..b.width() {
                let sq = b.get(row, col);
                match sq.index() {
                    Some(letter) => {
                        let s = self.alphabet.display(letter);
                        if sq.is_blank() {
                            for c in s.chars() {
                                write!(f, "{}", c.to_ascii_lowercase())?;
                            }
                        } else {
                            write!(f, "{s}")?;
                        }
                        write!(f, " ")?;
                    }
                    None => {
                        let p = self.layout.premium(row, col);
                        let ch = if p == Premium::Normal {
                            '.'
                        } else {
                            p.as_char()
                        };
                        write!(f, "{ch} ")?;
                    }
                }
            }
            if row + 1 < b.height() {
                writeln!(f)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Alphabet;

    fn standard() -> BoardLayout {
        BoardLayout::standard()
    }

    #[test]
    fn new_board_is_empty() {
        let b = Board::new(&standard());
        assert!(b.is_empty());
        assert_eq!(b.tiles_played(), 0);
        assert!(b.squares().iter().all(|s| s.is_empty()));
    }

    #[test]
    fn set_keeps_the_transpose_in_sync() {
        let mut b = Board::new(&standard());
        b.set(3, 7, Square::letter(4));
        assert_eq!(b.get(3, 7), Square::letter(4));
        assert_eq!(b.lane(Direction::Vertical, 7)[3], Square::letter(4));
        assert_eq!(b.lane(Direction::Horizontal, 3)[7], Square::letter(4));
        assert_eq!(b.tiles_played(), 1);
    }

    #[test]
    fn clearing_decrements_the_tile_count() {
        let mut b = Board::new(&standard());
        b.set(0, 0, Square::letter(0));
        b.set(0, 1, Square::letter(1));
        assert_eq!(b.tiles_played(), 2);
        b.clear(0, 0);
        assert_eq!(b.tiles_played(), 1);
        assert!(b.get(0, 0).is_empty());
        assert_eq!(b.lane(Direction::Vertical, 0)[0], Square::EMPTY);

        b.set(0, 1, Square::letter(2));
        assert_eq!(b.tiles_played(), 1);
    }

    #[test]
    fn has_neighbor_sees_all_four_sides() {
        let mut b = Board::new(&standard());
        assert!(!b.has_neighbor(7, 7));
        b.set(7, 8, Square::letter(0));
        assert!(b.has_neighbor(7, 7));
        assert!(b.has_neighbor(6, 8));
        assert!(!b.has_neighbor(5, 8));

        assert!(!b.has_neighbor(0, 0));
    }

    #[test]
    fn run_bounds_find_contiguous_tiles() {
        let mut b = Board::new(&standard());
        for (i, col) in (4..8).enumerate() {
            b.set(7, col, Square::letter(i as u8));
        }
        assert_eq!(b.run_start(Direction::Horizontal, 7, 6), 4);
        assert_eq!(b.run_end(Direction::Horizontal, 7, 4), 8);

        assert_eq!(b.run_start(Direction::Horizontal, 7, 0), 0);
        assert_eq!(b.run_end(Direction::Horizontal, 7, 0), 0);
    }

    #[test]
    fn coord_parses_both_notations() {
        assert_eq!(
            Coord::parse("8H"),
            Some((Coord::new(7, 7), Direction::Horizontal))
        );
        assert_eq!(
            Coord::parse("H8"),
            Some((Coord::new(7, 7), Direction::Vertical))
        );
        assert_eq!(
            Coord::parse("15O"),
            Some((Coord::new(14, 14), Direction::Horizontal))
        );
        assert_eq!(
            Coord::parse("1A"),
            Some((Coord::new(0, 0), Direction::Horizontal))
        );
        assert_eq!(Coord::parse(""), None);
        assert_eq!(Coord::parse("8"), None);
        assert_eq!(Coord::parse("H"), None);
        assert_eq!(Coord::parse("0A"), None, "rows are 1-based");
    }

    #[test]
    fn coord_format_round_trips() {
        for row in 0..15 {
            for col in 0..15 {
                for dir in Direction::ALL {
                    let c = Coord::new(row, col);
                    let s = c.format(dir);
                    assert_eq!(
                        Coord::parse(&s),
                        Some((c, dir)),
                        "round trip failed for {s}"
                    );
                }
            }
        }
    }

    #[test]
    fn coord_handles_columns_past_z() {
        assert_eq!(Coord::new(0, 25).format(Direction::Horizontal), "1Z");
        assert_eq!(Coord::new(0, 26).format(Direction::Horizontal), "1AA");
        assert_eq!(
            Coord::parse("1AA"),
            Some((Coord::new(0, 26), Direction::Horizontal))
        );
    }

    #[test]
    fn text_round_trips() {
        let layout = standard();
        let alpha = Alphabet::english();
        let mut b = Board::new(&layout);
        b.set(7, 7, Square::letter(2));
        b.set(7, 8, Square::letter(0));
        b.set(7, 9, Square::blank_letter(19));

        let text = b.to_text(&alpha);
        let back = Board::parse(&layout, &alpha, &text).expect("round trip should parse");
        assert_eq!(back, b);
        assert!(text.lines().nth(7).unwrap().contains("CAt"));
    }

    #[test]
    fn parse_rejects_wrong_dimensions() {
        let layout = standard();
        let alpha = Alphabet::english();
        assert!(matches!(
            Board::parse(&layout, &alpha, "..."),
            Err(BoardParseError::WrongHeight { .. })
        ));
    }

    #[test]
    fn lane_indexing_agrees_with_get() {
        let mut b = Board::new(&standard());
        b.set(2, 11, Square::letter(5));
        for dir in Direction::ALL {
            for lane in 0..b.lane_count(dir) {
                for off in 0..b.lane_len(dir) {
                    let c = Board::from_lane(dir, lane, off);
                    assert_eq!(b.lane(dir, lane)[off], b.get(c.row, c.col));
                    assert_eq!(Board::to_lane(dir, c), (lane, off));
                }
            }
        }
    }
}
