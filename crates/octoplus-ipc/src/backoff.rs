//! Jittered exponential backoff for reconnects (AB-10, NFR-REL-01):
//! 100 ms doubling to 10 s, each delay drawn uniformly from [d/2, d].

use std::time::Duration;

/// First retry delay (rules.md AB-10).
pub const INITIAL: Duration = Duration::from_millis(100);
/// Largest retry delay (rules.md AB-10).
pub const MAX: Duration = Duration::from_secs(10);

/// Backoff state. Pure: the caller supplies randomness, so tests are exact.
#[derive(Debug, Clone)]
pub struct Backoff {
    current: Duration,
    initial: Duration,
    max: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new(INITIAL, MAX)
    }
}

impl Backoff {
    /// Backoff from `initial` doubling up to `max`.
    #[must_use]
    pub fn new(initial: Duration, max: Duration) -> Self {
        let initial = initial.min(max);
        Self {
            current: initial,
            initial,
            max,
        }
    }

    /// The next delay, jittered with `rand`, then doubles the base.
    pub fn next_delay(&mut self, rand: u64) -> Duration {
        let base = self.current;
        let half = base / 2;
        let span = u64::try_from((base - half).as_nanos()).unwrap_or(u64::MAX);
        let jitter = if span == 0 { 0 } else { rand % (span + 1) };
        self.current = (self.current * 2).min(self.max);
        half + Duration::from_nanos(jitter)
    }

    /// Back to the initial delay after a success.
    pub fn reset(&mut self) {
        self.current = self.initial;
    }
}

/// A random `u64` for jitter; falls back to the clock if the OS RNG fails,
/// since jitter is not a security value.
#[must_use]
pub fn jitter_seed() -> u64 {
    getrandom::u64().unwrap_or_else(|_| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| u64::from(d.subsec_nanos()))
            .unwrap_or(0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn doubles_to_the_cap_then_resets() {
        let mut b = Backoff::default();
        let mut bases = Vec::new();
        for _ in 0..12 {
            // rand = u64::MAX % (span+1) is some value; use 0 to read the lower bound.
            bases.push(b.next_delay(0) * 2);
        }
        assert_eq!(bases[0], Duration::from_millis(100));
        assert_eq!(bases[1], Duration::from_millis(200));
        assert_eq!(*bases.last().unwrap(), MAX);
        b.reset();
        assert_eq!(b.next_delay(0), Duration::from_millis(50));
    }

    #[test]
    fn initial_above_max_is_clamped() {
        let mut b = Backoff::new(Duration::from_secs(20), Duration::from_secs(1));
        assert!(b.next_delay(u64::MAX) <= Duration::from_secs(1));
    }

    #[test]
    fn zero_durations_do_not_divide_by_zero() {
        let mut b = Backoff::new(Duration::ZERO, Duration::ZERO);
        assert_eq!(b.next_delay(12345), Duration::ZERO);
    }

    #[test]
    fn seed_is_available() {
        // Two draws are almost surely different; equal draws would hint at a stuck RNG.
        assert_ne!(jitter_seed(), jitter_seed());
    }

    proptest! {
        #[test]
        fn delay_within_half_to_full(steps in 0usize..20, rand in any::<u64>()) {
            let mut b = Backoff::default();
            for _ in 0..steps { b.next_delay(rand); }
            let base = b.current;
            let d = b.next_delay(rand);
            prop_assert!(d >= base / 2 && d <= base, "{d:?} vs base {base:?}");
            prop_assert!(d <= MAX);
        }
    }
}
