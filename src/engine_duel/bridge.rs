//! Átalakítás a mi világunk (`Board`, `Tile`, `Placed`) és a motor világa között.
//!
//! A mi zsetonindexünk: 0 = joker, 1..=38 = betűk; a motoré: betűk 0..=37, a joker külön kód. A táblán a joker a
//! választott betűvel áll (`is_blank`), a motornál `Square::blank_letter`.

use crate::board::{BOARD_SIZE, Board, Placed};
use crate::tiles::{TILE_DISTRIBUTION, Tile};
use pg_scrabble::board::{Board as EBoard, Direction};
use pg_scrabble::movegen::Play;
use pg_scrabble::prelude::*;

pub fn to_engine_tile(tile: Tile) -> pg_scrabble::tile::Tile {
    if tile.is_blank() { pg_scrabble::tile::Tile::BLANK } else { pg_scrabble::tile::Tile::letter(tile.0 - 1) }
}

pub fn from_engine_tile(tile: pg_scrabble::tile::Tile) -> Tile {
    match tile.index() {
        Some(letter) => Tile(letter + 1),
        None => Tile::BLANK,
    }
}

pub fn rack_to_engine(hand: &[Tile]) -> Rack {
    Rack::from_tiles(hand.iter().map(|t| to_engine_tile(*t)))
}

pub fn rack_from_engine(rack: &Rack) -> Vec<Tile> {
    let mut tiles: Vec<Tile> = rack.tiles().into_iter().map(from_engine_tile).collect();
    tiles.sort();
    tiles
}

pub fn board_to_engine(board: &Board, config: &GameConfig) -> EBoard {
    let mut out = EBoard::new(&config.layout);
    for r in 0..BOARD_SIZE {
        for c in 0..BOARD_SIZE {
            if let Some(cell) = board.cells[r][c] {
                let letter = cell.letter.0 - 1;
                out.set(r, c, if cell.is_blank { Square::blank_letter(letter) } else { Square::letter(letter) });
            }
        }
    }
    out
}

pub fn board_from_engine(board: &EBoard) -> Board {
    let mut out = Board::new();
    for r in 0..BOARD_SIZE {
        for c in 0..BOARD_SIZE {
            let square = board.get(r, c);
            if square.is_occupied() {
                out.set(r, c, Tile(square.index_unchecked() + 1), square.is_blank());
                out.is_empty = false;
            }
        }
    }
    out
}

/// A motor lépése a mi lerakás-alakunkban. Az üres táblán a függőleges nyitólépést vízszintesre tükrözi (a tábla az
/// átlóra szimmetrikus, a pontszám azonos): így a lépés nem függ attól, melyik irányú ikerét találta meg előbb a motor.
pub fn placed_from_play(play: &Play, board_is_empty: bool) -> Vec<Placed> {
    let transpose = board_is_empty && play.direction() == Direction::Vertical;
    let mut placed: Vec<Placed> = play
        .placements()
        .map(|(c, sq)| {
            let (row, col) = if transpose { (c.col, c.row) } else { (c.row, c.col) };
            Placed::new(row as i32, col as i32, Tile(sq.index_unchecked() + 1), sq.is_blank())
        })
        .collect();
    placed.sort_by_key(|p| (p.row, p.col));
    placed
}

/// Kanonikus kulcs (lerakás összehasonlításhoz): rendezett `(sor, oszlop, betű, joker)`.
pub type Key = Vec<(i32, i32, u8, bool)>;

pub fn key_of(placed: &[Placed]) -> Key {
    let mut key: Key = placed.iter().map(|p| (p.row, p.col, p.letter.0, p.is_blank)).collect();
    key.sort();
    key
}

/// A még nem látott zsetonok darabszáma zsetonindexenként (0 = joker): a teljes készlet − a táblán lévők − a saját kéz.
/// Ez a zsák és az ellenfél keze együtt; az ellenfél tényleges kezét sosem használja.
pub fn honest_unseen(board: &Board, own_hand: &[Tile]) -> [u8; 39] {
    let mut counts = [0u8; 39];
    for (i, (_, _, n)) in TILE_DISTRIBUTION.iter().enumerate() {
        counts[i] = *n;
    }
    for r in 0..BOARD_SIZE {
        for c in 0..BOARD_SIZE {
            if let Some(cell) = board.cells[r][c] {
                let i = if cell.is_blank { 0 } else { cell.letter.0 as usize };
                counts[i] = counts[i].saturating_sub(1);
            }
        }
    }
    for tile in own_hand {
        counts[tile.0 as usize] = counts[tile.0 as usize].saturating_sub(1);
    }
    counts
}

/// A motor `Unseen` készletének átfordítása a mi indexelésünkre (ellenőrzéshez).
pub fn unseen_counts_from_engine(unseen: &pg_scrabble::eval::Unseen) -> [u8; 39] {
    let mut out = [0u8; 39];
    for (i, slot) in out.iter_mut().enumerate().skip(1) {
        *slot = unseen.counts()[i - 1];
    }
    out[0] = unseen.counts()[pg_scrabble::tile::BLANK_CODE as usize];
    out
}
