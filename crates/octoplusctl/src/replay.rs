//! Replay of recorded, anonymised context monitor dumps (FR-MON-12, P3-S06).
//!
//! Provides CLI replay functionality: printing dump events to stdout (plain text
//! or JSON) or serving them over a Unix domain socket so clients (such as C3's
//! replay test harness) receive events just like a live context monitor.

use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use octoplus_ipc::server::{Handler, HandlerError, Request, Server, ServerConfig, StreamSink};
use octoplus_proto::dump::{DumpError, DumpReader};
use octoplus_proto::v1::envelope::Body;
use octoplus_proto::v1::monitor_event::Event;
use octoplus_proto::v1::{Ack, ErrorCode, MonitorEvent};

/// Validates that replay speed is a non-negative number and not NaN (FR-MON-12).
///
/// # Errors
/// Returns an error message if `s` is not a valid non-negative float.
pub fn validate_speed(s: &str) -> Result<f64, String> {
    let v: f64 = s.parse().map_err(|e| format!("invalid number: {e}"))?;
    if v.is_nan() || v < 0.0 {
        return Err("speed must be non-negative and not NaN".to_string());
    }
    Ok(v)
}

/// Returns the oneof variant name and debug representation of the inner message (FR-MON-12).
#[must_use]
pub fn event_kind_and_details(ev: &MonitorEvent) -> (&'static str, String) {
    match &ev.event {
        Some(Event::Focus(m)) => ("focus", format!("{m:?}")),
        Some(Event::Idle(m)) => ("idle", format!("{m:?}")),
        Some(Event::Lock(m)) => ("lock", format!("{m:?}")),
        Some(Event::Media(m)) => ("media", format!("{m:?}")),
        Some(Event::Resources(m)) => ("resources", format!("{m:?}")),
        Some(Event::Disk(m)) => ("disk", format!("{m:?}")),
        Some(Event::Battery(m)) => ("battery", format!("{m:?}")),
        Some(Event::Thermal(m)) => ("thermal", format!("{m:?}")),
        Some(Event::ProcessTree(m)) => ("process_tree", format!("{m:?}")),
        Some(Event::File(m)) => ("file", format!("{m:?}")),
        None => ("none", String::new()),
    }
}

/// Reads all events from a dump file into memory, validating header and end record (FR-MON-12).
///
/// # Errors
/// Returns [`DumpError`] on I/O, format, truncation, or count mismatch failure.
pub fn load_dump(path: &Path) -> Result<Vec<MonitorEvent>, DumpError> {
    let file = File::open(path)?;
    let mut reader = DumpReader::new(file)?;
    let mut events = Vec::new();
    while let Some(ev) = reader.next_event()? {
        events.push(ev);
    }
    Ok(events)
}

/// Replay handler for socket subscribers (P3-S06).
#[derive(Debug, Clone)]
struct ReplayHandler {
    events: Arc<Vec<MonitorEvent>>,
    speed: f64,
}

impl Handler for ReplayHandler {
    async fn handle(&self, req: Request, sink: StreamSink) -> Result<Body, HandlerError> {
        if !matches!(req.body, Body::MonitorSubscribe(_)) {
            return Err(HandlerError::new(
                ErrorCode::InvalidRequest,
                "expected MonitorSubscribe on stream socket",
                false,
            ));
        }

        let mut prev_ns: Option<u64> = None;
        for ev in self.events.iter() {
            if let Some(prev) = prev_ns
                && self.speed > 0.0
                && ev.observed_at_ns > prev
            {
                #[allow(clippy::cast_precision_loss)]
                let delta_ns = (ev.observed_at_ns - prev) as f64 / self.speed;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let dur = Duration::from_nanos(delta_ns as u64);
                tokio::time::sleep(dur).await;
            }
            prev_ns = Some(ev.observed_at_ns);

            if sink.send(Body::MonitorEvent(ev.clone())).await.is_err() {
                break;
            }
        }

        Ok(Body::Ack(Ack {}))
    }
}

/// Prints loaded events to stdout honoring timing delays (FR-MON-12).
async fn print_events(events: &[MonitorEvent], speed: f64, json: bool) {
    let mut prev_ns: Option<u64> = None;
    for ev in events {
        if let Some(prev) = prev_ns
            && speed > 0.0
            && ev.observed_at_ns > prev
        {
            #[allow(clippy::cast_precision_loss)]
            let delta_ns = (ev.observed_at_ns - prev) as f64 / speed;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let dur = Duration::from_nanos(delta_ns as u64);
            tokio::time::sleep(dur).await;
        }
        prev_ns = Some(ev.observed_at_ns);

        let (kind, details) = event_kind_and_details(ev);
        if json {
            let obj = serde_json::json!({
                "t_ns": ev.observed_at_ns,
                "kind": kind,
                "event": details,
            });
            println!("{obj}");
        } else {
            println!("t={} {} {}", ev.observed_at_ns, kind, details);
        }
    }
}

/// Serves dump events over a Unix domain stream socket (P3-S06).
async fn serve_events(sock: &Path, events: Vec<MonitorEvent>, speed: f64) -> i32 {
    let parent = match sock.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    if let Err(e) = octoplus_ipc::server::ensure_private_dir(parent) {
        eprintln!("{e}");
        return 1;
    }

    let config = ServerConfig::new(1024 * 1024, Duration::from_secs(3600));
    let server = match Server::bind(sock, config).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };

    println!("serving {}", sock.display());

    let handler = Arc::new(ReplayHandler {
        events: Arc::new(events),
        speed,
    });

    server
        .serve(handler, async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;

    0
}

/// Runs the `octoplusctl replay` subcommand (FR-MON-12, P3-S06).
pub async fn run_replay(dump: &Path, speed: f64, serve: Option<&Path>, json: bool) -> i32 {
    let events = match load_dump(dump) {
        Ok(evs) => evs,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };

    if let Some(sock) = serve {
        serve_events(sock, events, speed).await
    } else {
        print_events(&events, speed, json).await;
        0
    }
}
