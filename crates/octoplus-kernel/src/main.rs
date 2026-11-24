//! Binary entrypoint for `octoplus-kernel` (C6).
//!
//! C6's Landlock policy is the union of what it and its children need
//! (ADR-0010 decision 3); each child then narrows itself further:
//! - read-only: system files for the GTK dialog (`/usr`, `/etc`), its own
//!   binaries folder (the dialog and skill programs), `hmac.key` and `fetch.key`;
//! - read-write: `audit/`, the cleanup roots, the trash and the notes folders (skill children);
//!   calendar folders are read-only.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use octoplus_common::config::Config;
use octoplus_common::paths::{self, Part};
use octoplus_health::{ServiceBuilder, ServiceError};

fn existing(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    // Landlock rules need existing paths; a missing optional root is simply not granted.
    paths.into_iter().filter(|p| p.exists()).collect()
}

fn build_folders() -> Vec<PathBuf> {
    // Read before restrict, the same file ServiceBuilder loads (an invalid file fails there).
    let file = paths::config_file(
        std::env::var_os("OCTOPLUS_CONFIG").as_deref(),
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    );
    file.ok()
        .and_then(|f| Config::load_from(&f).ok())
        .map(|c| c.kernel.cleanup_build_folders)
        .unwrap_or_default()
}

/// Watched notes and calendar folders from the same config file (children read them; the notes
/// folders are also written by `notes.append`, a REVIEW skill, ADR-0013 decision 7).
fn watched_folders() -> (Vec<PathBuf>, Vec<PathBuf>) {
    use octoplus_common::config::FolderKind;
    let file = paths::config_file(
        std::env::var_os("OCTOPLUS_CONFIG").as_deref(),
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    );
    let folders = file
        .ok()
        .and_then(|f| Config::load_from(&f).ok())
        .map(|c| c.monitor.folders)
        .unwrap_or_default();
    let pick = |kind: FolderKind| {
        folders
            .iter()
            .filter(|f| f.kind == kind)
            .map(|f| f.path.clone())
            .collect::<Vec<_>>()
    };
    (pick(FolderKind::Notes), pick(FolderKind::Calendar))
}

fn main() -> Result<(), ServiceError> {
    let fail = |what: String| ServiceError::Custom(what);
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| fail("HOME is not set".into()))?;
    let audit_dir = paths::data_dir(
        std::env::var_os("XDG_DATA_HOME").as_deref(),
        Some(home.as_os_str()),
    )
    .map_err(|e| fail(format!("no data folder: {e}")))?
    .join("audit");
    if !audit_dir.exists() {
        std::fs::create_dir_all(&audit_dir).map_err(|e| fail(format!("audit folder: {e}")))?;
        std::fs::set_permissions(&audit_dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| fail(format!("audit folder mode: {e}")))?;
    }
    let config_dir = paths::config_dir(
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        Some(home.as_os_str()),
    )
    .map_err(|e| fail(format!("no config folder: {e}")))?;
    let (dialog_bin, _) = octoplus_kernel::sibling_binaries()?;
    let bin_dir = dialog_bin.parent().map(Path::to_path_buf);

    let (notes, calendars) = watched_folders();
    let read_only = existing(
        [
            PathBuf::from("/usr"),
            PathBuf::from("/etc"),
            PathBuf::from("/var/cache/fontconfig"),
        ]
        .into_iter()
        .chain(octoplus_kernel::key_files(&config_dir))
        .chain(bin_dir)
        .chain(calendars),
    );
    let mut read_write = vec![audit_dir];
    read_write.extend(existing(
        [home.join(".cache"), home.join(".local/share/Trash")]
            .into_iter()
            .chain(build_folders())
            .chain(notes),
    ));

    ServiceBuilder::new(Part::Kernel, "octoplus-kernel", env!("CARGO_PKG_VERSION"))
        .extra_read_only(read_only)
        .extra_read_write(read_write)
        .run(octoplus_kernel::init)
}
