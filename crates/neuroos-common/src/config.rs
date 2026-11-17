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
    /// Safety kernel options (C6, FR-KER-01, FR-KER-08).
    #[serde(default)]
    pub kernel: KernelConfig,
    /// Context monitor options (C1, FR-MON-01..12).
    #[serde(default)]
    pub monitor: MonitorConfig,
    /// Storage engine options (C3, FR-STO-01..13).
    #[serde(default)]
    pub storage: StorageConfig,
    /// Knowledge and conversation options (C5, FR-KNO-01..10, FR-CHT, FR-PRO).
    #[serde(default)]
    pub knowledge: KnowledgeConfig,
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
    /// C3 queries. The handler enforces it on queries; the socket's own deadline is `storage_job_ms`.
    pub storage_query_ms: u64,
    /// C3 jobs that are not queries: forget, purge, backup, GC, roll-up (PRD §6.1 sets queries at
    /// 150 ms; ADR-0012 item 11 adds this for the rest).
    pub storage_job_ms: u64,
    /// C6 capability checks (approval time excluded). Enforced by the caller.
    pub kernel_check_ms: u64,
    /// The kernel socket's server deadline: a dispatch waits for the user's
    /// approval (120 s, Architecture §8.4) plus the longest skill (30 s), plus margin.
    pub kernel_dispatch_ms: u64,
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
            storage_job_ms: 120_000,
            kernel_check_ms: 50,
            kernel_dispatch_ms: 155_000,
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

/// `[kernel]` section (PRD §5.9, Architecture §8.3).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct KernelConfig {
    /// Maximum review/dangerous requests allowed in a rolling 60 s window.
    pub review_per_minute: u32,
    /// Cooling period in seconds after user denies a request.
    pub denial_cooldown_s: u64,
    /// Build folders permitted for cleanup operations.
    pub cleanup_build_folders: Vec<PathBuf>,
}

impl Default for KernelConfig {
    fn default() -> Self {
        Self {
            review_per_minute: 6,
            denial_cooldown_s: 300,
            cleanup_build_folders: Vec::new(),
        }
    }
}

/// What a watched folder holds (FR-MON-11, OQ-05).
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FolderKind {
    /// A git repository: HEAD, index and refs.
    Git,
    /// Notes: markdown and text files.
    Notes,
    /// Calendar: `.ics` files.
    Calendar,
}

/// One folder C1 watches (FR-MON-11).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FolderConfig {
    /// Absolute path of the folder.
    pub path: PathBuf,
    /// What it holds.
    pub kind: FolderKind,
}

/// `[monitor]` section (PRD §5.2). Defaults are the PRD values.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct MonitorConfig {
    /// CPU and memory sampling interval, seconds (FR-MON-04).
    pub cpu_interval_s: u64,
    /// Disk sampling interval, seconds (FR-MON-05).
    pub disk_interval_s: u64,
    /// Battery re-emit interval, seconds (FR-MON-06).
    pub battery_interval_s: u64,
    /// Temperature sampling interval, seconds (FR-MON-07).
    pub thermal_interval_s: u64,
    /// Idle timeout, seconds (FR-MON-02).
    pub idle_timeout_s: u64,
    /// Events one subscriber may have queued before the oldest are dropped.
    pub queue_capacity: usize,
    /// Extra app ids to exclude, added to the defaults (FR-PRV-02). A trailing `*` is a prefix match.
    pub exclude_apps: Vec<String>,
    /// Extra title substrings that mark a window as private (FR-PRV-02).
    pub exclude_title_substrings: Vec<String>,
    /// Remove the built-in exclusion defaults. Off unless the user says so.
    pub drop_default_exclusions: bool,
    /// Folders to watch (FR-MON-11).
    pub folders: Vec<FolderConfig>,
    /// App ids a recording may keep in clear text, added to the built-in list (FR-MON-12).
    pub record_keep_app_ids: Vec<String>,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            cpu_interval_s: 5,
            disk_interval_s: 60,
            battery_interval_s: 60,
            thermal_interval_s: 10,
            idle_timeout_s: 60,
            queue_capacity: 1024,
            exclude_apps: Vec::new(),
            exclude_title_substrings: Vec::new(),
            drop_default_exclusions: false,
            folders: Vec::new(),
            record_keep_app_ids: Vec::new(),
        }
    }
}

impl MonitorConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        let intervals = [
            ("monitor.cpu_interval_s", self.cpu_interval_s),
            ("monitor.disk_interval_s", self.disk_interval_s),
            ("monitor.battery_interval_s", self.battery_interval_s),
            ("monitor.thermal_interval_s", self.thermal_interval_s),
            ("monitor.idle_timeout_s", self.idle_timeout_s),
        ];
        for (key, v) in intervals {
            if v == 0 {
                return Err(ConfigError::Invalid {
                    key,
                    reason: "must be greater than zero",
                });
            }
        }
        if self.queue_capacity == 0 {
            return Err(ConfigError::Invalid {
                key: "monitor.queue_capacity",
                reason: "must be greater than zero",
            });
        }
        for f in &self.folders {
            let bad = !f.path.is_absolute()
                || f.path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir));
            if bad {
                return Err(ConfigError::Invalid {
                    key: "monitor.folders",
                    reason: "paths must be absolute and must not contain '..'",
                });
            }
        }
        Ok(())
    }
}

/// `[storage]` section (PRD §5.3, §6.2, §6.5). Defaults are the PRD values.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct StorageConfig {
    /// Live-chunk cap (PRD §6.2, FR-STO-06).
    pub chunk_cap: usize,
    /// Read-only connections in the reader pool (ADR-0004).
    pub reader_pool: usize,
    /// Operations the writer queue holds before senders wait.
    pub write_queue: usize,
    /// Most events one ingest batch holds.
    pub ingest_batch: usize,
    /// A batch is flushed at least this often, in milliseconds.
    pub ingest_flush_ms: u64,
    /// Occurrences that promote an item (FR-STO-01 stage 3).
    pub promote_min_occurrences: u32,
    /// Active dwell, in seconds, above which an item is promoted (FR-STO-01 stage 3).
    pub promote_min_dwell_s: u64,
    /// A media change counts as interaction if its player was focused within this many seconds (stage 4).
    pub media_interaction_s: u64,
    /// App-id prefixes of NeuroOS's own windows, dropped at stage 2 (FR-STO-01).
    pub own_app_prefixes: Vec<String>,
    /// Process names that mean "build job" at stage 1 (FR-STO-01). Never stored as entities.
    pub build_process_names: Vec<String>,
    /// Threads the exact vector scan uses (ADR-0004 item 3).
    pub scan_threads: usize,
    /// Vector candidates and text candidates taken before fusion (FR-STO-05).
    pub query_candidates: usize,
    /// Most results one query may return.
    pub query_top_k_max: u32,
    /// Reciprocal-rank-fusion constant.
    pub rrf_k: u32,
    /// Retention in days: focus history, window and process activity (PRD §6.5).
    pub retention_focus_days: u32,
    /// Raw health samples (PRD §6.5).
    pub retention_health_raw_days: u32,
    /// Hourly health summaries (PRD §6.5).
    pub retention_health_rollup_days: u32,
    /// Media playback (PRD §6.5).
    pub retention_media_days: u32,
    /// Voice interactions (PRD §6.5).
    pub retention_voice_days: u32,
    /// Web documents (PRD §6.5).
    pub retention_web_days: u32,
    /// Nudge log (PRD §6.5).
    pub retention_nudge_days: u32,
    /// Backup interval in hours (FR-STO-11); used by the generated timer.
    pub backup_interval_h: u32,
    /// Backups kept (FR-STO-11).
    pub backup_keep: usize,
    /// Folder C7 leaves fetched documents in (FR-STO-10).
    pub spool_dir: PathBuf,
    /// Largest spool file accepted, in bytes (security.md §4.2).
    pub spool_max_bytes: u64,
    /// How often the spool folder is looked at, in seconds.
    pub spool_poll_s: u64,
    /// Folder holding the embedding model and its tokenizer (manifest). Empty = `~/.local/opt/neuroos/models/embedding`.
    pub embedding_dir: PathBuf,
    /// The ONNX Runtime shared library. Empty = `~/.local/opt/neuroos/lib/libonnxruntime.so.1.31.1`.
    pub onnxruntime_lib: PathBuf,
}

impl Default for StorageConfig {
    fn default() -> Self {
        let own = vec!["org.neuroos.".to_owned()];
        let build = [
            "rustc",
            "cargo",
            "cc1",
            "cc1plus",
            "gcc",
            "g++",
            "cc",
            "c++",
            "clang",
            "clang++",
            "ld",
            "lld",
            "ld.lld",
            "ld.bfd",
            "ld.gold",
            "as",
            "ar",
            "ranlib",
            "ninja",
            "make",
            "cmake",
            "cargo-clippy",
            "clippy-driver",
            "rustdoc",
            "tsc",
            "webpack",
            "javac",
            "go",
            "gradle",
            "mvn",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        Self {
            chunk_cap: 30_000,
            reader_pool: 4,
            write_queue: 1024,
            ingest_batch: 64,
            ingest_flush_ms: 250,
            promote_min_occurrences: 3,
            promote_min_dwell_s: 5,
            media_interaction_s: 10,
            own_app_prefixes: own,
            build_process_names: build,
            scan_threads: 4,
            query_candidates: 50,
            query_top_k_max: 20,
            rrf_k: 60,
            retention_focus_days: 30,
            retention_health_raw_days: 7,
            retention_health_rollup_days: 30,
            retention_media_days: 14,
            retention_voice_days: 90,
            retention_web_days: 30,
            retention_nudge_days: 90,
            backup_interval_h: 6,
            backup_keep: 8,
            spool_dir: PathBuf::from("/var/spool/neuroos-fetcher"),
            spool_max_bytes: 256 * 1024,
            spool_poll_s: 2,
            embedding_dir: PathBuf::new(),
            onnxruntime_lib: PathBuf::new(),
        }
    }
}

impl StorageConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        let nonzero = [
            ("storage.chunk_cap", self.chunk_cap as u64),
            ("storage.reader_pool", self.reader_pool as u64),
            ("storage.write_queue", self.write_queue as u64),
            ("storage.ingest_batch", self.ingest_batch as u64),
            ("storage.ingest_flush_ms", self.ingest_flush_ms),
            (
                "storage.promote_min_occurrences",
                u64::from(self.promote_min_occurrences),
            ),
            ("storage.scan_threads", self.scan_threads as u64),
            ("storage.query_candidates", self.query_candidates as u64),
            ("storage.query_top_k_max", u64::from(self.query_top_k_max)),
            ("storage.rrf_k", u64::from(self.rrf_k)),
            (
                "storage.backup_interval_h",
                u64::from(self.backup_interval_h),
            ),
            ("storage.backup_keep", self.backup_keep as u64),
            ("storage.spool_max_bytes", self.spool_max_bytes),
            ("storage.spool_poll_s", self.spool_poll_s),
            (
                "storage.retention_focus_days",
                u64::from(self.retention_focus_days),
            ),
            (
                "storage.retention_health_raw_days",
                u64::from(self.retention_health_raw_days),
            ),
            (
                "storage.retention_health_rollup_days",
                u64::from(self.retention_health_rollup_days),
            ),
            (
                "storage.retention_media_days",
                u64::from(self.retention_media_days),
            ),
            (
                "storage.retention_voice_days",
                u64::from(self.retention_voice_days),
            ),
            (
                "storage.retention_web_days",
                u64::from(self.retention_web_days),
            ),
            (
                "storage.retention_nudge_days",
                u64::from(self.retention_nudge_days),
            ),
        ];
        for (key, v) in nonzero {
            if v == 0 {
                return Err(ConfigError::Invalid {
                    key,
                    reason: "must be greater than zero",
                });
            }
        }
        if self.chunk_cap > 1_000_000 {
            return Err(ConfigError::Invalid {
                key: "storage.chunk_cap",
                reason: "must not exceed 1,000,000",
            });
        }
        if self.reader_pool > 64 || self.scan_threads > 64 {
            return Err(ConfigError::Invalid {
                key: "storage.reader_pool",
                reason: "reader_pool and scan_threads must not exceed 64",
            });
        }
        for p in [&self.spool_dir, &self.embedding_dir, &self.onnxruntime_lib] {
            let bad = !p.as_os_str().is_empty()
                && (!p.is_absolute()
                    || p.components()
                        .any(|c| matches!(c, std::path::Component::ParentDir)));
            if bad {
                return Err(ConfigError::Invalid {
                    key: "storage.spool_dir",
                    reason: "paths must be absolute and must not contain '..'",
                });
            }
        }
        if self.own_app_prefixes.iter().any(String::is_empty) {
            return Err(ConfigError::Invalid {
                key: "storage.own_app_prefixes",
                reason: "an empty prefix would match every window",
            });
        }
        Ok(())
    }
}

/// `[knowledge]` section (PRD §5.4-5.7, ADR-0005 layout, ADR-0013). Scores are in
/// thousandths so the config stays comparable (`Eq`).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct KnowledgeConfig {
    /// Evidence items asked of C3 (FR-KNO-04).
    pub evidence_top_k: u32,
    /// Evidence below this score (thousandths) is dropped (FR-KNO-04). Provisional: measured on
    /// the reference machine's live C3 (ADR-0013 §measurements) and to be re-measured on real
    /// recordings at integration (D-26).
    pub min_score_milli: u32,
    /// A retrieval score (thousandths) at or above which a turn counts as a desktop answer
    /// (Architecture §11 step 3). Set above the highest score any of 40 general questions reached
    /// (0.70): a general question sent down the desktop route would be answered "I don't have
    /// anything on that". Activity questions are mostly caught by their time and "I was" cues.
    pub desktop_route_score_milli: u32,
    /// Prompt block budgets in tokens (ADR-0005 decision 3). The persona block is a constant,
    /// cached prefix and is never cut, so it has no budget here.
    /// Personal-facts block.
    pub facts_tokens: u32,
    /// Focus-context block: ADR-0005's 32 tokens plus the untrusted-wrapper's own cost (ADR-0013).
    pub focus_tokens: u32,
    /// Session-history block (the question counts against it on the first turn).
    pub history_tokens: u32,
    /// Evidence block.
    pub evidence_tokens: u32,
    /// Bytes per estimated token; the estimate rounds up (ADR-0013 decision 3).
    pub bytes_per_token: u32,
    /// Turns the session history keeps (FR-KNO-09).
    pub history_turns: u32,
    /// The follow-up window in seconds; history ends when it closes (FR-VOI-08, FR-KNO-09).
    pub followup_window_s: u64,
    /// The same window for typed turns, which have no spoken answer to wait for (ADR-0013).
    pub typed_followup_window_s: u64,
    /// Focus-history window, in seconds, either side of the asked-about instant (FR-STO-07).
    pub focus_window_s: u64,
    /// Longest answer asked of C4, in tokens.
    pub max_answer_tokens: u32,
    /// Longest personal fact `neuroosctl profile add` accepts, in bytes.
    pub max_fact_bytes: u32,
}

impl Default for KnowledgeConfig {
    fn default() -> Self {
        Self {
            evidence_top_k: 5,
            min_score_milli: 500,
            desktop_route_score_milli: 720,
            facts_tokens: 48,
            focus_tokens: 48,
            history_tokens: 48,
            evidence_tokens: 128,
            bytes_per_token: 3,
            history_turns: 4,
            followup_window_s: 8,
            typed_followup_window_s: 120,
            focus_window_s: 30,
            max_answer_tokens: 512,
            max_fact_bytes: 512,
        }
    }
}

impl KnowledgeConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        let nonzero = [
            ("knowledge.evidence_top_k", u64::from(self.evidence_top_k)),
            ("knowledge.facts_tokens", u64::from(self.facts_tokens)),
            ("knowledge.focus_tokens", u64::from(self.focus_tokens)),
            ("knowledge.history_tokens", u64::from(self.history_tokens)),
            ("knowledge.evidence_tokens", u64::from(self.evidence_tokens)),
            ("knowledge.bytes_per_token", u64::from(self.bytes_per_token)),
            ("knowledge.history_turns", u64::from(self.history_turns)),
            ("knowledge.followup_window_s", self.followup_window_s),
            (
                "knowledge.typed_followup_window_s",
                self.typed_followup_window_s,
            ),
            ("knowledge.focus_window_s", self.focus_window_s),
            (
                "knowledge.max_answer_tokens",
                u64::from(self.max_answer_tokens),
            ),
            ("knowledge.max_fact_bytes", u64::from(self.max_fact_bytes)),
        ];
        for (key, v) in nonzero {
            if v == 0 {
                return Err(ConfigError::Invalid {
                    key,
                    reason: "must be greater than zero",
                });
            }
        }
        if self.min_score_milli > 1000 || self.desktop_route_score_milli > 1000 {
            return Err(ConfigError::Invalid {
                key: "knowledge.min_score_milli",
                reason: "scores are thousandths, at most 1000",
            });
        }
        if self.evidence_top_k > 20 || self.history_turns > 16 || self.bytes_per_token > 8 {
            return Err(ConfigError::Invalid {
                key: "knowledge.evidence_top_k",
                reason: "evidence_top_k <= 20, history_turns <= 16, bytes_per_token <= 8",
            });
        }
        Ok(())
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
        self.monitor.validate()?;
        self.storage.validate()?;
        self.knowledge.validate()?;
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
            ("deadlines.storage_job_ms", d.storage_job_ms),
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
        if self.kernel.review_per_minute < 1 {
            return Err(ConfigError::Invalid {
                key: "kernel.review_per_minute",
                reason: "review_per_minute must be at least 1",
            });
        }
        if self.kernel.denial_cooldown_s > 86400 {
            return Err(ConfigError::Invalid {
                key: "kernel.denial_cooldown_s",
                reason: "denial_cooldown_s must not exceed 86400 seconds",
            });
        }
        for path in &self.kernel.cleanup_build_folders {
            if !path.is_absolute() {
                return Err(ConfigError::Invalid {
                    key: "kernel.cleanup_build_folders",
                    reason: "paths must be absolute",
                });
            }
            if path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(ConfigError::Invalid {
                    key: "kernel.cleanup_build_folders",
                    reason: "paths must not contain '..'",
                });
            }
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
        assert_eq!(cfg.kernel.review_per_minute, 6);
        assert_eq!(cfg.kernel.denial_cooldown_s, 300);
        assert!(cfg.kernel.cleanup_build_folders.is_empty());
    }

    #[test]
    fn monitor_defaults_are_the_prd_values() {
        let m = Config::default().monitor;
        assert_eq!((m.cpu_interval_s, m.disk_interval_s), (5, 60));
        assert_eq!(
            (m.battery_interval_s, m.thermal_interval_s, m.idle_timeout_s),
            (60, 10, 60)
        );
        assert!(!m.drop_default_exclusions);
    }

    #[test]
    fn monitor_rejects_zero_intervals_unknown_keys_and_relative_folders() {
        for bad in [
            "[monitor]\ncpu_interval_s = 0\n",
            "[monitor]\nqueue_capacity = 0\n",
            "[monitor]\nnonsense = 1\n",
            "[[monitor.folders]]\npath = \"Notes\"\nkind = \"notes\"\n",
            "[[monitor.folders]]\npath = \"/home/u/../x\"\nkind = \"git\"\n",
        ] {
            assert!(Config::from_toml_str(bad).is_err(), "accepted: {bad}");
        }
        let ok = "[[monitor.folders]]\npath = \"/home/u/Notes\"\nkind = \"notes\"\n";
        assert_eq!(Config::from_toml_str(ok).unwrap().monitor.folders.len(), 1);
    }

    #[test]
    fn storage_defaults_are_the_prd_values_and_bad_values_are_refused() {
        let s = Config::default().storage;
        assert_eq!(
            (s.chunk_cap, s.backup_keep, s.backup_interval_h),
            (30_000, 8, 6)
        );
        assert_eq!((s.promote_min_occurrences, s.promote_min_dwell_s), (3, 5));
        assert_eq!(
            (s.retention_focus_days, s.retention_health_raw_days),
            (30, 7)
        );
        assert_eq!(
            (
                s.retention_media_days,
                s.retention_voice_days,
                s.retention_web_days
            ),
            (14, 90, 30)
        );
        for bad in [
            "[storage]\nchunk_cap = 0\n",
            "[storage]\nchunk_cap = 5000000\n",
            "[storage]\nreader_pool = 100\n",
            "[storage]\nbackup_keep = 0\n",
            "[storage]\nnonsense = 1\n",
            "[storage]\nspool_dir = \"relative/dir\"\n",
            "[storage]\nspool_dir = \"/a/../b\"\n",
            "[storage]\nown_app_prefixes = [\"\"]\n",
        ] {
            assert!(Config::from_toml_str(bad).is_err(), "accepted: {bad}");
        }
        assert!(Config::from_toml_str("[storage]\nchunk_cap = 100\n").is_ok());
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
            "[kernel]\nreview_per_minute = 0\n",
            "[kernel]\ndenial_cooldown_s = 86401\n",
            "[kernel]\ncleanup_build_folders = [\"relative/path\"]\n",
            "[kernel]\ncleanup_build_folders = [\"/var/log/../build\"]\n",
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
        let valid_kernel = "[kernel]\nreview_per_minute = 10\ndenial_cooldown_s = 60\ncleanup_build_folders = [\"/target\"]\n";
        let cfg = Config::from_toml_str(valid_kernel).unwrap();
        assert_eq!(cfg.kernel.review_per_minute, 10);
        assert_eq!(cfg.kernel.denial_cooldown_s, 60);
        assert_eq!(
            cfg.kernel.cleanup_build_folders,
            vec![PathBuf::from("/target")]
        );
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
