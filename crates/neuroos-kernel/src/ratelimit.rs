//! Review rate limiter with denial cooldown.
//!
//! Satisfies Architecture §8.3, rules.md AB-13 (pure functions taking time as u64 ns),
//! and rules.md §9.7 (bounded queues and caches).

use std::collections::{HashMap, VecDeque};

/// Rolling rate limit window duration in nanoseconds (60 seconds).
const WINDOW_NS: u64 = 60 * 1_000_000_000;

/// Maximum number of tracked denials in memory (rules.md §9.7).
const MAX_DENIALS: usize = 256;

/// Identifier for rate limiting and cooling period checks.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LimitKey {
    /// Capability name.
    pub capability: String,
    /// SHA-256 hash of canonical arguments.
    pub args_hash: [u8; 32],
}

impl LimitKey {
    /// Creates a new `LimitKey`.
    #[must_use]
    pub fn new(capability: impl Into<String>, args_hash: [u8; 32]) -> Self {
        Self {
            capability: capability.into(),
            args_hash,
        }
    }
}

/// Errors returned by the review rate limiter.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Limited {
    /// Denied action is still in cooling period.
    #[error("denial cooldown active")]
    Cooldown,
    /// Maximum review requests exceeded within the 60 s rolling window.
    #[error("review rate limit exceeded")]
    RateLimited,
}

/// Rate limiter and denial cooldown tracker for REVIEW and DANGEROUS capability requests.
#[derive(Debug)]
pub struct ReviewLimiter {
    review_per_minute: u32,
    denial_cooldown_ns: u64,
    /// Timestamps (in ns) of allowed reviews within rolling 60 s window.
    window: VecDeque<u64>,
    /// Map of key -> timestamp of denial.
    denials: HashMap<LimitKey, u64>,
    /// Insertion order for bounded eviction of denials: (key, timestamp).
    denial_order: VecDeque<(LimitKey, u64)>,
}

impl ReviewLimiter {
    /// Creates a new `ReviewLimiter`.
    #[must_use]
    pub fn new(review_per_minute: u32, denial_cooldown_s: u64) -> Self {
        Self {
            review_per_minute,
            denial_cooldown_ns: denial_cooldown_s.saturating_mul(1_000_000_000),
            window: VecDeque::new(),
            denials: HashMap::new(),
            denial_order: VecDeque::new(),
        }
    }

    /// Checks if a review request is allowed at `now_ns`.
    ///
    /// If allowed, records the request timestamp into the rolling window.
    ///
    /// # Errors
    /// - [`Limited::Cooldown`] if the key was denied and cooling period is active.
    /// - [`Limited::RateLimited`] if the rolling 60 s review limit is reached.
    pub fn check(&mut self, now_ns: u64, key: &LimitKey) -> Result<(), Limited> {
        // 1. Check denial cooling period
        if let Some(&denied_at_ns) = self.denials.get(key) {
            let elapsed = now_ns.saturating_sub(denied_at_ns);
            if elapsed < self.denial_cooldown_ns {
                return Err(Limited::Cooldown);
            }
            self.denials.remove(key);
        }

        // 2. Prune expired entries from the rolling 60 s window
        while let Some(&ts) = self.window.front() {
            if now_ns.saturating_sub(ts) >= WINDOW_NS {
                self.window.pop_front();
            } else {
                break;
            }
        }

        // 3. Check rate limit
        if self.window.len() >= self.review_per_minute as usize {
            return Err(Limited::RateLimited);
        }

        // 4. Allowed: record timestamp
        self.window.push_back(now_ns);
        Ok(())
    }

    /// Records a user denial at `now_ns` for a capability and arguments key.
    ///
    /// Enforces the 256-key memory bound by evicting the oldest denial.
    pub fn record_denial(&mut self, now_ns: u64, key: LimitKey) {
        if self.denials.contains_key(&key) {
            self.denials.insert(key.clone(), now_ns);
            self.denial_order.push_back((key, now_ns));
            return;
        }

        // Evict oldest until space is available
        while self.denials.len() >= MAX_DENIALS {
            if let Some((old_key, old_ts)) = self.denial_order.pop_front() {
                if self.denials.get(&old_key) == Some(&old_ts) {
                    self.denials.remove(&old_key);
                    break;
                }
            } else {
                break;
            }
        }

        self.denials.insert(key.clone(), now_ns);
        self.denial_order.push_back((key, now_ns));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_key(tag: u8) -> LimitKey {
        let mut hash = [0u8; 32];
        hash[0] = tag;
        LimitKey::new(format!("skill_{tag}"), hash)
    }

    #[test]
    fn window_limit_and_sliding() {
        let mut limiter = ReviewLimiter::new(3, 300);

        let k1 = dummy_key(1);
        let k2 = dummy_key(2);
        let k3 = dummy_key(3);
        let k4 = dummy_key(4);

        // First 3 requests at t=0 succeed
        assert_eq!(limiter.check(0, &k1), Ok(()));
        assert_eq!(limiter.check(0, &k2), Ok(()));
        assert_eq!(limiter.check(0, &k3), Ok(()));

        // 4th request at t=0 is rate limited
        assert_eq!(limiter.check(0, &k4), Err(Limited::RateLimited));

        // Advance time to 60 s + 1 ns: window slides and permits new requests
        let t_next = 60_000_000_001;
        assert_eq!(limiter.check(t_next, &k4), Ok(()));
    }

    #[test]
    fn cooldown_blocks_same_key_different_key_not_blocked() {
        let mut limiter = ReviewLimiter::new(6, 300);

        let k1 = dummy_key(1);
        let k2 = dummy_key(2);

        limiter.record_denial(1000, k1.clone());

        // Same key is blocked by cooldown
        assert_eq!(limiter.check(2000, &k1), Err(Limited::Cooldown));

        // Different key is not blocked
        assert_eq!(limiter.check(2000, &k2), Ok(()));
    }

    #[test]
    fn cooldown_expiry() {
        let mut limiter = ReviewLimiter::new(6, 300);
        let k1 = dummy_key(1);

        limiter.record_denial(0, k1.clone());

        // Before expiry: blocked (299 s)
        assert_eq!(
            limiter.check(299 * 1_000_000_000, &k1),
            Err(Limited::Cooldown)
        );

        // At/after expiry: allowed (300 s)
        assert_eq!(limiter.check(300 * 1_000_000_000, &k1), Ok(()));
    }

    #[test]
    fn eviction_bound_at_256() {
        let mut limiter = ReviewLimiter::new(1000, 300);

        // Fill up to 256 denials
        for i in 0..256 {
            let mut hash = [0u8; 32];
            hash[0] = (i & 0xff) as u8;
            hash[1] = ((i >> 8) & 0xff) as u8;
            limiter.record_denial(i as u64, LimitKey::new("test", hash));
        }

        assert_eq!(limiter.denials.len(), 256);

        // Key 0 should be in cooldown
        let k0 = {
            let mut hash = [0u8; 32];
            hash[0] = 0;
            LimitKey::new("test", hash)
        };
        assert_eq!(limiter.check(300, &k0), Err(Limited::Cooldown));

        // Add 257th denial (index 256)
        let k256 = {
            let mut hash = [0u8; 32];
            hash[0] = 0;
            hash[1] = 1;
            LimitKey::new("test", hash)
        };
        limiter.record_denial(256, k256.clone());

        // Capacity bound maintained
        assert!(limiter.denials.len() <= 256);

        // Oldest key (k0) should have been evicted
        assert_eq!(limiter.check(300, &k0), Ok(()));

        // Newest key is in cooldown
        assert_eq!(limiter.check(300, &k256), Err(Limited::Cooldown));
    }
}
