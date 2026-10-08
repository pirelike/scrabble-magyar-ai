use crate::board::{Board, Coord, Direction};
use crate::internal::bits;
use crate::lexicon::{Lexicon, NodeIdx, WordGraph};
use crate::rack::Rack;
use crate::rules::{Alphabet, GameConfig};
use crate::tile::{Square, Tile};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

pub const MAX_WORD: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Play {
    row: u8,
    col: u8,
    dir: Direction,
    len: u8,
    tiles_used: u8,
    word: [Square; MAX_WORD],
    from_rack: u32,
    score: i32,
}

impl Play {
    #[inline]
    pub fn coord(&self) -> Coord {
        Coord::new(self.row as usize, self.col as usize)
    }

    #[inline]
    pub fn direction(&self) -> Direction {
        self.dir
    }

    #[inline]
    pub fn word(&self) -> &[Square] {
        &self.word[..self.len as usize]
    }

    #[inline]
    pub fn tiles_used(&self) -> usize {
        self.tiles_used as usize
    }

    #[inline]
    pub fn score(&self) -> i32 {
        self.score
    }

    #[inline]
    pub fn is_bingo(&self, rack_size: usize) -> bool {
        self.tiles_used as usize >= rack_size
    }

    pub fn placements(&self) -> impl Iterator<Item = (Coord, Square)> + '_ {
        let (row, col, dir) = (self.row as usize, self.col as usize, self.dir);
        bits(self.from_rack).map(move |i| {
            let i = i as usize;
            let coord = match dir {
                Direction::Horizontal => Coord::new(row, col + i),
                Direction::Vertical => Coord::new(row + i, col),
            };
            (coord, self.word[i])
        })
    }

    pub fn tiles(&self) -> Rack {
        let mut r = Rack::new();
        for i in bits(self.from_rack) {
            let sq = self.word[i as usize];
            r.add(if sq.is_blank() {
                Tile::BLANK
            } else {
                Tile::letter(sq.index_unchecked())
            });
        }
        r
    }

    pub fn leave(&self, rack: &Rack) -> Option<Rack> {
        let mut left = *rack;
        if left.remove_all(&self.tiles()) {
            Some(left)
        } else {
            None
        }
    }

    pub fn apply(&self, board: &mut Board) {
        for (coord, square) in self.placements() {
            board.set(coord.row, coord.col, square);
        }
    }

    pub fn undo(&self, board: &mut Board) {
        for (coord, _) in self.placements() {
            board.clear(coord.row, coord.col);
        }
    }

    pub fn word_text(&self, alphabet: &Alphabet) -> String {
        self.word()
            .iter()
            .map(|sq| {
                let s = alphabet.display(sq.index_unchecked());
                if sq.is_blank() {
                    s.to_lowercase()
                } else {
                    s.into()
                }
            })
            .collect()
    }

    pub fn to_text(&self, alphabet: &Alphabet) -> String {
        alloc::format!(
            "{} {} {}",
            self.coord().format(self.dir),
            self.word_text(alphabet),
            self.score
        )
    }
}

impl fmt::Debug for Play {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let word: String = self
            .word()
            .iter()
            .map(|sq| {
                let c = (b'A' + sq.index_unchecked()) as char;
                if sq.is_blank() {
                    c.to_ascii_lowercase()
                } else {
                    c
                }
            })
            .collect();
        write!(
            f,
            "Play({} {} {}pts, {} tiles)",
            self.coord().format(self.dir),
            word,
            self.score,
            self.tiles_used
        )
    }
}

#[derive(Clone, Debug)]
pub struct Analysis {
    width: usize,
    height: usize,
    cross: [Vec<u64>; 2],
    cross_score: [Vec<i32>; 2],
    has_cross: [Vec<bool>; 2],
    anchor: [Vec<bool>; 2],
    board_empty: bool,
}

#[inline]
fn dir_index(dir: Direction) -> usize {
    match dir {
        Direction::Horizontal => 0,
        Direction::Vertical => 1,
    }
}

impl Analysis {
    pub fn new(width: usize, height: usize) -> Analysis {
        let n = width * height;
        Analysis {
            width,
            height,
            cross: [vec![0; n], vec![0; n]],
            cross_score: [vec![0; n], vec![0; n]],
            has_cross: [vec![false; n], vec![false; n]],
            anchor: [vec![false; n], vec![false; n]],
            board_empty: true,
        }
    }

    #[inline]
    pub fn cross_set(&self, dir: Direction, lane: usize, offset: usize, lane_len: usize) -> u64 {
        self.cross[dir_index(dir)][lane * lane_len + offset]
    }

    #[inline]
    pub fn is_anchor(&self, dir: Direction, lane: usize, offset: usize, lane_len: usize) -> bool {
        self.anchor[dir_index(dir)][lane * lane_len + offset]
    }

    #[inline]
    pub fn board_is_empty(&self) -> bool {
        self.board_empty
    }

    pub fn rebuild(&mut self, board: &Board, lexicon: &Lexicon, alphabet: &Alphabet) {
        debug_assert_eq!(board.width(), self.width);
        debug_assert_eq!(board.height(), self.height);
        self.board_empty = board.is_empty();

        let full = lexicon.letter_mask();
        let mut prefix: Vec<u8> = Vec::with_capacity(MAX_WORD);
        let mut suffix: Vec<u8> = Vec::with_capacity(MAX_WORD);

        for dir in Direction::ALL {
            let d = dir_index(dir);
            let cross_dir = dir.flip();
            let lane_len = board.lane_len(dir);

            for lane in 0..board.lane_count(dir) {
                for offset in 0..lane_len {
                    let idx = lane * lane_len + offset;
                    let coord = Board::from_lane(dir, lane, offset);

                    if board.get(coord.row, coord.col).is_occupied() {
                        self.cross[d][idx] = 0;
                        self.cross_score[d][idx] = 0;
                        self.has_cross[d][idx] = false;
                        self.anchor[d][idx] = false;
                        continue;
                    }

                    self.anchor[d][idx] = board.has_neighbor(coord.row, coord.col);

                    let (cl, co) = Board::to_lane(cross_dir, coord);
                    let cells = board.lane(cross_dir, cl);

                    prefix.clear();
                    suffix.clear();
                    let mut score = 0;

                    let mut i = co;
                    while i > 0 && cells[i - 1].is_occupied() {
                        i -= 1;
                    }
                    for cell in &cells[i..co] {
                        prefix.push(cell.index_unchecked());
                        if !cell.is_blank() {
                            score += alphabet.score(cell.index_unchecked());
                        }
                    }
                    let mut j = co + 1;
                    while j < cells.len() && cells[j].is_occupied() {
                        let cell = cells[j];
                        suffix.push(cell.index_unchecked());
                        if !cell.is_blank() {
                            score += alphabet.score(cell.index_unchecked());
                        }
                        j += 1;
                    }

                    let has = !prefix.is_empty() || !suffix.is_empty();
                    self.has_cross[d][idx] = has;
                    self.cross_score[d][idx] = score;
                    self.cross[d][idx] = if has {
                        lexicon.cross_set(&prefix, &suffix)
                    } else {
                        full
                    };
                }
            }
        }
    }
}

pub struct MoveGenerator {
    config: GameConfig,
    analysis: Analysis,
    plays: Vec<Play>,
}

impl MoveGenerator {
    pub fn new(config: &GameConfig) -> MoveGenerator {
        let (w, h) = (config.width(), config.height());
        assert!(
            w <= MAX_WORD && h <= MAX_WORD,
            "board is {w}x{h}, which exceeds the {MAX_WORD}-cell maximum"
        );
        MoveGenerator {
            config: config.clone(),
            analysis: Analysis::new(w, h),
            plays: Vec::new(),
        }
    }

    #[inline]
    pub fn config(&self) -> &GameConfig {
        &self.config
    }

    #[inline]
    pub fn analysis(&self) -> &Analysis {
        &self.analysis
    }

    pub fn generate(&mut self, board: &Board, rack: &Rack, lexicon: &Lexicon) -> &[Play] {
        self.prepare(board, lexicon).generate(rack);
        &self.plays
    }

    pub fn generate_sorted(&mut self, board: &Board, rack: &Rack, lexicon: &Lexicon) -> &[Play] {
        self.generate(board, rack, lexicon);
        self.plays
            .sort_unstable_by_key(|p| core::cmp::Reverse(p.score));
        &self.plays
    }

    pub fn best_by_score(&mut self, board: &Board, rack: &Rack, lexicon: &Lexicon) -> Option<Play> {
        self.generate(board, rack, lexicon)
            .iter()
            .max_by_key(|p| p.score)
            .copied()
    }

    pub fn prepare<'g>(&'g mut self, board: &'g Board, lexicon: &'g Lexicon) -> Prepared<'g> {
        self.analysis.rebuild(board, lexicon, &self.config.alphabet);
        Prepared {
            config: &self.config,
            analysis: &self.analysis,
            plays: &mut self.plays,
            board,
            lexicon,
        }
    }
}

pub struct Prepared<'a> {
    config: &'a GameConfig,
    analysis: &'a Analysis,
    plays: &'a mut Vec<Play>,
    board: &'a Board,
    lexicon: &'a Lexicon,
}

impl Prepared<'_> {
    pub fn generate(&mut self, rack: &Rack) -> &[Play] {
        self.plays.clear();
        if rack.is_empty() {
            return self.plays;
        }
        let mut gen = Gen::new(
            self.board,
            self.lexicon,
            self.analysis,
            self.config,
            self.plays,
            *rack,
        );
        gen.run();
        self.plays
    }

    pub fn best_by_score(&mut self, rack: &Rack) -> Option<Play> {
        self.generate(rack).iter().max_by_key(|p| p.score).copied()
    }

    #[inline]
    pub fn analysis(&self) -> &Analysis {
        self.analysis
    }

    #[cfg(feature = "rayon")]
    pub fn generate_parallel(&mut self, rack: &Rack) -> &[Play] {
        use rayon::prelude::*;

        self.plays.clear();
        if rack.is_empty() {
            return self.plays;
        }

        let mut lanes: Vec<(Direction, usize)> = Vec::new();
        for dir in Direction::ALL {
            for lane in 0..self.board.lane_count(dir) {
                lanes.push((dir, lane));
            }
        }

        let (board, lexicon, analysis, config) =
            (self.board, self.lexicon, self.analysis, self.config);
        let rack = *rack;

        let chunks: Vec<Vec<Play>> = lanes
            .par_iter()
            .map(|&(dir, lane)| {
                let mut out = Vec::new();
                let mut gen = Gen::new(board, lexicon, analysis, config, &mut out, rack);
                gen.run_one(dir, lane);
                out
            })
            .collect();

        for chunk in &chunks {
            self.plays.extend_from_slice(chunk);
        }
        self.plays
    }
}

struct Gen<'a> {
    board: &'a Board,
    lexicon: &'a Lexicon,
    graph: &'a WordGraph,
    analysis: &'a Analysis,
    config: &'a GameConfig,
    plays: &'a mut Vec<Play>,
    rack: Rack,
    buf: [Square; MAX_WORD],
    placed: u32,
    placed_count: u8,
    dir: Direction,
    lane: usize,
    lane_len: usize,
    anchor: usize,
    sep: u8,
    sep_bit: u64,
    cells: &'a [Square],
    cross: &'a [u64],
    has_cross: &'a [bool],
    cross_score: &'a [i32],
    premiums: &'a [crate::rules::Premium],
}

impl<'a> Gen<'a> {
    fn new(
        board: &'a Board,
        lexicon: &'a Lexicon,
        analysis: &'a Analysis,
        config: &'a GameConfig,
        plays: &'a mut Vec<Play>,
        rack: Rack,
    ) -> Gen<'a> {
        Gen {
            board,
            lexicon,
            graph: lexicon.gaddag(),
            analysis,
            config,
            plays,
            rack,
            buf: [Square::EMPTY; MAX_WORD],
            placed: 0,
            placed_count: 0,
            dir: Direction::Horizontal,
            lane: 0,
            lane_len: 0,
            anchor: 0,
            sep: lexicon.separator(),
            sep_bit: 1u64 << lexicon.separator(),
            cells: &[],
            cross: &[],
            has_cross: &[],
            cross_score: &[],
            premiums: &[],
        }
    }

    fn run(&mut self) {
        for dir in Direction::ALL {
            for lane in 0..self.board.lane_count(dir) {
                self.run_one(dir, lane);
            }
        }
    }

    fn run_one(&mut self, dir: Direction, lane: usize) {
        let d = dir_index(dir);
        let all_premiums = match dir {
            Direction::Horizontal => self.config.layout.premiums(),
            Direction::Vertical => self.config.layout.transposed_premiums(),
        };
        self.dir = dir;
        self.lane_len = self.board.lane_len(dir);
        let span = lane * self.lane_len..(lane + 1) * self.lane_len;
        self.lane = lane;
        self.cells = self.board.lane(dir, lane);
        self.cross = &self.analysis.cross[d][span.clone()];
        self.has_cross = &self.analysis.has_cross[d][span.clone()];
        self.cross_score = &self.analysis.cross_score[d][span.clone()];
        self.premiums = &all_premiums[span];
        self.run_lane();
    }

    fn run_lane(&mut self) {
        for offset in 0..self.lane_len {
            if !self.cells[offset].is_empty() || !self.is_anchor(offset) {
                continue;
            }
            self.anchor = offset;
            self.placed = 0;
            self.placed_count = 0;

            if offset > 0 && self.cells[offset - 1].is_occupied() {
                self.fixed_prefix(offset);
            } else {
                self.open_prefix(offset);
            }
        }
    }

    fn fixed_prefix(&mut self, anchor: usize) {
        let mut start = anchor - 1;
        while start > 0 && self.cells[start - 1].is_occupied() {
            start -= 1;
        }
        let mut node = self.graph.root();
        for cell in self.cells[start..anchor].iter().rev() {
            match self.graph.child(node, cell.index_unchecked()) {
                Some(n) => node = n,
                None => return,
            }
        }
        let Some(after_sep) = self.graph.child(node, self.lexicon.separator()) else {
            return;
        };
        self.extend_right(anchor, after_sep, start);
    }

    fn open_prefix(&mut self, anchor: usize) {
        let mut limit = 0usize;
        let mut q = anchor;
        while q > 0 && self.cells[q - 1].is_empty() && !self.is_anchor(q - 1) {
            limit += 1;
            q -= 1;
        }

        let limit = limit.min(self.rack.len().saturating_sub(1));

        let root = self.graph.root();
        let root_children = self.graph.children(root);
        let allowed = root_children & self.cross[anchor];
        if allowed == 0 {
            return;
        }
        self.for_each_placement(anchor, allowed, |gen, letter| {
            let next = gen.graph.child_with_mask(root, root_children, letter);
            gen.walk_left(anchor as isize - 1, next, limit);
        });
    }

    fn walk_left(&mut self, pos: isize, node: NodeIdx, room: usize) {
        let children = self.graph.children(node);

        let left_clear = pos < 0 || self.cells[pos as usize].is_empty();
        if left_clear && children & self.sep_bit != 0 {
            let after_sep = self.graph.child_with_mask(node, children, self.sep);
            self.extend_right(self.anchor + 1, after_sep, (pos + 1) as usize);
        }

        if room == 0 || pos < 0 {
            return;
        }
        let p = pos as usize;
        if self.cells[p].is_occupied() {
            return;
        }
        let allowed = children & self.cross[p];
        if allowed == 0 {
            return;
        }
        self.for_each_placement(p, allowed, |gen, letter| {
            let next = gen.graph.child_with_mask(node, children, letter);
            gen.walk_left(pos - 1, next, room - 1);
        });
    }

    fn extend_right(&mut self, pos: usize, node: NodeIdx, word_start: usize) {
        let at_end = pos >= self.lane_len;
        let (accepts, children) = WordGraph::split_header(self.graph.header(node));

        if accepts && (at_end || self.cells[pos].is_empty()) {
            self.record(word_start, pos);
        }
        if at_end {
            return;
        }

        if self.cells[pos].is_occupied() {
            let letter = self.cells[pos].index_unchecked();
            if children & (1u64 << letter) != 0 {
                let next = self.graph.child_with_mask(node, children, letter);
                self.extend_right(pos + 1, next, word_start);
            }
            return;
        }

        let allowed = children & self.cross[pos];
        if allowed == 0 {
            return;
        }
        self.for_each_placement(pos, allowed, |gen, letter| {
            let next = gen.graph.child_with_mask(node, children, letter);
            gen.extend_right(pos + 1, next, word_start);
        });
    }

    #[inline]
    fn for_each_placement(
        &mut self,
        offset: usize,
        allowed: u64,
        mut descend: impl FnMut(&mut Self, u8),
    ) {
        let mut real = allowed & self.rack.mask();
        while real != 0 {
            let letter = real.trailing_zeros() as u8;
            real &= real - 1;
            let tile = Tile::letter(letter);
            self.rack.remove_unchecked(tile);
            self.place(offset, Square::letter(letter));
            descend(self, letter);
            self.unplace(offset);
            self.rack.add_unchecked(tile);
        }

        if self.rack.has_blank() {
            let mut any = allowed;
            while any != 0 {
                let letter = any.trailing_zeros() as u8;
                any &= any - 1;
                self.rack.remove_unchecked(Tile::BLANK);
                self.place(offset, Square::blank_letter(letter));
                descend(self, letter);
                self.unplace(offset);
                self.rack.add_unchecked(Tile::BLANK);
            }
        }
    }

    #[inline]
    fn place(&mut self, offset: usize, square: Square) {
        self.buf[offset] = square;
        self.placed |= 1 << offset;
        self.placed_count += 1;
    }

    #[inline]
    fn unplace(&mut self, offset: usize) {
        self.placed &= !(1 << offset);
        self.placed_count -= 1;
    }

    #[inline]
    fn is_anchor(&self, offset: usize) -> bool {
        if self.analysis.board_empty {
            if !self.config.layout.start_required() {
                return true;
            }
            let (r, c) = self.config.layout.start();
            let (sl, so) = Board::to_lane(self.dir, Coord::new(r, c));
            return self.lane == sl && offset == so;
        }
        self.analysis
            .is_anchor(self.dir, self.lane, offset, self.lane_len)
    }

    fn record(&mut self, start: usize, end: usize) {
        if self.placed_count == 0 {
            return;
        }
        let len = end - start;
        if len < 2 {
            return;
        }

        if self.placed_count == 1 && self.dir == Direction::Vertical {
            let offset = self.placed.trailing_zeros() as usize;
            let coord = Board::from_lane(self.dir, self.lane, offset);
            let (cl, co) = Board::to_lane(Direction::Horizontal, coord);
            let row = self.board.lane(Direction::Horizontal, cl);
            let left = co > 0 && row[co - 1].is_occupied();
            let right = co + 1 < row.len() && row[co + 1].is_occupied();
            if left || right {
                return;
            }
        }

        let (score, word, from_rack) = self.score(start, end);
        let coord = Board::from_lane(self.dir, self.lane, start);
        self.plays.push(Play {
            row: coord.row as u8,
            col: coord.col as u8,
            dir: self.dir,
            len: len as u8,
            tiles_used: self.placed_count,
            word,
            from_rack,
            score,
        });
    }

    fn score(&self, start: usize, end: usize) -> (i32, [Square; MAX_WORD], u32) {
        let alphabet = &self.config.alphabet;
        let mut word = [Square::EMPTY; MAX_WORD];
        let mut from_rack = 0u32;
        let mut main = 0i32;
        let mut word_mult = 1i32;
        let mut cross_total = 0i32;

        for offset in start..end {
            let is_new = self.placed & (1 << offset) != 0;
            let square = if is_new {
                self.buf[offset]
            } else {
                self.cells[offset]
            };
            word[offset - start] = square;

            let value = if square.is_blank() {
                0
            } else {
                alphabet.score(square.index_unchecked())
            };

            if !is_new {
                main += value;
                continue;
            }

            from_rack |= 1 << (offset - start);
            let premium = self.premiums[offset];
            let letter_score = value * premium.letter_multiplier();
            main += letter_score;
            word_mult *= premium.word_multiplier();

            if self.has_cross[offset] {
                let cross = self.cross_score[offset] + letter_score;
                cross_total += cross * premium.word_multiplier();
            }
        }

        let mut total = main * word_mult + cross_total;
        if self.placed_count as usize >= self.config.rack_size {
            total += self.config.bingo_bonus;
        }
        (total, word, from_rack)
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PlayError {
    OffBoard(Coord),
    Occupied(Coord),
    NotInLine,
    NotContiguous,
    NothingPlaced,
    RackMissingTiles,
    Disconnected,
    MissesStart,
    WordTooShort,
    NotAWord(Vec<u8>),
    WrongScore { claimed: i32, actual: i32 },
}

impl fmt::Display for PlayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlayError::OffBoard(c) => write!(f, "square {}, {} is off the board", c.row, c.col),
            PlayError::Occupied(c) => {
                write!(f, "square {}, {} already holds a tile", c.row, c.col)
            }
            PlayError::NotInLine => write!(f, "tiles must all be in one row or one column"),
            PlayError::NotContiguous => write!(f, "the play leaves a gap"),
            PlayError::NothingPlaced => write!(f, "the play places no tiles"),
            PlayError::RackMissingTiles => write!(f, "the rack does not hold those tiles"),
            PlayError::Disconnected => write!(f, "the play does not touch the existing tiles"),
            PlayError::MissesStart => write!(f, "the opening play must cover the starting square"),
            PlayError::WordTooShort => write!(f, "a play must form a word of at least two letters"),
            PlayError::NotAWord(w) => write!(f, "{w:?} is not in the lexicon"),
            PlayError::WrongScore { claimed, actual } => {
                write!(f, "play claims {claimed} points but scores {actual}")
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for PlayError {}

pub fn validate(
    board: &Board,
    config: &GameConfig,
    lexicon: &Lexicon,
    play: &Play,
    rack: Option<&Rack>,
) -> Result<i32, PlayError> {
    let placements: Vec<(Coord, Square)> = play.placements().collect();
    if placements.is_empty() {
        return Err(PlayError::NothingPlaced);
    }
    for &(c, _) in &placements {
        if !board.contains(c.row, c.col) {
            return Err(PlayError::OffBoard(c));
        }
        if board.get(c.row, c.col).is_occupied() {
            return Err(PlayError::Occupied(c));
        }
    }
    if let Some(rack) = rack {
        if !rack.contains_all(&play.tiles()) {
            return Err(PlayError::RackMissingTiles);
        }
    }

    let first = placements[0].0;
    let same_row = placements.iter().all(|&(c, _)| c.row == first.row);
    let same_col = placements.iter().all(|&(c, _)| c.col == first.col);
    if !same_row && !same_col {
        return Err(PlayError::NotInLine);
    }

    let dir = if placements.len() == 1 {
        let c = placements[0].0;
        let horizontal_run = run_length(board, Direction::Horizontal, c, &placements);
        if horizontal_run >= 2 {
            Direction::Horizontal
        } else {
            Direction::Vertical
        }
    } else if same_row {
        Direction::Horizontal
    } else {
        Direction::Vertical
    };

    let mut probe = board.clone();
    for &(c, s) in &placements {
        probe.set(c.row, c.col, s);
    }

    let (lane, _) = Board::to_lane(dir, placements[0].0);
    let offsets: Vec<usize> = placements
        .iter()
        .map(|&(c, _)| Board::to_lane(dir, c).1)
        .collect();
    let (lo, hi) = (
        *offsets.iter().min().expect("at least one placement"),
        *offsets.iter().max().expect("at least one placement"),
    );
    let cells = probe.lane(dir, lane);
    if cells[lo..=hi].iter().any(|c| c.is_empty()) {
        return Err(PlayError::NotContiguous);
    }

    if board.is_empty() {
        if config.layout.start_required() {
            let (sr, sc) = config.layout.start();
            if !placements.iter().any(|&(c, _)| (c.row, c.col) == (sr, sc)) {
                return Err(PlayError::MissesStart);
            }
        }
    } else {
        let touches = placements
            .iter()
            .any(|&(c, _)| board.has_neighbor(c.row, c.col));
        if !touches {
            return Err(PlayError::Disconnected);
        }
    }

    let alphabet = &config.alphabet;
    let value = |s: Square| -> i32 {
        if s.is_blank() {
            0
        } else {
            alphabet.score(s.index_unchecked())
        }
    };
    let placed_at = |c: Coord| placements.iter().any(|&(p, _)| p == c);

    let mut total = 0;

    let start = probe.run_start(dir, lane, lo);
    let end = probe.run_end(dir, lane, hi);
    if end - start >= 2 {
        total += score_run(
            &probe, config, dir, lane, start, end, &placed_at, &value, lexicon,
        )?;
    } else if placements.len() > 1 {
        return Err(PlayError::WordTooShort);
    }

    let cross_dir = dir.flip();
    let mut formed_any = end - start >= 2;
    for &(c, _) in &placements {
        let (cl, co) = Board::to_lane(cross_dir, c);
        let s = probe.run_start(cross_dir, cl, co);
        let e = probe.run_end(cross_dir, cl, co);
        if e - s < 2 {
            continue;
        }
        formed_any = true;
        total += score_run(
            &probe, config, cross_dir, cl, s, e, &placed_at, &value, lexicon,
        )?;
    }
    if !formed_any {
        return Err(PlayError::WordTooShort);
    }

    if placements.len() >= config.rack_size {
        total += config.bingo_bonus;
    }
    Ok(total)
}

#[allow(clippy::too_many_arguments)]
fn score_run(
    board: &Board,
    config: &GameConfig,
    dir: Direction,
    lane: usize,
    start: usize,
    end: usize,
    placed_at: &dyn Fn(Coord) -> bool,
    value: &dyn Fn(Square) -> i32,
    lexicon: &Lexicon,
) -> Result<i32, PlayError> {
    let cells = board.lane(dir, lane);
    let mut word = Vec::with_capacity(end - start);
    let mut sum = 0;
    let mut mult = 1;

    #[allow(clippy::needless_range_loop)]
    for offset in start..end {
        let square = cells[offset];
        word.push(square.index_unchecked());
        let coord = Board::from_lane(dir, lane, offset);
        if placed_at(coord) {
            let premium = config.layout.premium(coord.row, coord.col);
            sum += value(square) * premium.letter_multiplier();
            mult *= premium.word_multiplier();
        } else {
            sum += value(square);
        }
    }

    if !lexicon.contains(&word) {
        return Err(PlayError::NotAWord(word));
    }
    Ok(sum * mult)
}

fn run_length(
    board: &Board,
    dir: Direction,
    coord: Coord,
    placements: &[(Coord, Square)],
) -> usize {
    let (lane, offset) = Board::to_lane(dir, coord);
    let cells = board.lane(dir, lane);
    let occupied = |o: usize| {
        cells[o].is_occupied()
            || placements
                .iter()
                .any(|&(c, _)| Board::to_lane(dir, c) == (lane, o))
    };
    let mut lo = offset;
    while lo > 0 && occupied(lo - 1) {
        lo -= 1;
    }
    let mut hi = offset + 1;
    while hi < cells.len() && occupied(hi) {
        hi += 1;
    }
    hi - lo
}
