//! A small pool of read-only connections. WAL lets readers run while the writer commits, so a
//! query never waits for ingest (ADR-0004 item 3).

use std::path::Path;
use std::sync::{Condvar, Mutex, PoisonError};

use rusqlite::{Connection, OpenFlags};

use super::{DbError, base_pragmas};

/// Read-only connections, handed out one at a time.
#[derive(Debug)]
pub struct ReaderPool {
    idle: Mutex<Vec<Connection>>,
    available: Condvar,
}

impl ReaderPool {
    /// Opens `n` (at least 1) read-only connections.
    ///
    /// # Errors
    /// [`DbError`] if a connection cannot be opened.
    pub fn open(path: &Path, n: usize) -> Result<Self, DbError> {
        let mut conns = Vec::new();
        for _ in 0..n.max(1) {
            let c = Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            base_pragmas(&c)?;
            c.pragma_update(None, "query_only", true)?;
            c.pragma_update(None, "cache_size", -4096_i64)?;
            conns.push(c);
        }
        Ok(Self {
            idle: Mutex::new(conns),
            available: Condvar::new(),
        })
    }

    /// Runs `f` on a connection, waiting for one if all are busy, and returns it afterwards.
    ///
    /// # Errors
    /// Whatever `f` returns.
    pub fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T, DbError>) -> Result<T, DbError> {
        let conn = {
            let mut idle = self.idle.lock().unwrap_or_else(PoisonError::into_inner);
            loop {
                if let Some(c) = idle.pop() {
                    break c;
                }
                idle = self
                    .available
                    .wait(idle)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        };
        // Returned on drop, so a panicking closure cannot shrink the pool.
        struct Back<'a> {
            pool: &'a ReaderPool,
            conn: Option<Connection>,
        }
        impl Drop for Back<'_> {
            fn drop(&mut self) {
                if let Some(c) = self.conn.take() {
                    self.pool
                        .idle
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(c);
                    self.pool.available.notify_one();
                }
            }
        }
        let back = Back {
            pool: self,
            conn: Some(conn),
        };
        match back.conn.as_ref() {
            Some(c) => f(c),
            None => Err(DbError::WriterGone),
        }
    }

    /// Connections currently idle (health, tests).
    #[must_use]
    pub fn idle_count(&self) -> usize {
        self.idle
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}
