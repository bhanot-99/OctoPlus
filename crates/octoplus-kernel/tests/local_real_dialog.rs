#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The whole approval path with the **real** GTK dialog (`octoplus-confirm`
//! in self-test mode, driving its own widgets) and the **real** skill child
//! (phases.md §5.3 IT "real kernel + mock C5 + dialog in test mode").
//! Needs the desktop session: `local_*` (rules.md §8.6).

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use octoplus_kernel::dispatch::{
    ActionMeta, AuditDown, AuditPort, DispatchConfig, Dispatcher, KernelSkills, Outcome,
};
use octoplus_kernel::keys::SecretKey;
use octoplus_kernel::registry::Registry;
use octoplus_proto::v1::{DispatchRequest, DispatchStatus, Origin, Taint};
use serde_json::Value;

#[derive(Debug, Default, Clone)]
struct Audit(Arc<Mutex<Vec<&'static str>>>);

impl AuditPort for Audit {
    async fn attempt(&self, _: u64, _: &ActionMeta) -> Result<(), AuditDown> {
        self.0.lock().unwrap().push("ATTEMPT");
        Ok(())
    }
    async fn result(&self, _: u64, _: &ActionMeta, o: Outcome) -> Result<(), AuditDown> {
        self.0.lock().unwrap().push(o.as_str());
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct NoKernelSkills;

impl KernelSkills for NoKernelSkills {
    async fn run(&self, _: &str, _: &Value) -> Result<Value, String> {
        Err("unused".into())
    }
}

fn sibling(name: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.ancestors()
        .map(|d| d.join(name))
        .find(|p| p.is_file())
        .unwrap_or_else(|| panic!("{name} not built: run `cargo build --workspace`"))
}

/// Runs a cleanup of `~/.cache/thumbs` with the real dialog doing `scenario`.
async fn cleanup_with_real_dialog(scenario: &str) -> (i32, bool, Vec<&'static str>) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(home.join(".cache/thumbs")).unwrap();
    std::fs::write(home.join(".cache/thumbs/a.png"), vec![0u8; 8192]).unwrap();
    let wrapper = home.join("dialog");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nexec '{}' --self-test {scenario}\n",
            sibling("octoplus-confirm").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let env = [
        "WAYLAND_DISPLAY",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "HOME",
    ]
    .iter()
    .filter_map(|k| std::env::var(k).ok().map(|v| ((*k).to_owned(), v)))
    .collect();
    let audit = Audit::default();
    let d = Dispatcher::new(
        Arc::new(Registry::load().unwrap()),
        Arc::new(SecretKey::from_bytes([9; 32])),
        DispatchConfig {
            dialog_bin: wrapper,
            skill_bin: sibling("octoplus-skill"),
            home: home.clone(),
            build_folders: vec![],
            notes_folders: Vec::new(),
            calendar_folders: Vec::new(),
            dialog_env: env,
            approval_window: Duration::from_secs(20),
            review_per_minute: 6,
            denial_cooldown_s: 300,
        },
        audit.clone(),
        NoKernelSkills,
    );
    let args = format!(
        r#"{{"paths":["{}"]}}"#,
        home.join(".cache/thumbs").display()
    );
    let r = d
        .dispatch(DispatchRequest {
            skill: "system.cleanup".into(),
            args_json: args,
            context_taint: Some(Taint { flags: 2 }),
            origin: Origin::Cli as i32,
        })
        .await
        .unwrap();
    let still_there = home.join(".cache/thumbs/a.png").exists();
    let log = audit.0.lock().unwrap().clone();
    (r.status, still_there, log)
}

fn have_compositor() -> bool {
    let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") else {
        return false;
    };
    let Some(display) = std::env::var_os("WAYLAND_DISPLAY") else {
        return false;
    };
    std::path::Path::new(&runtime).join(display).exists()
}

#[tokio::test]
async fn local_real_dialog_allow_after_delay_runs_the_cleanup() {
    if !have_compositor() {
        return;
    }
    let (status, still_there, log) = cleanup_with_real_dialog("allow-early").await;
    assert_eq!(status, DispatchStatus::Executed as i32);
    assert!(!still_there);
    assert_eq!(log, ["ATTEMPT", "EXECUTED"]);
}

#[tokio::test]
async fn local_real_dialog_enter_on_open_denies_and_nothing_is_deleted() {
    if !have_compositor() {
        return;
    }
    let (status, still_there, log) = cleanup_with_real_dialog("enter").await;
    assert_eq!(status, DispatchStatus::Rejected as i32);
    assert!(still_there);
    assert_eq!(log, ["ATTEMPT", "REJECTED"]);
}

#[tokio::test]
async fn local_real_dialog_escape_denies() {
    if !have_compositor() {
        return;
    }
    let (status, still_there, _) = cleanup_with_real_dialog("escape").await;
    assert_eq!(status, DispatchStatus::Rejected as i32);
    assert!(still_there);
}
