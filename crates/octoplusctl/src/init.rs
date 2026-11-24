//! First-run setup and initialization command (`octoplusctl init`, Phase 10, P10-S03).

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::style::Styler;

/// Where the installer puts the fetcher's approval key (C7 reads it; C6 needs the same bytes).
pub const SYSTEM_FETCH_KEY: &str = "/etc/octoplus/fetch.key";

/// Options for `octoplusctl init`.
#[derive(Debug, Clone)]
pub struct InitOptions {
    /// Replace existing keys and configuration files.
    pub force: bool,
    /// Do not hash-check the model files.
    pub skip_models: bool,
    /// Enable the services without asking.
    pub yes: bool,
    /// Configuration folder; `~/.config/octoplus` when not set.
    pub config_dir: Option<PathBuf>,
    /// The system fetch key; [`SYSTEM_FETCH_KEY`] when not set.
    pub system_fetch_key: Option<PathBuf>,
}

pub async fn run_init(opts: InitOptions, s: Styler) -> i32 {
    println!(
        "{}",
        s.bold("=== OctoPlus First-Run Setup (v1.0.0) ===")
    );

    let config_dir = match opts.config_dir {
        Some(d) => d,
        None => match std::env::var_os("HOME") {
            Some(h) => PathBuf::from(h).join(".config/octoplus"),
            None => PathBuf::from(".config/octoplus"),
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
    let sys_fetch = opts
        .system_fetch_key
        .clone()
        .unwrap_or_else(|| PathBuf::from(SYSTEM_FETCH_KEY));
    if let Err(e) = provision_keys(&config_dir, opts.force, &sys_fetch) {
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
        s.bold("octoplusctl status")
    );
    0
}

/// Writes a secret file that is mode 0600 from the moment it exists (audit A-15: writing
/// then chmod-ing left each new key world-readable for a moment).
fn write_secret(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if path.exists() {
        fs::remove_file(path).map_err(|e| format!("replacing {}: {e}", path.display()))?;
    }
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| format!("creating {}: {e}", path.display()))?;
    f.write_all(bytes)
        .and_then(|()| f.sync_all())
        .map_err(|e| format!("writing {}: {e}", path.display()))
}

fn random_key() -> Result<[u8; 32], String> {
    let mut key = [0u8; 32];
    getrandom::fill(&mut key).map_err(|e| format!("getrandom failed: {e}"))?;
    Ok(key)
}

/// The command that installs this user's fetch key for C7, printed when they differ.
fn install_key_hint(fetch_path: &Path, sys_fetch: &Path) -> String {
    format!(
        "sudo install -m 0640 -o root -g octoplus {} {} && sudo systemctl restart octoplus-fetcher",
        fetch_path.display(),
        sys_fetch.display()
    )
}

/// Provisions `hmac.key` and `fetch.key`. C6 signs fetch approvals with the user's
/// `fetch.key` and C7 verifies them with the system one, so they must hold the same bytes:
/// the system key is copied when it is readable; init fails (instead of inventing a key
/// that can never match) when it exists but is not readable; and it warns with the fix
/// when there is no system key yet or the two differ (audit A-15).
fn provision_keys(config_dir: &Path, force: bool, sys_fetch: &Path) -> Result<(), String> {
    let hmac_path = config_dir.join("hmac.key");
    if !hmac_path.exists() || force {
        write_secret(&hmac_path, &random_key()?)?;
        println!("  Generated {} (mode 0600)", hmac_path.display());
    } else {
        println!("  Existing {} found (retained)", hmac_path.display());
    }

    let fetch_path = config_dir.join("fetch.key");
    let system_key = if sys_fetch.exists() {
        match fs::read(sys_fetch) {
            Ok(bytes) if bytes.len() == 32 => Some(bytes),
            Ok(bytes) => {
                return Err(format!(
                    "{} has {} bytes, expected 32",
                    sys_fetch.display(),
                    bytes.len()
                ));
            }
            Err(e) => {
                return Err(format!(
                    "cannot read {}: {e}. Add yourself to the 'octoplus' group \
                     (sudo usermod -aG octoplus $USER), log in again, then rerun init.",
                    sys_fetch.display()
                ));
            }
        }
    } else {
        None
    };

    if !fetch_path.exists() || force {
        if let Some(bytes) = &system_key {
            write_secret(&fetch_path, bytes)?;
            println!(
                "  Copied the system fetch key into {}",
                fetch_path.display()
            );
        } else {
            write_secret(&fetch_path, &random_key()?)?;
            println!("  Generated {} (mode 0600)", fetch_path.display());
        }
    } else {
        println!("  Existing {} found (retained)", fetch_path.display());
    }

    let user_key =
        fs::read(&fetch_path).map_err(|e| format!("reading {}: {e}", fetch_path.display()))?;
    match &system_key {
        None => println!(
            "  warning: {} is missing, so web lookups stay off until it is installed:\n    {}",
            sys_fetch.display(),
            install_key_hint(&fetch_path, sys_fetch)
        ),
        Some(sys) if *sys != user_key => println!(
            "  warning: {} and {} differ, so every web lookup would be refused. Rerun \
             'octoplusctl init --force' to copy the system key, or install yours:\n    {}",
            fetch_path.display(),
            sys_fetch.display(),
            install_key_hint(&fetch_path, sys_fetch)
        ),
        Some(_) => println!("  The fetch key matches the fetcher's."),
    }
    Ok(())
}

fn provision_configs(config_dir: &Path, force: bool, s: &Styler) -> Result<(), String> {
    let config_path = config_dir.join("config.toml");
    if !config_path.exists() || force {
        let default_config = r#"# OctoPlus Configuration (v1.0.0)
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
        if let Err(e) = octoplus_common::config::Config::load_from(&config_path) {
            println!(
                "  {} Existing {} contains invalid configuration: {e}",
                s.degraded("WARNING:"),
                config_path.display()
            );
            println!(
                "  Fix the file or run {} to overwrite with defaults.",
                s.bold("octoplusctl init --force")
            );
        }
    }

    let nudges_path = config_dir.join("nudges.toml");
    if !nudges_path.exists() || force {
        let default_nudges = r#"# OctoPlus Nudge Rules Overrides & Additions (v1.0.0)
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
            groups.split_whitespace().any(|g| g == "octoplus")
        }
        Err(_) => false,
    };

    if in_group {
        println!("  User belongs to group 'octoplus'. (OK)");
    } else {
        println!(
            "  {} Current user is not in group 'octoplus'.",
            s.degraded("WARNING:")
        );
        println!(
            "  To enable fetcher communication, run: {}",
            s.bold("sudo usermod -aG octoplus $USER")
        );
        println!("  (A session logout/login is required for group changes to take effect).");
    }
}

fn verify_models(s: &Styler) {
    let home = std::env::var("HOME").unwrap_or_default();
    let user_sums = PathBuf::from(&home).join(".local/opt/octoplus/models/SHA256SUMS");
    let opt_sums = PathBuf::from("/opt/octoplus/models/SHA256SUMS");

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
            "  {} SHA256SUMS not found in ~/.local/opt/octoplus/models/ or /opt/octoplus/models/.",
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
    println!(" OctoPlus stores conversation history, memory embeddings, and");
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
        println!("  Enabling and starting octoplus.target (--yes)...");
        let _ = Command::new("systemctl")
            .args(["--user", "enable", "--now", "octoplus.target"])
            .status();
    } else {
        println!("  Enable and start octoplus.target now?");
        println!(
            "  Run: {}",
            s.bold("systemctl --user enable --now octoplus.target")
        );
    }
}
