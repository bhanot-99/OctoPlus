//! Library interface and CLI definitions for `neuroosctl` (PRD §5.13, FR-CLI-01..02).

#![allow(clippy::print_stdout, clippy::print_stderr)]

pub mod ask;
pub mod audit;
pub mod egress;
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

    /// Inspect the safety kernel network egress ledger.
    Egress {
        #[command(subcommand)]
        command: EgressCommands,
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
