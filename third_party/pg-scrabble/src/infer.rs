use crate::eval::Unseen;
use crate::rack::Rack;
use crate::rng::Rng;
use crate::tile::{Tile, TILE_CODES};
use alloc::vec::Vec;

pub trait RackFilter {
    fn accepts(&self, rack: &Rack) -> bool;
}

impl<F: Fn(&Rack) -> bool> RackFilter for F {
    fn accepts(&self, rack: &Rack) -> bool {
        self(rack)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AnyRack;

impl RackFilter for AnyRack {
    fn accepts(&self, _rack: &Rack) -> bool {
        true
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WithoutTiles {
    excluded: [u8; TILE_CODES],
}

impl WithoutTiles {
    pub fn new() -> WithoutTiles {
        WithoutTiles {
            excluded: [u8::MAX; TILE_CODES],
        }
    }

    pub fn at_most(mut self, tile: Tile, max: u8) -> WithoutTiles {
        self.excluded[tile.code() as usize] = max;
        self
    }

    pub fn without(self, tile: Tile) -> WithoutTiles {
        self.at_most(tile, 0)
    }
}

impl Default for WithoutTiles {
    fn default() -> Self {
        WithoutTiles::new()
    }
}

impl RackFilter for WithoutTiles {
    fn accepts(&self, rack: &Rack) -> bool {
        rack.counts()
            .iter()
            .zip(self.excluded.iter())
            .all(|(&held, &max)| held <= max)
    }
}

#[derive(Clone, Debug)]
pub struct Inference {
    pool: Vec<Tile>,
    pub max_attempts: usize,
}

impl Inference {
    pub fn uniform(unseen: &Unseen) -> Inference {
        let mut pool = Vec::with_capacity(unseen.len());
        for (code, &n) in unseen.counts().iter().enumerate() {
            for _ in 0..n {
                pool.push(Tile::from_code(code as u8));
            }
        }
        Inference {
            pool,
            max_attempts: 64,
        }
    }

    #[inline]
    pub fn pool_size(&self) -> usize {
        self.pool.len()
    }

    pub fn sample(&self, rng: &mut Rng, size: usize) -> Rack {
        let take = size.min(self.pool.len());
        let mut rack = Rack::new();

        let mut indices: Vec<usize> = (0..self.pool.len()).collect();
        for i in 0..take {
            let j = i + rng.below((indices.len() - i) as u64) as usize;
            indices.swap(i, j);
            rack.add(self.pool[indices[i]]);
        }
        rack
    }

    pub fn sample_filtered(
        &self,
        rng: &mut Rng,
        size: usize,
        filter: &impl RackFilter,
    ) -> Option<Rack> {
        for _ in 0..self.max_attempts {
            let rack = self.sample(rng, size);
            if filter.accepts(&rack) {
                return Some(rack);
            }
        }
        None
    }

    pub fn sample_split(&self, rng: &mut Rng, size: usize) -> (Rack, Vec<Tile>) {
        let mut shuffled = self.pool.clone();
        rng.shuffle(&mut shuffled);
        let take = size.min(shuffled.len());
        let rest = shuffled.split_off(take);
        (Rack::from_tiles(shuffled), rest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::rules::{Alphabet, GameConfig};

    fn setup() -> (GameConfig, Board) {
        let config = GameConfig::standard();
        let board = Board::new(&config.layout);
        (config, board)
    }

    #[test]
    fn sampling_never_invents_tiles() {
        let (config, board) = setup();
        let own = Rack::parse(&Alphabet::english(), "AEINRST").unwrap();
        let unseen = Unseen::new(&config, &board, &own);
        let inference = Inference::uniform(&unseen);
        let mut rng = Rng::seed_from_u64(1);

        for _ in 0..200 {
            let rack = inference.sample(&mut rng, 7);
            assert_eq!(rack.len(), 7);
            for (code, &n) in rack.counts().iter().enumerate() {
                assert!(
                    n <= unseen.counts()[code],
                    "drew {n} of tile {code} but only {} are unseen",
                    unseen.counts()[code]
                );
            }
        }
    }

    #[test]
    fn sampling_from_a_small_pool_takes_what_is_there() {
        let (config, board) = setup();
        let unseen = Unseen::new(&config, &board, &Rack::new());
        let mut inference = Inference::uniform(&unseen);
        inference.pool.truncate(3);
        let mut rng = Rng::seed_from_u64(2);
        assert_eq!(inference.sample(&mut rng, 7).len(), 3);
    }

    #[test]
    fn samples_vary_and_cover_the_pool() {
        let (config, board) = setup();
        let unseen = Unseen::new(&config, &board, &Rack::new());
        let inference = Inference::uniform(&unseen);
        let mut rng = Rng::seed_from_u64(3);

        let mut seen_blank = false;
        let mut distinct = alloc::collections::BTreeSet::new();
        for _ in 0..500 {
            let rack = inference.sample(&mut rng, 7);
            distinct.insert(rack.key());
            if rack.has_blank() {
                seen_blank = true;
            }
        }
        assert!(distinct.len() > 400, "samples should not repeat much");
        assert!(
            seen_blank,
            "500 draws of 7 from 100 tiles should turn up a blank"
        );
    }

    #[test]
    fn a_filter_excludes_what_it_says_it_does() {
        let (config, board) = setup();
        let unseen = Unseen::new(&config, &board, &Rack::new());
        let inference = Inference::uniform(&unseen);
        let mut rng = Rng::seed_from_u64(4);
        let filter = WithoutTiles::new().without(Tile::BLANK);

        for _ in 0..100 {
            if let Some(rack) = inference.sample_filtered(&mut rng, 7, &filter) {
                assert!(!rack.has_blank(), "the filter rules blanks out");
            }
        }
    }

    #[test]
    fn an_impossible_filter_gives_up_rather_than_looping() {
        let (config, board) = setup();
        let unseen = Unseen::new(&config, &board, &Rack::new());
        let inference = Inference::uniform(&unseen);
        let mut rng = Rng::seed_from_u64(5);
        let impossible = |_: &Rack| false;
        assert_eq!(inference.sample_filtered(&mut rng, 7, &impossible), None);
    }

    #[test]
    fn a_split_partitions_the_pool_exactly() {
        let (config, board) = setup();
        let own = Rack::parse(&Alphabet::english(), "AEINRST").unwrap();
        let unseen = Unseen::new(&config, &board, &own);
        let inference = Inference::uniform(&unseen);
        let mut rng = Rng::seed_from_u64(6);

        let (rack, rest) = inference.sample_split(&mut rng, 7);
        assert_eq!(rack.len() + rest.len(), unseen.len());

        let mut counts = [0u8; TILE_CODES];
        for (code, n) in rack.counts().iter().enumerate() {
            counts[code] += n;
        }
        for t in &rest {
            counts[t.code() as usize] += 1;
        }
        assert_eq!(&counts, unseen.counts(), "the split must conserve tiles");
    }

    #[test]
    fn a_closure_works_as_a_filter() {
        let (config, board) = setup();
        let unseen = Unseen::new(&config, &board, &Rack::new());
        let inference = Inference::uniform(&unseen);
        let mut rng = Rng::seed_from_u64(7);
        let no_q = |r: &Rack| !r.contains(Tile::letter(16));
        for _ in 0..50 {
            if let Some(rack) = inference.sample_filtered(&mut rng, 7, &no_q) {
                assert!(!rack.contains(Tile::letter(16)));
            }
        }
    }
}
