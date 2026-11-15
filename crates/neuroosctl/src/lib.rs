//! Library interface and CLI definitions for `neuroosctl` (PRD §5.13, FR-CLI-01..02).

#![allow(clippy::print_stdout, clippy::print_stderr)]

pub mod ask;
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
}
