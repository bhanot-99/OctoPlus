//! Binary entrypoint for `neuroos-storage` (C3).

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use neuroos_common::paths::{self, Part};
use neuroos_health::{ServiceBuilder, ServiceError};

fn main() -> Result<(), ServiceError> {
    let data_storage_dir = paths::data_dir(
        std::env::var_os("XDG_DATA_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
    .unwrap_or_else(|_| PathBuf::from("/tmp/neuroos-storage-fallback"))
    .join("storage");

    if !data_storage_dir.exists() {
        let _ = std::fs::create_dir_all(&data_storage_dir);
        let _ = std::fs::set_permissions(&data_storage_dir, std::fs::Permissions::from_mode(0o700));
    }

    ServiceBuilder::new(Part::Storage, "neuroos-storage", env!("CARGO_PKG_VERSION"))
        .extra_read_write(vec![data_storage_dir])
        .run(neuroos_storage::init)
}
