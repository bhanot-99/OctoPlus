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
    /// A configured path cannot be put in a unit file safely.
    #[error("path not allowed in a unit file: {0}")]
    BadPath(String),
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

/// C6's extra paths (ADR-0010 decision 3): its own key and logs, the dialog's
/// display and the notification bus, and the cleanup roots its skill children
/// work on. Configured build folders are added by a drop-in
/// ([`generate_kernel_cleanup_dropin`]), since they come from the user's config.
fn push_kernel_paths(out: &mut String) {
    out.push_str("BindReadOnlyPaths=%h/.config/neuroos/hmac.key\n");
    out.push_str("BindPaths=%h/.local/share/neuroos/audit\n");
    // neuroos-confirm (GTK4, cairo renderer): the compositor socket; the name
    // comes from WAYLAND_DISPLAY in the user manager's environment.
    out.push_str("BindPaths=-%t/wayland-0\n");
    out.push_str("BindPaths=-%t/wayland-1\n");
    // Desktop notifications (FR-KER-09) go over the session bus; the dialog's
    // screen-reader support (design.md §6.4 rule 9) needs the accessibility bus.
    out.push_str("BindPaths=-%t/bus\n");
    out.push_str("BindPaths=-%t/at-spi\n");
    // Cleanup roots (OQ-07): caches and the trash.
    out.push_str("BindPaths=-%h/.cache\n");
    out.push_str("BindPaths=-%h/.local/share/Trash\n");
}

/// Relative path of the kernel drop-in that binds configured build folders.
pub const KERNEL_CLEANUP_DROPIN: &str =
    "systemd/user/neuroos-kernel.service.d/cleanup-folders.conf";

/// Drop-in binding each configured `[kernel] cleanup_build_folders` entry into
/// C6's unit (OQ-07, ADR-0010). Paths must be absolute; a missing folder is
/// skipped (`-`), so a stale entry never stops C6 from starting.
///
/// # Errors
/// [`DeployError::BadPath`] for a relative path, `..`, or characters systemd
/// would interpret (whitespace, quotes, `%`, `\`).
pub fn generate_kernel_cleanup_dropin(folders: &[PathBuf]) -> Result<String, DeployError> {
    let mut out = String::from(
        "# Generated by neuroos-gen-units from [kernel] cleanup_build_folders. Do not edit.\n[Service]\n",
    );
    for f in folders {
        let s = f
            .to_str()
            .ok_or_else(|| DeployError::BadPath(f.display().to_string()))?;
        let bad = !f.is_absolute()
            || f.components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            || s.chars()
                .any(|c| c.is_whitespace() || c.is_control() || "\"'%\\;:".contains(c));
        if bad {
            return Err(DeployError::BadPath(s.to_owned()));
        }
        out.push_str(&format!("BindPaths=-{s}\n"));
    }
    Ok(out)
}

/// Generates the systemd user service content for `part`.
#[must_use]
pub fn generate_user_service(part: Part, llm_model_file: &str) -> String {
    generate_user_service_internal(part, None, Some(llm_model_file))
}

/// Generates the systemd user service content for `part` with a custom `ExecStart`
/// (for security probe units); every other line is identical to the real unit.
#[must_use]
pub fn generate_user_service_with_exec_start(part: Part, exec_start: &str) -> String {
    generate_user_service_internal(part, Some(exec_start), None)
}

fn generate_user_service_internal(
    part: Part,
    custom_exec_start: Option<&str>,
    llm_model_file: Option<&str>,
) -> String {
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
    if part == Part::Inference {
        // FR-INF-01 hash check runs in its own unit: on systemd 255 user scope a
        // unit with more than one exec (e.g. ExecStartPre) silently loses
        // PrivateNetwork for its main process (ADR-0002 addendum, BUG-028).
        out.push_str(&format!("Requires={MODEL_CHECK_UNIT}\n"));
        out.push_str(&format!("After={after_line} {MODEL_CHECK_UNIT}\n\n"));
    } else {
        out.push_str(&format!("After={after_line}\n\n"));
    }

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
        Part::Kernel => push_kernel_paths(&mut out),
        _ => {}
    }

    // Decision 5: fetcher isolation.
    match part {
        Part::Kernel => {
            out.push_str("InaccessiblePaths=-/run/neuroos-fetcher/health\n");
            out.push_str("BindPaths=-/run/neuroos-fetcher/fetch\n");
        }
        Part::Healthd => {
            out.push_str("InaccessiblePaths=-/run/neuroos-fetcher/fetch\n");
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
    if part == Part::Kernel {
        // Skill children install their own, narrower seccomp filter (ADR-0010
        // decision 2). Adding a filter can only take syscalls away, never give them.
        out.push_str("SystemCallFilter=seccomp\n");
    }
    out.push_str("CapabilityBoundingSet=\n");
    out.push_str("AmbientCapabilities=\n");
    out.push_str("LimitCORE=0\n");
    out.push_str("UMask=0077\n");
    out.push_str(&format!("MemoryMax={mem_max}\n"));
    out.push_str("Restart=on-failure\n");
    out.push_str("RestartSec=2s\n");

    // ExecStartPre and ExecStart
    if let Some(custom) = custom_exec_start {
        out.push_str(&format!("ExecStart={custom}\n"));
    } else {
        let model = llm_model_file.unwrap_or("");
        match part {
            Part::Inference => {
                out.push_str(&format!(
                    "ExecStart=%h/.local/opt/neuroos/bin/neuroos-inference \
                    --socket %t/neuroos/inference/inference/inference.sock=neuroos-knowledge.service \
                    --health-socket %t/neuroos/inference/health/health.sock=neuroos-healthd.service \
                    --model %h/.local/opt/neuroos/models/{model} \
                    --threads 8 --ctx 2048 --batch 512\n"
                ));
            }
            Part::Voice => {
                out.push_str(
                    "ExecStart=%h/.local/opt/neuroos/bin/neuroos-voice \
                    --socket %t/neuroos/voice/voice/voice.sock=neuroos-knowledge.service \
                    --socket %t/neuroos/voice/control/control.sock=uid \
                    --health-socket %t/neuroos/voice/health/health.sock=neuroos-healthd.service\n",
                );
            }
            _ => {
                out.push_str(&format!(
                    "ExecStart=%h/.local/opt/neuroos/bin/neuroos-{}\n",
                    part.name()
                ));
            }
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
    out.push_str("ProtectHome=tmpfs\n");
    out.push_str("BindPaths=%t\n");
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

    // One exec only: see `single_exec` (BUG-028). Parents come first, so
    // `install -d` creates them in order with this unit's UMask (0700).
    out.push_str(&format!(
        "ExecStart=/usr/bin/install -d -m 0700 {}\n",
        runtime_dirs().join(" ")
    ));

    out.push_str("\n[Install]\n");
    out.push_str("WantedBy=neuroos.target\n");

    out
}

/// The model hash-check unit that C4 requires (FR-INF-01, ADR-0009).
pub const MODEL_CHECK_UNIT: &str = "neuroos-model-check.service";

/// Every runtime folder to pre-create, parents first, from the socket map
/// (ADR-0009 decision 4). Fetcher folders are made by tmpfiles, not here.
#[must_use]
pub fn runtime_dirs() -> Vec<String> {
    let mut dirs = vec!["%t/neuroos".to_owned()];
    let mut parts: Vec<&str> = Vec::new();
    for e in socketmap::SOCKET_MAP {
        if e.server != Part::Fetcher && !parts.contains(&e.server.name()) {
            parts.push(e.server.name());
        }
    }
    dirs.extend(parts.iter().map(|p| format!("%t/neuroos/{p}")));
    for e in socketmap::SOCKET_MAP {
        if e.server != Part::Fetcher {
            dirs.push(format!("%t/neuroos/{}/{}", e.server.name(), e.socket));
        }
    }
    dirs
}

/// Generates `neuroos-model-check.service`: a oneshot that verifies every model
/// against the manifest hashes before C4 may start (FR-INF-01). Not
/// `RemainAfterExit`, so every C4 (re)start re-runs the check.
#[must_use]
pub fn generate_model_check_service() -> String {
    let mut out = String::new();
    out.push_str("[Unit]\n");
    out.push_str("Description=Semantic-NeuroOS Model Hash Check\n");
    out.push_str("PartOf=neuroos.target\n\n");
    out.push_str("[Service]\n");
    out.push_str("Type=oneshot\n");
    for line in [
        "PrivateNetwork=true",
        "InaccessiblePaths=-/run/systemd/resolve",
        "InaccessiblePaths=-/run/dbus/system_bus_socket",
        "NoNewPrivileges=true",
        "ProtectSystem=strict",
        "ProtectHome=tmpfs",
        "BindReadOnlyPaths=%h/.local/opt/neuroos/models",
        "InaccessiblePaths=-/run/neuroos-fetcher",
        "PrivateTmp=true",
        "PrivateDevices=true",
        "ProtectKernelTunables=true",
        "ProtectKernelModules=true",
        "ProtectKernelLogs=true",
        "ProtectControlGroups=true",
        "ProtectClock=true",
        "ProtectHostname=true",
        "RestrictNamespaces=true",
        "RestrictRealtime=true",
        "RestrictSUIDSGID=true",
        "RestrictAddressFamilies=AF_UNIX",
        "LockPersonality=true",
        "MemoryDenyWriteExecute=true",
        "SystemCallArchitectures=native",
        "SystemCallFilter=@system-service",
        "CapabilityBoundingSet=",
        "AmbientCapabilities=",
        "LimitCORE=0",
        "UMask=0077",
    ] {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(
        "ExecStart=/usr/bin/sha256sum --check --quiet %h/.local/opt/neuroos/models/SHA256SUMS\n",
    );
    out
}

/// True if a unit has exactly one exec line and no pre/post/stop execs.
/// systemd 255 in user scope drops `PrivateNetwork=` for the main process of a
/// unit with several execs (measured 2026-11-16, BUG-028), so every user unit
/// must pass this check.
#[must_use]
pub fn single_exec(unit: &str) -> bool {
    let execs = unit
        .lines()
        .filter(|l| l.trim_start().starts_with("Exec"))
        .collect::<Vec<_>>();
    execs.len() == 1 && execs[0].trim_start().starts_with("ExecStart=")
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
    out.push_str("ExecStart=/opt/neuroos/bin/neuroos-fetcher --allow-uid @OWNER_UID@\n\n");

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

    // 2b. Model hash-check unit (C4 requires it)
    files.push(GeneratedFile {
        rel_path: PathBuf::from("systemd/user").join(MODEL_CHECK_UNIT),
        content: generate_model_check_service(),
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
