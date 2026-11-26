//! Binary entrypoint for `octoplusctl` (PRD §5.13).

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::PathBuf;

use clap::Parser;
use octoplus_common::paths;
use octoplusctl::ask::{AskOptions, run_ask};
use octoplusctl::audit::run_audit;
use octoplusctl::egress::run_egress;
use octoplusctl::nudges::run_nudges;
use octoplusctl::profile::run_profile;
use octoplusctl::status::run_status;
use octoplusctl::storage::run_storage;
use octoplusctl::style::Styler;
use octoplusctl::voice::run_voice;
use octoplusctl::{Cli, Commands};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let styler = Styler::new(cli.color);

    let runtime_root = match cli.runtime_dir {
        Some(r) => r,
        None => paths::runtime_root(std::env::var_os("XDG_RUNTIME_DIR").as_deref())
            .unwrap_or_else(|_| PathBuf::from("/tmp/octoplus")),
    };

    let exit_code = match cli.command {
        Commands::Status { json, .. } => run_status(&runtime_root, json, styler).await,
        Commands::Ask {
            question,
            json,
            explain,
            once,
            at_ns,
            companion,
            ..
        } => {
            use std::io::IsTerminal;
            let question_text = question.join(" ");
            let interactive =
                !once && !json && std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
            run_ask(
                &runtime_root,
                question_text,
                AskOptions {
                    json,
                    explain,
                    at_ns,
                    interactive,
                    companion,
                },
                styler,
            )
            .await
        }
        Commands::Chat {
            question,
            json,
            explain,
        } => {
            use std::io::IsTerminal;
            if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
                eprintln!(
                    "{}",
                    styler.error(
                        "chat requires an interactive terminal; for non-interactive use, try 'octoplusctl ask --companion \"text\"'"
                    )
                );
                1
            } else {
                let question_text = question.join(" ");
                run_ask(
                    &runtime_root,
                    question_text,
                    AskOptions {
                        json,
                        explain,
                        at_ns: 0,
                        interactive: true,
                        companion: true,
                    },
                    styler,
                )
                .await
            }
        }
        Commands::Run {
            capability,
            args,
            web_tainted,
        } => {
            octoplusctl::run::run_skill(
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
            octoplusctl::init::run_init(
                octoplusctl::init::InitOptions {
                    force,
                    skip_models,
                    yes,
                    config_dir,
                    system_fetch_key: None,
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
        } => octoplusctl::replay::run_replay(&dump, speed, serve.as_deref(), json).await,
    };

    std::process::exit(exit_code);
}
