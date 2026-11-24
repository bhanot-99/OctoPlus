//! Binary entrypoint for `octoplus-healthd` (PRD §5.11, FR-HLT-01).

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use octoplus_common::paths::{self, Part};
use octoplus_health::{ServiceBuilder, ServiceError};

fn main() -> Result<(), ServiceError> {
    // 1. Ensure soak directory exists with mode 0700
    let soak_dir = paths::data_dir(
        std::env::var_os("XDG_DATA_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
    .unwrap_or_else(|_| PathBuf::from("/tmp/octoplus-data"))
    .join("soak");

    if !soak_dir.exists() {
        let _ = std::fs::create_dir_all(&soak_dir);
        let _ = std::fs::set_permissions(&soak_dir, std::fs::Permissions::from_mode(0o700));
    }

    // 2. Extra read-only paths for Landlock:
    // - /sys/fs/cgroup (for cgroup v2 metrics)
    // - client socket folders for the 7 remote parts so Landlock permits traversal
    let mut extra_read_only = vec![PathBuf::from("/sys/fs/cgroup")];

    let runtime_root = paths::runtime_root(std::env::var_os("XDG_RUNTIME_DIR").as_deref())
        .unwrap_or_else(|_| PathBuf::from("/tmp/octoplus"));

    for part in Part::ALL {
        if part != Part::Healthd {
            let sock_path = paths::health_socket(&runtime_root, part);
            if let Some(parent) = sock_path.parent() {
                if parent.exists() {
                    extra_read_only.push(parent.to_path_buf());
                } else if let Some(grandparent) = parent.parent()
                    && grandparent.exists()
                {
                    extra_read_only.push(grandparent.to_path_buf());
                }
            }
        }
    }

    ServiceBuilder::new(Part::Healthd, "octoplus-healthd", env!("CARGO_PKG_VERSION"))
        .extra_read_only(extra_read_only)
        .extra_read_write(vec![soak_dir])
        .run(octoplus_healthd::init)
}
