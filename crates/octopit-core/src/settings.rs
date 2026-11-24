//! Dashboard settings that survive a restart (ADR-0017).
//!
//! Stored in `$XDG_CONFIG_HOME/octoplus/octopit.json` (or `~/.config/octoplus/octopit.json`).
//! Atomic save with secure permissions (`0600` for the file, `0700` for the directory).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The user's theme choice.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    /// Follow the desktop system appearance.
    #[default]
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
}

/// Clamps a text scale percentage to the nearest allowed value (`100`, `115`, or `130`).
#[must_use]
pub fn clamp_text_scale(val: u8) -> u8 {
    if val <= 107 {
        100
    } else if val <= 122 {
        115
    } else {
        130
    }
}

fn deserialize_text_scale<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let val: serde_json::Value = serde::Deserialize::deserialize(deserializer)?;
    match val {
        serde_json::Value::Number(n) => {
            if let Some(v) = n.as_u64() {
                #[allow(clippy::cast_possible_truncation)]
                let clamped = if v <= 107 {
                    100
                } else if v <= 122 {
                    115
                } else {
                    130
                };
                Ok(clamped)
            } else if let Some(v) = n.as_i64() {
                let clamped = if v <= 107 {
                    100
                } else if v <= 122 {
                    115
                } else {
                    130
                };
                Ok(clamped)
            } else {
                Ok(100)
            }
        }
        _ => Ok(100),
    }
}

/// Settings that survive an application restart.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Color theme.
    #[serde(default)]
    pub theme: ThemeChoice,
    /// Whether reduced motion is enabled.
    #[serde(default)]
    pub reduce_motion: bool,
    /// Text scale percentage (`100`, `115`, or `130`).
    #[serde(
        default = "default_text_scale",
        deserialize_with = "deserialize_text_scale"
    )]
    pub text_scale: u8,
    /// Whether the first-run welcome dialog was completed.
    #[serde(default)]
    pub welcome_done: bool,
}

const fn default_text_scale() -> u8 {
    100
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::System,
            reduce_motion: false,
            text_scale: 100,
            welcome_done: false,
        }
    }
}

impl Settings {
    /// Loads settings from `path`.
    ///
    /// Returns defaults if the file is missing, unreadable, larger than 8 KiB,
    /// or contains invalid JSON. Clamps `text_scale` to the allowed set.
    /// Never errors and never panics.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        let Ok(meta) = std::fs::metadata(path) else {
            return Self::default();
        };
        if !meta.is_file() || meta.len() > 8 * 1024 {
            return Self::default();
        }
        let Ok(bytes) = std::fs::read(path) else {
            return Self::default();
        };
        let Ok(mut settings) = serde_json::from_slice::<Self>(&bytes) else {
            return Self::default();
        };
        settings.text_scale = clamp_text_scale(settings.text_scale);
        settings
    }

    /// Saves settings atomically to `path`.
    ///
    /// Writes to a temporary file (`.tmp`) in the same directory with mode `0600` and renames it.
    /// Creates the parent directory with mode `0700` if missing.
    ///
    /// # Errors
    /// Returns an error if directory creation, serialization, writing, or renaming fails.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent().filter(|p| !p.exists()) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                let mut builder = std::fs::DirBuilder::new();
                builder.recursive(true);
                builder.mode(0o700);
                builder.create(parent)?;
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
            }
            #[cfg(not(unix))]
            {
                std::fs::create_dir_all(parent)?;
            }
        }

        let tmp_path = path.with_extension("tmp");
        {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&tmp_path)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600));
            }
            let mut clamped = self.clone();
            clamped.text_scale = clamp_text_scale(clamped.text_scale);
            let json = serde_json::to_vec_pretty(&clamped)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            use std::io::Write;
            file.write_all(&json)?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }
}

/// Returns `$XDG_CONFIG_HOME/octoplus/octopit.json` or `$HOME/.config/octoplus/octopit.json`.
///
/// Rejects relative paths and returns `None` if no valid absolute path can be constructed.
#[must_use]
pub fn default_path(config_home: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    octoplus_common::paths::config_dir(config_home, home)
        .ok()
        .map(|d| d.join("octopit.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use tempfile::tempdir;

    #[test]
    fn test_defaults() {
        let s = Settings::default();
        assert_eq!(s.theme, ThemeChoice::System);
        assert!(!s.reduce_motion);
        assert_eq!(s.text_scale, 100);
        assert!(!s.welcome_done);
    }

    #[test]
    fn test_round_trip() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("sub").join("octopit.json");
        let original = Settings {
            theme: ThemeChoice::Dark,
            reduce_motion: true,
            text_scale: 115,
            welcome_done: true,
        };
        original.save(&path).expect("save");
        let loaded = Settings::load(&path);
        assert_eq!(loaded, original);
    }

    #[test]
    fn test_missing_file() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("nonexistent.json");
        assert_eq!(Settings::load(&path), Settings::default());
    }

    #[test]
    fn test_garbage_file() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("garbage.json");
        std::fs::write(&path, b"not valid json {{{").expect("write");
        assert_eq!(Settings::load(&path), Settings::default());
    }

    #[test]
    fn test_huge_file() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("huge.json");
        let data = vec![b' '; 9 * 1024];
        std::fs::write(&path, &data).expect("write");
        assert_eq!(Settings::load(&path), Settings::default());
    }

    #[test]
    fn test_unknown_field_ignored() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("extra.json");
        let json = r#"{
            "theme": "light",
            "reduce_motion": false,
            "text_scale": 130,
            "welcome_done": true,
            "future_feature_flag": "enabled"
        }"#;
        std::fs::write(&path, json).expect("write");
        let loaded = Settings::load(&path);
        assert_eq!(
            loaded,
            Settings {
                theme: ThemeChoice::Light,
                reduce_motion: false,
                text_scale: 130,
                welcome_done: true,
            }
        );
    }

    #[test]
    fn test_text_scale_clamping() {
        assert_eq!(clamp_text_scale(0), 100);
        assert_eq!(clamp_text_scale(100), 100);
        assert_eq!(clamp_text_scale(107), 100);
        assert_eq!(clamp_text_scale(108), 115);
        assert_eq!(clamp_text_scale(115), 115);
        assert_eq!(clamp_text_scale(122), 115);
        assert_eq!(clamp_text_scale(123), 130);
        assert_eq!(clamp_text_scale(130), 130);
        assert_eq!(clamp_text_scale(255), 130);

        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("scales.json");

        // 999 clamps to 130
        std::fs::write(&path, r#"{"text_scale": 999}"#).expect("write");
        let loaded_999 = Settings::load(&path);
        assert_eq!(loaded_999.text_scale, 130);

        // 0 clamps to 100
        std::fs::write(&path, r#"{"text_scale": 0}"#).expect("write");
        let loaded_0 = Settings::load(&path);
        assert_eq!(loaded_0.text_scale, 100);
    }

    #[test]
    fn test_atomic_save_leaves_no_tmp_and_modes() {
        let dir = tempdir().expect("tempdir");
        let sub = dir.path().join("config_dir");
        let path = sub.join("octopit.json");
        let tmp = sub.join("octopit.tmp");

        let s = Settings::default();
        s.save(&path).expect("save");

        assert!(path.exists());
        assert!(!tmp.exists(), "Temporary file must not remain");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file_mode = std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(file_mode, 0o600, "File mode must be 0600");

            let folder_mode = std::fs::metadata(&sub)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(folder_mode, 0o700, "Folder mode must be 0700");
        }
    }

    #[test]
    fn test_default_path_precedence_and_relative_rejection() {
        let cfg = OsString::from("/custom/config");
        let home = OsString::from("/home/user");
        let rel = OsString::from("relative/path");

        // Precedence: config_home over home
        assert_eq!(
            default_path(Some(&cfg), Some(&home)),
            Some(PathBuf::from("/custom/config/octoplus/octopit.json"))
        );

        // Fallback to home when config_home is None
        assert_eq!(
            default_path(None, Some(&home)),
            Some(PathBuf::from("/home/user/.config/octoplus/octopit.json"))
        );

        // Relative config_home falls back to home if home is absolute
        assert_eq!(
            default_path(Some(&rel), Some(&home)),
            Some(PathBuf::from("/home/user/.config/octoplus/octopit.json"))
        );

        // Rejection when relative and no valid absolute fallback
        assert_eq!(default_path(Some(&rel), None), None);
        assert_eq!(default_path(None, Some(&rel)), None);
        assert_eq!(default_path(Some(&rel), Some(&rel)), None);
        assert_eq!(default_path(None, None), None);
    }
}
