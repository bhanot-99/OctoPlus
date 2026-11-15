//! CLI generator binary for Semantic-NeuroOS deployment units (P1-S01, P1-S02).

#![allow(clippy::print_stdout)]

use std::path::PathBuf;

use anyhow::{Context, Result};
use neuroos_deploy::{generate_all_files, write_all};

fn find_repo_root() -> Result<PathBuf> {
    let mut dir = std::env::current_dir().context("getting current directory")?;
    loop {
        if dir.join("models/manifest.toml").exists() && dir.join("Cargo.toml").exists() {
            return Ok(dir);
        }
        if !dir.pop() {
            break;
        }
    }
    // Fallback to current directory
    std::env::current_dir().context("fallback current directory")
}

fn print_help() {
    println!(
        "neuroos-gen-units — generate systemd units from the socket map\n\n\
        Usage:\n  \
          neuroos-gen-units [OPTIONS]\n\n\
        Options:\n  \
          --out-dir <DIR>      Output directory (default: <repo-root>/deploy)\n  \
          --manifest <PATH>    Path to models/manifest.toml (default: <repo-root>/models/manifest.toml)\n  \
          --check              Verify generated files match disk without modifying\n  \
          -h, --help           Show this help"
    );
}

fn main() -> Result<()> {
    let repo_root = find_repo_root()?;
    let mut out_dir = repo_root.join("deploy");
    let mut manifest_path = repo_root.join("models/manifest.toml");
    let mut check_mode = false;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            "--out-dir" => {
                i += 1;
                if i >= args.len() {
                    anyhow::bail!("--out-dir requires an argument");
                }
                out_dir = PathBuf::from(&args[i]);
            }
            "--manifest" => {
                i += 1;
                if i >= args.len() {
                    anyhow::bail!("--manifest requires an argument");
                }
                manifest_path = PathBuf::from(&args[i]);
            }
            "--check" => {
                check_mode = true;
            }
            other => {
                anyhow::bail!("unknown argument '{other}'. Run with --help for usage.");
            }
        }
        i += 1;
    }

    if check_mode {
        let generated =
            generate_all_files(&manifest_path).context("generating files for drift check")?;
        let mut drift = false;
        for file in generated {
            let full_path = out_dir.join(&file.rel_path);
            if !full_path.exists() {
                println!("Drift: missing file {}", full_path.display());
                drift = true;
                continue;
            }
            let on_disk = std::fs::read_to_string(&full_path)
                .with_context(|| format!("reading {}", full_path.display()))?;
            if on_disk != file.content {
                println!("Drift: content mismatch in {}", full_path.display());
                drift = true;
            }
        }
        if drift {
            anyhow::bail!(
                "drift detected between generated units and files in {}",
                out_dir.display()
            );
        }
        println!("All generated files match disk.");
    } else {
        let written = write_all(&out_dir, &manifest_path).context("writing deployment units")?;
        println!(
            "Generated {} deployment files in {}",
            written.len(),
            out_dir.display()
        );
    }

    Ok(())
}
