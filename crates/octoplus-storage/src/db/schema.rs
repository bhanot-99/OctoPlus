//! Ordered migrations (FR-STO-04). Each file runs in one transaction and is recorded in
//! `schema_migrations`; a database newer than this build is refused, never downgraded.

use rusqlite::Connection;

use super::DbError;

/// `(version, name, sql)` in order. New schema changes are new files, never edits.
const MIGRATIONS: &[(i64, &str, &str)] =
    &[(1, "init", include_str!("../../migrations/0001_init.sql"))];

/// The newest schema version this build writes.
pub const LATEST_VERSION: i64 = 1;

/// Applies every migration not yet applied.
///
/// # Errors
/// [`DbError::TooNew`] if the file is from a newer build; [`DbError::Sqlite`] if a migration fails.
pub fn migrate(conn: &mut Connection) -> Result<(), DbError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_ns INTEGER NOT NULL
        )",
    )?;
    let current: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |r| r.get(0),
    )?;
    if current > LATEST_VERSION {
        return Err(DbError::TooNew {
            found: current,
            supported: LATEST_VERSION,
        });
    }
    for (version, name, sql) in MIGRATIONS.iter().filter(|(v, _, _)| *v > current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute(
            "INSERT INTO schema_migrations (version, name, applied_ns) VALUES (?1, ?2, ?3)",
            rusqlite::params![version, name, now_ns()],
        )?;
        tx.commit()?;
    }
    Ok(())
}

fn now_ns() -> i64 {
    octoplus_common::time::now_ns()
        .ok()
        .and_then(|n| i64::try_from(n).ok())
        .unwrap_or(0)
}
