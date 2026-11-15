//! The single `config.toml` (rules.md §5): one section per concern,
//! validated at start, and **unknown keys are fatal**.
//!
//! Defaults come from PRD §6.1 (deadlines) and Architecture §5.1 (frame size).
//! Later parts add their own sections here as they are built.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

/// Environment variable that overrides the config file location. Together with
/// `NEUROOS_LOG` it is the only environment input that changes behaviour.
pub const CONFIG_ENV: &str = "NEUROOS_CONFIG";

/// Largest frame the protocol will ever accept, whatever the config says.
/// The 4-byte length prefix could express 4 GiB; this ceiling keeps one bad
/// config value from turning into a memory-exhaustion bug.
pub const FRAME_BYTES_CEILING: u32 = 16 * 1024 * 1024;

/// Errors loading or validating the config.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file exists but could not be read.
    #[error("cannot read config file {path}: {source}")]
    Read {
        /// File that failed.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
    /// TOML syntax error, wrong type or an unknown key.
    #[error("invalid config: {0}")]
    Parse(#[from] toml::de::Error),
    /// A value parsed but is outside its allowed range.
    #[error("invalid config value `{key}`: {reason}")]
    Invalid {
        /// Dotted key path.
        key: &'static str,
        /// What is wrong with it.
        reason: &'static str,
    },
}

/// The whole configuration file.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// IPC framing limits.
    #[serde(default)]
    pub ipc: IpcConfig,
    /// Per-call deadlines.
    #[serde(default)]
    pub deadlines: DeadlinesConfig,
    /// Health daemon options.
    #[serde(default)]
    pub healthd: HealthdConfig,
    /// Verification hooks (SC proof 3). Read-only probes; they never widen anything.
    #[serde(default)]
    pub debug: DebugConfig,
}

/// `[debug]` section.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct DebugConfig {
    /// A file each part tries to read right after Landlock restrict; the result is
    /// a health counter (security.md §8, "Landlock applied before input").
    pub landlock_canary: Option<PathBuf>,
}

/// `[ipc]` section.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct IpcConfig {
    /// Maximum frame payload in bytes (Architecture §5.1: 1 MiB by default).
    pub max_frame_bytes: u32,
}

impl Default for IpcConfig {
    fn default() -> Self {
        Self {
            max_frame_bytes: 1024 * 1024,
        }
    }
}

/// `[deadlines]` section, in milliseconds. Defaults: PRD §6.1 "IPC deadlines".
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct DeadlinesConfig {
    /// Control calls.
    pub control_ms: u64,
    /// C3 queries.
    pub storage_query_ms: u64,
    /// C6 capability checks (approval time excluded).
    pub kernel_check_ms: u64,
    /// A whole generation from C4.
    pub generation_ms: u64,
    /// One fetch by C7.
    pub fetch_ms: u64,
}

impl Default for DeadlinesConfig {
    fn default() -> Self {
        Self {
            control_ms: 250,
            storage_query_ms: 150,
            kernel_check_ms: 50,
            generation_ms: 30_000,
            fetch_ms: 15_000,
        }
    }
}

impl DeadlinesConfig {
    /// Control-call deadline as a [`Duration`].
    #[must_use]
    pub fn control(&self) -> Duration {
        Duration::from_millis(self.control_ms)
    }
}

/// `[healthd]` section.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct HealthdConfig {
    /// Scrape interval in seconds (Architecture §6.6: default 30).
    pub scrape_interval_s: u64,
    /// Whether soak tracking is enabled.
    pub soak: bool,
}

impl Default for HealthdConfig {
    fn default() -> Self {
        Self {
            scrape_interval_s: 30,
            soak: false,
        }
    }
}

impl Config {
    /// Parses and validates config text.
    ///
    /// # Errors
    /// Syntax errors, unknown keys and out-of-range values.
    pub fn from_toml_str(text: &str) -> Result<Self, ConfigError> {
        let cfg: Self = toml::from_str(text)?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Reads and validates a config file. A missing file yields the defaults;
    /// any other read failure is an error.
    ///
    /// # Errors
    /// See [`ConfigError`].
    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_toml_str(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Read {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    fn validate(&self) -> Result<(), ConfigError> {
        let frame = self.ipc.max_frame_bytes;
        if frame == 0 || frame > FRAME_BYTES_CEILING {
            return Err(ConfigError::Invalid {
                key: "ipc.max_frame_bytes",
                reason: "must be between 1 byte and 16 MiB",
            });
        }
        let d = &self.deadlines;
        let all = [
            ("deadlines.control_ms", d.control_ms),
            ("deadlines.storage_query_ms", d.storage_query_ms),
            ("deadlines.kernel_check_ms", d.kernel_check_ms),
            ("deadlines.generation_ms", d.generation_ms),
            ("deadlines.fetch_ms", d.fetch_ms),
        ];
        for (key, v) in all {
            if v == 0 {
                return Err(ConfigError::Invalid {
                    key,
                    reason: "a deadline must be greater than zero",
                });
            }
        }
        if self.healthd.scrape_interval_s == 0 {
            return Err(ConfigError::Invalid {
                key: "healthd.scrape_interval_s",
                reason: "scrape interval must be greater than zero",
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_gives_prd_defaults() {
        let cfg = Config::from_toml_str("").unwrap();
        assert_eq!(cfg, Config::default());
        assert_eq!(cfg.deadlines.control(), Duration::from_millis(250));
        assert_eq!(cfg.ipc.max_frame_bytes, 1_048_576);
    }

    #[test]
    fn unknown_top_level_key_is_fatal() {
        let err = Config::from_toml_str("[nonsense]\na = 1\n").unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)), "{err}");
    }

    #[test]
    fn unknown_nested_key_is_fatal() {
        let err = Config::from_toml_str("[ipc]\nmax_frame_byte = 10\n").unwrap_err();
        assert!(err.to_string().contains("max_frame_byte"), "{err}");
    }

    #[test]
    fn wrong_type_is_fatal() {
        assert!(Config::from_toml_str("[ipc]\nmax_frame_bytes = \"big\"\n").is_err());
    }

    #[test]
    fn partial_section_keeps_other_defaults() {
        let cfg = Config::from_toml_str("[deadlines]\nfetch_ms = 9000\n").unwrap();
        assert_eq!(cfg.deadlines.fetch_ms, 9000);
        assert_eq!(cfg.deadlines.control_ms, 250);
    }

    #[test]
    fn out_of_range_values_are_rejected() {
        for text in [
            "[ipc]\nmax_frame_bytes = 0\n",
            "[ipc]\nmax_frame_bytes = 16777217\n",
            "[deadlines]\ncontrol_ms = 0\n",
            "[deadlines]\nfetch_ms = 0\n",
        ] {
            assert!(
                matches!(
                    Config::from_toml_str(text),
                    Err(ConfigError::Invalid { .. })
                ),
                "{text}"
            );
        }
        assert!(Config::from_toml_str("[ipc]\nmax_frame_bytes = 16777216\n").is_ok());
    }

    #[test]
    fn missing_file_gives_defaults_but_unreadable_file_fails() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("absent.toml");
        assert_eq!(Config::load_from(&missing).unwrap(), Config::default());
        // A directory where a file is expected cannot be read.
        assert!(matches!(
            Config::load_from(dir.path()),
            Err(ConfigError::Read { .. })
        ));
    }

    #[test]
    fn file_is_read_and_validated() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[deadlines]\ngeneration_ms = 1000\n").unwrap();
        assert_eq!(Config::load_from(&p).unwrap().deadlines.generation_ms, 1000);
        std::fs::write(&p, "bogus = true\n").unwrap();
        assert!(Config::load_from(&p).is_err());
    }
}
