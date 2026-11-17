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
          --kernel-dropin <CONFIG> <UNIT_DIR>\n                       Write C6's cleanup-folder drop-in from [kernel] cleanup_build_folders\n  \
          --monitor-dropin <CONFIG> <UNIT_DIR>\n                       Write C1's watched-folder drop-in from [[monitor.folders]]\n  \
          -h, --help           Show this help"
    );
}

/// Writes `<unit_dir>/neuroos-kernel.service.d/cleanup-folders.conf` (OQ-07, ADR-0010).
/// A missing config file means the defaults: no build folders.
fn write_kernel_dropin(config: &std::path::Path, unit_dir: &std::path::Path) -> Result<()> {
    let cfg = neuroos_common::config::Config::load_from(config)
        .with_context(|| format!("loading {}", config.display()))?;
    use neuroos_common::config::FolderKind;
    let of_kind = |kind: FolderKind| -> Vec<std::path::PathBuf> {
        cfg.monitor
            .folders
            .iter()
            .filter(|f| f.kind == kind)
            .map(|f| f.path.clone())
            .collect()
    };
    let text = neuroos_deploy::generate_kernel_folders_dropin(
        &cfg.kernel.cleanup_build_folders,
        &of_kind(FolderKind::Notes),
        &of_kind(FolderKind::Calendar),
    )
    .context("building the drop-in")?;
    let dir = unit_dir.join("neuroos-kernel.service.d");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join("cleanup-folders.conf");
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    println!("Wrote {}", path.display());
    Ok(())
}

/// Writes `<unit_dir>/neuroos-monitor.service.d/folders.conf` (FR-MON-11, OQ-05).
/// A missing config file means the defaults: no watched folders.
fn write_monitor_dropin(config: &std::path::Path, unit_dir: &std::path::Path) -> Result<()> {
    let cfg = neuroos_common::config::Config::load_from(config)
        .with_context(|| format!("loading {}", config.display()))?;
    let text = neuroos_deploy::generate_monitor_folders_dropin(&cfg.monitor.folders)
        .context("building the drop-in")?;
    let dir = unit_dir.join("neuroos-monitor.service.d");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join("folders.conf");
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    println!("Wrote {}", path.display());
    Ok(())
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
            "--kernel-dropin" => {
                let (Some(config), Some(unit_dir)) = (args.get(i + 1), args.get(i + 2)) else {
                    anyhow::bail!("--kernel-dropin requires <CONFIG> <UNIT_DIR>");
                };
                return write_kernel_dropin(&PathBuf::from(config), &PathBuf::from(unit_dir));
            }
            "--monitor-dropin" => {
                let (Some(config), Some(unit_dir)) = (args.get(i + 1), args.get(i + 2)) else {
                    anyhow::bail!("--monitor-dropin requires <CONFIG> <UNIT_DIR>");
                };
                return write_monitor_dropin(&PathBuf::from(config), &PathBuf::from(unit_dir));
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
