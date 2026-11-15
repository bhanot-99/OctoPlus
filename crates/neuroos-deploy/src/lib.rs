//! Generator for Semantic-NeuroOS systemd unit files, sysusers, and tmpfiles (P1-S01, P1-S02).
//!
//! Generates hardened systemd units from the socket map data in `neuroos-common::socketmap`.

use std::fs;
use std::path::{Path, PathBuf};

use neuroos_common::paths::Part;
use neuroos_common::socketmap;
use serde::Deserialize;

// PRD §6.2 hard limits (MemoryMax constants).
// Citing PRD §6.2 table rows:

/// Hard limit (MemoryMax) for C1 monitor from PRD §6.2 row C1 monitor (48 MiB).
pub const MEMORY_MAX_MONITOR: &str = "48M";

/// Hard limit (MemoryMax) for C2 voice from PRD §6.2 row C2 voice (768 MiB).
pub const MEMORY_MAX_VOICE: &str = "768M";

/// Hard limit (MemoryMax) for C3 storage from PRD §6.2 row C3 storage (320 MiB).
pub const MEMORY_MAX_STORAGE: &str = "320M";

/// Hard limit (MemoryMax) for C4 inference from PRD §6.2 row C4 inference (3,220 MiB).
pub const MEMORY_MAX_INFERENCE: &str = "3220M";

/// Hard limit (MemoryMax) for C5 knowledge from PRD §6.2 row C5 knowledge (96 MiB).
pub const MEMORY_MAX_KNOWLEDGE: &str = "96M";

/// Hard limit (MemoryMax) for C6 kernel from PRD §6.2 row C6 kernel (64 MiB).
pub const MEMORY_MAX_KERNEL: &str = "64M";

/// Hard limit (MemoryMax) for healthd from PRD §6.2 row healthd (32 MiB).
pub const MEMORY_MAX_HEALTHD: &str = "32M";

/// Hard limit (MemoryMax) for C7 fetcher from PRD §6.2 row C7 fetcher (48 MiB).
pub const MEMORY_MAX_FETCHER: &str = "48M";

/// Returns the MemoryMax limit for a given Part.
#[must_use]
pub fn memory_max_for_part(part: Part) -> &'static str {
    match part {
        Part::Monitor => MEMORY_MAX_MONITOR,
        Part::Voice => MEMORY_MAX_VOICE,
        Part::Storage => MEMORY_MAX_STORAGE,
        Part::Inference => MEMORY_MAX_INFERENCE,
        Part::Knowledge => MEMORY_MAX_KNOWLEDGE,
        Part::Kernel => MEMORY_MAX_KERNEL,
        Part::Healthd => MEMORY_MAX_HEALTHD,
        Part::Fetcher => MEMORY_MAX_FETCHER,
    }
}

/// Errors during unit generation.
#[derive(Debug, thiserror::Error)]
pub enum DeployError {
    /// IO error reading or writing files.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// Failed to parse manifest TOML.
    #[error("failed to parse model manifest: {0}")]
    ManifestParse(#[from] toml::de::Error),
    /// Missing LLM model in manifest.
    #[error("no model with role 'llm' found in manifest {0}")]
    MissingLlmModel(String),
}

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(rename = "model", default)]
    models: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    role: String,
    file: String,
}

/// Reads `models/manifest.toml` and returns the file path of the model with role "llm".
///
/// # Errors
/// Returns [`DeployError`] if the file cannot be read or parsed, or lacks an LLM entry.
pub fn read_llm_model_file(manifest_path: &Path) -> Result<String, DeployError> {
    let content = fs::read_to_string(manifest_path)?;
    let manifest: Manifest = toml::from_str(&content)?;
    let entry = manifest
        .models
        .into_iter()
        .find(|m| m.role == "llm")
        .ok_or_else(|| DeployError::MissingLlmModel(manifest_path.display().to_string()))?;
    Ok(entry.file)
}

/// Start order from Architecture §13:
/// healthd → storage, inference → monitor, kernel → knowledge → voice → fetcher.
fn prior_units_in_start_order(part: Part) -> Vec<&'static str> {
    match part {
        Part::Healthd => vec![],
        Part::Storage | Part::Inference => vec!["neuroos-healthd.service"],
        Part::Monitor | Part::Kernel => vec![
            "neuroos-healthd.service",
            "neuroos-storage.service",
            "neuroos-inference.service",
        ],
        Part::Knowledge => vec![
            "neuroos-healthd.service",
            "neuroos-storage.service",
            "neuroos-inference.service",
            "neuroos-monitor.service",
            "neuroos-kernel.service",
        ],
        Part::Voice => vec![
            "neuroos-healthd.service",
            "neuroos-storage.service",
            "neuroos-inference.service",
            "neuroos-monitor.service",
            "neuroos-kernel.service",
            "neuroos-knowledge.service",
        ],
        Part::Fetcher => vec![
            "neuroos-healthd.service",
            "neuroos-storage.service",
            "neuroos-inference.service",
            "neuroos-monitor.service",
            "neuroos-kernel.service",
            "neuroos-knowledge.service",
            "neuroos-voice.service",
        ],
    }
}

fn part_description(part: Part) -> &'static str {
    match part {
        Part::Monitor => "Semantic-NeuroOS Context Monitor",
        Part::Voice => "Semantic-NeuroOS Voice Interface",
        Part::Storage => "Semantic-NeuroOS Storage Engine",
        Part::Inference => "Semantic-NeuroOS LLM Inference Service",
        Part::Knowledge => "Semantic-NeuroOS Knowledge and Nudge Engine",
        Part::Kernel => "Semantic-NeuroOS Safety Kernel",
        Part::Healthd => "Semantic-NeuroOS Health Daemon",
        Part::Fetcher => "Semantic-NeuroOS Network Fetcher",
    }
}

/// Generates the systemd user service content for `part`.
#[must_use]
pub fn generate_user_service(part: Part, llm_model_file: &str) -> String {
    let desc = part_description(part);
    let mut after = vec!["neuroos-runtime.service"];
    after.extend(prior_units_in_start_order(part));
    let after_line = after.join(" ");

    let mem_max = memory_max_for_part(part);

    let mut out = String::new();
    out.push_str("[Unit]\n");
    out.push_str(&format!("Description={desc}\n"));
    out.push_str("PartOf=neuroos.target\n");
    out.push_str("Requires=neuroos-runtime.service\n");
    out.push_str(&format!("After={after_line}\n\n"));

    out.push_str("[Service]\n");
    out.push_str("PrivateNetwork=true\n");
    out.push_str("InaccessiblePaths=-/run/systemd/resolve\n");
    out.push_str("InaccessiblePaths=-/run/dbus/system_bus_socket\n");
    out.push_str("NoNewPrivileges=true\n");
    out.push_str("ProtectSystem=strict\n");
    out.push_str("ProtectHome=tmpfs\n");
    out.push_str("BindReadOnlyPaths=%h/.local/opt/neuroos\n");
    out.push_str("BindReadOnlyPaths=-%h/.config/neuroos/config.toml\n");

    // Decision 4: specific persistent data paths.
    match part {
        Part::Healthd => {
            out.push_str("BindPaths=%h/.local/share/neuroos/soak\n");
        }
        Part::Storage => {
            out.push_str("BindPaths=%h/.local/share/neuroos/storage\n");
        }
        _ => {}
    }

    // Decision 5: fetcher isolation.
    match part {
        Part::Kernel => {
            out.push_str("TemporaryFileSystem=/run/neuroos-fetcher\n");
            out.push_str("BindPaths=-/run/neuroos-fetcher/fetch\n");
        }
        Part::Healthd => {
            out.push_str("TemporaryFileSystem=/run/neuroos-fetcher\n");
            out.push_str("BindPaths=-/run/neuroos-fetcher/health\n");
        }
        _ => {
            out.push_str("InaccessiblePaths=-/run/neuroos-fetcher\n");
        }
    }

    // Decision 1 & 2: runtime root and socket bindings.
    out.push_str("TemporaryFileSystem=%t/neuroos\n");
    for bind_path in socketmap::user_runtime_visible_folders(part) {
        out.push_str(&format!("BindPaths={bind_path}\n"));
    }

    out.push_str("PrivateTmp=true\n");

    if part == Part::Voice {
        out.push_str("# C2: Architecture §8.1 permits PrivateDevices and RestrictRealtime exemptions for audio;\n");
        out.push_str("# kept fully hardened for Phase 1 (no audio yet)\n");
    }

    out.push_str("PrivateDevices=true\n");
    out.push_str("ProtectKernelTunables=true\n");
    out.push_str("ProtectKernelModules=true\n");
    out.push_str("ProtectKernelLogs=true\n");
    out.push_str("ProtectControlGroups=true\n");
    out.push_str("ProtectClock=true\n");
    out.push_str("ProtectHostname=true\n");
    out.push_str("RestrictNamespaces=true\n");
    out.push_str("RestrictRealtime=true\n");
    out.push_str("RestrictSUIDSGID=true\n");
    out.push_str("RestrictAddressFamilies=AF_UNIX\n");
    out.push_str("LockPersonality=true\n");
    out.push_str("MemoryDenyWriteExecute=true\n");
    out.push_str("SystemCallArchitectures=native\n");
    out.push_str("SystemCallFilter=@system-service landlock_create_ruleset landlock_add_rule landlock_restrict_self\n");
    out.push_str("CapabilityBoundingSet=\n");
    out.push_str("AmbientCapabilities=\n");
    out.push_str("LimitCORE=0\n");
    out.push_str("UMask=0077\n");
    out.push_str(&format!("MemoryMax={mem_max}\n"));
    out.push_str("Restart=on-failure\n");
    out.push_str("RestartSec=2s\n");

    // ExecStartPre and ExecStart
    match part {
        Part::Inference => {
            out.push_str("ExecStartPre=/usr/bin/sha256sum --check --quiet %h/.local/opt/neuroos/models/SHA256SUMS\n");
            out.push_str(&format!(
                "ExecStart=%h/.local/opt/neuroos/bin/neuroos-inference \
                --socket %t/neuroos/inference/inference/inference.sock \
                --health-socket %t/neuroos/inference/health/health.sock \
                --allow-unit neuroos-knowledge.service \
                --allow-unit neuroos-healthd.service \
                --model %h/.local/opt/neuroos/models/{llm_model_file} \
                --threads 8 --ctx 2048 --batch 512\n"
            ));
        }
        Part::Voice => {
            out.push_str(
                "ExecStart=%h/.local/opt/neuroos/bin/neuroos-voice \
                --socket %t/neuroos/voice/voice/voice.sock \
                --socket %t/neuroos/voice/control/control.sock \
                --health-socket %t/neuroos/voice/health/health.sock \
                --allow-unit neuroos-knowledge.service \
                --allow-unit neuroos-kernel.service \
                --allow-unit neuroos-healthd.service\n",
            );
        }
        _ => {
            out.push_str(&format!(
                "ExecStart=%h/.local/opt/neuroos/bin/neuroos-{}\n",
                part.name()
            ));
        }
    }

    out.push_str("\n[Install]\n");
    out.push_str("WantedBy=neuroos.target\n");

    out
}

/// Generates the oneshot `neuroos-runtime.service` unit file.
#[must_use]
pub fn generate_runtime_service() -> String {
    let mut out = String::new();
    out.push_str("[Unit]\n");
    out.push_str("Description=Semantic-NeuroOS Runtime Directories\n");
    out.push_str("PartOf=neuroos.target\n\n");

    out.push_str("[Service]\n");
    out.push_str("Type=oneshot\n");
    out.push_str("RemainAfterExit=yes\n");
    out.push_str("PrivateNetwork=true\n");
    out.push_str("InaccessiblePaths=-/run/systemd/resolve\n");
    out.push_str("InaccessiblePaths=-/run/dbus/system_bus_socket\n");
    out.push_str("NoNewPrivileges=true\n");
    out.push_str("ProtectSystem=strict\n");
    out.push_str("ProtectHome=true\n");
    out.push_str("PrivateTmp=true\n");
    out.push_str("PrivateDevices=true\n");
    out.push_str("ProtectKernelTunables=true\n");
    out.push_str("ProtectKernelModules=true\n");
    out.push_str("ProtectKernelLogs=true\n");
    out.push_str("ProtectControlGroups=true\n");
    out.push_str("ProtectClock=true\n");
    out.push_str("ProtectHostname=true\n");
    out.push_str("RestrictNamespaces=true\n");
    out.push_str("RestrictRealtime=true\n");
    out.push_str("RestrictSUIDSGID=true\n");
    out.push_str("RestrictAddressFamilies=AF_UNIX\n");
    out.push_str("LockPersonality=true\n");
    out.push_str("MemoryDenyWriteExecute=true\n");
    out.push_str("SystemCallArchitectures=native\n");
    out.push_str("SystemCallFilter=@system-service\n");
    out.push_str("CapabilityBoundingSet=\n");
    out.push_str("AmbientCapabilities=\n");
    out.push_str("LimitCORE=0\n");
    out.push_str("UMask=0077\n");

    out.push_str("ExecStart=/usr/bin/install -d -m 0700 %t/neuroos\n");
    out.push_str("ExecStart=/usr/bin/install -d -m 0700 %t/neuroos/monitor %t/neuroos/voice %t/neuroos/storage %t/neuroos/inference %t/neuroos/knowledge %t/neuroos/kernel %t/neuroos/healthd\n");
    out.push_str("ExecStart=/usr/bin/install -d -m 0700 %t/neuroos/monitor/stream %t/neuroos/monitor/control %t/neuroos/monitor/health\n");
    out.push_str("ExecStart=/usr/bin/install -d -m 0700 %t/neuroos/voice/voice %t/neuroos/voice/control %t/neuroos/voice/health\n");
    out.push_str("ExecStart=/usr/bin/install -d -m 0700 %t/neuroos/storage/storage %t/neuroos/storage/health\n");
    out.push_str("ExecStart=/usr/bin/install -d -m 0700 %t/neuroos/inference/inference %t/neuroos/inference/health\n");
    out.push_str("ExecStart=/usr/bin/install -d -m 0700 %t/neuroos/knowledge/knowledge %t/neuroos/knowledge/health\n");
    out.push_str(
        "ExecStart=/usr/bin/install -d -m 0700 %t/neuroos/kernel/kernel %t/neuroos/kernel/health\n",
    );
    out.push_str("ExecStart=/usr/bin/install -d -m 0700 %t/neuroos/healthd/healthd\n");

    out.push_str("\n[Install]\n");
    out.push_str("WantedBy=neuroos.target\n");

    out
}

/// Generates the `neuroos.target` unit file.
#[must_use]
pub fn generate_user_target() -> String {
    let mut out = String::new();
    out.push_str("[Unit]\n");
    out.push_str("Description=Semantic-NeuroOS Target\n");
    out.push_str(
        "Wants=neuroos-healthd.service neuroos-storage.service neuroos-inference.service \
        neuroos-monitor.service neuroos-kernel.service neuroos-knowledge.service \
        neuroos-voice.service\n\n",
    );
    out.push_str("[Install]\n");
    out.push_str("WantedBy=default.target\n");
    out
}

/// Generates the system-scope `neuroos-fetcher.service` unit file.
#[must_use]
pub fn generate_fetcher_system_service() -> String {
    let mut out = String::new();
    out.push_str("[Unit]\n");
    out.push_str("Description=Semantic-NeuroOS Network Fetcher\n");
    out.push_str("After=network.target\n\n");

    out.push_str("[Service]\n");
    out.push_str("User=neuroos-fetcher\n");
    out.push_str("Group=neuroos\n");
    out.push_str("InaccessiblePaths=-/run/dbus/system_bus_socket\n");
    out.push_str("NoNewPrivileges=true\n");
    out.push_str("ProtectSystem=strict\n");
    out.push_str("ProtectHome=true\n");
    out.push_str("ReadWritePaths=/run/neuroos-fetcher /var/spool/neuroos-fetcher\n");
    out.push_str("PrivateTmp=true\n");
    out.push_str("PrivateDevices=true\n");
    out.push_str("ProtectKernelTunables=true\n");
    out.push_str("ProtectKernelModules=true\n");
    out.push_str("ProtectKernelLogs=true\n");
    out.push_str("ProtectControlGroups=true\n");
    out.push_str("ProtectClock=true\n");
    out.push_str("ProtectHostname=true\n");
    out.push_str("RestrictNamespaces=true\n");
    out.push_str("RestrictRealtime=true\n");
    out.push_str("RestrictSUIDSGID=true\n");
    out.push_str("RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6\n");
    out.push_str("LockPersonality=true\n");
    out.push_str("MemoryDenyWriteExecute=true\n");
    out.push_str("SystemCallArchitectures=native\n");
    out.push_str("SystemCallFilter=@system-service landlock_create_ruleset landlock_add_rule landlock_restrict_self\n");
    out.push_str("CapabilityBoundingSet=\n");
    out.push_str("AmbientCapabilities=\n");
    out.push_str("LimitCORE=0\n");
    out.push_str("UMask=0027\n");
    out.push_str(&format!("MemoryMax={MEMORY_MAX_FETCHER}\n"));
    out.push_str("Restart=on-failure\n");
    out.push_str("RestartSec=2s\n");
    out.push_str("ExecStart=/opt/neuroos/bin/neuroos-fetcher\n\n");

    out.push_str("[Install]\n");
    out.push_str("WantedBy=multi-user.target\n");

    out
}

/// Generates the `sysusers.d/neuroos.conf` file content.
#[must_use]
pub fn generate_sysusers_conf() -> String {
    let mut out = String::new();
    out.push_str("# Semantic-NeuroOS fetcher system user and group (Architecture §13, FR-HLT-02, FR-STO-10).\n");
    out.push_str("g neuroos -\n");
    out.push_str("u neuroos-fetcher -:neuroos \"Semantic-NeuroOS Fetcher\" /run/neuroos-fetcher /usr/sbin/nologin\n");
    out
}

/// Generates the `tmpfiles.d/neuroos-fetcher.conf` file content.
#[must_use]
pub fn generate_tmpfiles_conf() -> String {
    let mut out = String::new();
    out.push_str(
        "# Semantic-NeuroOS fetcher runtime and spool folders (Architecture §7.1, §13).\n",
    );
    out.push_str("d /run/neuroos-fetcher 0750 neuroos-fetcher neuroos -\n");
    out.push_str("d /run/neuroos-fetcher/fetch 2770 neuroos-fetcher neuroos -\n");
    out.push_str("d /run/neuroos-fetcher/health 2770 neuroos-fetcher neuroos -\n");
    out.push_str("d /var/spool/neuroos-fetcher 2770 neuroos-fetcher neuroos -\n");
    out
}

/// Represents the generated files in memory (relative path -> file content).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    /// Relative path inside deploy output directory.
    pub rel_path: PathBuf,
    /// UTF-8 file content.
    pub content: String,
}

/// Generates all deployment files in memory.
///
/// # Errors
/// Returns [`DeployError`] if `manifest_path` cannot be read or parsed.
pub fn generate_all_files(manifest_path: &Path) -> Result<Vec<GeneratedFile>, DeployError> {
    let llm_model_file = read_llm_model_file(manifest_path)?;

    let mut files = Vec::new();

    // 1. User service units
    let user_parts = [
        Part::Healthd,
        Part::Storage,
        Part::Inference,
        Part::Monitor,
        Part::Kernel,
        Part::Knowledge,
        Part::Voice,
    ];

    for part in user_parts {
        let content = generate_user_service(part, &llm_model_file);
        let rel_path =
            PathBuf::from("systemd/user").join(format!("neuroos-{}.service", part.name()));
        files.push(GeneratedFile { rel_path, content });
    }

    // 2. Runtime service
    files.push(GeneratedFile {
        rel_path: PathBuf::from("systemd/user/neuroos-runtime.service"),
        content: generate_runtime_service(),
    });

    // 3. User target
    files.push(GeneratedFile {
        rel_path: PathBuf::from("systemd/user/neuroos.target"),
        content: generate_user_target(),
    });

    // 4. Fetcher system unit
    files.push(GeneratedFile {
        rel_path: PathBuf::from("systemd/system/neuroos-fetcher.service"),
        content: generate_fetcher_system_service(),
    });

    // 5. sysusers.d
    files.push(GeneratedFile {
        rel_path: PathBuf::from("sysusers.d/neuroos.conf"),
        content: generate_sysusers_conf(),
    });

    // 6. tmpfiles.d
    files.push(GeneratedFile {
        rel_path: PathBuf::from("tmpfiles.d/neuroos-fetcher.conf"),
        content: generate_tmpfiles_conf(),
    });

    Ok(files)
}

/// Writes all generated files to `out_dir`.
///
/// # Errors
/// Returns [`DeployError`] if reading manifest or writing files fails.
pub fn write_all(out_dir: &Path, manifest_path: &Path) -> Result<Vec<PathBuf>, DeployError> {
    let files = generate_all_files(manifest_path)?;
    let mut written = Vec::new();

    for file in files {
        let full_path = out_dir.join(&file.rel_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(parent, fs::Permissions::from_mode(0o755))?;
            }
        }
        fs::write(&full_path, file.content)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&full_path, fs::Permissions::from_mode(0o644))?;
        }
        written.push(full_path);
    }

    Ok(written)
}
