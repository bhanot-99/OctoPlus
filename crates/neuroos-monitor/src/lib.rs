//! Semantic-NeuroOS Context Monitor (C1).
//!
//! Serves:
//! - `monitor/stream`: accepts `MonitorSubscribe` and streams raw `MonitorEvent` frames.
//! - `monitor/control`: accepts `PauseRequest` and `ResumeRequest`, returns `PrivacyState`.

pub mod anonymise;
pub mod bus;
pub mod clock;
pub mod exclusion;
pub mod privacy;
pub mod record;
pub mod sensor;
pub mod sensors;
pub mod stream;
pub mod supervisor;

use std::sync::Arc;

use neuroos_ipc::{Handler, HandlerError, Request, StreamSink};
use neuroos_proto::v1::ErrorCode;
use neuroos_proto::v1::envelope::Body;

pub use privacy::Privacy;
pub use stream::StreamHandler;
pub use supervisor::supervise;

/// Handles privacy control requests on `monitor/control` (FR-PRV-01).
#[derive(Debug)]
pub struct ControlHandler {
    privacy: Arc<Privacy>,
}

impl ControlHandler {
    /// Handler over the shared pause state.
    #[must_use]
    pub fn new(privacy: Arc<Privacy>) -> Self {
        Self { privacy }
    }
}

impl Handler for ControlHandler {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::PauseRequest(p) => {
                let state = self.privacy.pause(p.duration_ns);
                if p.duration_ns > 0 {
                    // The timer only wakes the expiry check; a stale timer is harmless
                    // because `expire_if_due` re-checks the real deadline.
                    let privacy = Arc::clone(&self.privacy);
                    let wait = std::time::Duration::from_nanos(p.duration_ns);
                    tokio::spawn(async move {
                        tokio::time::sleep(wait).await;
                        privacy.expire_if_due();
                    });
                }
                Ok(Body::PrivacyState(state))
            }
            Body::ResumeRequest(_) => Ok(Body::PrivacyState(self.privacy.resume())),
            _ => Err(HandlerError::new(
                ErrorCode::InvalidRequest,
                "expected PauseRequest or ResumeRequest on control socket",
                false,
            )),
        }
    }
}

/// Start-up options that are not configuration (command-line only).
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// `--record <file>`: write an anonymised dump of the stream (FR-MON-12).
    pub record: Option<std::path::PathBuf>,
}

/// Initializes the C1 monitor with default options (no recording).
///
/// # Errors
/// Returns [`neuroos_health::ServiceError`] if socket registration fails.
pub fn init(ctx: &mut neuroos_health::ServiceContext) -> Result<(), neuroos_health::ServiceError> {
    init_with(ctx, &Options::default())
}

/// Starts every sensor under its own supervisor (P3 steps 1–2). Each reports its own health;
/// one failing never stops the others. The handles end when `stop` turns true.
#[must_use]
pub fn spawn_sensors(
    cfg: &neuroos_common::config::MonitorConfig,
    bus: &Arc<bus::Bus>,
    privacy: &Arc<Privacy>,
    health: &Arc<neuroos_health::Health>,
    stop: &tokio::sync::watch::Receiver<bool>,
) -> Vec<tokio::task::JoinHandle<()>> {
    use sensors::{battery, cosmic, disk, folders, idle, mpris, proc, resources, thermal};
    let table = proc::ProcTable::new(std::path::Path::new("/proc"));
    let snapshot = privacy.snapshot_requests();
    let clock: Arc<dyn clock::Clock> = Arc::new(clock::SystemClock);
    let mut handles = Vec::new();

    macro_rules! run {
        ($sensor:expr) => {
            handles.push(tokio::spawn(supervisor::supervise(
                $sensor,
                Arc::clone(bus),
                Arc::clone(&clock),
                Arc::clone(health),
                stop.clone(),
            )))
        };
    }
    run!(resources::ResourcesSensor::new(cfg));
    run!(disk::DiskSensor::new(cfg));
    run!(battery::BatterySensor::new(cfg));
    run!(thermal::ThermalSensor::new(cfg));
    run!(cosmic::FocusSensor::new(
        Arc::clone(&table),
        snapshot.clone()
    ));
    run!(proc::ProcSensor::new(table, Arc::clone(bus)));
    run!(idle::IdleSensor::new(cfg, snapshot.clone()));
    run!(mpris::MprisSensor::new(snapshot));
    run!(folders::FoldersSensor::new(cfg));
    handles
}

/// Initializes the C1 monitor: privacy gate, bus, stream and control sockets, recorder.
///
/// # Errors
/// Returns [`neuroos_health::ServiceError`] if socket registration or the dump file fails.
pub fn init_with(
    ctx: &mut neuroos_health::ServiceContext,
    options: &Options,
) -> Result<(), neuroos_health::ServiceError> {
    let clock: Arc<dyn clock::Clock> = Arc::new(clock::SystemClock);
    let privacy = Privacy::new(
        Arc::clone(&clock),
        exclusion::Exclusion::from_config(&ctx.config.monitor),
        Arc::clone(&ctx.health),
    );
    let bus = bus::Bus::new(
        ctx.config.monitor.queue_capacity,
        Arc::clone(&privacy) as Arc<dyn bus::Gate>,
        clock,
        Arc::clone(&ctx.health),
    );
    if let Some(path) = &options.record {
        let writer = record::create_dump(path).map_err(|e| {
            neuroos_health::ServiceError::Custom(format!("cannot create the dump file: {e}"))
        })?;
        let anonymiser = anonymise::Anonymiser::new(&ctx.config.monitor.record_keep_app_ids)
            .map_err(|e| neuroos_health::ServiceError::Custom(e.to_string()))?;
        let sub = bus.subscribe();
        let health = Arc::clone(&ctx.health);
        let stop = ctx.stop_rx.clone();
        ctx.spawn_task(async move {
            match record::run_recorder(sub, writer, anonymiser, health, stop).await {
                Ok(n) => tracing::info!(events = n, "recording finished"),
                Err(e) => tracing::error!(error = %e, "recording failed"),
            }
        });
    }
    let handles = spawn_sensors(
        &ctx.config.monitor,
        &bus,
        &privacy,
        &ctx.health,
        &ctx.stop_rx,
    );
    ctx.handles.extend(handles);
    let stream_handler = Arc::new(StreamHandler::new(bus, ctx.stop_rx.clone()));
    let control_handler = Arc::new(ControlHandler::new(privacy));

    ctx.spawn_server("stream", stream_handler)?;
    ctx.spawn_server("control", control_handler)?;
    ctx.health.set_ready();
    Ok(())
}
