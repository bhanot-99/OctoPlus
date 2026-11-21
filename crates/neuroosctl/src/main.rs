//! Binary entrypoint for `neuroosctl` (PRD §5.13).

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::PathBuf;

use clap::Parser;
use neuroos_common::paths;
use neuroosctl::ask::{AskOptions, run_ask};
use neuroosctl::audit::run_audit;
use neuroosctl::egress::run_egress;
use neuroosctl::nudges::run_nudges;
use neuroosctl::profile::run_profile;
use neuroosctl::status::run_status;
use neuroosctl::storage::run_storage;
use neuroosctl::style::Styler;
use neuroosctl::voice::run_voice;
use neuroosctl::{Cli, Commands};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let styler = Styler::new(cli.color);

    let runtime_root = match cli.runtime_dir {
        Some(r) => r,
        None => paths::runtime_root(std::env::var_os("XDG_RUNTIME_DIR").as_deref())
            .unwrap_or_else(|_| PathBuf::from("/tmp/neuroos")),
    };

    let exit_code = match cli.command {
        Commands::Status { json, .. } => run_status(&runtime_root, json, styler).await,
        Commands::Ask {
            question,
            json,
            explain,
            once,
            at_ns,
            ..
        } => {
            use std::io::IsTerminal;
            let question_text = question.join(" ");
            let interactive = !once && !json && std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
            run_ask(
                &runtime_root,
                question_text,
                AskOptions {
                    json,
                    explain,
                    at_ns,
                    interactive,
                },
                styler,
            )
            .await
        }
        Commands::Run {
            capability,
            args,
            web_tainted,
        } => {
            neuroosctl::run::run_skill(
                &runtime_root,
                &capability,
                args.as_deref(),
                web_tainted,
                styler,
            )
            .await
        }
        Commands::Storage { command } => run_storage(&runtime_root, command, styler).await,
        Commands::Nudges { command } => run_nudges(&runtime_root, command, styler).await,
        Commands::Voice { command } => run_voice(&runtime_root, command, styler).await,
        Commands::Profile { command } => run_profile(&runtime_root, command, styler).await,
        Commands::Audit { command } => run_audit(&runtime_root, command, styler).await,
        Commands::Egress { command } => run_egress(&runtime_root, command, styler).await,
        Commands::Init {
            force,
            skip_models,
            yes,
            config_dir,
        } => {
            neuroosctl::init::run_init(
                neuroosctl::init::InitOptions {
                    force,
                    skip_models,
                    yes,
                    config_dir,
                },
                styler,
            )
            .await
        }
        Commands::Replay {
            dump,
            speed,
            serve,
            json,
        } => neuroosctl::replay::run_replay(&dump, speed, serve.as_deref(), json).await,
    };

    std::process::exit(exit_code);
}
