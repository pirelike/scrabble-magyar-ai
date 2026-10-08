#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Rng {
    state: [u64; 4],
}

impl Rng {
    pub fn seed_from_u64(seed: u64) -> Rng {
        let mut z = seed;
        let mut next = || {
            z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut x = z;
            x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            x ^ (x >> 31)
        };
        Rng {
            state: [next(), next(), next(), next()],
        }
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let s = &mut self.state;
        let result = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    #[inline]
    pub fn below(&mut self, bound: u64) -> u64 {
        assert!(bound > 0, "bound must be positive");
        let mut m = (self.next_u64() as u128).wrapping_mul(bound as u128);
        let mut low = m as u64;
        if low < bound {
            let threshold = bound.wrapping_neg() % bound;
            while low < threshold {
                m = (self.next_u64() as u128).wrapping_mul(bound as u128);
                low = m as u64;
            }
        }
        (m >> 64) as u64
    }

    #[inline]
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    pub fn shuffle<T>(&mut self, slice: &mut [T]) {
        for i in (1..slice.len()).rev() {
            let j = self.below(i as u64 + 1) as usize;
            slice.swap(i, j);
        }
    }
}

impl Default for Rng {
    fn default() -> Self {
        Rng::seed_from_u64(0x5EED_5EED_5EED_5EED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn same_seed_gives_same_stream() {
        let mut a = Rng::seed_from_u64(42);
        let mut b = Rng::seed_from_u64(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = Rng::seed_from_u64(1);
        let mut b = Rng::seed_from_u64(2);
        let diff = (0..64).filter(|_| a.next_u64() != b.next_u64()).count();
        assert!(
            diff > 60,
            "sequential seeds must not produce similar streams"
        );
    }

    #[test]
    fn below_stays_in_range() {
        let mut r = Rng::seed_from_u64(7);
        for bound in 1..=32u64 {
            for _ in 0..200 {
                assert!(r.below(bound) < bound);
            }
        }
    }

    #[test]
    fn below_is_reasonably_uniform() {
        let mut r = Rng::seed_from_u64(99);
        let mut buckets = [0u32; 10];
        for _ in 0..100_000 {
            buckets[r.below(10) as usize] += 1;
        }

        for (i, &c) in buckets.iter().enumerate() {
            assert!(
                (8_500..=11_500).contains(&c),
                "bucket {i} had {c} samples, expected around 10000"
            );
        }
    }

    #[test]
    fn next_f64_is_in_unit_interval() {
        let mut r = Rng::seed_from_u64(3);
        for _ in 0..10_000 {
            let x = r.next_f64();
            assert!((0.0..1.0).contains(&x));
        }
    }

    #[test]
    fn shuffle_is_a_permutation() {
        let mut r = Rng::seed_from_u64(11);
        let mut v: Vec<u32> = (0..50).collect();
        r.shuffle(&mut v);
        let mut sorted = v.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..50).collect::<Vec<_>>());
        assert_ne!(
            v, sorted,
            "a 50-element shuffle should not return sorted order"
        );
    }
}
