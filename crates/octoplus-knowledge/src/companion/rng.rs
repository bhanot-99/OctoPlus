//! Deterministic pseudo-random number generator for companion logic.
//!
//! Implements SplitMix64 for fast, high-quality 64-bit pseudo-random numbers
//! without external crate dependencies.

/// A deterministic SplitMix64 random number generator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Creates a new `Rng` initialized with the given 64-bit seed.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Advances the generator state and returns the next pseudo-random `u64`.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Returns a pseudo-random floating-point number in the half-open interval `[0.0, 1.0)`.
    #[must_use]
    pub fn next_f64(&mut self) -> f64 {
        // Use the top 53 bits for IEEE 754 double precision float.
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Returns `true` with probability `p`.
    ///
    /// If `p <= 0.0`, always returns `false`.
    /// If `p >= 1.0`, always returns `true`.
    #[must_use]
    pub fn chance(&mut self, p: f64) -> bool {
        if p <= 0.0 {
            return false;
        }
        if p >= 1.0 {
            return true;
        }
        self.next_f64() < p
    }

    /// Picks a pseudo-random index in `0..len`.
    ///
    /// If `len == 0`, returns `0`.
    #[must_use]
    pub fn pick(&mut self, len: usize) -> usize {
        if len == 0 {
            0
        } else {
            (self.next_u64() % (len as u64)) as usize
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determinism() {
        let mut rng1 = Rng::new(42);
        let mut rng2 = Rng::new(42);
        for _ in 0..1000 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
        }
    }

    #[test]
    fn test_f64_range() {
        let mut rng = Rng::new(12345);
        for _ in 0..10_000 {
            let val = rng.next_f64();
            assert!((0.0..1.0).contains(&val));
        }
    }

    #[test]
    fn test_chance_bounds() {
        let mut rng = Rng::new(999);
        for _ in 0..100 {
            assert!(!rng.chance(0.0));
            assert!(!rng.chance(-0.5));
            assert!(rng.chance(1.0));
            assert!(rng.chance(1.5));
        }
    }

    #[test]
    fn test_chance_distribution() {
        let mut rng = Rng::new(777);
        let draws = 100_000;
        let mut hits = 0;
        for _ in 0..draws {
            if rng.chance(0.25) {
                hits += 1;
            }
        }
        let rate = hits as f64 / draws as f64;
        assert!(
            (0.24..=0.26).contains(&rate),
            "expected rate in 0.24..0.26, got {rate}"
        );
    }

    #[test]
    fn test_pick_range() {
        let mut rng = Rng::new(888);
        assert_eq!(rng.pick(0), 0);
        for _ in 0..1000 {
            let idx = rng.pick(10);
            assert!(idx < 10);
        }
    }
}
