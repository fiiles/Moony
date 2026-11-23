//! Deterministic pseudo-random generator (LCG), the same one `bin/seed_demo.rs` uses.
//! No wall clock and no OS randomness: the same seed always yields the same data.

/// Seed every generator run starts from.
pub const SEED: u64 = 0x4D4F_4F4E_5921;

/// Linear congruential generator.
pub struct Rng(u64);

impl Default for Rng {
    fn default() -> Self {
        Self::new()
    }
}

impl Rng {
    /// Generator seeded with the module constant.
    pub fn new() -> Self {
        Self(SEED)
    }

    /// Generator with an explicit seed (for independent streams).
    pub fn with_seed(seed: u64) -> Self {
        Self(seed)
    }

    /// Uniform in [0, 1).
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        // the top 31 bits, scaled to [0, 1) (the original seed_demo divides by u32::MAX and so
        // only reaches 0.5)
        ((self.0 >> 33) as f64) / ((1u64 << 31) as f64)
    }

    /// Uniform in [lo, hi).
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }

    /// True with probability `p` (0..=1).
    pub fn chance(&mut self, p: f64) -> bool {
        self.next() < p
    }

    /// Uniform integer in `lo..=hi` (swapped bounds are tolerated).
    pub fn int_range(&mut self, lo: i64, hi: i64) -> i64 {
        let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        let span = (hi - lo + 1) as f64;
        (lo + (self.next() * span).floor() as i64).min(hi)
    }

    /// A random element, `None` for an empty slice.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            return None;
        }
        let i = self.int_range(0, items.len() as i64 - 1) as usize;
        items.get(i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let (mut a, mut b) = (Rng::new(), Rng::new());
        for _ in 0..100 {
            assert_eq!(a.next().to_bits(), b.next().to_bits());
        }
    }

    #[test]
    fn helpers_stay_in_bounds() {
        let mut r = Rng::new();
        for _ in 0..2000 {
            let x = r.range(2.0, 5.0);
            assert!((2.0..5.0).contains(&x));
            let i = r.int_range(-3, 4);
            assert!((-3..=4).contains(&i));
        }
        assert!(r.pick::<u8>(&[]).is_none());
        assert_eq!(r.pick(&[7]), Some(&7));
        assert!(!r.chance(0.0));
    }

    #[test]
    fn int_range_reaches_both_ends() {
        let mut r = Rng::new();
        let v: Vec<i64> = (0..500).map(|_| r.int_range(1, 3)).collect();
        assert!(v.contains(&1) && v.contains(&3));
    }
}
