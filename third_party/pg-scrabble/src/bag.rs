use crate::rng::Rng;
use crate::rules::TileDistribution;
use crate::tile::{Tile, TILE_CODES};
use alloc::vec::Vec;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Bag {
    tiles: Vec<Tile>,
    counts: [u8; TILE_CODES],
    rng: Rng,
}

impl Bag {
    pub fn new(distribution: &TileDistribution, seed: u64) -> Bag {
        let mut tiles = Vec::with_capacity(distribution.total());
        let counts = *distribution.counts();
        for (code, &n) in counts.iter().enumerate() {
            for _ in 0..n {
                tiles.push(Tile::from_code(code as u8));
            }
        }
        let mut bag = Bag {
            tiles,
            counts,
            rng: Rng::seed_from_u64(seed),
        };
        bag.shuffle();
        bag
    }

    pub fn from_tiles(tiles: Vec<Tile>, seed: u64) -> Bag {
        let mut counts = [0u8; TILE_CODES];
        for t in &tiles {
            counts[t.code() as usize] += 1;
        }
        Bag {
            tiles,
            counts,
            rng: Rng::seed_from_u64(seed),
        }
    }

    pub fn empty(seed: u64) -> Bag {
        Bag {
            tiles: Vec::new(),
            counts: [0; TILE_CODES],
            rng: Rng::seed_from_u64(seed),
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    #[inline]
    pub fn counts(&self) -> &[u8; TILE_CODES] {
        &self.counts
    }

    #[inline]
    pub fn remaining(&self) -> &[Tile] {
        &self.tiles
    }

    pub fn shuffle(&mut self) {
        let mut rng = core::mem::replace(&mut self.rng, Rng::seed_from_u64(0));
        rng.shuffle(&mut self.tiles);
        self.rng = rng;
    }

    #[inline]
    pub fn draw(&mut self) -> Option<Tile> {
        let t = self.tiles.pop()?;
        self.counts[t.code() as usize] -= 1;
        Some(t)
    }

    pub fn draw_n(&mut self, n: usize) -> Vec<Tile> {
        let take = n.min(self.tiles.len());
        let mut out = Vec::with_capacity(take);
        for _ in 0..take {
            out.push(self.draw().expect("bag has at least `take` tiles"));
        }
        out
    }

    pub fn refill(&mut self, rack: &mut crate::rack::Rack, target: usize) -> Vec<Tile> {
        let mut drawn = Vec::new();
        while rack.len() < target {
            match self.draw() {
                Some(t) => {
                    rack.add(t);
                    drawn.push(t);
                }
                None => break,
            }
        }
        drawn
    }

    pub fn put_back(&mut self, tiles: impl IntoIterator<Item = Tile>) {
        for t in tiles {
            self.counts[t.code() as usize] += 1;
            self.tiles.push(t);
        }
        self.shuffle();
    }

    pub fn return_exact(&mut self, tiles: impl IntoIterator<Item = Tile>) {
        for t in tiles {
            self.counts[t.code() as usize] += 1;
            self.tiles.push(t);
        }
    }

    pub fn take_specific(&mut self, tile: Tile) -> bool {
        match self.tiles.iter().rposition(|&t| t == tile) {
            Some(i) => {
                self.tiles.swap_remove(i);
                self.counts[tile.code() as usize] -= 1;
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rack::Rack;

    #[test]
    fn a_fresh_english_bag_holds_one_hundred_tiles() {
        let bag = Bag::new(&TileDistribution::english(), 1);
        assert_eq!(bag.len(), 100);
        assert_eq!(bag.counts()[Tile::BLANK.code() as usize], 2);
        assert_eq!(bag.counts()[Tile::letter(4).code() as usize], 12);
    }

    #[test]
    fn drawing_decrements_both_the_list_and_the_counts() {
        let mut bag = Bag::new(&TileDistribution::english(), 1);
        let t = bag.draw().unwrap();
        assert_eq!(bag.len(), 99);
        let expected = TileDistribution::english().count(t) - 1;
        assert_eq!(bag.counts()[t.code() as usize], expected);
    }

    #[test]
    fn counts_always_agree_with_the_tile_list() {
        let mut bag = Bag::new(&TileDistribution::english(), 5);
        for _ in 0..40 {
            bag.draw();
        }
        bag.put_back([Tile::letter(0), Tile::BLANK]);
        let mut recount = [0u8; TILE_CODES];
        for t in bag.remaining() {
            recount[t.code() as usize] += 1;
        }
        assert_eq!(&recount, bag.counts());
    }

    #[test]
    fn draining_the_bag_is_safe() {
        let mut bag = Bag::new(&TileDistribution::english(), 2);
        let all = bag.draw_n(500);
        assert_eq!(all.len(), 100);
        assert!(bag.is_empty());
        assert_eq!(bag.draw(), None);
        assert!(bag.counts().iter().all(|&c| c == 0));
    }

    #[test]
    fn refill_stops_at_the_target_and_at_an_empty_bag() {
        let mut bag = Bag::from_tiles(alloc::vec![Tile::letter(0); 3], 0);
        let mut rack = Rack::new();
        assert_eq!(bag.refill(&mut rack, 7).len(), 3);
        assert_eq!(rack.len(), 3);
        assert!(bag.is_empty());

        let mut bag = Bag::new(&TileDistribution::english(), 3);
        let mut rack = Rack::new();
        assert_eq!(bag.refill(&mut rack, 7).len(), 7);
        assert_eq!(rack.len(), 7);
        assert!(bag.refill(&mut rack, 7).is_empty());
    }

    #[test]
    fn the_same_seed_deals_the_same_game() {
        let a = Bag::new(&TileDistribution::english(), 12345);
        let b = Bag::new(&TileDistribution::english(), 12345);
        assert_eq!(a.remaining(), b.remaining());
        let c = Bag::new(&TileDistribution::english(), 12346);
        assert_ne!(a.remaining(), c.remaining());
    }

    #[test]
    fn return_exact_inverts_a_draw() {
        let mut bag = Bag::new(&TileDistribution::english(), 21);
        let before = bag.remaining().to_vec();
        let drawn = bag.draw_n(7);
        bag.return_exact(drawn.into_iter().rev());
        assert_eq!(
            bag.remaining(),
            &before[..],
            "the bag's order must be restored"
        );
    }

    #[test]
    fn take_specific_finds_and_removes_one_copy() {
        let mut bag = Bag::new(&TileDistribution::english(), 9);
        assert!(bag.take_specific(Tile::letter(16)), "the bag has a Q");
        assert_eq!(bag.counts()[Tile::letter(16).code() as usize], 0);
        assert_eq!(bag.len(), 99);
        assert!(!bag.take_specific(Tile::letter(16)), "only one Q exists");
    }
}
