//! Per-part health (FR-HLT-02, Architecture §6.6, §12).
//!
//! A part reports **real** status: `STARTING` until it says it is ready, then
//! the worst status among its subsystems. Subsystem status is either set by the
//! part as things happen or computed at scrape time by a probe (for example
//! "is my client to C3 connected?"), so a stopped subsystem shows DEGRADED
//! without anyone remembering to update it. A lesson from the prototype: every
//! health endpoint always said OK (docs/lessons-from-v1.md §1).

pub mod service;

pub use service::{CommonArgs, ServiceBuilder, ServiceContext, ServiceError};

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use neuroos_ipc::{Handler, HandlerError, Request, StreamSink};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{
    Counter, Gauge, HealthResponse, HealthStatus, Histogram as WireHistogram, SubsystemHealth,
};

/// Upper bounds of the latency buckets, in nanoseconds. Spans the PRD §6.1
/// targets from sub-millisecond IPC to the 30 s generation deadline.
pub const BUCKETS_NS: [u64; 16] = [
    100_000,
    250_000,
    500_000,
    1_000_000,
    2_500_000,
    5_000_000,
    10_000_000,
    25_000_000,
    50_000_000,
    100_000_000,
    250_000_000,
    500_000_000,
    1_000_000_000,
    2_500_000_000,
    10_000_000_000,
    30_000_000_000,
];

/// A lock-free latency histogram with fixed buckets plus an overflow bucket.
#[derive(Debug)]
pub struct LatencyHistogram {
    counts: [AtomicU64; BUCKETS_NS.len() + 1],
    sum_ns: AtomicU64,
}

impl Default for LatencyHistogram {
    fn default() -> Self {
        Self {
            counts: std::array::from_fn(|_| AtomicU64::new(0)),
            sum_ns: AtomicU64::new(0),
        }
    }
}

impl LatencyHistogram {
    /// Records one sample.
    pub fn record(&self, d: Duration) {
        let ns = u64::try_from(d.as_nanos()).unwrap_or(u64::MAX);
        let i = BUCKETS_NS.partition_point(|&b| b < ns);
        self.counts[i].fetch_add(1, Ordering::Relaxed);
        self.sum_ns.fetch_add(ns, Ordering::Relaxed);
    }

    fn to_wire(&self, name: &str) -> WireHistogram {
        let counts: Vec<u64> = self
            .counts
            .iter()
            .map(|c| c.load(Ordering::Relaxed))
            .collect();
        WireHistogram {
            name: name.to_owned(),
            upper_bounds_ns: BUCKETS_NS.to_vec(),
            sample_count: counts.iter().sum(),
            counts,
            sum_ns: self.sum_ns.load(Ordering::Relaxed),
        }
    }
}

/// Computes a subsystem's status at scrape time.
pub type Probe = Box<dyn Fn() -> (HealthStatus, &'static str) + Send + Sync>;

enum Source {
    Fixed(HealthStatus, &'static str),
    Probe(Probe),
}

/// Health state of one part. Cheap to share; every method is thread-safe.
pub struct Health {
    component: String,
    version: String,
    started_at_ns: u64,
    ready: AtomicBool,
    subsystems: Mutex<BTreeMap<String, Source>>,
    counters: Mutex<BTreeMap<String, Arc<AtomicU64>>>,
    gauges: Mutex<BTreeMap<String, f64>>,
    histograms: Mutex<BTreeMap<String, Arc<LatencyHistogram>>>,
}

impl std::fmt::Debug for Health {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Health")
            .field("component", &self.component)
            .field("ready", &self.ready.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

fn rank(s: HealthStatus) -> u8 {
    match s {
        HealthStatus::Ok => 0,
        HealthStatus::Starting => 1,
        HealthStatus::Degraded => 2,
        // UNSPECIFIED from a probe is a bug; treat it as the worst case.
        HealthStatus::Down | HealthStatus::Unspecified => 3,
    }
}

/// The worse of two statuses (DOWN > DEGRADED > STARTING > OK).
#[must_use]
pub fn worst(a: HealthStatus, b: HealthStatus) -> HealthStatus {
    if rank(b) > rank(a) { b } else { a }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Health {
    /// New state for `component`; reports STARTING until [`Health::set_ready`].
    #[must_use]
    pub fn new(component: &str, version: &str) -> Arc<Self> {
        Arc::new(Self {
            component: component.to_owned(),
            version: version.to_owned(),
            started_at_ns: now_ns(),
            ready: AtomicBool::new(false),
            subsystems: Mutex::new(BTreeMap::new()),
            counters: Mutex::new(BTreeMap::new()),
            gauges: Mutex::new(BTreeMap::new()),
            histograms: Mutex::new(BTreeMap::new()),
        })
    }

    /// Marks start-up complete.
    pub fn set_ready(&self) {
        self.ready.store(true, Ordering::Release);
    }

    /// Sets a subsystem's status. `detail` is fixed text, never user content.
    pub fn set(&self, subsystem: &str, status: HealthStatus, detail: &'static str) {
        lock(&self.subsystems).insert(subsystem.to_owned(), Source::Fixed(status, detail));
    }

    /// Registers a probe evaluated on every scrape.
    pub fn probe(&self, subsystem: &str, probe: Probe) {
        lock(&self.subsystems).insert(subsystem.to_owned(), Source::Probe(probe));
    }

    /// A named counter (created on first use).
    #[must_use]
    pub fn counter(&self, name: &str) -> Arc<AtomicU64> {
        Arc::clone(lock(&self.counters).entry(name.to_owned()).or_default())
    }

    /// Sets a gauge.
    pub fn gauge(&self, name: &str, value: f64) {
        lock(&self.gauges).insert(name.to_owned(), value);
    }

    /// A named latency histogram (created on first use).
    #[must_use]
    pub fn histogram(&self, name: &str) -> Arc<LatencyHistogram> {
        Arc::clone(lock(&self.histograms).entry(name.to_owned()).or_default())
    }

    /// The current health, with probes evaluated now.
    #[must_use]
    pub fn snapshot(&self) -> HealthResponse {
        let subsystems: Vec<SubsystemHealth> = lock(&self.subsystems)
            .iter()
            .map(|(name, src)| {
                let (status, detail) = match src {
                    Source::Fixed(s, d) => (*s, *d),
                    Source::Probe(p) => p(),
                };
                // UNSPECIFIED is never a real answer: report the subsystem as DOWN.
                let (status, detail) = if status == HealthStatus::Unspecified {
                    (HealthStatus::Down, "invalid status")
                } else {
                    (status, detail)
                };
                SubsystemHealth {
                    name: name.clone(),
                    status: status.into(),
                    detail: detail.to_owned(),
                }
            })
            .collect();
        let status = if self.ready.load(Ordering::Acquire) {
            subsystems
                .iter()
                .map(|s| HealthStatus::try_from(s.status).unwrap_or(HealthStatus::Down))
                .fold(HealthStatus::Ok, worst)
        } else {
            HealthStatus::Starting
        };
        HealthResponse {
            component: self.component.clone(),
            version: self.version.clone(),
            status: status.into(),
            started_at_ns: self.started_at_ns,
            sampled_at_ns: now_ns(),
            rss_bytes: rss_bytes().unwrap_or(0),
            subsystems,
            counters: lock(&self.counters)
                .iter()
                .map(|(n, v)| Counter {
                    name: n.clone(),
                    value: v.load(Ordering::Relaxed),
                })
                .collect(),
            gauges: lock(&self.gauges)
                .iter()
                .map(|(n, v)| Gauge {
                    name: n.clone(),
                    value: *v,
                })
                .collect(),
            histograms: lock(&self.histograms)
                .iter()
                .map(|(n, h)| h.to_wire(n))
                .collect(),
        }
    }
}

/// Serves `HealthRequest` on a part's `health.sock`.
#[derive(Debug, Clone)]
pub struct HealthHandler(pub Arc<Health>);

impl Handler for HealthHandler {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::HealthRequest(_) => Ok(Body::HealthResponse(self.0.snapshot())),
            _ => Err(HandlerError::unsupported()),
        }
    }
}

/// Resident memory of this process from `/proc/self/statm`.
#[must_use]
pub fn rss_bytes() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/self/statm").ok()?;
    parse_statm_rss(&text, rustix::param::page_size())
}

/// Second field of statm (resident pages) times the page size.
#[must_use]
pub fn parse_statm_rss(text: &str, page_size: usize) -> Option<u64> {
    let pages: u64 = text.split_whitespace().nth(1)?.parse().ok()?;
    pages.checked_mul(u64::try_from(page_size).ok()?)
}

fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_nanos()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn status(r: &HealthResponse) -> HealthStatus {
        HealthStatus::try_from(r.status).unwrap()
    }

    #[test]
    fn starting_until_ready_even_if_subsystems_ok() {
        let h = Health::new("neuroos-test", "0.1.0");
        h.set("db", HealthStatus::Ok, "");
        assert_eq!(status(&h.snapshot()), HealthStatus::Starting);
        h.set_ready();
        assert_eq!(status(&h.snapshot()), HealthStatus::Ok);
    }

    #[test]
    fn worst_subsystem_wins() {
        let h = Health::new("p", "v");
        h.set_ready();
        h.set("a", HealthStatus::Ok, "");
        h.set("b", HealthStatus::Degraded, "peer unavailable");
        let s = h.snapshot();
        assert_eq!(status(&s), HealthStatus::Degraded);
        assert_eq!(s.subsystems[1].detail, "peer unavailable");
        h.set("c", HealthStatus::Down, "crashed");
        assert_eq!(status(&h.snapshot()), HealthStatus::Down);
        h.set("c", HealthStatus::Ok, "");
        h.set("b", HealthStatus::Ok, "");
        assert_eq!(status(&h.snapshot()), HealthStatus::Ok);
    }

    #[test]
    fn probes_are_evaluated_at_scrape_time() {
        let h = Health::new("p", "v");
        h.set_ready();
        let up = Arc::new(AtomicBool::new(true));
        let u = Arc::clone(&up);
        h.probe(
            "storage-client",
            Box::new(move || {
                if u.load(Ordering::Relaxed) {
                    (HealthStatus::Ok, "")
                } else {
                    (HealthStatus::Degraded, "storage unreachable")
                }
            }),
        );
        assert_eq!(status(&h.snapshot()), HealthStatus::Ok);
        up.store(false, Ordering::Relaxed);
        assert_eq!(status(&h.snapshot()), HealthStatus::Degraded);
    }

    #[test]
    fn unspecified_from_a_probe_is_treated_as_down() {
        let h = Health::new("p", "v");
        h.set_ready();
        h.probe("bug", Box::new(|| (HealthStatus::Unspecified, "")));
        let s = h.snapshot();
        assert_eq!(status(&s), HealthStatus::Down);
        assert_eq!(s.subsystems[0].status, i32::from(HealthStatus::Down));
        assert_eq!(s.subsystems[0].detail, "invalid status");
    }

    #[test]
    fn counters_gauges_histograms_and_rss() {
        let h = Health::new("p", "v");
        h.counter("requests").fetch_add(3, Ordering::Relaxed);
        h.counter("requests").fetch_add(1, Ordering::Relaxed);
        h.gauge("chunks", 42.0);
        let hist = h.histogram("ipc");
        hist.record(Duration::from_micros(50));
        hist.record(Duration::from_millis(3));
        hist.record(Duration::from_secs(60));
        let s = h.snapshot();
        assert_eq!(s.counters[0].value, 4);
        assert!((s.gauges[0].value - 42.0).abs() < f64::EPSILON);
        let w = &s.histograms[0];
        assert_eq!(w.sample_count, 3);
        assert_eq!(w.counts.len(), w.upper_bounds_ns.len() + 1);
        assert_eq!(w.counts[0], 1);
        assert_eq!(w.counts[5], 1); // <= 5 ms
        assert_eq!(*w.counts.last().unwrap(), 1); // overflow
        assert!(s.rss_bytes > 0, "real RSS, not a constant");
        assert!(s.sampled_at_ns >= s.started_at_ns);
        assert_eq!(s.component, "p");
        assert!(format!("{h:?}").contains("Health"));
    }

    #[test]
    fn statm_parsing() {
        assert_eq!(parse_statm_rss("100 25 3 1 0 20 0\n", 4096), Some(102_400));
        assert_eq!(parse_statm_rss("100\n", 4096), None);
        assert_eq!(parse_statm_rss("a b c", 4096), None);
        assert_eq!(parse_statm_rss("1 18446744073709551615", 4096), None);
    }

    #[test]
    fn worst_table() {
        use HealthStatus::{Degraded, Down, Ok, Starting, Unspecified};
        let cases = [
            (Ok, Ok, Ok),
            (Ok, Starting, Starting),
            (Starting, Degraded, Degraded),
            (Degraded, Ok, Degraded),
            (Down, Degraded, Down),
            (Ok, Unspecified, Unspecified),
        ];
        for (a, b, want) in cases {
            assert_eq!(worst(a, b), want, "{a:?} {b:?}");
        }
    }

    proptest! {
        #[test]
        fn histogram_buckets_cover_every_sample(ns in proptest::collection::vec(any::<u64>(), 0..64)) {
            let h = LatencyHistogram::default();
            for n in &ns { h.record(Duration::from_nanos(*n)); }
            let w = h.to_wire("x");
            prop_assert_eq!(w.sample_count, ns.len() as u64);
            // Each sample sits in the first bucket whose bound is >= it.
            for n in &ns {
                let i = BUCKETS_NS.partition_point(|&b| b < *n);
                prop_assert!(w.counts[i] > 0);
            }
        }
    }
}
