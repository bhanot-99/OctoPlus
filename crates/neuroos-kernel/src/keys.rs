//! `hmac.key` handling (security.md §6, NFR-SEC-06, ADR-0010 decision 6).
//!
//! The installer creates the key (32 random bytes, mode 0600). C6 only loads
//! it, and refuses to start if anything about the file is off: missing, a
//! symlink, not a regular file, not owned by us, readable by group or others,
//! or the wrong size. The bytes live in one boxed buffer that is locked in RAM
//! and zeroised on drop; the key never appears in `Debug` output or logs.

use std::io::Read;
use std::path::Path;

use rustix::fs::{FileType, Mode, OFlags};
use zeroize::Zeroize;

/// Key length in bytes.
pub const KEY_LEN: usize = 32;

/// A secret key in locked, zeroised memory.
pub struct SecretKey(Box<[u8; KEY_LEN]>);

impl SecretKey {
    /// Wraps raw bytes (tests and key generation). Production keys come from [`load`].
    #[must_use]
    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self(Box::new(bytes))
    }

    /// The key bytes, for the MAC only.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.0[..]
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

/// Why the key could not be loaded. Every variant is fatal at start-up.
#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    /// Missing, a symlink, or unreadable.
    #[error("key file cannot be opened safely: {0}")]
    Open(rustix::io::Errno),
    /// Not a regular file.
    #[error("key file is not a regular file")]
    NotRegular,
    /// Owned by someone else.
    #[error("key file is not owned by this user")]
    Owner,
    /// Group or others have any access.
    #[error("key file permissions too open (must be 0600 or stricter)")]
    Mode,
    /// Wrong length.
    #[error("key file has the wrong size")]
    Size,
    /// Read failed.
    #[error("key file read failed")]
    Read,
    /// The key could not be locked in memory.
    #[error("key cannot be locked in memory")]
    Lock,
}

/// Loads and checks the key at `path`.
///
/// # Errors
/// Any [`KeyError`]; C6 must exit non-zero (fail closed, rules.md §6.5).
pub fn load(path: &Path) -> Result<SecretKey, KeyError> {
    // O_NOFOLLOW: a symlink planted at the key path is refused, not followed.
    let fd = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(KeyError::Open)?;
    let st = rustix::fs::fstat(&fd).map_err(KeyError::Open)?;
    if FileType::from_raw_mode(st.st_mode) != FileType::RegularFile {
        return Err(KeyError::NotRegular);
    }
    if st.st_uid != rustix::process::geteuid().as_raw() {
        return Err(KeyError::Owner);
    }
    if st.st_mode & 0o077 != 0 {
        return Err(KeyError::Mode);
    }
    if usize::try_from(st.st_size).ok() != Some(KEY_LEN) {
        return Err(KeyError::Size);
    }
    let mut key = SecretKey(Box::new([0u8; KEY_LEN]));
    neuroos_sandbox::lock_memory(&mut key.0[..]).map_err(|_| KeyError::Lock)?;
    let mut file = std::fs::File::from(fd);
    file.read_exact(&mut key.0[..]).map_err(|_| KeyError::Read)?;
    // A file that grew between fstat and read is refused too.
    let mut extra = [0u8; 1];
    if file.read(&mut extra).map_err(|_| KeyError::Read)? != 0 {
        return Err(KeyError::Size);
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn write_key(dir: &Path, bytes: &[u8], mode: u32) -> std::path::PathBuf {
        let p = dir.join("hmac.key");
        std::fs::write(&p, bytes).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        p
    }

    #[test]
    fn loads_a_good_key() {
        let d = tempfile::tempdir().unwrap();
        let p = write_key(d.path(), &[9u8; KEY_LEN], 0o600);
        let k = load(&p).unwrap();
        assert_eq!(k.bytes(), &[9u8; KEY_LEN]);
        let p = write_key(d.path(), &[9u8; KEY_LEN], 0o400);
        assert!(load(&p).is_ok());
    }

    #[test]
    fn refuses_bad_files() {
        let d = tempfile::tempdir().unwrap();
        assert!(matches!(load(&d.path().join("missing")), Err(KeyError::Open(_))));
        for mode in [0o640, 0o604, 0o660, 0o666] {
            let p = write_key(d.path(), &[1u8; KEY_LEN], mode);
            assert!(matches!(load(&p), Err(KeyError::Mode)), "{mode:o}");
        }
        let p = write_key(d.path(), &[1u8; KEY_LEN - 1], 0o600);
        assert!(matches!(load(&p), Err(KeyError::Size)));
        let p = write_key(d.path(), &[1u8; KEY_LEN + 1], 0o600);
        assert!(matches!(load(&p), Err(KeyError::Size)));
        let real = write_key(d.path(), &[1u8; KEY_LEN], 0o600);
        let link = d.path().join("link.key");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(matches!(load(&link), Err(KeyError::Open(_))));
        assert!(matches!(load(d.path()), Err(KeyError::NotRegular)));
    }

    #[test]
    fn debug_hides_bytes() {
        assert_eq!(format!("{:?}", SecretKey::from_bytes([7; KEY_LEN])), "SecretKey(..)");
    }
}
