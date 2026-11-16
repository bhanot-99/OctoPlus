//! `--record <file>`: another subscriber on the bus, after the gate, so paused and excluded
//! data never reach the file. Everything is anonymised before it is written; there is no raw
//! mode (FR-MON-12, rules §8.7).

use std::fs::{File, OpenOptions};
use std::io::BufWriter;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use neuroos_health::Health;
use neuroos_proto::dump::{DumpError, DumpWriter};
use neuroos_proto::v1::HealthStatus;
use tokio::sync::watch;

use crate::anonymise::Anonymiser;
use crate::bus::Subscription;

/// How often buffered events reach the disk, so a crash loses at most this much.
const FLUSH_EVERY: Duration = Duration::from_secs(1);

/// Creates the dump file, mode 0600, refusing to overwrite an existing one.
///
/// # Errors
/// [`DumpError::Io`] if the file exists or cannot be created.
pub fn create_dump(path: &Path) -> Result<DumpWriter<BufWriter<File>>, DumpError> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    DumpWriter::new(BufWriter::new(file))
}

/// Writes every event from `sub` (anonymised) until `stop`, then drains what is queued and
/// writes the end record. Returns the number of events recorded.
///
/// # Errors
/// [`DumpError`] if the file cannot be written; health is marked DEGRADED first.
pub async fn run_recorder(
    sub: Subscription,
    mut writer: DumpWriter<BufWriter<File>>,
    anonymiser: Anonymiser,
    health: Arc<Health>,
    mut stop: watch::Receiver<bool>,
) -> Result<u64, DumpError> {
    health.set("recorder", HealthStatus::Ok, "recording");
    let mut tick = tokio::time::interval(FLUSH_EVERY);
    let result = loop {
        tokio::select! {
            ev = sub.next() => {
                if let Err(e) = writer.write(&anonymiser.anonymise(ev)) { break Err(e); }
            }
            _ = tick.tick() => {
                if let Err(e) = writer.flush() { break Err(e); }
            }
            () = crate::supervisor::stopped(&mut stop) => {
                let mut drained = Ok(());
                while let Some(ev) = sub.try_next() {
                    if let Err(e) = writer.write(&anonymiser.anonymise(ev)) { drained = Err(e); break; }
                }
                break drained;
            }
        }
    };
    match result {
        Ok(()) => writer.finish(),
        Err(e) => {
            health.set("recorder", HealthStatus::Degraded, "recording failed");
            Err(e)
        }
    }
}
