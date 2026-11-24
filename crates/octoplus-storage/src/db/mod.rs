//! The one SQLite database of C3 (FR-STO-04, ADR-0004): migrations, one writer thread and a pool
//! of read-only connections. Nothing outside this module opens the database file.

mod readers;
mod schema;
mod writer;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::Connection;

pub use readers::ReaderPool;
pub use schema::{LATEST_VERSION, migrate};
pub use writer::{MatrixOp, PostCommit, Writer};

/// Why a database operation failed. Messages never contain stored text (R0-10).
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// SQLite reported an error.
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The file was written by a newer version of OctoPlus.
    #[error("database schema version {found} is newer than this build supports ({supported})")]
    TooNew {
        /// Version found in the file.
        found: i64,
        /// Newest version this build knows.
        supported: i64,
    },
    /// The disk is full; ingest should pause and retry (FI).
    #[error("the disk is full")]
    DiskFull,
    /// The writer thread is gone (shutting down).
    #[error("the database writer has stopped")]
    WriterGone,
    /// The file system failed.
    #[error("database i/o failed: {0}")]
    Io(#[from] std::io::Error),
    /// A taint value was missing or out of range (R0-4).
    #[error("taint value is missing or invalid")]
    Taint,
    /// The request itself was invalid.
    #[error("invalid request: {0}")]
    Invalid(&'static str),
}

impl DbError {
    /// Classifies a SQLite error, picking out "disk full".
    #[must_use]
    pub fn from_sqlite(e: rusqlite::Error) -> Self {
        if let rusqlite::Error::SqliteFailure(f, _) = &e
            && f.code == rusqlite::ErrorCode::DiskFull
        {
            return Self::DiskFull;
        }
        Self::Sqlite(e)
    }
}

/// Pragmas every connection gets (Architecture §7.2).
fn base_pragmas(conn: &Connection) -> Result<(), DbError> {
    conn.pragma_update(None, "foreign_keys", true)?;
    // Temporary tables and sort spills stay in memory: C3's sandbox grants no temp folder
    // (a temp file failed every web page, BUG-119), and personal text never lands in an
    // unencrypted file.
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

/// The database: a writer and a reader pool over one file.
#[derive(Debug)]
pub struct Db {
    path: PathBuf,
    writer: Writer,
    readers: Arc<ReaderPool>,
}

impl Db {
    /// Opens (creating if needed) the database at `path`, applies migrations, starts the writer.
    /// `post_commit` receives the vector-matrix changes of every committed batch.
    ///
    /// # Errors
    /// [`DbError`] if the file cannot be opened, is newer than this build, or a migration fails.
    pub fn open(
        path: &Path,
        readers: usize,
        queue: usize,
        post_commit: PostCommit,
    ) -> Result<Self, DbError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        // auto_vacuum only takes effect on a new file or after VACUUM; set before the first table.
        conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
        // Deleted text and vectors are overwritten, not left in free pages (FR-STO-13).
        conn.pragma_update(None, "secure_delete", true)?;
        conn.pragma_update(None, "journal_size_limit", 64 * 1024 * 1024_i64)?;
        conn.pragma_update(None, "cache_size", -8192_i64)?; // 8 MiB, bounded (rules §9.7)
        base_pragmas(&conn)?;
        migrate(&mut conn)?;
        let pool = Arc::new(ReaderPool::open(path, readers)?);
        let writer = Writer::start(conn, queue, post_commit)?;
        Ok(Self {
            path: path.to_path_buf(),
            writer,
            readers: pool,
        })
    }

    /// The database file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The writer (the only read-write connection).
    #[must_use]
    pub fn writer(&self) -> &Writer {
        &self.writer
    }

    /// The reader pool.
    #[must_use]
    pub fn readers(&self) -> &Arc<ReaderPool> {
        &self.readers
    }

    /// Runs `f` on a pooled read-only connection (blocking; call from the blocking pool).
    ///
    /// # Errors
    /// Whatever `f` returns, or [`DbError`] if no connection could be taken.
    pub fn read<T>(&self, f: impl FnOnce(&Connection) -> Result<T, DbError>) -> Result<T, DbError> {
        self.readers.with(f)
    }

    /// `PRAGMA integrity_check` through a reader; `true` when the database is sound.
    ///
    /// # Errors
    /// [`DbError`] if the check cannot run.
    pub fn integrity_ok(&self) -> Result<bool, DbError> {
        self.read(|c| {
            let r: String = c.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
            Ok(r == "ok")
        })
    }
}
