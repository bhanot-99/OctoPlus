//! The keyed anonymiser for recordings (FR-MON-12, rules §8.7, ADR-0011 decision 7).
//!
//! Personal text becomes `t_<12 hex>` = HMAC-SHA256(per-run key, text). The key comes from
//! the OS RNG, lives only in memory, is zeroed when the anonymiser is dropped, and is never
//! written anywhere — so a placeholder cannot be reversed or recomputed by anyone, including
//! the owner. The same text maps to the same placeholder within one run (so the noise gate
//! can still see "the same window title again") and to a different one in the next run.

use hmac::{Hmac, KeyInit, Mac};
use neuroos_proto::v1::MonitorEvent;
use neuroos_proto::v1::monitor_event::Event;
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::exclusion::Exclusion;

type HmacSha256 = Hmac<Sha256>;

/// App ids and process names a recording keeps readable: widely shipped software, not
/// personal. Noise-gate work needs app classes (Phase 4). More come from
/// `[monitor] record_keep_app_ids`.
pub const PUBLIC_APPS: &[&str] = &[
    "firefox*",
    "org.mozilla.*",
    "google-chrome*",
    "chromium*",
    "brave*",
    "code",
    "code-oss",
    "com.visualstudio.code*",
    "alacritty",
    "kitty",
    "gnome-terminal*",
    "org.gnome.*",
    "com.system76.*",
    "org.kde.*",
    "org.videolan.*",
    "vlc",
    "spotify",
    "thunderbird",
    "slack",
    "discord",
    "obsidian",
    "org.libreoffice.*",
    "libreoffice*",
    "steam",
    "mpv",
];

/// Path components that are structure, not personal text.
const PUBLIC_PATH_PARTS: &[&str] = &[
    "boot", "efi", "home", "mnt", "media", "var", "usr", "opt", "run", ".git", "HEAD", "index",
    "refs", "heads", "tags", "logs", "objects",
];

/// Why an anonymiser could not be created.
#[derive(Debug, thiserror::Error)]
pub enum AnonymiseError {
    /// The OS random generator failed; there is no safe fallback.
    #[error("cannot draw a random anonymiser key")]
    Rng,
}

/// Replaces personal text with keyed placeholders.
pub struct Anonymiser {
    key: Zeroizing<[u8; 32]>,
    keep: Exclusion,
}

impl std::fmt::Debug for Anonymiser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the key.
        f.debug_struct("Anonymiser").finish_non_exhaustive()
    }
}

impl Anonymiser {
    /// A fresh anonymiser with a random per-run key. `extra_keep` extends [`PUBLIC_APPS`].
    ///
    /// # Errors
    /// [`AnonymiseError::Rng`] if the OS cannot supply random bytes.
    pub fn new(extra_keep: &[String]) -> Result<Self, AnonymiseError> {
        let mut key = Zeroizing::new([0u8; 32]);
        getrandom::fill(key.as_mut_slice()).map_err(|_| AnonymiseError::Rng)?;
        Ok(Self::with_key(key, extra_keep))
    }

    /// An anonymiser with a given key (tests only need this; production uses [`Anonymiser::new`]).
    #[must_use]
    pub fn with_key(key: Zeroizing<[u8; 32]>, extra_keep: &[String]) -> Self {
        let mut patterns: Vec<&str> = PUBLIC_APPS.to_vec();
        patterns.extend(extra_keep.iter().map(String::as_str));
        Self {
            key,
            keep: Exclusion::new(&patterns, &[]),
        }
    }

    /// `t_<12 hex>` for `text`; empty text stays empty (it carries nothing).
    #[must_use]
    pub fn placeholder(&self, text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }
        // A 32-byte key is always a valid HMAC key; an error here cannot happen, and if it
        // somehow did, the safe answer is a constant — never the original text.
        let Ok(mut mac) = <HmacSha256 as KeyInit>::new_from_slice(self.key.as_slice()) else {
            return "t_error".to_owned();
        };
        mac.update(text.as_bytes());
        let tag = mac.finalize().into_bytes();
        let mut out = String::with_capacity(14);
        out.push_str("t_");
        for b in tag.iter().take(6) {
            out.push_str(&format!("{b:02x}"));
        }
        out
    }

    /// App ids and process names: kept when public, hashed otherwise.
    fn app(&self, name: &str) -> String {
        if name.is_empty() || self.keep.matches_app(name) {
            name.to_owned()
        } else {
            self.placeholder(name)
        }
    }

    /// Paths keep their shape and public structure; every other component is hashed,
    /// keeping a short alphanumeric extension (`.md`, `.ics`) which carries the file kind.
    fn path(&self, path: &str) -> String {
        let mut out = String::with_capacity(path.len());
        for (i, part) in path.split('/').enumerate() {
            if i > 0 {
                out.push('/');
            }
            if part.is_empty() || PUBLIC_PATH_PARTS.contains(&part) {
                out.push_str(part);
                continue;
            }
            match part.rsplit_once('.') {
                Some((stem, ext))
                    if !stem.is_empty()
                        && (1..=5).contains(&ext.len())
                        && ext.bytes().all(|b| b.is_ascii_alphanumeric()) =>
                {
                    out.push_str(&self.placeholder(stem));
                    out.push('.');
                    out.push_str(&ext.to_ascii_lowercase());
                }
                _ => out.push_str(&self.placeholder(part)),
            }
        }
        out
    }

    /// The anonymised copy of one event. Timestamps and numbers are not personal text.
    #[must_use]
    pub fn anonymise(&self, mut event: MonitorEvent) -> MonitorEvent {
        match event.event.as_mut() {
            Some(Event::Focus(f)) => {
                f.app_id = self.app(&f.app_id);
                f.title = self.placeholder(&f.title);
            }
            Some(Event::Media(m)) => {
                m.player = self.app(&m.player);
                m.title = self.placeholder(&m.title);
                m.artist = self.placeholder(&m.artist);
            }
            Some(Event::Resources(r)) => r.top_process = self.app(&r.top_process),
            Some(Event::Disk(d)) => d.mount_point = self.path(&d.mount_point),
            Some(Event::ProcessTree(t)) => {
                for n in &mut t.nodes {
                    n.name = self.app(&n.name);
                }
            }
            Some(Event::File(f)) => f.path = self.path(&f.path),
            // Idle, lock, battery and temperature carry no text of the user's.
            Some(Event::Idle(_) | Event::Lock(_) | Event::Battery(_) | Event::Thermal(_))
            | None => {}
        }
        event
    }
}
