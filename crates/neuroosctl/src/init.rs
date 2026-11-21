//! First-run setup and initialization command (`neuroosctl init`, Phase 10, P10-S03).

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::style::Styler;

/// Options for `neuroosctl init`.
#[derive(Debug, Clone)]
pub struct InitOptions {
    pub force: bool,
    pub skip_models: bool,
    pub yes: bool,
    pub config_dir: Option<PathBuf>,
}

pub async fn run_init(opts: InitOptions, s: Styler) -> i32 {
    println!(
        "{}",
        s.bold("=== Semantic-NeuroOS First-Run Setup (v1.0.0) ===")
    );

    let config_dir = match opts.config_dir {
        Some(d) => d,
        None => match std::env::var_os("HOME") {
            Some(h) => PathBuf::from(h).join(".config/neuroos"),
            None => PathBuf::from(".config/neuroos"),
        },
    };

    if let Err(e) = fs::create_dir_all(&config_dir) {
        eprintln!(
            "error: failed to create config directory {}: {e}",
            config_dir.display()
        );
        return 1;
    }
    let _ = fs::set_permissions(&config_dir, fs::Permissions::from_mode(0o700));

    // 1. Cryptographic Keys
    println!("\n{}", s.bold("1. Provisioning Cryptographic Keys..."));
    if let Err(e) = provision_keys(&config_dir, opts.force) {
        eprintln!("error: key provisioning failed: {e}");
        return 1;
    }

    // 2. Configuration Templates
    println!("\n{}", s.bold("2. Provisioning Configuration Files..."));
    if let Err(e) = provision_configs(&config_dir, opts.force, &s) {
        eprintln!("error: config provisioning failed: {e}");
        return 1;
    }

    // 3. User Group Verification
    println!("\n{}", s.bold("3. Verifying Group Membership..."));
    verify_group_membership(&s);

    // 4. Model Verification
    if !opts.skip_models {
        println!("\n{}", s.bold("4. Verifying Model Integrity..."));
        verify_models(&s);
    } else {
        println!(
            "\n{}",
            s.dim("4. Skipping model verification (--skip-models set).")
        );
    }

    // 5. Security & Encryption Advisory (RR-1)
    print_encryption_advisory(&s);

    // 6. Systemd Target Activation
    println!("{}", s.bold("6. Systemd Service Activation..."));
    activate_services(opts.yes, &s);

    println!("\n{}", s.ok("✓ First-run setup completed successfully."));
    println!(
        "Run {} to check system health.",
        s.bold("neuroosctl status")
    );
    0
}

fn provision_keys(config_dir: &Path, force: bool) -> Result<(), String> {
    let hmac_path = config_dir.join("hmac.key");
    if !hmac_path.exists() || force {
        let mut key = [0u8; 32];
        getrandom::fill(&mut key).map_err(|e| format!("getrandom failed: {e}"))?;
        fs::write(&hmac_path, key).map_err(|e| format!("writing {}: {e}", hmac_path.display()))?;
        fs::set_permissions(&hmac_path, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("setting permissions on {}: {e}", hmac_path.display()))?;
        println!("  Generated {} (mode 0600)", hmac_path.display());
    } else {
        println!("  Existing {} found (retained)", hmac_path.display());
    }

    let fetch_path = config_dir.join("fetch.key");
    if !fetch_path.exists() || force {
        let sys_fetch = Path::new("/etc/neuroos/fetch.key");
        if sys_fetch.exists() && fs::read(sys_fetch).is_ok() {
            let _ = fs::copy(sys_fetch, &fetch_path);
            let _ = fs::set_permissions(&fetch_path, fs::Permissions::from_mode(0o600));
            println!("  Copied system fetch.key into {}", fetch_path.display());
        } else {
            let mut key = [0u8; 32];
            getrandom::fill(&mut key).map_err(|e| format!("getrandom failed: {e}"))?;
            fs::write(&fetch_path, key)
                .map_err(|e| format!("writing {}: {e}", fetch_path.display()))?;
            fs::set_permissions(&fetch_path, fs::Permissions::from_mode(0o600))
                .map_err(|e| format!("setting permissions on {}: {e}", fetch_path.display()))?;
            println!("  Generated {} (mode 0600)", fetch_path.display());
        }
    } else {
        println!("  Existing {} found (retained)", fetch_path.display());
    }

    Ok(())
}

fn provision_configs(config_dir: &Path, force: bool, s: &Styler) -> Result<(), String> {
    let config_path = config_dir.join("config.toml");
    if !config_path.exists() || force {
        let default_config = r#"# Semantic-NeuroOS Configuration (v1.0.0)
# Rules.md §5: Unknown keys are fatal. All sections are optional with safe defaults.

[kernel]
# Build folders allowed for cleanup operations (empty by default, ADR-0010)
cleanup_build_folders = []

[monitor]
# Context monitor sample intervals (seconds)
cpu_interval_s = 5
disk_interval_s = 60
battery_interval_s = 60
thermal_interval_s = 10
idle_timeout_s = 60

[nudges]
# Nudge engine master switch and quiet hours (OQ-03: 22:00 to 08:00, minutes after midnight)
enabled = true
quiet_start_min = 1320
quiet_end_min = 480
"#;
        fs::write(&config_path, default_config)
            .map_err(|e| format!("writing {}: {e}", config_path.display()))?;
        let _ = fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600));
        println!("  Created {} (mode 0600)", config_path.display());
    } else {
        println!("  Existing {} found (retained)", config_path.display());
        if let Err(e) = neuroos_common::config::Config::load_from(&config_path) {
            println!(
                "  {} Existing {} contains invalid configuration: {e}",
                s.degraded("WARNING:"),
                config_path.display()
            );
            println!(
                "  Fix the file or run {} to overwrite with defaults.",
                s.bold("neuroosctl init --force")
            );
        }
    }

    let nudges_path = config_dir.join("nudges.toml");
    if !nudges_path.exists() || force {
        let default_nudges = r#"# Semantic-NeuroOS Nudge Rules Overrides & Additions (v1.0.0)
# Defaults for disk, battery, thermal, session, and health rules are compiled in (design.md §8.5).
# To override a default rule or add a custom rule, define [rule.<id>].

# Example override:
# [rule.disk_nearly_full]
# fire_at = 90
# cooldown = "24h"
"#;
        fs::write(&nudges_path, default_nudges)
            .map_err(|e| format!("writing {}: {e}", nudges_path.display()))?;
        let _ = fs::set_permissions(&nudges_path, fs::Permissions::from_mode(0o600));
        println!("  Created {} (mode 0600)", nudges_path.display());
    } else {
        println!("  Existing {} found (retained)", nudges_path.display());
    }

    Ok(())
}

fn verify_group_membership(s: &Styler) {
    let output = Command::new("id").arg("-Gn").output();
    let in_group = match output {
        Ok(out) => {
            let groups = String::from_utf8_lossy(&out.stdout);
            groups.split_whitespace().any(|g| g == "neuroos")
        }
        Err(_) => false,
    };

    if in_group {
        println!("  User belongs to group 'neuroos'. (OK)");
    } else {
        println!(
            "  {} Current user is not in group 'neuroos'.",
            s.degraded("WARNING:")
        );
        println!(
            "  To enable fetcher communication, run: {}",
            s.bold("sudo usermod -aG neuroos $USER")
        );
        println!("  (A session logout/login is required for group changes to take effect).");
    }
}

fn verify_models(s: &Styler) {
    let home = std::env::var("HOME").unwrap_or_default();
    let user_sums = PathBuf::from(&home).join(".local/opt/neuroos/models/SHA256SUMS");
    let opt_sums = PathBuf::from("/opt/neuroos/models/SHA256SUMS");

    let sums_file = if user_sums.exists() {
        Some(user_sums)
    } else if opt_sums.exists() {
        Some(opt_sums)
    } else {
        None
    };

    if let Some(sums) = sums_file {
        println!("  Checking models against {}...", sums.display());
        let parent = sums.parent().unwrap_or(Path::new("."));
        let status = Command::new("sha256sum")
            .arg("--check")
            .arg("--quiet")
            .arg(&sums)
            .current_dir(parent)
            .status();

        match status {
            Ok(st) if st.success() => {
                println!("  Model checksums verified. (OK)");
            }
            _ => {
                println!(
                    "  {} Some model checksums failed verification.",
                    s.degraded("WARNING:")
                );
                println!(
                    "  Run {} to re-download missing weights.",
                    s.bold("just fetch-models")
                );
            }
        }
    } else {
        println!(
            "  {} SHA256SUMS not found in ~/.local/opt/neuroos/models/ or /opt/neuroos/models/.",
            s.degraded("NOTE:")
        );
        println!(
            "  Run {} to download and verify models.",
            s.bold("just fetch-models")
        );
    }
}

fn print_encryption_advisory(s: &Styler) {
    println!(
        "\n{}",
        s.bold("================================================================================")
    );
    println!(
        " {}",
        s.degraded("SECURITY ADVISORY: Full-Disk Encryption Recommended (security.md RR-1)")
    );
    println!(
        "{}",
        s.bold("================================================================================")
    );
    println!(" Semantic-NeuroOS stores conversation history, memory embeddings, and");
    println!(" sensor telemetry locally on disk with strict user-only permissions (0700/0600).");
    println!(" It does NOT defend against malicious code running under the same user UID.");
    println!(" To safeguard your data against offline theft and same-user processes, ensure");
    println!(" your home and root filesystems are protected by Linux Full-Disk Encryption (LUKS).");
    println!(
        "{}\n",
        s.bold("================================================================================")
    );
}

fn activate_services(non_interactive: bool, s: &Styler) {
    if !Command::new("systemctl").arg("--version").output().is_ok() {
        println!("  systemctl not found; skipping service activation.");
        return;
    }

    println!("  Reloading user systemd daemon...");
    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();

    if non_interactive {
        println!("  Enabling and starting neuroos.target (--yes)...");
        let _ = Command::new("systemctl")
            .args(["--user", "enable", "--now", "neuroos.target"])
            .status();
    } else {
        println!("  Enable and start neuroos.target now?");
        println!(
            "  Run: {}",
            s.bold("systemctl --user enable --now neuroos.target")
        );
    }
}
