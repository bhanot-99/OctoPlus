#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use neuroos_common::paths::Part;
use neuroos_common::socketmap;
use neuroos_deploy::{generate_all_files, memory_max_for_part, read_llm_model_file, write_all};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn manifest_path() -> PathBuf {
    repo_root().join("models/manifest.toml")
}

#[test]
fn drift_check_matches_committed_deploy_files() {
    let root = repo_root();
    let manifest = manifest_path();
    let committed_deploy = root.join("deploy");

    let temp_dir = tempfile::tempdir().unwrap();
    let written = write_all(temp_dir.path(), &manifest).unwrap();
    assert!(!written.is_empty());

    // Check every generated file against committed deploy/
    for gen_path in written {
        let rel = gen_path.strip_prefix(temp_dir.path()).unwrap();
        let committed_file = committed_deploy.join(rel);
        assert!(
            committed_file.exists(),
            "committed file {} does not exist",
            committed_file.display()
        );

        let gen_content = fs::read_to_string(&gen_path).unwrap();
        let committed_content = fs::read_to_string(&committed_file).unwrap();
        assert_eq!(
            gen_content,
            committed_content,
            "drift detected in {}",
            rel.display()
        );
    }
}

#[test]
fn every_baseline_line_present_in_user_units() {
    let manifest = manifest_path();
    let files = generate_all_files(&manifest).unwrap();

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
        let unit_name = format!("systemd/user/neuroos-{}.service", part.name());
        let file = files
            .iter()
            .find(|f| f.rel_path == Path::new(&unit_name))
            .unwrap();
        let lines: Vec<&str> = file.content.lines().map(str::trim).collect();

        // Baseline stanzas:
        let expected_stanzas = [
            "PrivateNetwork=true",
            "InaccessiblePaths=-/run/systemd/resolve",
            "InaccessiblePaths=-/run/dbus/system_bus_socket",
            "NoNewPrivileges=true",
            "ProtectSystem=strict",
            "ProtectHome=tmpfs",
            "BindReadOnlyPaths=%h/.local/opt/neuroos",
            "BindReadOnlyPaths=-%h/.config/neuroos/config.toml",
            "TemporaryFileSystem=%t/neuroos",
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
            "SystemCallFilter=@system-service landlock_create_ruleset landlock_add_rule landlock_restrict_self",
            "CapabilityBoundingSet=",
            "AmbientCapabilities=",
            "LimitCORE=0",
            "UMask=0077",
            "Restart=on-failure",
            "RestartSec=2s",
        ];

        for stanza in expected_stanzas {
            assert!(
                lines.contains(&stanza),
                "part {:?} unit missing baseline stanza '{}'",
                part,
                stanza
            );
        }

        // MemoryMax
        let expected_mem = format!("MemoryMax={}", memory_max_for_part(part));
        assert!(
            lines.contains(&expected_mem.as_str()),
            "part {:?} unit missing '{}'",
            part,
            expected_mem
        );

        // Unit section basics
        assert!(lines.contains(&"PartOf=neuroos.target"));
        assert!(lines.contains(&"Requires=neuroos-runtime.service"));
        assert!(lines.contains(&"WantedBy=neuroos.target"));
    }
}

#[test]
fn bind_paths_equal_socketmap_visible_folders() {
    let manifest = manifest_path();
    let files = generate_all_files(&manifest).unwrap();

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
        let unit_name = format!("systemd/user/neuroos-{}.service", part.name());
        let file = files
            .iter()
            .find(|f| f.rel_path == Path::new(&unit_name))
            .unwrap();

        // Extract all BindPaths=%t/neuroos/... lines
        let unit_bind_paths: Vec<String> = file
            .content
            .lines()
            .map(str::trim)
            .filter_map(|l| l.strip_prefix("BindPaths="))
            .filter(|p| p.starts_with("%t/neuroos"))
            .map(ToString::to_string)
            .collect();

        let socketmap_paths = socketmap::user_runtime_visible_folders(part);

        assert_eq!(
            unit_bind_paths, socketmap_paths,
            "part {:?} BindPaths do not match socketmap::user_runtime_visible_folders",
            part
        );
    }
}

#[test]
fn fetcher_isolation_handled_per_decision_5() {
    let manifest = manifest_path();
    let files = generate_all_files(&manifest).unwrap();

    // C6 kernel and healthd get TemporaryFileSystem=/run/neuroos-fetcher plus BindPaths
    let kernel = files
        .iter()
        .find(|f| f.rel_path == Path::new("systemd/user/neuroos-kernel.service"))
        .unwrap();
    let k_lines: Vec<&str> = kernel.content.lines().map(str::trim).collect();
    assert!(k_lines.contains(&"TemporaryFileSystem=/run/neuroos-fetcher"));
    assert!(k_lines.contains(&"BindPaths=-/run/neuroos-fetcher/fetch"));
    assert!(!k_lines.contains(&"InaccessiblePaths=-/run/neuroos-fetcher"));

    let healthd = files
        .iter()
        .find(|f| f.rel_path == Path::new("systemd/user/neuroos-healthd.service"))
        .unwrap();
    let h_lines: Vec<&str> = healthd.content.lines().map(str::trim).collect();
    assert!(h_lines.contains(&"TemporaryFileSystem=/run/neuroos-fetcher"));
    assert!(h_lines.contains(&"BindPaths=-/run/neuroos-fetcher/health"));
    assert!(!h_lines.contains(&"InaccessiblePaths=-/run/neuroos-fetcher"));

    // Other user units get InaccessiblePaths=-/run/neuroos-fetcher
    let non_fetcher_parts = [
        Part::Monitor,
        Part::Voice,
        Part::Storage,
        Part::Inference,
        Part::Knowledge,
    ];
    for part in non_fetcher_parts {
        let unit_name = format!("systemd/user/neuroos-{}.service", part.name());
        let file = files
            .iter()
            .find(|f| f.rel_path == Path::new(&unit_name))
            .unwrap();
        let lines: Vec<&str> = file.content.lines().map(str::trim).collect();
        assert!(
            lines.contains(&"InaccessiblePaths=-/run/neuroos-fetcher"),
            "part {:?} must mask fetcher dir",
            part
        );
        assert!(
            !lines.contains(&"TemporaryFileSystem=/run/neuroos-fetcher"),
            "part {:?} must not mount tmpfs over fetcher dir",
            part
        );
    }
}

#[test]
fn persistent_data_bind_paths_per_decision_4() {
    let manifest = manifest_path();
    let files = generate_all_files(&manifest).unwrap();

    let storage = files
        .iter()
        .find(|f| f.rel_path == Path::new("systemd/user/neuroos-storage.service"))
        .unwrap();
    assert!(
        storage
            .content
            .contains("BindPaths=%h/.local/share/neuroos/storage")
    );

    let healthd = files
        .iter()
        .find(|f| f.rel_path == Path::new("systemd/user/neuroos-healthd.service"))
        .unwrap();
    assert!(
        healthd
            .content
            .contains("BindPaths=%h/.local/share/neuroos/soak")
    );

    let monitor = files
        .iter()
        .find(|f| f.rel_path == Path::new("systemd/user/neuroos-monitor.service"))
        .unwrap();
    assert!(
        !monitor
            .content
            .contains("BindPaths=%h/.local/share/neuroos/")
    );
}

#[test]
fn runtime_service_hardening_protect_home_tmpfs_and_bind_paths() {
    let manifest = manifest_path();
    let files = generate_all_files(&manifest).unwrap();

    let runtime = files
        .iter()
        .find(|f| f.rel_path == Path::new("systemd/user/neuroos-runtime.service"))
        .unwrap();

    let lines: Vec<&str> = runtime.content.lines().map(str::trim).collect();
    assert!(
        lines.contains(&"ProtectHome=tmpfs"),
        "neuroos-runtime.service must use ProtectHome=tmpfs to allow binding %t"
    );
    assert!(
        lines.contains(&"BindPaths=%t"),
        "neuroos-runtime.service must have BindPaths=%t"
    );
    assert!(
        !lines.contains(&"ProtectHome=true"),
        "neuroos-runtime.service must not have ProtectHome=true"
    );
}

#[test]
fn cpp_parts_flags_and_model_from_manifest() {
    let manifest = manifest_path();
    let llm_file = read_llm_model_file(&manifest).unwrap();
    assert_eq!(llm_file, "llm/Qwen3-4B-Instruct-2507-Q4_K_M.gguf");

    let files = generate_all_files(&manifest).unwrap();

    // C4 Inference
    let inf = files
        .iter()
        .find(|f| f.rel_path == Path::new("systemd/user/neuroos-inference.service"))
        .unwrap();
    assert!(inf.content.contains(
        "ExecStartPre=/usr/bin/sha256sum --check --quiet %h/.local/opt/neuroos/models/SHA256SUMS"
    ));
    assert!(inf.content.contains(
        "--socket %t/neuroos/inference/inference/inference.sock=neuroos-knowledge.service"
    ));
    assert!(inf.content.contains(
        "--health-socket %t/neuroos/inference/health/health.sock=neuroos-healthd.service"
    ));
    assert!(
        !inf.content.contains("--allow-unit"),
        "C4 unit must not use flat --allow-unit flags"
    );
    assert!(
        inf.content
            .contains(&format!("--model %h/.local/opt/neuroos/models/{llm_file}"))
    );
    assert!(inf.content.contains("--threads 8 --ctx 2048 --batch 512"));

    // C2 Voice
    let voi = files
        .iter()
        .find(|f| f.rel_path == Path::new("systemd/user/neuroos-voice.service"))
        .unwrap();
    assert!(
        voi.content
            .contains("--socket %t/neuroos/voice/voice/voice.sock=neuroos-knowledge.service")
    );
    assert!(
        voi.content
            .contains("--socket %t/neuroos/voice/control/control.sock=uid")
    );
    assert!(
        voi.content.contains(
            "--health-socket %t/neuroos/voice/health/health.sock=neuroos-healthd.service"
        )
    );
    assert!(
        !voi.content.contains("--allow-unit"),
        "C2 unit must not use flat --allow-unit flags"
    );
}

#[test]
fn fetcher_system_unit_and_permissions() {
    let manifest = manifest_path();
    let files = generate_all_files(&manifest).unwrap();

    let fetcher = files
        .iter()
        .find(|f| f.rel_path == Path::new("systemd/system/neuroos-fetcher.service"))
        .unwrap();
    let lines: Vec<&str> = fetcher.content.lines().map(str::trim).collect();

    assert!(lines.contains(&"User=neuroos-fetcher"));
    assert!(lines.contains(&"Group=neuroos"));
    assert!(lines.contains(&"RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6"));
    assert!(!lines.contains(&"PrivateNetwork=true"));
    assert!(lines.contains(&"ReadWritePaths=/run/neuroos-fetcher /var/spool/neuroos-fetcher"));
    assert!(lines.contains(&"ExecStart=/opt/neuroos/bin/neuroos-fetcher --allow-uid @OWNER_UID@"));
    assert!(lines.contains(&"MemoryMax=48M"));
    assert!(lines.contains(&"UMask=0027"));

    let sysusers = files
        .iter()
        .find(|f| f.rel_path == Path::new("sysusers.d/neuroos.conf"))
        .unwrap();
    assert!(sysusers.content.contains("g neuroos -"));
    assert!(sysusers.content.contains("u neuroos-fetcher -:neuroos"));

    let tmpfiles = files
        .iter()
        .find(|f| f.rel_path == Path::new("tmpfiles.d/neuroos-fetcher.conf"))
        .unwrap();
    assert!(
        tmpfiles
            .content
            .contains("d /run/neuroos-fetcher 0750 neuroos-fetcher neuroos -")
    );
    assert!(
        tmpfiles
            .content
            .contains("d /run/neuroos-fetcher/fetch 2770 neuroos-fetcher neuroos -")
    );
    assert!(
        tmpfiles
            .content
            .contains("d /run/neuroos-fetcher/health 2770 neuroos-fetcher neuroos -")
    );
    assert!(
        tmpfiles
            .content
            .contains("d /var/spool/neuroos-fetcher 2770 neuroos-fetcher neuroos -")
    );
}

#[test]
fn runtime_service_has_all_socket_folders() {
    let manifest = manifest_path();
    let files = generate_all_files(&manifest).unwrap();

    let runtime = files
        .iter()
        .find(|f| f.rel_path == Path::new("systemd/user/neuroos-runtime.service"))
        .unwrap();

    // Check that every socket entry's directory is covered in runtime service
    let root = Path::new("%t/neuroos");
    let mut covered_dirs = HashSet::new();
    for line in runtime.content.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("ExecStart=/usr/bin/install -d -m 0700 ") {
            for dir in rest.split_whitespace() {
                covered_dirs.insert(dir.to_string());
            }
        }
    }

    assert!(covered_dirs.contains("%t/neuroos"));
    for entry in socketmap::SOCKET_MAP {
        if entry.server != Part::Fetcher {
            let dir = entry.socket_dir(root);
            let dir_str = dir.to_string_lossy().to_string();
            assert!(
                covered_dirs.contains(&dir_str),
                "runtime service does not create directory: {dir_str}"
            );
        }
    }
}
