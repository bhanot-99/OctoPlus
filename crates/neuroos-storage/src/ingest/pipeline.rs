//! The live ingest path: C1's stream → filter → embed → one writer job per batch (FR-STO-01..03,
//! P4-S01/S02). The stream reader never blocks on the database: it hands events to a bounded
//! channel and counts what it had to drop (rules §9.7).

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use neuroos_ipc::ReconnectingClient;
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{HealthStatus, MonitorEvent, MonitorSubscribe};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use super::apply::{Prepared, apply_batch};
use super::filter::IngestFilter;
use super::{Demotion, Output};
use crate::db::DbError;
use crate::lifecycle::gc::trim_to_cap;
use crate::store::Store;

/// The stream is an hour-long request on the server side; the client asks for the same.
const STREAM_DEADLINE: Duration = Duration::from_secs(3600);
/// Events the reader may hand over before it starts dropping.
const EVENT_CHANNEL: usize = 4096;
/// Output batches kept while the disk is full; older ones are dropped beyond this.
const HELD_BATCHES: usize = 8;
/// Source tag stored with chunks that come from C1.
const SOURCE: &str = "c1";

/// What one ingest call did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IngestStats {
    /// Items promoted (entity + chunk).
    pub promoted: u64,
    /// Items the filter dropped.
    pub demoted: u64,
    /// Health samples stored.
    pub health: u64,
}

impl Store {
    /// Stores one batch of filter outputs: embeds the promoted texts on the blocking pool, then
    /// writes everything in one writer job, then trims to the chunk cap in the same job.
    ///
    /// # Errors
    /// [`DbError::DiskFull`] when the disk is full (the caller pauses and retries), or any other
    /// [`DbError`]. An embedding failure is not an error: the text is stored without a vector.
    pub async fn ingest(self: &Arc<Self>, outputs: Vec<Output>) -> Result<IngestStats, DbError> {
        self.ingest_from(outputs, SOURCE).await
    }

    /// As [`Store::ingest`], tagging the stored chunks with where they came from (`c1`, `c7`).
    ///
    /// # Errors
    /// As [`Store::ingest`].
    pub async fn ingest_from(
        self: &Arc<Self>,
        outputs: Vec<Output>,
        source: &'static str,
    ) -> Result<IngestStats, DbError> {
        let mut stats = IngestStats::default();
        for o in &outputs {
            match o {
                Output::Promote(_) => stats.promoted += 1,
                Output::Demoted(_) => stats.demoted += 1,
                Output::Health(_) => stats.health += 1,
                _ => {}
            }
        }
        let texts: Vec<String> = outputs
            .iter()
            .filter_map(|o| {
                if let Output::Promote(i) = o {
                    Some(i.text.clone())
                } else {
                    None
                }
            })
            .collect();
        let vectors = self.embed_blocking(texts).await;
        let model = self.embedder.id().to_owned();
        let mut vec_iter = vectors.into_iter();
        let prepared: Vec<Prepared> = outputs
            .into_iter()
            .map(|output| {
                let vector = match &output {
                    Output::Promote(_) => vec_iter.next().flatten().map(|v| (v, model.clone())),
                    _ => None,
                };
                Prepared { output, vector }
            })
            .collect();
        let cap = self.cfg.chunk_cap;
        self.db
            .writer()
            .exec(move |c| {
                let mut ops = apply_batch(c, &prepared, source)?;
                ops.extend(trim_to_cap(c, cap)?);
                Ok(ops)
            })
            .await?;
        Ok(stats)
    }

    /// Embeds texts off the runtime; a failure yields `None` per text (stored text-only, stale).
    pub(crate) async fn embed_blocking(
        self: &Arc<Self>,
        texts: Vec<String>,
    ) -> Vec<Option<Vec<f32>>> {
        if texts.is_empty() {
            return Vec::new();
        }
        let n = texts.len();
        let me = Arc::clone(self);
        let res = tokio::task::spawn_blocking(move || me.embedder.embed(&texts)).await;
        match res {
            Ok(Ok(v)) if v.len() == n => v.into_iter().map(Some).collect(),
            _ => {
                self.health
                    .counter("embed_failures")
                    .fetch_add(1, Ordering::Relaxed);
                vec![None; n]
            }
        }
    }
}

/// Starts the subscription reader and the batching processor. Both end when `stop` turns true.
#[must_use]
pub fn spawn(
    store: Arc<Store>,
    client: Arc<ReconnectingClient>,
    stop: watch::Receiver<bool>,
) -> Vec<JoinHandle<()>> {
    let (tx, rx) = mpsc::channel::<MonitorEvent>(EVENT_CHANNEL);
    let reader = tokio::spawn(read_stream(Arc::clone(&store), client, tx, stop.clone()));
    let processor = tokio::spawn(process(store, rx, stop));
    vec![reader, processor]
}

async fn read_stream(
    store: Arc<Store>,
    client: Arc<ReconnectingClient>,
    tx: mpsc::Sender<MonitorEvent>,
    mut stop: watch::Receiver<bool>,
) {
    let dropped = store.health.counter("events_dropped");
    let received = store.health.counter("events_received");
    loop {
        if *stop.borrow() {
            return;
        }
        let connected = match client.get().await {
            Ok(c) => c
                .stream(Body::MonitorSubscribe(MonitorSubscribe {}), STREAM_DEADLINE)
                .await
                .ok(),
            Err(_) => None,
        };
        let Some(mut stream) = connected else {
            tokio::select! {
                () = tokio::time::sleep(Duration::from_millis(500)) => continue,
                _ = stop.changed() => return,
            }
        };
        loop {
            tokio::select! {
                item = stream.next() => match item {
                    Some(Ok(Body::MonitorEvent(ev))) => {
                        received.fetch_add(1, Ordering::Relaxed);
                        if tx.try_send(ev).is_err() {
                            dropped.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Some(Ok(_)) => {}
                    // End frame, error or deadline: subscribe again after a short pause.
                    Some(Err(_)) | None => break,
                },
                _ = stop.changed() => return,
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn process(
    store: Arc<Store>,
    mut rx: mpsc::Receiver<MonitorEvent>,
    mut stop: watch::Receiver<bool>,
) {
    let mut filter = IngestFilter::new(&store.cfg);
    match store.load_counters().await {
        Ok(rows) => filter.restore(rows),
        Err(_) => store.health.set(
            "ingest",
            HealthStatus::Degraded,
            "promotion counters could not be loaded",
        ),
    }
    let mut pending: Vec<Output> = Vec::new();
    let mut held: Vec<Vec<Output>> = Vec::new();
    let mut events_in_batch = 0usize;
    let mut tick = tokio::time::interval(Duration::from_millis(store.cfg.ingest_flush_ms));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut epoch = store.forget_epoch.load(Ordering::SeqCst);
    loop {
        let now_epoch = store.forget_epoch.load(Ordering::SeqCst);
        if now_epoch != epoch {
            epoch = now_epoch;
            filter.reset_counters();
        }
        tokio::select! {
            ev = rx.recv() => {
                let Some(ev) = ev else { break };
                store.observe(&ev);
                pending.extend(filter.push(&ev));
                events_in_batch += 1;
                if events_in_batch >= store.cfg.ingest_batch {
                    flush(&store, &mut pending, &mut held).await;
                    events_in_batch = 0;
                }
            }
            _ = tick.tick() => {
                if !pending.is_empty() || !held.is_empty() {
                    flush(&store, &mut pending, &mut held).await;
                    events_in_batch = 0;
                }
            }
            _ = stop.changed() => break,
        }
    }
    pending.extend(filter.finish(now_ns()));
    flush(&store, &mut pending, &mut held).await;
}

fn now_ns() -> u64 {
    neuroos_common::time::now_ns().unwrap_or(0)
}

/// Writes held batches first (oldest first), then the new one. On a full disk everything stays
/// held (bounded) and ingest reports DEGRADED; queries are not affected (FI).
async fn flush(store: &Arc<Store>, pending: &mut Vec<Output>, held: &mut Vec<Vec<Output>>) {
    if !pending.is_empty() {
        held.push(std::mem::take(pending));
    }
    while let Some(batch) = held.first().cloned() {
        match store.ingest(batch.clone()).await {
            Ok(stats) => {
                held.remove(0);
                store.health.set("ingest", HealthStatus::Ok, "ok");
                store
                    .health
                    .counter("promoted")
                    .fetch_add(stats.promoted, Ordering::Relaxed);
                store
                    .health
                    .counter("demoted")
                    .fetch_add(stats.demoted, Ordering::Relaxed);
                count_demotions(store, &batch);
            }
            Err(DbError::DiskFull) => {
                store
                    .health
                    .set("ingest", HealthStatus::Degraded, "disk full: ingest paused");
                while held.len() > HELD_BATCHES {
                    held.remove(0);
                    store
                        .health
                        .counter("batches_dropped")
                        .fetch_add(1, Ordering::Relaxed);
                }
                return;
            }
            Err(_) => {
                held.remove(0);
                store
                    .health
                    .counter("batches_dropped")
                    .fetch_add(1, Ordering::Relaxed);
                store.health.set(
                    "ingest",
                    HealthStatus::Degraded,
                    "a batch could not be stored",
                );
            }
        }
    }
    store.refresh_gauges();
}

fn count_demotions(store: &Arc<Store>, batch: &[Output]) {
    for o in batch {
        if let Output::Demoted(d) = o {
            let name = match d {
                Demotion::OwnWindow => "demoted_own_window",
                Demotion::PassiveMedia => "demoted_passive_media",
                Demotion::BelowThreshold => "demoted_below_threshold",
                Demotion::Subprocess => "demoted_subprocess",
            };
            store.health.counter(name).fetch_add(1, Ordering::Relaxed);
        }
    }
}
