//! A small random number generator with a fixed seed (SplitMix64), so the
//! demo data is the same on every run and every machine: the same
//! customers, the same bikes, the same busy Saturday. Dates still move with
//! today, because they are counted back from `renox::db::now()`.

/// SplitMix64: fast, good enough for demo data, no dependency.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    /// A generator that always starts the same way for the same `seed`.
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A whole number in `low..high` (`low` when the range is empty).
    pub fn range(&mut self, low: i64, high: i64) -> i64 {
        if high <= low {
            return low;
        }
        low + (self.next_u64() % (high - low) as u64) as i64
    }

    /// `true` with probability `percent` / 100.
    pub fn chance(&mut self, percent: u64) -> bool {
        self.next_u64() % 100 < percent
    }

    /// One item of a non-empty slice.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.range(0, items.len() as i64) as usize]
    }

    /// An index chosen by weight: `[70, 20, 10]` gives 0 most often.
    pub fn weighted(&mut self, weights: &[u64]) -> usize {
        let total: u64 = weights.iter().sum();
        let mut roll = self.next_u64() % total.max(1);
        for (i, w) in weights.iter().enumerate() {
            if roll < *w {
                return i;
            }
            roll -= w;
        }
        weights.len() - 1
    }

    /// A price in whole dollars between `low` and `high` (cents): `4500`
    /// for $45.00, for rates and fees.
    pub fn price(&mut self, low: i64, high: i64) -> i64 {
        self.range(low / 100, high / 100 + 1) * 100
    }

    /// A shelf price between `low` and `high` (cents), ending in 9.99 as
    /// shops write them: $19.99, $1,249.99.
    pub fn retail(&mut self, low: i64, high: i64) -> i64 {
        (self.range(low / 1_000, high / 1_000 + 1) * 1_000).max(1_000) - 1
    }
}

#[cfg(test)]
mod tests {
    use super::Rng;

    #[test]
    fn the_same_seed_gives_the_same_numbers() {
        let (mut a, mut b) = (Rng::new(7), Rng::new(7));
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        let mut r = Rng::new(1);
        for _ in 0..1_000 {
            let n = r.range(3, 9);
            assert!((3..9).contains(&n));
        }
        assert_eq!(r.range(5, 5), 5);
    }
}
