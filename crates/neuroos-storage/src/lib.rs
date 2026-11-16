//! Semantic-NeuroOS Storage Engine (C3, Phase 4): the single home of persistent truth
//! (Architecture §7). One SQLite database, one writer, an exact in-memory vector scan, one
//! embedding model. Serves `storage/storage`, subscribes to C1's stream.

pub mod db;
pub mod embed;
pub mod fusion;
pub mod handler;
pub mod ingest;
pub mod lifecycle;
pub mod query;
pub mod replay;
pub mod search;
pub mod spool;
// pub mod spool_ingest; // enabled when spool.rs (agy A3) lands
pub mod store;
pub mod vectors;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use neuroos_common::config::StorageConfig;
use neuroos_common::paths::{self, Part};
use neuroos_health::{Health, ServiceError};
use neuroos_ipc::ReconnectingClient;
use neuroos_proto::v1::HealthStatus;

pub use handler::StorageHandler;
pub use store::{Store, StorePaths};

use embed::{Embedder, HashEmbedder, OnnxEmbedder};

/// The model file, its tokenizer and the runtime library, as installed by `install-dev.sh`.
const MODEL_FILE: &str = "bge-small-en-v1.5.onnx";
const TOKENIZER_FILE: &str = "bge-small-en-v1.5.tokenizer.json";
const RUNTIME_FILE: &str = "libonnxruntime.so.1.31.1";
/// Intra-op threads of the embedder: enough for a query, small enough to leave the scan its cores.
const EMBED_THREADS: usize = 2;
/// How often the health gauges are refreshed.
const GAUGE_EVERY: Duration = Duration::from_secs(15);

/// Folders C3 reads or writes besides its own data (Architecture §8.2).
#[derive(Debug, Clone)]
pub struct Dirs {
    /// Embedding model and tokenizer.
    pub embedding: PathBuf,
    /// The ONNX Runtime library file.
    pub runtime_lib: PathBuf,
    /// Where C7 leaves documents.
    pub spool: PathBuf,
}

impl Dirs {
    /// Resolves the configured folders; empty config values mean the install defaults under
    /// `<home>/.local/opt/neuroos`.
    #[must_use]
    pub fn resolve(cfg: &StorageConfig, home: &Path) -> Self {
        let opt = home.join(".local/opt/neuroos");
        let pick = |p: &Path, default: PathBuf| {
            if p.as_os_str().is_empty() {
                default
            } else {
                p.to_path_buf()
            }
        };
        Self {
            embedding: pick(&cfg.embedding_dir, opt.join("models/embedding")),
            runtime_lib: pick(&cfg.onnxruntime_lib, opt.join("lib").join(RUNTIME_FILE)),
            spool: cfg.spool_dir.clone(),
        }
    }
}

/// Loads the real model, or falls back to the stand-in and says so (DEGRADED): C3 still ingests
/// text and answers text searches without it.
fn load_embedder(dirs: &Dirs, health: &Health) -> Arc<dyn Embedder> {
    let model = dirs.embedding.join(MODEL_FILE);
    let tok = dirs.embedding.join(TOKENIZER_FILE);
    match OnnxEmbedder::load(&dirs.runtime_lib, &model, &tok, EMBED_THREADS) {
        Ok(e) => {
            health.set("embedder", HealthStatus::Ok, "bge-small-en-v1.5");
            Arc::new(e)
        }
        Err(e) => {
            tracing::warn!(error = %e, "embedding model unavailable; text search only");
            health.set(
                "embedder",
                HealthStatus::Degraded,
                "embedding model unavailable: text search only",
            );
            Arc::new(HashEmbedder)
        }
    }
}

/// Initializes C3 and registers its socket with `ServiceContext`.
///
/// # Errors
/// [`ServiceError`] if the data folder, the database or the socket cannot be set up.
pub fn init(ctx: &mut neuroos_health::ServiceContext) -> Result<(), ServiceError> {
    let fail = |m: String| ServiceError::Custom(m);
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| fail("HOME is not set".into()))?;
    let root = paths::data_dir(
        std::env::var_os("XDG_DATA_HOME").as_deref(),
        Some(home.as_os_str()),
    )
    .map_err(|e| fail(format!("no data folder: {e}")))?;
    let cfg = ctx.config.storage.clone();
    let dirs = Dirs::resolve(&cfg, &home);
    let store_paths = StorePaths::under(&root);
    if let Some(dir) = store_paths.backups.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let embedder = load_embedder(&dirs, &ctx.health);
    let health = Arc::clone(&ctx.health);
    // `init` runs on the runtime; opening the database is async, and the runtime is multi-threaded.
    let store = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(Store::open(
            &store_paths,
            cfg.clone(),
            embedder,
            Arc::clone(&health),
        ))
    })
    .map_err(|e| fail(format!("cannot open the database: {e}")))?;
    let store = Arc::new(store);
    ctx.health.set("db", HealthStatus::Ok, "ok");

    let stream_sock = paths::socket_path(&ctx.runtime_dir, Part::Monitor, "stream");
    let client = Arc::new(ReconnectingClient::new(
        &stream_sock,
        ctx.config.ipc.max_frame_bytes,
    ));
    let probe = Arc::clone(&client);
    ctx.health.probe(
        "monitor_stream",
        Box::new(move || {
            if probe.is_connected() {
                (HealthStatus::Ok, "")
            } else {
                (
                    HealthStatus::Degraded,
                    "disconnected from C1 monitor stream",
                )
            }
        }),
    );
    let handles = ingest::pipeline::spawn(Arc::clone(&store), client, ctx.stop_rx.clone());
    ctx.handles.extend(handles);

    let s = Arc::clone(&store);
    let stop = ctx.stop_rx.clone();
    ctx.spawn_task(async move {
        s.reembed_all(stop).await;
    });
    let s = Arc::clone(&store);
    let mut stop = ctx.stop_rx.clone();
    ctx.spawn_task(async move {
        let mut tick = tokio::time::interval(GAUGE_EVERY);
        loop {
            tokio::select! {
                _ = tick.tick() => { let st = Arc::clone(&s); let _ = tokio::task::spawn_blocking(move || st.refresh_gauges()).await; }
                _ = stop.changed() => return,
            }
        }
    });
    // spool: enabled with spool_ingest

    let query = Duration::from_millis(ctx.config.deadlines.storage_query_ms);
    ctx.spawn_server("storage", Arc::new(StorageHandler::new(store, query)))?;
    ctx.health.set_ready();
    Ok(())
}
