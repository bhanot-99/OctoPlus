//! Binary entrypoint for `neuroos-storage` (C3).

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use neuroos_common::config::Config;
use neuroos_common::paths::{self, Part};
use neuroos_health::{ServiceBuilder, ServiceError};
use neuroos_storage::Dirs;

fn private_dir(p: &std::path::Path) {
    if !p.exists() {
        let _ = std::fs::create_dir_all(p);
        let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700));
    }
}

/// The config, read before Landlock is applied (the same file `ServiceBuilder` loads afterwards).
fn early_config() -> Config {
    paths::config_file(
        std::env::var_os(neuroos_common::config::CONFIG_ENV).as_deref(),
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
    .ok()
    .and_then(|f| Config::load_from(&f).ok())
    .unwrap_or_default()
}

fn main() -> Result<(), ServiceError> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let root = paths::data_dir(
        std::env::var_os("XDG_DATA_HOME").as_deref(),
        Some(home.as_os_str()),
    )
    .unwrap_or_else(|_| PathBuf::from("/tmp/neuroos-storage-fallback"));
    let (storage_dir, backups_dir) = (root.join("storage"), root.join("backups"));
    private_dir(&storage_dir);
    private_dir(&backups_dir);
    let dirs = Dirs::resolve(&early_config().storage, &home);

    // Architecture §8.2: models and the runtime library read-only; storage, backups and the spool
    // (read and remove) read-write. A missing optional folder is simply not granted.
    let read_only: Vec<PathBuf> = [
        dirs.embedding.clone(),
        dirs.runtime_lib
            .parent()
            .map(PathBuf::from)
            .unwrap_or_default(),
    ]
    .into_iter()
    .filter(|p| p.exists())
    .collect();
    let mut read_write = vec![storage_dir, backups_dir];
    if dirs.spool.exists() {
        read_write.push(dirs.spool);
    }
    ServiceBuilder::new(Part::Storage, "neuroos-storage", env!("CARGO_PKG_VERSION"))
        .extra_read_only(read_only)
        .extra_read_write(read_write)
        .run(neuroos_storage::init)
}
