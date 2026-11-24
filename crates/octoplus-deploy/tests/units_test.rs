#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use octoplus_common::paths::Part;
use octoplus_common::socketmap;
use octoplus_deploy::{
    generate_all_files, generate_user_service, generate_user_service_with_exec_start,
    memory_max_for_part, read_llm_model_file, single_exec, write_all,
};

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
        let unit_name = format!("systemd/user/octoplus-{}.service", part.name());
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
            "BindReadOnlyPaths=%h/.local/opt/octoplus",
            "BindReadOnlyPaths=-%h/.config/octoplus/config.toml",
            "TemporaryFileSystem=%t/octoplus",
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
            // C2 is the one documented exemption: audio scheduling (Architecture §8.1).
            if part == Part::Voice && stanza == "RestrictRealtime=true" {
                assert!(
                    !lines.contains(&stanza),
                    "C2 must not carry the restriction"
                );
                continue;
            }
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
        assert!(lines.contains(&"PartOf=octoplus.target"));
        assert!(lines.contains(&"Requires=octoplus-runtime.service"));
        assert!(lines.contains(&"WantedBy=octoplus.target"));
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
        let unit_name = format!("systemd/user/octoplus-{}.service", part.name());
        let file = files
            .iter()
            .find(|f| f.rel_path == Path::new(&unit_name))
            .unwrap();

        // Extract all BindPaths=%t/octoplus/... lines
        let unit_bind_paths: Vec<String> = file
            .content
            .lines()
            .map(str::trim)
            .filter_map(|l| l.strip_prefix("BindPaths="))
            .filter(|p| p.starts_with("%t/octoplus"))
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

    // C6 kernel and healthd get InaccessiblePaths of peer socket plus BindPaths
    let kernel = files
        .iter()
        .find(|f| f.rel_path == Path::new("systemd/user/octoplus-kernel.service"))
        .unwrap();
    let k_lines: Vec<&str> = kernel.content.lines().map(str::trim).collect();
    assert!(k_lines.contains(&"InaccessiblePaths=-/run/octoplus-fetcher/health"));
    assert!(k_lines.contains(&"BindPaths=-/run/octoplus-fetcher/fetch"));
    assert!(!k_lines.contains(&"InaccessiblePaths=-/run/octoplus-fetcher"));

    let healthd = files
        .iter()
        .find(|f| f.rel_path == Path::new("systemd/user/octoplus-healthd.service"))
        .unwrap();
    let h_lines: Vec<&str> = healthd.content.lines().map(str::trim).collect();
    assert!(h_lines.contains(&"InaccessiblePaths=-/run/octoplus-fetcher/fetch"));
    assert!(h_lines.contains(&"BindPaths=-/run/octoplus-fetcher/health"));
    assert!(!h_lines.contains(&"InaccessiblePaths=-/run/octoplus-fetcher"));

    // Other user units get InaccessiblePaths=-/run/octoplus-fetcher
    let non_fetcher_parts = [
        Part::Monitor,
        Part::Voice,
        Part::Storage,
        Part::Inference,
        Part::Knowledge,
    ];
    for part in non_fetcher_parts {
        let unit_name = format!("systemd/user/octoplus-{}.service", part.name());
        let file = files
            .iter()
            .find(|f| f.rel_path == Path::new(&unit_name))
            .unwrap();
        let lines: Vec<&str> = file.content.lines().map(str::trim).collect();
        assert!(
            lines.contains(&"InaccessiblePaths=-/run/octoplus-fetcher"),
            "part {:?} must mask fetcher dir",
            part
        );
        assert!(
            !lines.contains(&"TemporaryFileSystem=/run/octoplus-fetcher"),
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
        .find(|f| f.rel_path == Path::new("systemd/user/octoplus-storage.service"))
        .unwrap();
    assert!(
        storage
            .content
            .contains("BindPaths=%h/.local/share/octoplus/storage")
    );

    let healthd = files
        .iter()
        .find(|f| f.rel_path == Path::new("systemd/user/octoplus-healthd.service"))
        .unwrap();
    assert!(
        healthd
            .content
            .contains("BindPaths=%h/.local/share/octoplus/soak")
    );

    let monitor = files
        .iter()
        .find(|f| f.rel_path == Path::new("systemd/user/octoplus-monitor.service"))
        .unwrap();
    assert!(
        !monitor
            .content
            .contains("BindPaths=%h/.local/share/octoplus/")
    );
}

#[test]
fn runtime_service_hardening_protect_home_tmpfs_and_bind_paths() {
    let manifest = manifest_path();
    let files = generate_all_files(&manifest).unwrap();

    let runtime = files
        .iter()
        .find(|f| f.rel_path == Path::new("systemd/user/octoplus-runtime.service"))
        .unwrap();

    let lines: Vec<&str> = runtime.content.lines().map(str::trim).collect();
    assert!(
        lines.contains(&"ProtectHome=tmpfs"),
        "octoplus-runtime.service must use ProtectHome=tmpfs to allow binding %t"
    );
    assert!(
        lines.contains(&"BindPaths=%t"),
        "octoplus-runtime.service must have BindPaths=%t"
    );
    assert!(
        !lines.contains(&"ProtectHome=true"),
        "octoplus-runtime.service must not have ProtectHome=true"
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
        .find(|f| f.rel_path == Path::new("systemd/user/octoplus-inference.service"))
        .unwrap();
    // The hash check is its own unit (BUG-028): C4 requires and orders after it.
    assert!(!inf.content.contains("ExecStartPre="));
    assert!(inf.content.contains("Requires=octoplus-model-check.service"));
    assert!(
        inf.content
            .contains("octoplus-model-check.service\n\n[Service]")
    );
    let check = files
        .iter()
        .find(|f| f.rel_path == Path::new("systemd/user/octoplus-model-check.service"))
        .unwrap();
    assert!(check.content.contains(
        "ExecStart=/usr/bin/sha256sum --check --quiet %h/.local/opt/octoplus/models/SHA256SUMS"
    ));
    assert!(check.content.contains("PrivateNetwork=true"));
    assert!(
        !check.content.contains("RemainAfterExit"),
        "check must re-run on every C4 start"
    );
    assert!(inf.content.contains(
        "--socket %t/octoplus/inference/inference/inference.sock=octoplus-knowledge.service"
    ));
    assert!(inf.content.contains(
        "--health-socket %t/octoplus/inference/health/health.sock=octoplus-healthd.service"
    ));
    assert!(
        !inf.content.contains("--allow-unit"),
        "C4 unit must not use flat --allow-unit flags"
    );
    assert!(
        inf.content
            .contains(&format!("--model %h/.local/opt/octoplus/models/{llm_file}"))
    );
    assert!(inf.content.contains("--threads 8 --ctx 2048 --batch 512"));

    // C2 Voice
    let voi = files
        .iter()
        .find(|f| f.rel_path == Path::new("systemd/user/octoplus-voice.service"))
        .unwrap();
    assert!(
        voi.content
            .contains("--socket %t/octoplus/voice/voice/voice.sock=octoplus-knowledge.service")
    );
    assert!(
        voi.content
            .contains("--socket %t/octoplus/voice/control/control.sock=uid")
    );
    assert!(
        voi.content.contains(
            "--health-socket %t/octoplus/voice/health/health.sock=octoplus-healthd.service"
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
        .find(|f| f.rel_path == Path::new("systemd/system/octoplus-fetcher.service"))
        .unwrap();
    let lines: Vec<&str> = fetcher.content.lines().map(str::trim).collect();

    assert!(lines.contains(&"User=octoplus-fetcher"));
    assert!(lines.contains(&"Group=octoplus"));
    assert!(lines.contains(&"RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6"));
    assert!(!lines.contains(&"PrivateNetwork=true"));
    assert!(lines.contains(&"ReadWritePaths=/run/octoplus-fetcher /var/spool/octoplus-fetcher"));
    assert!(lines.contains(&"ExecStart=/opt/octoplus/bin/octoplus-fetcher --allow-uid @OWNER_UID@"));
    assert!(lines.contains(&"MemoryMax=48M"));
    assert!(lines.contains(&"UMask=0027"));

    let sysusers = files
        .iter()
        .find(|f| f.rel_path == Path::new("sysusers.d/octoplus.conf"))
        .unwrap();
    assert!(sysusers.content.contains("g octoplus -"));
    assert!(sysusers.content.contains("u octoplus-fetcher -:octoplus"));

    let tmpfiles = files
        .iter()
        .find(|f| f.rel_path == Path::new("tmpfiles.d/octoplus-fetcher.conf"))
        .unwrap();
    assert!(
        tmpfiles
            .content
            .contains("d /run/octoplus-fetcher 0750 octoplus-fetcher octoplus -")
    );
    assert!(
        tmpfiles
            .content
            .contains("d /run/octoplus-fetcher/fetch 2770 octoplus-fetcher octoplus -")
    );
    assert!(
        tmpfiles
            .content
            .contains("d /run/octoplus-fetcher/health 2770 octoplus-fetcher octoplus -")
    );
    assert!(
        tmpfiles
            .content
            .contains("d /var/spool/octoplus-fetcher 2770 octoplus-fetcher octoplus -")
    );
}

#[test]
fn runtime_service_has_all_socket_folders() {
    let manifest = manifest_path();
    let files = generate_all_files(&manifest).unwrap();

    let runtime = files
        .iter()
        .find(|f| f.rel_path == Path::new("systemd/user/octoplus-runtime.service"))
        .unwrap();

    // Check that every socket entry's directory is covered in runtime service
    let root = Path::new("%t/octoplus");
    let mut covered_dirs = HashSet::new();
    for line in runtime.content.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("ExecStart=/usr/bin/install -d -m 0700 ") {
            for dir in rest.split_whitespace() {
                covered_dirs.insert(dir.to_string());
            }
        }
    }

    assert!(covered_dirs.contains("%t/octoplus"));
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

#[test]
fn service_section_differs_only_in_exec_start_for_probe_rendering() {
    let manifest = manifest_path();
    let model = read_llm_model_file(&manifest).unwrap();
    let user_parts = [
        Part::Healthd,
        Part::Storage,
        Part::Inference,
        Part::Monitor,
        Part::Kernel,
        Part::Knowledge,
        Part::Voice,
    ];
    let probe_cmd = "%h/.local/opt/octoplus/bin/octoplus-s04-probe";

    for part in user_parts {
        let normal = generate_user_service(part, &model);
        let probed = generate_user_service_with_exec_start(part, probe_cmd);

        let extract_service_lines = |content: &str| -> Vec<String> {
            let mut in_service = false;
            let mut lines = Vec::new();
            for line in content.lines() {
                let t = line.trim();
                if t == "[Service]" {
                    in_service = true;
                    continue;
                } else if t.starts_with('[') && in_service {
                    break;
                }
                if in_service
                    && !t.is_empty()
                    && !t.starts_with('#')
                    && !t.starts_with("ExecStart=")
                {
                    lines.push(t.to_string());
                }
            }
            lines
        };

        let normal_service = extract_service_lines(&normal);
        let probed_service = extract_service_lines(&probed);
        assert_eq!(
            normal_service, probed_service,
            "Part {:?} service stanzas must match except ExecStart",
            part
        );
        assert!(probed.contains(&format!("ExecStart={probe_cmd}")));
        assert!(!probed.contains("ExecStartPre="));
    }
}

/// BUG-028: systemd 255 user scope drops PrivateNetwork for the main process of a
/// unit with more than one exec. Every generated user unit must have exactly one.
#[test]
fn every_user_unit_has_exactly_one_exec() {
    let manifest = manifest_path();
    let files = generate_all_files(&manifest).unwrap();
    let mut checked = 0;
    for f in &files {
        if f.rel_path.starts_with("systemd/user")
            && f.rel_path.extension().is_some_and(|e| e == "service")
        {
            assert!(
                single_exec(&f.content),
                "{} must have exactly one ExecStart and no other Exec*",
                f.rel_path.display()
            );
            checked += 1;
        }
    }
    assert_eq!(
        checked, 12,
        "7 parts + runtime + model check + 3 storage jobs"
    );
    // Negative control: the checker rejects a unit with ExecStartPre.
    assert!(!single_exec("ExecStartPre=/bin/true\nExecStart=/bin/x\n"));
    assert!(!single_exec("ExecStart=/bin/a\nExecStart=/bin/b\n"));
}

#[test]
fn kernel_paths_per_adr_0010() {
    let files = generate_all_files(&manifest_path()).unwrap();
    let kernel = &files
        .iter()
        .find(|f| f.rel_path == Path::new("systemd/user/octoplus-kernel.service"))
        .unwrap()
        .content;
    for line in [
        "BindReadOnlyPaths=%h/.config/octoplus/hmac.key",
        // BUG-114: without it C6 cannot sign fetch requests (optional: web skills stay off).
        "BindReadOnlyPaths=-%h/.config/octoplus/fetch.key",
        "BindPaths=%h/.local/share/octoplus/audit",
        "BindPaths=-%t/wayland-1",
        "BindPaths=-%t/bus",
        "BindPaths=-%t/at-spi",
        "BindPaths=-%h/.cache",
        "BindPaths=-%h/.local/share/Trash",
        "SystemCallFilter=seccomp",
    ] {
        assert!(
            kernel.lines().any(|l| l == line),
            "kernel unit lacks {line}"
        );
    }
    // Only C6 gets these; in particular no other part sees the key, the audit log or the cleanup roots.
    for f in &files {
        if f.rel_path == Path::new("systemd/user/octoplus-kernel.service") {
            continue;
        }
        // C1 also sees the compositor and the session bus, read-only (ADR-0007, ADR-0011);
        // `monitor_unit_gets_compositor_and_session_bus_read_only_and_nothing_wider` pins that form.
        let is_monitor = f.rel_path == Path::new("systemd/user/octoplus-monitor.service");
        for secret in [
            "hmac.key",
            "fetch.key",
            "octoplus/audit",
            "%h/.cache",
            "Trash",
            "%t/bus",
            "wayland",
            "SystemCallFilter=seccomp",
        ] {
            if is_monitor && (secret == "%t/bus" || secret == "wayland") {
                continue;
            }
            assert!(
                !f.content.contains(secret),
                "{} must not mention {secret}",
                f.rel_path.display()
            );
        }
    }
}

#[test]
fn kernel_cleanup_dropin_validates_paths() {
    use octoplus_deploy::generate_kernel_cleanup_dropin;
    let ok =
        generate_kernel_cleanup_dropin(&[PathBuf::from("/home/u/projects/app/target")]).unwrap();
    assert!(ok.contains("BindPaths=-/home/u/projects/app/target\n"));
    assert!(
        generate_kernel_cleanup_dropin(&[])
            .unwrap()
            .contains("[Service]")
    );
    for bad in [
        "relative/target",
        "/home/u/../etc",
        "/home/u/my target",
        "/home/u/%h",
        "/home/u/a\\b",
        "/home/u/a;b",
        "/home/u/a:/home/u/.config/octoplus",
        "/home/u/a\nExecStart=/bin/sh",
    ] {
        assert!(
            generate_kernel_cleanup_dropin(&[PathBuf::from(bad)]).is_err(),
            "{bad:?} must be refused"
        );
    }
}

#[test]
fn monitor_unit_gets_compositor_and_session_bus_read_only_and_nothing_wider() {
    let files = generate_all_files(&manifest_path()).unwrap();
    let unit = files
        .iter()
        .find(|f| f.rel_path == Path::new("systemd/user/octoplus-monitor.service"))
        .unwrap();
    for line in [
        "BindReadOnlyPaths=-%t/wayland-0",
        "BindReadOnlyPaths=-%t/wayland-1",
        "BindReadOnlyPaths=-%t/bus",
        "InaccessiblePaths=-/run/dbus/system_bus_socket",
        "PrivateNetwork=true",
    ] {
        assert!(unit.content.contains(line), "missing {line}");
    }
    // Read-only only: no writable bind of the bus or the compositor, no accessibility bus.
    assert!(!unit.content.contains("BindPaths=-%t/bus"));
    assert!(!unit.content.contains("at-spi"));
    // Other parts do not get the compositor or the bus (AB-7).
    let storage = files
        .iter()
        .find(|f| f.rel_path == Path::new("systemd/user/octoplus-storage.service"))
        .unwrap();
    assert!(!storage.content.contains("wayland") && !storage.content.contains("%t/bus"));
}

#[test]
fn monitor_folders_dropin_binds_read_only_and_validates_paths() {
    use octoplus_common::config::{FolderConfig, FolderKind};
    use octoplus_deploy::generate_monitor_folders_dropin;
    let ok = generate_monitor_folders_dropin(&[FolderConfig {
        path: PathBuf::from("/home/u/Notes"),
        kind: FolderKind::Notes,
    }])
    .unwrap();
    assert!(ok.contains("[Service]\nBindReadOnlyPaths=-/home/u/Notes\n"));
    assert!(!ok.contains("BindPaths=-"), "never writable");
    assert!(
        !generate_monitor_folders_dropin(&[])
            .unwrap()
            .contains("BindReadOnlyPaths")
    );
    for bad in [
        "relative/dir",
        "/a/../b",
        "/has space",
        "/pct%h",
        "/semi;colon",
    ] {
        let f = FolderConfig {
            path: PathBuf::from(bad),
            kind: FolderKind::Git,
        };
        assert!(
            generate_monitor_folders_dropin(&[f]).is_err(),
            "accepted {bad}"
        );
    }
}

#[test]
fn storage_unit_sees_backups_and_the_spool_and_the_jobs_are_small_one_shots() {
    let files = generate_all_files(&manifest_path()).unwrap();
    let get = |name: &str| {
        files
            .iter()
            .find(|f| f.rel_path == Path::new(&format!("systemd/user/{name}")))
            .unwrap_or_else(|| panic!("{name} missing"))
            .content
            .clone()
    };
    let storage = get("octoplus-storage.service");
    for line in [
        "BindPaths=%h/.local/share/octoplus/storage",
        "BindPaths=%h/.local/share/octoplus/backups",
        "BindPaths=-/var/spool/octoplus-fetcher",
        "PrivateNetwork=true",
    ] {
        assert!(storage.lines().any(|l| l == line), "storage lacks {line}");
    }
    for other in [
        "monitor",
        "knowledge",
        "kernel",
        "inference",
        "voice",
        "healthd",
    ] {
        let c = get(&format!("octoplus-{other}.service"));
        assert!(
            !c.contains("/var/spool/octoplus-fetcher") && !c.contains("octoplus/backups"),
            "{other} must not see the spool or backups"
        );
    }
    for (job, calendar) in octoplus_deploy::STORAGE_JOBS {
        let svc = get(&format!("octoplus-storage-{job}.service"));
        assert!(svc.contains("Type=oneshot") && svc.contains("PrivateNetwork=true"));
        assert_eq!(
            svc.lines().filter(|l| l.starts_with("Exec")).count(),
            1,
            "exactly one exec"
        );
        assert!(svc.contains(&format!(
            "octoplusctl --runtime-dir %t/octoplus storage job {job}"
        )));
        assert!(
            svc.contains("BindPaths=%t/octoplus/storage/storage"),
            "only the storage socket"
        );
        assert!(
            !svc.contains("octoplus/storage\n") || svc.contains("storage/storage"),
            "not the whole runtime"
        );
        assert!(
            !svc.contains("BindPaths=%h"),
            "a job writes nothing in the home folder"
        );
        let timer = get(&format!("octoplus-storage-{job}.timer"));
        assert!(
            timer.contains(&format!("OnCalendar={calendar}"))
                && timer.contains(&format!("Unit=octoplus-storage-{job}.service"))
        );
        assert!(timer.contains("Persistent=true"));
    }
}

#[test]
fn kernel_dropin_binds_notes_writable_and_calendars_read_only() {
    use octoplus_deploy::generate_kernel_folders_dropin;
    let text = generate_kernel_folders_dropin(
        &[PathBuf::from("/home/u/build")],
        &[PathBuf::from("/home/u/notes")],
        &[PathBuf::from("/home/u/calendars")],
    )
    .unwrap();
    assert!(text.contains("BindPaths=-/home/u/build\n"));
    assert!(text.contains("BindPaths=-/home/u/notes\n"));
    assert!(text.contains("BindReadOnlyPaths=-/home/u/calendars\n"));
    assert!(!text.contains("BindPaths=-/home/u/calendars"));
    for bad in ["relative/notes", "/home/u/../etc", "/home/u/no tes"] {
        assert!(
            generate_kernel_folders_dropin(&[], &[PathBuf::from(bad)], &[]).is_err(),
            "{bad}"
        );
        assert!(
            generate_kernel_folders_dropin(&[], &[], &[PathBuf::from(bad)]).is_err(),
            "{bad}"
        );
    }
}

#[test]
fn only_the_knowledge_unit_binds_the_nudge_rules_file() {
    use octoplus_deploy::generate_user_service;
    let generate_unit = |p| generate_user_service(p, "model.gguf");
    let bind = "BindReadOnlyPaths=-%h/.config/octoplus/nudges.toml";
    for part in [
        Part::Storage,
        Part::Kernel,
        Part::Monitor,
        Part::Healthd,
        Part::Inference,
    ] {
        assert!(!generate_unit(part).contains(bind), "{part:?}");
    }
    assert!(generate_unit(Part::Knowledge).contains(bind));
}

#[test]
fn the_voice_unit_binds_its_settings_keeps_the_hardening_except_realtime_and_names_the_models() {
    let files = generate_all_files(&manifest_path()).unwrap();
    let unit = &files
        .iter()
        .find(|f| f.rel_path == Path::new("systemd/user/octoplus-voice.service"))
        .unwrap()
        .content;
    assert!(unit.contains("BindReadOnlyPaths=-%h/.config/octoplus/voice.conf"));
    assert!(
        unit.contains("BindPaths=-%t/pipewire-0"),
        "the audio socket"
    );
    assert!(unit.contains("--models-dir %h/.local/opt/octoplus/models/voice"));
    assert!(unit.contains("--voice-config %h/.config/octoplus/voice.conf"));
    assert!(
        !unit.contains("RestrictRealtime=true"),
        "audio scheduling is the one documented exemption"
    );
    for kept in [
        "PrivateNetwork=true",
        "PrivateDevices=true",
        "RestrictAddressFamilies=AF_UNIX",
        "MemoryDenyWriteExecute=true",
        "CapabilityBoundingSet=",
        "ProtectHome=tmpfs",
    ] {
        assert!(unit.contains(kept), "{kept}");
    }
    // No other part gets the settings file, and every other part keeps its real-time restriction.
    for part in [
        Part::Storage,
        Part::Inference,
        Part::Monitor,
        Part::Kernel,
        Part::Knowledge,
    ] {
        let name = format!("systemd/user/octoplus-{}.service", part.name());
        let other = &files
            .iter()
            .find(|f| f.rel_path == Path::new(&name))
            .unwrap()
            .content;
        assert!(!other.contains("voice.conf"), "{name}");
        assert!(
            !other.contains("pipewire"),
            "{name}: only C2 sees the audio socket"
        );
        assert!(other.contains("RestrictRealtime=true"), "{name}");
    }
}

/// FR-FET-10 / BUG-110: C7's unit loads the TCP 443 + DNS packet rule before it starts,
/// with full privileges, and refuses to start when an installed rule cannot be loaded.
#[test]
fn fetcher_loads_its_port_rule_before_starting() {
    let unit = octoplus_deploy::generate_fetcher_system_service();
    let lines: Vec<&str> = unit.lines().collect();
    let pre = lines
        .iter()
        .position(|l| {
            l.starts_with("ExecStartPre=+")
                && l.contains("/usr/sbin/nft -f /etc/octoplus/octoplus-fetcher.nft")
        })
        .expect("ExecStartPre loads the rule");
    let start = lines
        .iter()
        .position(|l| l.starts_with("ExecStart="))
        .unwrap();
    assert!(pre < start);
    assert!(
        !lines[pre].starts_with("ExecStartPre=-"),
        "a failed load must stop the start"
    );
}
