//! The ingest side of the spool (FR-STO-10, security.md §4.2): look at the folder C7 writes to,
//! validate each file with the strict parser in [`crate::spool`], store its text as
//! `external_documents` with `EXTERNAL_UNTRUSTED` forced whatever the file says, and delete the
//! file whether it was accepted or rejected. A bad file never stops C3.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use neuroos_health::Health;
use neuroos_proto::v1::{Domain, HealthStatus};
use neuroos_taint::TaintFlags;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::ingest::{Item, Output};
use crate::spool::{self, SpoolDoc};
use crate::store::Store;

/// Source tag for chunks that came through the spool.
const SOURCE: &str = "c7";
/// Longest chunk of a document, in characters (a few sentences; keeps one embedding meaningful).
pub const CHUNK_CHARS: usize = 1200;
/// Files handled per poll, so a flood cannot starve everything else.
const PER_POLL: usize = 32;

/// What happened to one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Stored.
    Ingested,
    /// Refused (and deleted).
    Rejected,
}

/// Reads at most `max + 1` bytes of a regular file that is not a symlink.
fn read_limited(path: &Path, max: u64) -> Option<Vec<u8>> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_file() {
        return None;
    }
    let mut buf = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(max.saturating_add(1))
        .read_to_end(&mut buf)
        .ok()?;
    Some(buf)
}

/// Turns a document into promoted items: one per chunk, all tainted `EXTERNAL_UNTRUSTED` (R0-4).
#[must_use]
pub fn document_items(doc: &SpoolDoc, now_ns: u64) -> Vec<Output> {
    let at = doc.fetched_at_ns.min(now_ns);
    spool::chunk_text(&doc.text, CHUNK_CHARS)
        .into_iter()
        .enumerate()
        .map(|(i, chunk)| {
            let text = if i == 0 && !doc.title.is_empty() {
                format!("{} — {chunk}", doc.title)
            } else {
                chunk
            };
            Output::Promote(Item {
                domain: Domain::ExternalDocuments,
                kind: "document".into(),
                label: doc.url.clone(),
                app_id: String::new(),
                text,
                taint: TaintFlags::EXTERNAL_UNTRUSTED,
                at_ns: at,
            })
        })
        .collect()
}

/// Handles one spool file and always removes it.
pub async fn ingest_file(store: &Arc<Store>, path: &Path, now_ns: u64) -> Verdict {
    let max = store.cfg.spool_max_bytes;
    let name_ok = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(spool::valid_name);
    // Why a file was refused is logged (never its text or URL, R0-10): a silent refusal hid
    // that no fetched page was ever stored (BUG-119).
    let verdict = match (name_ok, read_limited(path, max)) {
        (true, Some(bytes)) => match spool::parse_spool(&bytes, max) {
            Ok(doc) => match store
                .ingest_from(document_items(&doc, now_ns), SOURCE)
                .await
            {
                Ok(_) => Verdict::Ingested,
                Err(e) => {
                    tracing::warn!(reason = %e, "spool file not stored");
                    Verdict::Rejected
                }
            },
            Err(e) => {
                tracing::warn!(reason = ?e, "spool file refused");
                Verdict::Rejected
            }
        },
        (false, _) => {
            tracing::warn!("spool file refused: bad name");
            Verdict::Rejected
        }
        (true, None) => {
            tracing::warn!("spool file refused: unreadable or too large");
            Verdict::Rejected
        }
    };
    // A symlink is removed as a link, never followed; a directory is left alone.
    if std::fs::symlink_metadata(path).is_ok_and(|m| !m.file_type().is_dir()) {
        let _ = std::fs::remove_file(path);
    }
    verdict
}

async fn poll_once(store: &Arc<Store>, dir: &Path, health: &Health) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        health.set("spool", HealthStatus::Ok, "no spool folder");
        return;
    };
    let files: Vec<PathBuf> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .take(PER_POLL)
        .collect();
    health.set("spool", HealthStatus::Ok, "ok");
    for f in files {
        let now = neuroos_common::time::now_ns().unwrap_or(0);
        match ingest_file(store, &f, now).await {
            Verdict::Ingested => health
                .counter("spool_ingested")
                .fetch_add(1, Ordering::Relaxed),
            Verdict::Rejected => health
                .counter("spool_rejected")
                .fetch_add(1, Ordering::Relaxed),
        };
    }
}

/// Starts the spool poller; it ends when `stop` turns true.
#[must_use]
pub fn spawn(
    store: Arc<Store>,
    dir: PathBuf,
    mut stop: watch::Receiver<bool>,
) -> Vec<JoinHandle<()>> {
    let every = Duration::from_secs(store.cfg.spool_poll_s);
    let health = Arc::clone(&store.health);
    vec![tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = tick.tick() => poll_once(&store, &dir, &health).await,
                _ = stop.changed() => return,
            }
        }
    })]
}
