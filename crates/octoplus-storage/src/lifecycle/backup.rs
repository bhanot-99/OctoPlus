//! Snapshot backups (FR-STO-11, Architecture §7.5): checkpoint, `VACUUM INTO` a temporary
//! folder, fsync, atomic rename, keep the newest N. A crash at any point leaves the previous
//! backups intact; leftovers of a crashed run are swept at the next one.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::params;

use crate::db::DbError;
use crate::store::Store;

/// File name of the database inside a backup folder.
pub const BACKUP_DB: &str = "octoplus.sqlite3";
/// Prefix of a backup folder still being written.
const TMP_PREFIX: &str = ".tmp-";

/// `20261116T153000Z` for a time in nanoseconds since the epoch.
#[must_use]
pub fn stamp(now_ns: u64) -> String {
    octoplus_common::time::from_ns(now_ns).map_or_else(
        |_| "00000000T000000Z".to_owned(),
        |t| t.strftime("%Y%m%dT%H%M%SZ").to_string(),
    )
}

fn is_stamp(name: &str) -> bool {
    name.len() == 16
        && name.as_bytes()[8] == b'T'
        && name.ends_with('Z')
        && name
            .bytes()
            .enumerate()
            .all(|(i, b)| i == 8 || i == 15 || b.is_ascii_digit())
}

/// Finished backups, newest first.
///
/// # Errors
/// [`DbError::Io`] if the folder cannot be listed (a missing folder is "no backups").
pub fn list(backups: &Path) -> Result<Vec<PathBuf>, DbError> {
    let Ok(rd) = std::fs::read_dir(backups) else {
        return Ok(Vec::new());
    };
    let mut v: Vec<PathBuf> = rd
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| e.file_name().to_str().is_some_and(is_stamp))
        .map(|e| e.path())
        .collect();
    v.sort();
    v.reverse();
    Ok(v)
}

fn sync_dir(p: &Path) {
    if let Ok(d) = std::fs::File::open(p) {
        let _ = d.sync_all();
    }
}

impl Store {
    /// Takes a backup now (named by `now_ns`) and rotates old ones. Returns the new folder.
    ///
    /// # Errors
    /// [`DbError`] if any step fails; no partial backup remains and older ones are untouched.
    pub async fn backup(self: &Arc<Self>, now_ns: u64) -> Result<PathBuf, DbError> {
        let root = self.paths.backups.clone();
        std::fs::create_dir_all(&root)?;
        sweep_stale(&root);
        let name = stamp(now_ns);
        let tmp = root.join(format!("{TMP_PREFIX}{name}"));
        let fin = root.join(&name);
        if fin.exists() {
            return Ok(fin); // one backup per second is enough
        }
        std::fs::create_dir_all(&tmp)?;
        let target = tmp.join(BACKUP_DB);
        let target_s = target.to_string_lossy().into_owned();
        let made = self
            .db
            .writer()
            .maintain(move |c| {
                c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
                c.execute("VACUUM INTO ?1", params![target_s])?;
                Ok(())
            })
            .await;
        let finish = made.and_then(|()| {
            std::fs::File::open(&target)?.sync_all()?;
            sync_dir(&tmp);
            std::fs::rename(&tmp, &fin)?;
            sync_dir(&root);
            Ok(())
        });
        if let Err(e) = finish {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(e);
        }
        for old in list(&root)?.into_iter().skip(self.cfg.backup_keep) {
            let _ = std::fs::remove_dir_all(old);
        }
        Ok(fin)
    }
}

/// Removes folders of backups that were being written when a run crashed.
fn sweep_stale(root: &Path) {
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.filter_map(Result::ok) {
            if e.file_name().to_string_lossy().starts_with(TMP_PREFIX) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
}
