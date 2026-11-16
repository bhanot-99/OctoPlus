//! The event bus: sensors publish, subscribers (C3's stream, the recorder) each get a
//! bounded queue. A slow subscriber loses its *oldest* events and a counter says so;
//! it never blocks a sensor (AB-11, rules §9.7). The [`Gate`] is the only place an event
//! may be stopped or changed (AB-5, FR-MON-10).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use neuroos_health::{Health, LatencyHistogram};
use neuroos_proto::v1::MonitorEvent;
use tokio::sync::Notify;

use crate::clock::Clock;

/// Decides what leaves C1. The pause and exclusion rules are its only implementation
/// in production; sensors never know about it.
pub trait Gate: Send + Sync + 'static {
    /// `Some(event)` (possibly redacted) to publish, `None` to drop silently.
    fn admit(&self, event: MonitorEvent) -> Option<MonitorEvent>;
}

/// Lets everything through (tests and the foundation only).
#[derive(Debug, Default, Clone, Copy)]
pub struct PassGate;

impl Gate for PassGate {
    fn admit(&self, event: MonitorEvent) -> Option<MonitorEvent> {
        Some(event)
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug)]
struct Queue {
    items: Mutex<VecDeque<MonitorEvent>>,
    notify: Notify,
    dropped: AtomicU64,
}

/// One subscriber's view of the bus. Dropping it unsubscribes.
#[derive(Debug)]
pub struct Subscription {
    id: u64,
    queue: Arc<Queue>,
    bus: Arc<Bus>,
}

impl Subscription {
    /// The next event, waiting if none is queued.
    pub async fn next(&self) -> MonitorEvent {
        loop {
            if let Some(ev) = lock(&self.queue.items).pop_front() {
                return ev;
            }
            self.queue.notify.notified().await;
        }
    }

    /// The next event if one is already queued.
    #[must_use]
    pub fn try_next(&self) -> Option<MonitorEvent> {
        lock(&self.queue.items).pop_front()
    }

    /// Events this subscriber lost to overflow.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.queue.dropped.load(Ordering::Relaxed)
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let mut subs = lock(&self.bus.subs);
        subs.retain(|(id, _)| *id != self.id);
        self.bus.health.gauge("subscribers", subs.len() as f64);
    }
}

/// The bus. Cheap to share.
pub struct Bus {
    gate: Arc<dyn Gate>,
    clock: Arc<dyn Clock>,
    health: Arc<Health>,
    capacity: usize,
    subs: Mutex<Vec<(u64, Arc<Queue>)>>,
    next_id: AtomicU64,
    emitted: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
    latency: Arc<LatencyHistogram>,
}

impl std::fmt::Debug for Bus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bus")
            .field("capacity", &self.capacity)
            .finish_non_exhaustive()
    }
}

impl Bus {
    /// A bus whose subscribers each hold at most `capacity` events (at least 1).
    #[must_use]
    pub fn new(
        capacity: usize,
        gate: Arc<dyn Gate>,
        clock: Arc<dyn Clock>,
        health: Arc<Health>,
    ) -> Arc<Self> {
        health.gauge("subscribers", 0.0);
        Arc::new(Self {
            gate,
            clock,
            capacity: capacity.max(1),
            subs: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
            emitted: health.counter("events_emitted"),
            dropped: health.counter("events_dropped"),
            latency: health.histogram("capture_latency"),
            health,
        })
    }

    /// Publishes one observation through the gate to every subscriber.
    /// Never blocks and never fails: a full queue drops its oldest event.
    pub fn publish(&self, event: MonitorEvent) {
        let observed = event.observed_at_ns;
        let Some(event) = self.gate.admit(event) else {
            return;
        };
        let subs = lock(&self.subs);
        for (_, q) in subs.iter() {
            let mut items = lock(&q.items);
            if items.len() >= self.capacity {
                items.pop_front();
                q.dropped.fetch_add(1, Ordering::Relaxed);
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
            items.push_back(event.clone());
            drop(items);
            q.notify.notify_one();
        }
        drop(subs);
        self.emitted.fetch_add(1, Ordering::Relaxed);
        // NFR-PERF-07: observation to enqueue.
        let lag = self.clock.now_ns().saturating_sub(observed);
        self.latency.record(Duration::from_nanos(lag));
    }

    /// How many subscriptions are open.
    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        lock(&self.subs).len()
    }

    /// Opens a new subscription.
    #[must_use]
    pub fn subscribe(self: &Arc<Self>) -> Subscription {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let queue = Arc::new(Queue {
            items: Mutex::new(VecDeque::new()),
            notify: Notify::new(),
            dropped: AtomicU64::new(0),
        });
        let mut subs = lock(&self.subs);
        subs.push((id, Arc::clone(&queue)));
        self.health.gauge("subscribers", subs.len() as f64);
        drop(subs);
        Subscription {
            id,
            queue,
            bus: Arc::clone(self),
        }
    }
}
