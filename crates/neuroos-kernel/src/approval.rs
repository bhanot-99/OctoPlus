//! Approval tokens (FR-KER-04, Architecture §8.4, security.md §4.5).
//!
//! `token = HMAC-SHA256(key, action_id ‖ nonce ‖ capability ‖ args_hash ‖ decision ‖ expiry)`
//!
//! The message is length-prefixed and domain-separated, so no two different
//! approvals encode to the same bytes. The executor rebuilds the [`Approval`]
//! from what it is **about to run** (capability, canonical arguments) plus the
//! pending record's id, nonce and expiry, then calls [`NonceSet::verify`]:
//! a token issued for other arguments or another capability does not verify.
//!
//! Security-critical (NFR-QA-01): every outcome of `verify` has a test.

use std::collections::HashMap;

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

use crate::keys::SecretKey;

/// Longest life of a token (Architecture §8.4).
pub const MAX_TOKEN_LIFETIME_NS: u64 = 120 * 1_000_000_000;
/// Most outstanding nonces; beyond this, new approvals are refused (fail closed, rules.md §9.7).
pub const MAX_OUTSTANDING_NONCES: usize = 1024;
/// Domain separation: a token for any other purpose can never verify here.
const DOMAIN: &[u8] = b"neuroos-approval-v1\0";

type HmacSha256 = Hmac<Sha256>;

/// What the user decided, as bound into the token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Approved once.
    Allow,
    /// Refused. A deny token never authorises execution.
    Deny,
}

impl Verdict {
    const fn byte(self) -> u8 {
        match self {
            Self::Allow => 1,
            Self::Deny => 2,
        }
    }
}

/// Everything a token binds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    /// UUIDv7 of the action.
    pub action_id: [u8; 16],
    /// 128-bit single-use nonce.
    pub nonce: [u8; 16],
    /// Capability that will run.
    pub capability: String,
    /// SHA-256 of the canonical final arguments.
    pub args_hash: [u8; 32],
    /// The user's decision.
    pub verdict: Verdict,
    /// Expiry, UTC epoch ns.
    pub expires_at_ns: u64,
}

/// A 32-byte MAC.
#[derive(Clone, PartialEq, Eq)]
pub struct Token(pub [u8; 32]);

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print token bytes (NFR-SEC-06 spirit: no secrets in logs).
        f.write_str("Token(..)")
    }
}

/// Why a token was refused. Every variant means "do not run".
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TokenError {
    /// MAC mismatch: forged, bit-flipped, wrong key, wrong arguments or wrong capability.
    #[error("approval token does not verify")]
    BadMac,
    /// Past its expiry.
    #[error("approval expired")]
    Expired,
    /// Expiry further away than any token may live.
    #[error("approval expiry out of range")]
    ExpiryOutOfRange,
    /// The decision bound in the token is not Allow.
    #[error("approval is not an allow")]
    NotAllowed,
    /// Nonce unknown to this kernel or already used.
    #[error("approval nonce unknown or already used")]
    Replayed,
    /// Too many outstanding approvals.
    #[error("too many outstanding approvals")]
    Full,
    /// The key could not be used.
    #[error("approval key unusable")]
    Key,
}

/// SHA-256 of the canonical argument text (Architecture §8.4 step 3).
#[must_use]
pub fn args_hash(canonical_args: &str) -> [u8; 32] {
    Sha256::digest(canonical_args.as_bytes()).into()
}

fn message(a: &Approval) -> Vec<u8> {
    let cap = a.capability.as_bytes();
    let mut m = Vec::with_capacity(DOMAIN.len() + 16 + 16 + 4 + cap.len() + 32 + 1 + 8);
    m.extend_from_slice(DOMAIN);
    m.extend_from_slice(&a.action_id);
    m.extend_from_slice(&a.nonce);
    // Length prefix keeps the capability unambiguous whatever follows it.
    m.extend_from_slice(&u32::try_from(cap.len()).unwrap_or(u32::MAX).to_le_bytes());
    m.extend_from_slice(cap);
    m.extend_from_slice(&a.args_hash);
    m.push(a.verdict.byte());
    m.extend_from_slice(&a.expires_at_ns.to_le_bytes());
    m
}

fn mac(key: &SecretKey) -> Result<HmacSha256, TokenError> {
    <HmacSha256 as KeyInit>::new_from_slice(key.bytes()).map_err(|_| TokenError::Key)
}

/// Issues the token for `approval`.
///
/// # Errors
/// [`TokenError::Key`] if the key cannot be used.
pub fn issue(key: &SecretKey, approval: &Approval) -> Result<Token, TokenError> {
    let mut m = mac(key)?;
    m.update(&message(approval));
    Ok(Token(m.finalize().into_bytes().into()))
}

/// Outstanding single-use nonces with their expiry. Bounded.
#[derive(Debug, Default)]
pub struct NonceSet {
    live: HashMap<[u8; 16], u64>,
}

impl NonceSet {
    /// Empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn prune(&mut self, now_ns: u64) {
        self.live.retain(|_, exp| *exp >= now_ns);
    }

    /// Registers a fresh nonce for an approval that is about to be shown.
    ///
    /// # Errors
    /// [`TokenError::Full`] when too many approvals are outstanding;
    /// [`TokenError::Replayed`] if the nonce is already registered.
    pub fn register(
        &mut self,
        nonce: [u8; 16],
        expires_at_ns: u64,
        now_ns: u64,
    ) -> Result<(), TokenError> {
        self.prune(now_ns);
        if self.live.len() >= MAX_OUTSTANDING_NONCES {
            return Err(TokenError::Full);
        }
        if self.live.insert(nonce, expires_at_ns).is_some() {
            return Err(TokenError::Replayed);
        }
        Ok(())
    }

    /// Number of outstanding nonces.
    #[must_use]
    pub fn len(&self) -> usize {
        self.live.len()
    }

    /// True when nothing is outstanding.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// Verifies `token` for `approval` at `now_ns` and consumes the nonce.
    /// The MAC is checked first, in constant time (`Mac::verify_slice`),
    /// so a forged token learns nothing from which check failed.
    ///
    /// # Errors
    /// Any [`TokenError`]; the caller must not run the action.
    pub fn verify(
        &mut self,
        key: &SecretKey,
        approval: &Approval,
        token: &Token,
        now_ns: u64,
    ) -> Result<(), TokenError> {
        let mut m = mac(key)?;
        m.update(&message(approval));
        m.verify_slice(&token.0).map_err(|_| TokenError::BadMac)?;
        if approval.verdict != Verdict::Allow {
            return Err(TokenError::NotAllowed);
        }
        if approval.expires_at_ns > now_ns.saturating_add(MAX_TOKEN_LIFETIME_NS) {
            return Err(TokenError::ExpiryOutOfRange);
        }
        if now_ns > approval.expires_at_ns {
            self.live.remove(&approval.nonce);
            return Err(TokenError::Expired);
        }
        // Single use: remove returns None if it was never issued or already used.
        if self.live.remove(&approval.nonce).is_none() {
            return Err(TokenError::Replayed);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const NOW: u64 = 1_800_000_000_000_000_000;
    const SEC: u64 = 1_000_000_000;

    fn key(b: u8) -> SecretKey {
        SecretKey::from_bytes([b; 32])
    }

    fn approval() -> Approval {
        Approval {
            action_id: [1; 16],
            nonce: [2; 16],
            capability: "system.cleanup".into(),
            args_hash: args_hash(r#"{"paths":["/home/u/.cache/x"]}"#),
            verdict: Verdict::Allow,
            expires_at_ns: NOW + 120 * SEC,
        }
    }

    fn fresh(a: &Approval) -> NonceSet {
        let mut n = NonceSet::new();
        n.register(a.nonce, a.expires_at_ns, NOW).unwrap();
        n
    }

    #[test]
    fn valid_token_verifies_once() {
        let a = approval();
        let t = issue(&key(7), &a).unwrap();
        let mut n = fresh(&a);
        assert_eq!(n.verify(&key(7), &a, &t, NOW + SEC), Ok(()));
        // Replay of the same token.
        assert_eq!(
            n.verify(&key(7), &a, &t, NOW + 2 * SEC),
            Err(TokenError::Replayed)
        );
    }

    #[test]
    fn expired_token_is_refused_and_nonce_dropped() {
        let a = approval();
        let t = issue(&key(7), &a).unwrap();
        let mut n = fresh(&a);
        assert_eq!(
            n.verify(&key(7), &a, &t, a.expires_at_ns + 1),
            Err(TokenError::Expired)
        );
        assert!(n.is_empty());
        // Exactly at expiry is still valid (inclusive bound).
        let mut n = fresh(&a);
        assert_eq!(n.verify(&key(7), &a, &t, a.expires_at_ns), Ok(()));
    }

    #[test]
    fn wrong_arguments_or_capability_fail() {
        let a = approval();
        let t = issue(&key(7), &a).unwrap();
        let mut other_args = a.clone();
        other_args.args_hash = args_hash(r#"{"paths":["/etc"]}"#);
        assert_eq!(
            fresh(&a).verify(&key(7), &other_args, &t, NOW),
            Err(TokenError::BadMac)
        );
        let mut other_cap = a.clone();
        other_cap.capability = "system.disk_report".into();
        assert_eq!(
            fresh(&a).verify(&key(7), &other_cap, &t, NOW),
            Err(TokenError::BadMac)
        );
    }

    #[test]
    fn wrong_key_fails() {
        let a = approval();
        let t = issue(&key(7), &a).unwrap();
        assert_eq!(
            fresh(&a).verify(&key(8), &a, &t, NOW),
            Err(TokenError::BadMac)
        );
    }

    #[test]
    fn every_token_bit_flip_fails() {
        let a = approval();
        let t = issue(&key(7), &a).unwrap();
        for byte in 0..32 {
            for bit in 0..8 {
                let mut bad = t.clone();
                bad.0[byte] ^= 1 << bit;
                assert_eq!(
                    fresh(&a).verify(&key(7), &a, &bad, NOW),
                    Err(TokenError::BadMac)
                );
            }
        }
    }

    #[test]
    fn every_field_change_fails() {
        let a = approval();
        let t = issue(&key(7), &a).unwrap();
        let mut changes: Vec<Approval> = Vec::new();
        let mut c = a.clone();
        c.action_id[0] ^= 1;
        changes.push(c);
        let mut c = a.clone();
        c.nonce[15] ^= 1;
        changes.push(c);
        let mut c = a.clone();
        c.args_hash[31] ^= 1;
        changes.push(c);
        let mut c = a.clone();
        c.expires_at_ns += 1;
        changes.push(c);
        let mut c = a.clone();
        c.verdict = Verdict::Deny;
        changes.push(c);
        for c in changes {
            let mut n = NonceSet::new();
            n.register(c.nonce, c.expires_at_ns, NOW).unwrap();
            assert_eq!(n.verify(&key(7), &c, &t, NOW), Err(TokenError::BadMac));
        }
    }

    #[test]
    fn deny_token_never_executes() {
        let mut a = approval();
        a.verdict = Verdict::Deny;
        let t = issue(&key(7), &a).unwrap();
        assert_eq!(
            fresh(&a).verify(&key(7), &a, &t, NOW),
            Err(TokenError::NotAllowed)
        );
    }

    #[test]
    fn unregistered_nonce_is_replay() {
        let a = approval();
        let t = issue(&key(7), &a).unwrap();
        assert_eq!(
            NonceSet::new().verify(&key(7), &a, &t, NOW),
            Err(TokenError::Replayed)
        );
    }

    #[test]
    fn expiry_too_far_is_refused() {
        let mut a = approval();
        a.expires_at_ns = NOW + MAX_TOKEN_LIFETIME_NS + 1;
        let t = issue(&key(7), &a).unwrap();
        assert_eq!(
            fresh(&a).verify(&key(7), &a, &t, NOW),
            Err(TokenError::ExpiryOutOfRange)
        );
    }

    #[test]
    fn nonce_set_is_bounded_and_prunes() {
        let mut n = NonceSet::new();
        for i in 0..MAX_OUTSTANDING_NONCES {
            let mut nonce = [0u8; 16];
            nonce[..8].copy_from_slice(&(i as u64).to_le_bytes());
            n.register(nonce, NOW + SEC, NOW).unwrap();
        }
        assert_eq!(
            n.register([0xff; 16], NOW + SEC, NOW),
            Err(TokenError::Full)
        );
        // After they expire, room returns.
        assert_eq!(n.register([0xff; 16], NOW + 3 * SEC, NOW + 2 * SEC), Ok(()));
        assert_eq!(n.len(), 1);
        // Same nonce twice is refused.
        assert_eq!(
            n.register([0xff; 16], NOW + 3 * SEC, NOW + 2 * SEC),
            Err(TokenError::Replayed)
        );
    }

    #[test]
    fn capability_encoding_is_unambiguous() {
        // "a" + hash starting with 'b' must not equal "ab" + shifted hash.
        let mut x = approval();
        x.capability = "a".into();
        let mut y = approval();
        y.capability = "ab".into();
        assert_ne!(message(&x), message(&y));
        assert_ne!(issue(&key(1), &x).unwrap(), issue(&key(1), &y).unwrap());
    }

    #[test]
    fn debug_does_not_leak() {
        let t = issue(&key(7), &approval()).unwrap();
        assert_eq!(format!("{t:?}"), "Token(..)");
    }

    proptest! {
        #[test]
        fn random_tokens_never_verify(bytes in proptest::array::uniform32(any::<u8>())) {
            let a = approval();
            let real = issue(&key(7), &a).unwrap();
            prop_assume!(bytes != real.0);
            prop_assert_eq!(fresh(&a).verify(&key(7), &a, &Token(bytes), NOW), Err(TokenError::BadMac));
        }
    }
}
