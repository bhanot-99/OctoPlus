//! Library interface and CLI definitions for `neuroosctl` (PRD §5.13, FR-CLI-01..02).

#![allow(clippy::print_stdout, clippy::print_stderr)]

pub mod ask;
pub mod audit;
pub mod egress;
pub mod replay;
pub mod run;
pub mod status;
pub mod style;

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
        question: String,

        /// Emit stream events as JSON lines.
        #[arg(long)]
        json: bool,

        /// Suppress non-essential output.
        #[arg(long, short)]
        quiet: bool,
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
