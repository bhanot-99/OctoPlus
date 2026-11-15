#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use neuroos_common::paths::Part;
use neuroos_common::socketmap::SOCKET_MAP;

struct UnitGuard {
    name: &'static str,
}

impl Drop for UnitGuard {
    fn drop(&mut self) {
        let _ = Command::new("systemctl")
            .args(["--user", "stop", self.name])
            .status();
        let _ = Command::new("systemctl")
            .args(["--user", "reset-failed", self.name])
            .status();
    }
}

#[test]
fn local_runtime_unit_creates_socket_dirs_with_mode_0700() {
    let xdg = match std::env::var("XDG_RUNTIME_DIR") {
        Ok(dir) => dir,
        Err(_) => {
            eprintln!("Skipping test: XDG_RUNTIME_DIR not set (needs a user session)");
            return;
        }
    };

    // Check if user systemd bus is reachable
    let bus_check = Command::new("systemctl")
        .args(["--user", "status"])
        .output();
    match bus_check {
        Ok(out)
            if !out.status.success()
                && String::from_utf8_lossy(&out.stderr).contains("Failed to connect to bus") =>
        {
            eprintln!("Skipping test: user systemd session bus not accessible");
            return;
        }
        Err(_) => {
            eprintln!("Skipping test: systemctl command failed to execute");
            return;
        }
        _ => {}
    }

    let runtime_root = PathBuf::from(&xdg).join("neuroos");

    let _guard = UnitGuard {
        name: "neuroos-runtime.service",
    };

    // First stop if already running
    let _ = Command::new("systemctl")
        .args(["--user", "stop", "neuroos-runtime.service"])
        .status();

    // Start installed unit
    let status = Command::new("systemctl")
        .args(["--user", "start", "neuroos-runtime.service"])
        .status()
        .expect("failed to execute systemctl --user start neuroos-runtime.service");

    assert!(
        status.success(),
        "systemctl --user start neuroos-runtime.service failed"
    );

    // Verify runtime root mode 0700
    assert!(
        runtime_root.is_dir(),
        "runtime root {} does not exist",
        runtime_root.display()
    );
    let root_meta = std::fs::metadata(&runtime_root).expect("metadata runtime root");
    let root_mode = root_meta.permissions().mode() & 0o777;
    assert_eq!(
        root_mode,
        0o700,
        "runtime root {} mode is {:o}, expected 0700",
        runtime_root.display(),
        root_mode
    );

    // Verify every user socket dir from SOCKET_MAP exists with mode 0700
    for entry in SOCKET_MAP {
        if entry.server != Part::Fetcher {
            let socket_dir = entry.socket_dir(&runtime_root);
            assert!(
                socket_dir.is_dir(),
                "socket dir {} does not exist",
                socket_dir.display()
            );
            let meta = std::fs::metadata(&socket_dir)
                .unwrap_or_else(|e| panic!("failed to stat {}: {e}", socket_dir.display()));
            let mode = meta.permissions().mode() & 0o777;
            assert_eq!(
                mode,
                0o700,
                "socket dir {} mode is {:o}, expected 0700",
                socket_dir.display(),
                mode
            );
        }
    }
}
