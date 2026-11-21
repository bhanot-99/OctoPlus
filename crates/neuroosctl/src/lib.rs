//! Library interface and CLI definitions for `neuroosctl` (PRD §5.13, FR-CLI-01..02).

#![allow(clippy::print_stdout, clippy::print_stderr)]

pub mod ask;
pub mod audit;
pub mod egress;
pub mod init;
pub mod nudges;
pub mod profile;
pub mod replay;
pub mod run;
pub mod status;
pub mod storage;
pub mod style;
pub mod voice;

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use style::ColorChoice;

/// `neuroosctl` command-line options.
#[derive(Debug, Parser)]
#[command(
    name = "neuroosctl",
    about = "Operator CLI for Semantic-NeuroOS",
    version = env!("CARGO_PKG_VERSION")
)]
pub struct Cli {
    /// Runtime root directory (defaults to $XDG_RUNTIME_DIR/neuroos).
    #[arg(long, global = true)]
    pub runtime_dir: Option<PathBuf>,

    /// Color output mode (auto, always, never).
    #[arg(long, global = true, value_enum, default_value = "auto")]
    pub color: ColorChoice,

    #[command(subcommand)]
    pub command: Commands,
}

/// Available subcommands for `neuroosctl`.
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Inspect health, RSS, and latency across all components.
    Status {
        /// Emit structured machine-readable JSON.
        #[arg(long)]
        json: bool,

        /// Suppress non-essential output.
        #[arg(long, short)]
        quiet: bool,
    },

    /// Ask a question to the conversational assistant.
    Ask {
        /// The query text to submit.
        #[arg(required = true, num_args = 1..)]
        question: Vec<String>,

        /// Emit stream events as JSON lines.
        #[arg(long)]
        json: bool,

        /// After the answer, print which route it took and its taint.
        #[arg(long)]
        explain: bool,

        /// Epoch nanoseconds the question counts as asked at (evaluation harness only).
        #[arg(long, hide = true, default_value_t = 0)]
        at_ns: u64,

        /// Suppress non-essential output.
        #[arg(long, short)]
        quiet: bool,
    },

    /// Storage: run a lifecycle job (gc, backup, rollup) or search what C3 remembers.
    Storage {
        #[command(subcommand)]
        command: StorageCommands,
    },

    /// The nudge rules: list, switch on or off, show a test line.
    Nudges {
        #[command(subcommand)]
        command: NudgesCommands,
    },

    /// The voice service: what it is doing right now.
    Voice {
        #[command(subcommand)]
        command: VoiceCommands,
    },

    /// Personal facts the buddy remembers (list, add, remove).
    Profile {
        #[command(subcommand)]
        command: ProfileCommands,
    },

    /// First-run setup: generate keys, verify models, seed configuration, and check encryption.
    Init {
        /// Force regeneration of keys and configuration files.
        #[arg(long)]
        force: bool,

        /// Skip model download/verification.
        #[arg(long)]
        skip_models: bool,

        /// Non-interactive mode (accept defaults and start target without prompting).
        #[arg(long, short = 'y')]
        yes: bool,

        /// Target configuration directory (defaults to ~/.config/neuroos).
        #[arg(long)]
        config_dir: Option<PathBuf>,
    },

    /// Inspect and verify the safety kernel action audit log.
    Audit {
        #[command(subcommand)]
        command: AuditCommands,
    },

    /// Ask the safety kernel to run one skill (a dialog appears for REVIEW skills).
    Run {
        /// Capability id, e.g. `system.disk_report`.
        capability: String,

        /// Arguments as a JSON object.
        #[arg(long)]
        args: Option<String>,

        /// Mark the request as influenced by web content (it can only make it stricter).
        #[arg(long)]
        web_tainted: bool,
    },

    /// Inspect the safety kernel network egress ledger.
    Egress {
        #[command(subcommand)]
        command: EgressCommands,
    },

    /// Replay a recorded monitor dump (FR-MON-12, P3-S06).
    Replay {
        /// Recorded dump file to replay.
        dump: PathBuf,

        /// Replay speed multiplier (0 = no delay, 1.0 = real time, 10.0 = 10x faster).
        #[arg(long, default_value_t = 1.0, value_parser = replay::validate_speed)]
        speed: f64,

        /// Serve the dump over a Unix domain socket for MonitorSubscribe clients.
        #[arg(long)]
        serve: Option<PathBuf>,

        /// Emit one JSON object per line.
        #[arg(long)]
        json: bool,
    },
}

/// Subcommands under `neuroosctl storage`.
#[derive(Debug, Subcommand)]
pub enum StorageCommands {
    /// Run one lifecycle job now: `gc`, `backup` or `rollup` (the systemd timers call this).
    Job {
        /// Job name.
        name: String,
    },
    /// Hybrid search over what C3 remembers (prints personal text to the terminal).
    Search {
        /// What to look for.
        text: String,
        /// Most results.
        #[arg(long, default_value_t = 5)]
        top_k: u32,
    },
}

/// Subcommands under `neuroosctl voice` (text equivalent of the voice status, DP-7).
#[derive(Debug, Subcommand)]
pub enum VoiceCommands {
    /// Is the microphone being captured, is a turn running, is anyone else on the microphone.
    Status {
        /// One JSON object.
        #[arg(long)]
        json: bool,
    },
}

/// Subcommands under `neuroosctl nudges` (FR-NUD-02).
#[derive(Debug, Subcommand)]
pub enum NudgesCommands {
    /// Show every rule: on, off or snoozed, and when it last fired.
    List {
        /// One JSON object per line.
        #[arg(long)]
        json: bool,
    },
    /// Switch a rule on.
    Enable {
        /// Rule id, as shown by `list`.
        rule: String,
    },
    /// Switch a rule off until it is enabled again.
    Disable {
        /// Rule id, as shown by `list`.
        rule: String,
    },
    /// Show the rule's own notification line now (it does not count as a nudge).
    Test {
        /// Rule id, as shown by `list`.
        rule: String,
        /// Say the line aloud instead (also how to hear the buddy's voice).
        #[arg(long)]
        voice: bool,
    },
}

/// Subcommands under `neuroosctl profile` (FR-PRO-04).
#[derive(Debug, Subcommand)]
pub enum ProfileCommands {
    /// List saved facts (prints personal text to the terminal).
    List {
        /// Only facts relevant to this text.
        #[arg(long)]
        relevant_to: Option<String>,
        /// Emit one JSON object per line.
        #[arg(long)]
        json: bool,
    },
    /// Save a fact (goes through the safety kernel as `profile.remember`).
    Add {
        /// The fact, in your own words.
        text: String,
    },
    /// Remove a fact by id (goes through the safety kernel; a dialog asks you to confirm).
    Remove {
        /// Id shown by `profile list`.
        id: u64,
    },
}

/// Subcommands under `neuroosctl audit`.
#[derive(Debug, Subcommand)]
pub enum AuditCommands {
    /// Tail the most recent action audit log records.
    Tail {
        /// Maximum number of records to return (default 20, max 1000).
        #[arg(long, default_value_t = 20)]
        limit: u32,

        /// Emit structured machine-readable JSON.
        #[arg(long)]
        json: bool,

        /// Suppress non-essential output.
        #[arg(long, short)]
        quiet: bool,
    },

    /// Verify cryptographic hash-chain integrity of the audit log.
    Verify {
        /// Emit structured machine-readable JSON.
        #[arg(long)]
        json: bool,

        /// Suppress non-essential output.
        #[arg(long, short)]
        quiet: bool,
    },
}

/// Subcommands under `neuroosctl egress`.
#[derive(Debug, Subcommand)]
pub enum EgressCommands {
    /// List recorded network egress events.
    List {
        /// Maximum number of records to return (default 20, max 1000).
        #[arg(long, default_value_t = 20)]
        limit: u32,

        /// Emit structured machine-readable JSON.
        #[arg(long)]
        json: bool,

        /// Suppress non-essential output.
        #[arg(long, short)]
        quiet: bool,
    },
}
