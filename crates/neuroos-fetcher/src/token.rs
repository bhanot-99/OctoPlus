//! Approval token verification for C7 fetcher (Architecture §8.4, FR-FET-02, security.md §6).
//!
//! Evaluates incoming [`neuroos_proto::v1::ApprovalToken`] against `fetch.key` using
//! constant-time HMAC-SHA256 verification, single-use nonce tracking, and expiration enforcement.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

/// Length of a secret key in bytes (256 bits).
pub const KEY_LEN: usize = 32;

/// Domain separation string matching C6 approval tokens (Architecture §8.4).
pub const DOMAIN: &[u8] = b"neuroos-approval-v1\0";

/// Maximum token lifetime from time of verification (120 seconds).
pub const MAX_TOKEN_LIFETIME_NS: u64 = 120 * 1_000_000_000;

/// Bound on the number of tracked nonces (prevents unbounded memory growth).
pub const MAX_OUTSTANDING_NONCES: usize = 1024;

type HmacSha256 = Hmac<Sha256>;

/// Outcome verdict bound into the token (Architecture §8.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Approved action.
    Allow,
    /// Denied action.
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

/// In-memory secret key, zeroized on drop.
pub struct SecretKey(Box<[u8; KEY_LEN]>);

impl SecretKey {
    /// Creates a key from a 32-byte array.
    #[must_use]
    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self(Box::new(bytes))
    }

    /// Borrows the raw key bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }
}

impl Drop for SecretKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretKey(..)")
    }
}

/// Errors when loading `fetch.key` from disk.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KeyError {
    /// Could not open or read the key file.
    #[error("could not read key file: {0}")]
    Io(String),
    /// File size is not exactly 32 bytes.
    #[error("key file must be exactly 32 bytes, got {0}")]
    WrongSize(usize),
    /// Permissions are too permissive: the group may only read and others nothing.
    #[error("key file must not be group-writable or readable by others, got mode {0:o}")]
    BadPermissions(u32),
}

/// Loads a secret key from `path`. The installers make `/etc/neuroos/fetch.key`
/// `0640 root:neuroos` so this service's group can read it; anything wider (group write,
/// any access for others) is refused (audit A-08: the old exact-0600 check refused the key
/// every installer writes).
///
/// # Errors
/// Returns [`KeyError`] if the file cannot be read, has invalid permissions, or is not 32 bytes.
pub fn load_key(path: &Path) -> Result<SecretKey, KeyError> {
    let mut file = File::open(path).map_err(|e| KeyError::Io(e.to_string()))?;
    let meta = file.metadata().map_err(|e| KeyError::Io(e.to_string()))?;
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o027 != 0 {
        return Err(KeyError::BadPermissions(mode));
    }
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)
        .map_err(|e| KeyError::Io(e.to_string()))?;
    if buf.len() != KEY_LEN {
        return Err(KeyError::WrongSize(buf.len()));
    }
    let mut arr = [0u8; KEY_LEN];
    arr.copy_from_slice(&buf);
    buf.zeroize();
    Ok(SecretKey::from_bytes(arr))
}

/// Everything an approval token binds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    /// UUIDv7 of the action (16 bytes).
    pub action_id: [u8; 16],
    /// 128-bit single-use nonce (16 bytes).
    pub nonce: [u8; 16],
    /// Capability name ("web.fetch" or "web.search").
    pub capability: String,
    /// SHA-256 hash of canonical arguments.
    pub args_hash: [u8; 32],
    /// Decision verdict.
    pub verdict: Verdict,
    /// Expiration timestamp in UTC nanoseconds.
    pub expires_at_ns: u64,
}

/// Computes SHA-256 of canonical argument JSON.
#[must_use]
pub fn args_hash(canonical_args: &str) -> [u8; 32] {
    Sha256::digest(canonical_args.as_bytes()).into()
}

/// Assembles the canonical serialized message for HMAC calculation (Architecture §8.4).
fn serialize_message(a: &Approval) -> Vec<u8> {
    let cap = a.capability.as_bytes();
    let mut m = Vec::with_capacity(DOMAIN.len() + 16 + 16 + 4 + cap.len() + 32 + 1 + 8);
    m.extend_from_slice(DOMAIN);
    m.extend_from_slice(&a.action_id);
    m.extend_from_slice(&a.nonce);
    m.extend_from_slice(&u32::try_from(cap.len()).unwrap_or(u32::MAX).to_le_bytes());
    m.extend_from_slice(cap);
    m.extend_from_slice(&a.args_hash);
    m.push(a.verdict.byte());
    m.extend_from_slice(&a.expires_at_ns.to_le_bytes());
    m
}

/// Reasons why token verification fails (fails closed, R0-2, NFR-SEC-08).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TokenError {
    /// Token MAC is invalid, forged, or parameters mismatch.
    #[error("approval token does not verify")]
    BadMac,
    /// Token has expired.
    #[error("approval expired")]
    Expired,
    /// Expiry timestamp is further in the future than maximum permitted lifetime.
    #[error("approval expiry out of range")]
    ExpiryOutOfRange,
    /// Approval verdict is not Allow.
    #[error("approval is not an allow")]
    NotAllowed,
    /// Nonce has already been consumed (replay attack prevention).
    #[error("approval nonce already consumed")]
    Replayed,
    /// Too many outstanding nonces recorded.
    #[error("nonce tracking capacity exceeded")]
    Full,
    /// Cryptographic key is invalid.
    #[error("approval key unusable")]
    Key,
    /// Nonce or action_id length is invalid.
    #[error("invalid token structure")]
    Malformed,
}

/// Tracks consumed single-use nonces to enforce single execution per approval.
#[derive(Debug, Default)]
pub struct NonceTracker {
    consumed: HashMap<[u8; 16], u64>,
}

impl NonceTracker {
    /// Creates an empty tracker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Prunes expired nonces older than `now_ns`.
    pub fn prune(&mut self, now_ns: u64) {
        self.consumed.retain(|_, exp| *exp >= now_ns);
    }

    /// Verifies `token_mac` against `approval` using `key`, ensuring constant-time check,
    /// validity window, and single-use nonce consumption.
    ///
    /// # Errors
    /// Returns [`TokenError`] if the token is invalid, expired, or replayed.
    pub fn verify(
        &mut self,
        key: &SecretKey,
        approval: &Approval,
        token_mac: &[u8],
        now_ns: u64,
    ) -> Result<(), TokenError> {
        self.prune(now_ns);

        if approval.verdict != Verdict::Allow {
            return Err(TokenError::NotAllowed);
        }

        if approval.expires_at_ns > now_ns.saturating_add(MAX_TOKEN_LIFETIME_NS) {
            return Err(TokenError::ExpiryOutOfRange);
        }

        if now_ns > approval.expires_at_ns {
            return Err(TokenError::Expired);
        }

        if self.consumed.contains_key(&approval.nonce) {
            return Err(TokenError::Replayed);
        }

        let mut mac =
            <HmacSha256 as KeyInit>::new_from_slice(key.bytes()).map_err(|_| TokenError::Key)?;
        mac.update(&serialize_message(approval));
        mac.verify_slice(token_mac)
            .map_err(|_| TokenError::BadMac)?;

        if self.consumed.len() >= MAX_OUTSTANDING_NONCES {
            return Err(TokenError::Full);
        }

        self.consumed.insert(approval.nonce, approval.expires_at_ns);
        Ok(())
    }
}

/// Issues a valid token (helper for tests and C6 issuance).
#[must_use]
pub fn issue_token(key: &SecretKey, approval: &Approval) -> [u8; 32] {
    let Ok(mut mac) = <HmacSha256 as KeyInit>::new_from_slice(key.bytes()) else {
        return [0u8; 32];
    };
    mac.update(&serialize_message(approval));
    mac.finalize().into_bytes().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    const NOW: u64 = 1_000_000_000_000;
    const SEC: u64 = 1_000_000_000;

    fn sample_approval() -> Approval {
        Approval {
            action_id: [1; 16],
            nonce: [2; 16],
            capability: "web.fetch".to_string(),
            args_hash: args_hash(r#"{"url":"https://example.com"}"#),
            verdict: Verdict::Allow,
            expires_at_ns: NOW + 60 * SEC,
        }
    }

    #[test]
    fn valid_token_verifies_and_cannot_replay() {
        let key = SecretKey::from_bytes([42; KEY_LEN]);
        let app = sample_approval();
        let token = issue_token(&key, &app);

        let mut tracker = NonceTracker::new();
        assert_eq!(tracker.verify(&key, &app, &token, NOW), Ok(()));
        assert_eq!(
            tracker.verify(&key, &app, &token, NOW + SEC),
            Err(TokenError::Replayed)
        );
    }

    #[test]
    fn expired_token_is_rejected() {
        let key = SecretKey::from_bytes([42; KEY_LEN]);
        let app = sample_approval();
        let token = issue_token(&key, &app);

        let mut tracker = NonceTracker::new();
        assert_eq!(
            tracker.verify(&key, &app, &token, app.expires_at_ns + 1),
            Err(TokenError::Expired)
        );
    }

    #[test]
    fn wrong_capability_or_args_fails_mac() {
        let key = SecretKey::from_bytes([42; KEY_LEN]);
        let app = sample_approval();
        let token = issue_token(&key, &app);

        let mut wrong_cap = app.clone();
        wrong_cap.capability = "web.search".to_string();

        let mut tracker = NonceTracker::new();
        assert_eq!(
            tracker.verify(&key, &wrong_cap, &token, NOW),
            Err(TokenError::BadMac)
        );

        let mut wrong_args = app.clone();
        wrong_args.args_hash = args_hash(r#"{"url":"https://attacker.com"}"#);
        assert_eq!(
            tracker.verify(&key, &wrong_args, &token, NOW),
            Err(TokenError::BadMac)
        );
    }

    #[test]
    fn bit_flipped_token_fails() {
        let key = SecretKey::from_bytes([42; KEY_LEN]);
        let app = sample_approval();
        let mut token = issue_token(&key, &app);
        token[0] ^= 0x01;

        let mut tracker = NonceTracker::new();
        assert_eq!(
            tracker.verify(&key, &app, &token, NOW),
            Err(TokenError::BadMac)
        );
    }

    #[test]
    fn deny_verdict_fails() {
        let key = SecretKey::from_bytes([42; KEY_LEN]);
        let mut app = sample_approval();
        app.verdict = Verdict::Deny;
        let token = issue_token(&key, &app);

        let mut tracker = NonceTracker::new();
        assert_eq!(
            tracker.verify(&key, &app, &token, NOW),
            Err(TokenError::NotAllowed)
        );
    }

    #[test]
    fn load_key_enforces_permissions_and_size() {
        let file = NamedTempFile::new().unwrap();
        std::fs::write(file.path(), [7u8; 32]).unwrap();
        std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o644)).unwrap();

        assert!(matches!(
            load_key(file.path()),
            Err(KeyError::BadPermissions(0o644))
        ));

        for bad in [0o604, 0o660, 0o620, 0o601] {
            std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(bad)).unwrap();
            assert!(
                matches!(load_key(file.path()), Err(KeyError::BadPermissions(_))),
                "{bad:o}"
            );
        }
        for good in [0o600, 0o640, 0o400, 0o440] {
            std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(good)).unwrap();
            let loaded = load_key(file.path()).unwrap();
            assert_eq!(loaded.bytes(), &[7u8; 32], "{good:o}");
        }
    }
}
