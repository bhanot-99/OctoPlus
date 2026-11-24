//! Shared service bootstrap for OctoPlus parts (Phase 1, P1-S03).
//!
//! # Placement justification (TASK P1-T2):
//! AB-1 allows only `octoplus-common`, `octoplus-proto`, `octoplus-ipc`, `octoplus-taint`,
//! `octoplus-health`, and `octoplus-sandbox`.
//!
//! `octoplus-health::service` is chosen because `octoplus-health` sits directly above
//! `octoplus-ipc`, `octoplus-proto`, `octoplus-common`, and `octoplus-sandbox` without
//! introducing cyclic dependencies (unlike placing it in `octoplus-ipc`, which would
//! require `octoplus-ipc` to depend on `octoplus-health`). Every part binary already
//! reports health and serves a health socket, so `octoplus-health` is the natural home
//! for the shared service lifecycle and startup sequence.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use octoplus_common::config::{self, Config};
use octoplus_common::paths::{self, Part};
use octoplus_common::socketmap::{self, SocketEntry};
use octoplus_ipc::Handler;
use octoplus_ipc::peer::PeerPolicy;
use octoplus_ipc::server::{Server, ServerConfig, ensure_private_dir};
use octoplus_proto::v1::HealthStatus;
use octoplus_sandbox::{Policy, restrict};
use tokio::signal::unix::{SignalKind, signal};
use tracing::{error, info, warn};

use crate::{Health, HealthHandler};

/// Common CLI arguments for all OctoPlus services.
#[derive(Debug, Clone, Default, Parser)]
#[command(about = "OctoPlus service")]
pub struct CommonArgs {
    /// Runtime root; defaults to $XDG_RUNTIME_DIR/octoplus (or /run/octoplus-fetcher for fetcher).
    #[arg(long)]
    pub runtime_dir: Option<PathBuf>,

    /// Path to config.toml; defaults to standard XDG location.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Allowed client UID (defaults to own UID; fetcher uses this to accept desktop user connections).
    #[arg(long)]
    pub allow_uid: Option<u32>,

    /// Enable soak mode (FR-HLT-04, NFR-REL-04; used by healthd).
    #[arg(long, default_value_t = false)]
    pub soak: bool,
}

/// Errors occurring during service bootstrap or shutdown.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    /// CLI argument parsing error.
    #[error("CLI parse error: {0}")]
    Clap(#[from] clap::Error),
    /// Path resolution error.
    #[error("Path error: {0}")]
    Path(#[from] paths::PathError),
    /// Config load or validation error.
    #[error("Config error: {0}")]
    Config(#[from] config::ConfigError),
    /// Logging initialization error.
    #[error("Logging error: {0}")]
    Logging(#[from] octoplus_common::logging::LoggingError),
    /// IPC error.
    #[error("IPC error: {0}")]
    Ipc(#[from] octoplus_ipc::IpcError),
    /// Sandbox / Landlock error.
    #[error("Sandbox error: {0}")]
    Sandbox(#[from] octoplus_sandbox::SandboxError),
    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// Tokio join error.
    #[error("Task join error: {0}")]
    Join(#[from] tokio::task::JoinError),
    /// Custom service logic error.
    #[error("Service error: {0}")]
    Custom(String),
}

/// Helper to get the socket directory for an entry, respecting runtime_dir overrides in tests.
#[must_use]
pub fn socket_dir_for_entry(entry: &SocketEntry, runtime_root: &Path) -> PathBuf {
    if entry.server == Part::Fetcher {
        if runtime_root == Path::new(paths::FETCHER_RUNTIME_DIR) {
            Path::new(paths::FETCHER_RUNTIME_DIR).join(entry.socket)
        } else {
            runtime_root.join(entry.socket)
        }
    } else {
        runtime_root.join(entry.server.name()).join(entry.socket)
    }
}

/// The fetcher's sockets live in tmpfiles-made folders shared with group
/// `octoplus` (Architecture §5.2). Tests that pass `--runtime-dir` get private
/// folders as for any other part.
fn group_shared(part: Part, runtime_root: &Path) -> bool {
    part == Part::Fetcher && runtime_root == Path::new(paths::FETCHER_RUNTIME_DIR)
}

/// Helper to get the socket file path for an entry.
#[must_use]
pub fn socket_path_for_entry(entry: &SocketEntry, runtime_root: &Path) -> PathBuf {
    socket_dir_for_entry(entry, runtime_root).join(format!("{}.sock", entry.socket))
}

fn deadline_for_socket(socket_name: &str, config: &Config) -> Duration {
    match socket_name {
        "health" | "control" => config.deadlines.control(),
        // Queries get their 150 ms inside the handler; forget, purge and the lifecycle jobs are
        // longer, so the socket itself allows `storage_job_ms` (ADR-0012).
        "storage" => Duration::from_millis(config.deadlines.storage_job_ms),
        // One socket serves quick checks and dispatches that wait for the dialog;
        // the 50 ms check deadline is the caller's (PRD §6.1).
        "kernel" => Duration::from_millis(config.deadlines.kernel_dispatch_ms),
        "knowledge" => Duration::from_millis(config.deadlines.generation_ms),
        "fetch" => Duration::from_millis(config.deadlines.fetch_ms),
        _ => Duration::from_secs(3600), // streaming sockets
    }
}

/// Context provided to the service implementation callback.
#[derive(Debug)]
pub struct ServiceContext {
    /// The part identity.
    pub part: Part,
    /// Loaded, validated configuration.
    pub config: Config,
    /// Health reporter and registry.
    pub health: Arc<Health>,
    /// Runtime root directory.
    pub runtime_dir: PathBuf,
    /// Sockets bound for this part (socket name -> bound Server), excluding health.
    pub servers: HashMap<String, Server>,
    /// Shutdown signal receiver.
    pub stop_rx: tokio::sync::watch::Receiver<bool>,
    /// Spawned task handles.
    pub handles: Vec<tokio::task::JoinHandle<()>>,
    /// Whether soak mode is enabled.
    pub soak: bool,
}

impl ServiceContext {
    /// Spawns a server for `socket_name` with the provided `handler`.
    ///
    /// # Errors
    /// Returns [`ServiceError`] if `socket_name` was not bound or already taken.
    pub fn spawn_server<H: Handler>(
        &mut self,
        socket_name: &str,
        handler: Arc<H>,
    ) -> Result<(), ServiceError> {
        let server = self.servers.remove(socket_name).ok_or_else(|| {
            ServiceError::Custom(format!("socket '{socket_name}' not found or already taken"))
        })?;
        let mut rx = self.stop_rx.clone();
        let _name = socket_name.to_string();
        let handle = tokio::spawn(async move {
            let wait = async move {
                let _ = rx.wait_for(|s| *s).await;
            };
            server.serve(handler, wait).await;
        });
        self.handles.push(handle);
        Ok(())
    }

    /// Spawns a background task that will be awaited on shutdown.
    pub fn spawn_task<Fut>(&mut self, fut: Fut)
    where
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        self.handles.push(tokio::spawn(fut));
    }
}

/// Configures and runs an OctoPlus service following the strict startup sequence.
#[derive(Debug)]
pub struct ServiceBuilder {
    part: Part,
    name: &'static str,
    version: &'static str,
    extra_read_only: Vec<PathBuf>,
    extra_read_write: Vec<PathBuf>,
    connect_tcp: Vec<u16>,
    skip_restrict: bool,
    args: Option<CommonArgs>,
    external_stop: Option<tokio::sync::oneshot::Receiver<()>>,
}

impl ServiceBuilder {
    /// Creates a new builder for `part`.
    #[must_use]
    pub fn new(part: Part, name: &'static str, version: &'static str) -> Self {
        Self {
            part,
            name,
            version,
            extra_read_only: Vec::new(),
            extra_read_write: Vec::new(),
            connect_tcp: Vec::new(),
            skip_restrict: false,
            args: None,
            external_stop: None,
        }
    }

    /// Adds extra read-only paths to the Landlock policy.
    #[must_use]
    pub fn extra_read_only(mut self, paths: Vec<PathBuf>) -> Self {
        self.extra_read_only.extend(paths);
        self
    }

    /// Adds extra read-write paths to the Landlock policy.
    #[must_use]
    pub fn extra_read_write(mut self, paths: Vec<PathBuf>) -> Self {
        self.extra_read_write.extend(paths);
        self
    }

    /// TCP ports this part may connect to (Landlock). Only C7 sets one: 443 (FR-FET-10).
    #[must_use]
    pub fn connect_tcp(mut self, ports: Vec<u16>) -> Self {
        self.connect_tcp.extend(ports);
        self
    }

    /// Skips Landlock restrict, for in-process test runners only. Compiled only
    /// with the `test-support` feature, which shipped binaries never enable
    /// (dev-dependency features are not unified into release builds; BUG-025).
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn skip_restrict(mut self, skip: bool) -> Self {
        self.skip_restrict = skip;
        self
    }

    /// Sets explicit CLI arguments (bypassing `std::env::args`).
    #[must_use]
    pub fn args(mut self, args: CommonArgs) -> Self {
        self.args = Some(args);
        self
    }

    /// Provides an external shutdown trigger (useful for testing `run()` in-process).
    #[must_use]
    pub fn external_stop(mut self, stop: tokio::sync::oneshot::Receiver<()>) -> Self {
        self.external_stop = Some(stop);
        self
    }

    /// Executes the full service startup sequence and blocks until shutdown.
    ///
    /// # Errors
    /// Returns [`ServiceError`] on any failure during initialization, socket binding, or execution.
    pub fn run<F>(self, init: F) -> Result<(), ServiceError>
    where
        F: FnOnce(&mut ServiceContext) -> Result<(), ServiceError>,
    {
        // 1. Parse common flags
        let args = match self.args {
            Some(a) => a,
            None => CommonArgs::parse(),
        };

        // 2. Init logging
        let log_env = std::env::var(octoplus_common::logging::LOG_ENV).ok();
        let _ = octoplus_common::logging::init(self.name, log_env.as_deref());

        // Resolve runtime root
        let runtime_root = match args.runtime_dir {
            Some(r) => r,
            None => match self.part {
                Part::Fetcher => PathBuf::from(paths::FETCHER_RUNTIME_DIR),
                _ => paths::runtime_root(std::env::var_os("XDG_RUNTIME_DIR").as_deref())?,
            },
        };

        // Resolve config file path
        let config_path = match args.config {
            Some(c) => c,
            None => paths::config_file(
                std::env::var_os(config::CONFIG_ENV).as_deref(),
                std::env::var_os("XDG_CONFIG_HOME").as_deref(),
                std::env::var_os("HOME").as_deref(),
            )?,
        };

        // 3. Create own socket folders (create nothing else)
        let part_entries: Vec<&SocketEntry> = socketmap::SOCKET_MAP
            .iter()
            .filter(|e| e.server == self.part)
            .collect();

        let mut socket_dirs = Vec::new();
        for entry in &part_entries {
            let sdir = socket_dir_for_entry(entry, &runtime_root);
            if group_shared(self.part, &runtime_root) {
                // Made by tmpfiles as 2770 octoplus-fetcher:octoplus (BUG-026).
                octoplus_ipc::server::ensure_group_dir(&sdir)?;
            } else {
                ensure_private_dir(&sdir)?;
            }
            socket_dirs.push(sdir);
        }

        // 4. Landlock restrict BEFORE reading any input (NFR-SEC-04)
        if !self.skip_restrict {
            let mut read_only = vec![PathBuf::from("/proc")];
            // Tolerant config path logic: add config path if it exists, or parent if it exists
            if config_path.exists() {
                read_only.push(config_path.clone());
            } else if let Some(parent) = config_path.parent()
                && parent.exists()
            {
                read_only.push(parent.to_path_buf());
            }
            read_only.extend(self.extra_read_only);

            let mut read_write = socket_dirs.clone();
            read_write.extend(self.extra_read_write);

            let policy = Policy {
                read_only,
                read_write,
                connect_tcp: self.connect_tcp.clone(),
            };

            let report = restrict(&policy)?;
            info!(
                fs_and_tcp = report.fs_and_tcp,
                scoped = report.scoped,
                "landlock sandbox applied"
            );
        }

        // 5. Load and validate config AFTER restrict
        let config = Config::load_from(&config_path)?;

        // 6. Build tokio runtime
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;

        // 7. Bind sockets with peer policies derived from socketmap
        let allowed_uid = args
            .allow_uid
            .unwrap_or_else(octoplus_ipc::peer::current_uid);
        let max_frame = config.ipc.max_frame_bytes;

        rt.block_on(async {
            let mut health_server: Option<Server> = None;
            let mut other_servers: HashMap<String, Server> = HashMap::new();

            for entry in part_entries {
                let s_path = socket_path_for_entry(entry, &runtime_root);
                let deadline = deadline_for_socket(entry.socket, &config);

                let policy = if entry.socket == "health" {
                    PeerPolicy::uids(vec![allowed_uid])
                        .with_units(vec!["octoplus-healthd.service".to_string()])
                } else if entry.allows_user_client() {
                    // Any admitted peer may connect, but its unit is still looked up: per-message
                    // checks (C3's writes from C5 and C6 only) depend on it.
                    PeerPolicy::uids(vec![allowed_uid]).resolving_unit()
                } else {
                    PeerPolicy::uids(vec![allowed_uid]).with_units(entry.allowed_client_units())
                };

                let mut scfg = ServerConfig::new(max_frame, deadline);
                scfg.policy = policy;
                if group_shared(self.part, &runtime_root) {
                    scfg.access = octoplus_ipc::SocketAccess::Group;
                }

                let server = Server::bind(&s_path, scfg).await?;
                if entry.socket == "health" {
                    health_server = Some(server);
                } else {
                    other_servers.insert(entry.socket.to_string(), server);
                }
            }

            if health_server.is_none() && self.part != Part::Healthd {
                return Err(ServiceError::Custom(format!(
                    "no health socket entry found in socketmap for {:?}",
                    self.part
                )));
            }

            let health = Health::new(self.name, self.version);

            if let Some(canary) = &config.debug.landlock_canary {
                match std::fs::read(canary) {
                    Ok(_) => {
                        error!(canary = %canary.display(), "SECURITY: landlock canary was readable!");
                        health
                            .counter("landlock_canary_allowed")
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                        info!(canary = %canary.display(), "landlock canary access denied as expected");
                        health
                            .counter("landlock_canary_denied")
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    Err(e) => {
                        // e.g. NotFound: the path is not visible at all, which proves nothing.
                        warn!(canary = %canary.display(), err = %e, "landlock canary check unexpected error");
                        health
                            .counter("landlock_canary_unexpected")
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            }

            // Shutdown coordination
            let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);

            let mut ctx = ServiceContext {
                part: self.part,
                config,
                health: health.clone(),
                runtime_dir: runtime_root,
                servers: other_servers,
                stop_rx: stop_rx.clone(),
                handles: Vec::new(),
                soak: args.soak,
            };

            if let Some(hs) = health_server {
                // Probe server accept stats on health
                let hstats = hs.stats();
                health.probe(
                    "health_accept",
                    Box::new(move || {
                        use std::sync::atomic::Ordering;
                        if hstats.accept_errors.load(Ordering::Relaxed) > 0 {
                            (
                                HealthStatus::Degraded,
                                "accept errors seen on health socket",
                            )
                        } else {
                            (HealthStatus::Ok, "")
                        }
                    }),
                );

                // 8. Serve health on <part>/health/health.sock
                let wait = |mut rx: tokio::sync::watch::Receiver<bool>| async move {
                    let _ = rx.wait_for(|s| *s).await;
                };

                ctx.handles.push(tokio::spawn(hs.serve(
                    Arc::new(HealthHandler(health.clone())),
                    wait(stop_rx.clone()),
                )));
            }

            // Let the service configure its handlers and tasks
            init(&mut ctx)?;

            // Warn on any unhandled server
            for name in ctx.servers.keys() {
                warn!("socket '{}' bound but no handler registered", name);
            }

            info!("{}: serving", self.name);

            // 9. Stop cleanly on SIGTERM / SIGINT / external stop
            let mut sigterm = signal(SignalKind::terminate())?;
            let mut sigint = signal(SignalKind::interrupt())?;
            let mut external_stop = self.external_stop;

            tokio::select! {
                _ = sigterm.recv() => {
                    info!("{}: received SIGTERM, stopping", self.name);
                }
                _ = sigint.recv() => {
                    info!("{}: received SIGINT, stopping", self.name);
                }
                _ = async {
                    if let Some(stop) = external_stop.take() {
                        let _ = stop.await;
                    } else {
                        std::future::pending::<()>().await;
                    }
                } => {
                    info!("{}: received external stop signal, stopping", self.name);
                }
            }

            let _ = stop_tx.send(true);
            for h in ctx.handles {
                let _ = h.await;
            }

            info!("{}: stopped cleanly", self.name);
            Ok(())
        })
    }
}
