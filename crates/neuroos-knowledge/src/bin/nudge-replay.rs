#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::env;
use std::fs;
use std::process::ExitCode;

use neuroos_knowledge::nudge::replay;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: nudge-replay <scenario.json>...");
        return ExitCode::from(2);
    }

    let mut any_failed = false;

    for path in &args[1..] {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Error reading `{path}`: {e}");
                return ExitCode::from(2);
            }
        };

        let scenario = match replay::parse(&content) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error parsing `{path}`: {e}");
                return ExitCode::from(2);
            }
        };

        let report = match replay::run(&scenario) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Error executing `{path}`: {e}");
                return ExitCode::from(2);
            }
        };

        for event in &report.events {
            if let Ok(json) = serde_json::to_string(event) {
                println!("{json}");
            }
        }

        let mismatches = replay::check(&scenario, &report);
        let ok = mismatches.is_empty();
        if !ok {
            any_failed = true;
        }

        let summary = serde_json::json!({
            "scenario": scenario.name,
            "ok": ok,
            "mismatches": mismatches,
        });
        if let Ok(json) = serde_json::to_string(&summary) {
            println!("{json}");
        }
    }

    if any_failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
