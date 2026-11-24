//! OctoPlus Network Fetcher (C7).
//!
//! Serves `/run/octoplus-fetcher/fetch/fetch.sock`:
//! - Accepts [`FetchRequest`] and [`SearchRequest`] bound by valid [`ApprovalToken`]s.
//! - Anti-SSRF, request hygiene, HTML text extraction, Format v1 spooling, and egress tracking.
//!
//! Every request that leaves the machine is returned to C6 as an [`EgressEntry`], including
//! the requests of a fetch that then failed (non-text page, error status, too large, time
//! out): the ledger records what left, not only what succeeded (R0-2, FR-FET-09).
//!
//! [`ApprovalToken`]: octoplus_proto::v1::ApprovalToken

pub mod extract;
pub mod search;
pub mod spool;
pub mod ssrf;
pub mod token;
pub mod transport;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use octoplus_ipc::{Handler, HandlerError, Request, StreamSink};
use octoplus_proto::v1::envelope::Body;
use octoplus_proto::v1::{
    ApprovalToken, EgressEntry, ErrorCode, FetchRequest, FetchResult, SearchRequest,
};

use crate::search::{GenericSearchAdapter, SearchAdapter};
use crate::spool::SpoolEntry;
use crate::ssrf::SafeUrl;
use crate::token::{Approval, NonceTracker, SecretKey, Verdict};
use crate::transport::{Attempt, HttpsTransport, MockTransport, SystemResolver, Transport};

/// The only TCP port C7 may connect to (FR-FET-10), enforced by its Landlock policy.
pub const HTTPS_PORT: u16 = 443;

/// Where the installer puts the key C6 signs fetch approvals with.
pub const FETCH_KEY_PATH: &str = "/etc/octoplus/fetch.key";

/// Default spool size limit; the service uses `storage.spool_max_bytes` from its config.
const DEFAULT_SPOOL_MAX_BYTES: u64 = 256 * 1024;

/// Most result pages one search may read (FR-FET-05 "up to N").
pub const MAX_RESULT_PAGES: u32 = 5;

/// Handles network fetch and search requests (Architecture §8.4, FR-FET-01..10).
pub struct FetcherHandler {
    key: Arc<SecretKey>,
    spool_dir: PathBuf,
    spool_max_bytes: u64,
    budget: Duration,
    tracker: Arc<Mutex<NonceTracker>>,
    search_adapter: Arc<dyn SearchAdapter>,
    transport: Arc<dyn Transport>,
}

impl std::fmt::Debug for FetcherHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FetcherHandler")
            .field("spool_dir", &self.spool_dir)
            .field("budget", &self.budget)
            .finish_non_exhaustive()
    }
}

impl FetcherHandler {
    /// Creates a new `FetcherHandler` with explicit parameters, the default spool limit and a
    /// 15 s budget per request (FR-FET-04).
    #[must_use]
    pub fn new(
        key: Arc<SecretKey>,
        spool_dir: PathBuf,
        search_adapter: Arc<dyn SearchAdapter>,
        transport: Arc<dyn Transport>,
    ) -> Self {
        Self {
            key,
            spool_dir,
            spool_max_bytes: DEFAULT_SPOOL_MAX_BYTES,
            budget: Duration::from_secs(15),
            tracker: Arc::new(Mutex::new(NonceTracker::new())),
            search_adapter,
            transport,
        }
    }

    /// Sets the spool size limit (`storage.spool_max_bytes`) and the time budget for one
    /// whole request, search pages included (`deadlines.fetch_ms`).
    #[must_use]
    pub fn with_limits(mut self, spool_max_bytes: u64, budget: Duration) -> Self {
        self.spool_max_bytes = spool_max_bytes;
        self.budget = budget;
        self
    }

    /// Creates a handler using the mock transport (for testing without internet access).
    #[must_use]
    pub fn with_mock_transport(
        key: Arc<SecretKey>,
        spool_dir: PathBuf,
        mock: MockTransport,
    ) -> Self {
        Self::new(
            key,
            spool_dir,
            Arc::new(GenericSearchAdapter::default()),
            Arc::new(mock),
        )
    }

    /// FR-FET-02: the token must bind exactly this capability and these arguments.
    async fn verify_token(
        &self,
        token: Option<ApprovalToken>,
        capability: &str,
        canonical_args: &str,
    ) -> Result<(), HandlerError> {
        let denied = |msg: &'static str| HandlerError::new(ErrorCode::PermissionDenied, msg, false);
        let token = token.ok_or_else(|| denied("missing approval token"))?;
        let nonce: [u8; 16] = token
            .nonce
            .as_slice()
            .try_into()
            .map_err(|_| denied("invalid token nonce length"))?;
        // Fail closed (rules.md §6.5): a token without a full action id is not accepted.
        let action_id: [u8; 16] = token
            .action_id
            .as_slice()
            .try_into()
            .map_err(|_| denied("invalid token action id length"))?;
        let approval = Approval {
            action_id,
            nonce,
            capability: capability.to_string(),
            args_hash: token::args_hash(canonical_args),
            verdict: Verdict::Allow,
            expires_at_ns: token.expires_at_ns,
        };
        let now_ns = now_ns()?;
        let mut tracker = self.tracker.lock().await;
        tracker
            .verify(&self.key, &approval, &token.mac, now_ns)
            .map_err(|_| denied("approval token verification failed"))
    }

    async fn handle_fetch(&self, req: FetchRequest) -> Result<Body, HandlerError> {
        let canonical_args = format!(r#"{{"url":"{}"}}"#, req.url);
        self.verify_token(req.token, "web.fetch", &canonical_args)
            .await?;
        let started = Instant::now();

        let safe_url = SafeUrl::parse(&req.url).map_err(|_| {
            HandlerError::new(
                ErrorCode::InvalidRequest,
                "url failed SSRF validation",
                false,
            )
        })?;

        let attempt = self.fetch_within(&safe_url, started).await;
        let mut egress = Vec::new();
        record_hops(&mut egress, &attempt, None)?;
        let spool_files = self.spool_page(&attempt, "fetch", 0).into_iter().collect();
        Ok(Body::FetchResult(FetchResult {
            egress,
            spool_files,
        }))
    }

    async fn handle_search(&self, req: SearchRequest) -> Result<Body, HandlerError> {
        let canonical_args = format!(r#"{{"query":"{}"}}"#, req.query);
        self.verify_token(req.token, "web.search", &canonical_args)
            .await?;
        let started = Instant::now();

        let search_url_str = self
            .search_adapter
            .format_query_url(&req.query)
            .map_err(|_| {
                HandlerError::new(
                    ErrorCode::InvalidRequest,
                    "search query formatting failed",
                    false,
                )
            })?;
        let safe_search_url = SafeUrl::parse(&search_url_str).map_err(|_| {
            HandlerError::new(
                ErrorCode::InvalidRequest,
                "search provider failed SSRF validation",
                false,
            )
        })?;

        let mut egress = Vec::new();
        let search = self.fetch_within(&safe_search_url, started).await;
        // The first hop carries exactly the query the user approved (FR-FET-05).
        record_hops(&mut egress, &search, Some(&req.query))?;
        let Ok(search_page) = &search.outcome else {
            log_failure("search", &search);
            return Ok(Body::FetchResult(FetchResult {
                egress,
                spool_files: Vec::new(),
            }));
        };

        let max_pages = req.max_pages.clamp(1, MAX_RESULT_PAGES) as usize;
        let results = self
            .search_adapter
            .parse_results(&search_page.body, max_pages)
            .unwrap_or_default();

        let mut spool_files = Vec::new();
        for (idx, item) in results.into_iter().enumerate() {
            if started.elapsed() >= self.budget {
                // Out of time: what was read so far is returned, nothing more is sent.
                break;
            }
            let Ok(item_url) = SafeUrl::parse(&item.url) else {
                continue;
            };
            let attempt = self.fetch_within(&item_url, started).await;
            record_hops(&mut egress, &attempt, None)?;
            if let Some(name) = self.spool_page(&attempt, "search", idx) {
                spool_files.push(name);
            }
        }

        Ok(Body::FetchResult(FetchResult {
            egress,
            spool_files,
        }))
    }

    /// One fetch, given only the time left of this request's budget.
    async fn fetch_within(&self, url: &SafeUrl, started: Instant) -> Attempt {
        let left = self.budget.saturating_sub(started.elapsed());
        match tokio::time::timeout(left, self.transport.fetch(url)).await {
            Ok(attempt) => attempt,
            // The transport's own clock normally ends first; if not, the request may have
            // left, so it is still recorded (bytes unknown, counted as 0).
            Err(_) => Attempt {
                hops: vec![transport::Hop {
                    host: url.host.clone(),
                    url: url.full_url.clone(),
                    bytes_out: 0,
                    bytes_in: 0,
                }],
                outcome: Err(transport::TransportError::Timeout),
            },
        }
    }

    /// Extracts and spools a successful page; returns its spool file name.
    fn spool_page(&self, attempt: &Attempt, kind: &str, idx: usize) -> Option<String> {
        let page = match &attempt.outcome {
            Ok(page) => page,
            Err(_) => {
                log_failure(kind, attempt);
                return None;
            }
        };
        let final_url = attempt.hops.last()?.url.as_str();
        let host = attempt.hops.last()?.host.as_str();
        let doc = match extract::extract_html(&page.body, host) {
            Ok(doc) => doc,
            Err(e) => {
                tracing::warn!(kind, error = %e, "page text extraction failed");
                return None;
            }
        };
        let fetched_at_ns = now_ns().ok()?;
        let filename = format!("{kind}_{fetched_at_ns:016x}_{idx}.json");
        let entry = SpoolEntry {
            url: final_url,
            title: &doc.title,
            fetched_at_ns,
            text: &doc.text,
        };
        match spool::write_spool(&self.spool_dir, &filename, &entry, self.spool_max_bytes) {
            Ok(_) => Some(filename),
            Err(e) => {
                tracing::warn!(kind, error = %e, "spool write failed");
                None
            }
        }
    }
}

/// Appends one ledger entry per hop. `first_text`, when given, replaces the first hop's URL
/// as the text sent (a search records the approved query).
fn record_hops(
    egress: &mut Vec<EgressEntry>,
    attempt: &Attempt,
    first_text: Option<&str>,
) -> Result<(), HandlerError> {
    for (i, hop) in attempt.hops.iter().enumerate() {
        let sent_text = match (i, first_text) {
            (0, Some(text)) => text.to_string(),
            _ => hop.url.clone(),
        };
        egress.push(EgressEntry {
            host: hop.host.clone(),
            sent_text,
            bytes_out: hop.bytes_out,
            bytes_in: hop.bytes_in,
            at_ns: now_ns()?,
        });
    }
    Ok(())
}

/// Logs why a fetch produced nothing; never the URL or page content (R0-10).
fn log_failure(kind: &str, attempt: &Attempt) {
    if let Err(e) = &attempt.outcome {
        tracing::info!(kind, hops = attempt.hops.len(), reason = %e, "fetch produced no page");
    }
}

fn now_ns() -> Result<u64, HandlerError> {
    octoplus_common::time::now_ns()
        .map_err(|_| HandlerError::new(ErrorCode::Internal, "clock unavailable", false))
}

impl Handler for FetcherHandler {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::FetchRequest(fetch_req) => self.handle_fetch(fetch_req).await,
            Body::SearchRequest(search_req) => self.handle_search(search_req).await,
            _ => Err(HandlerError::unsupported()),
        }
    }
}

/// Initializes C7 fetcher services with the installed key ([`FETCH_KEY_PATH`]).
///
/// # Errors
/// Returns [`octoplus_health::ServiceError`] on failure; see [`init_with_key`].
pub fn init(ctx: &mut octoplus_health::ServiceContext) -> Result<(), octoplus_health::ServiceError> {
    init_with_key(Path::new(FETCH_KEY_PATH), ctx)
}

/// Initializes C7 fetcher services and registers them with `ServiceContext`.
///
/// Fails (the unit exits non-zero, rules.md §6.4) when the approval key is missing or
/// unsafe: without it no approval can be verified, and there is no built-in fallback key
/// (audit A-03; a fixed key would let anyone who reaches the socket forge approvals).
///
/// # Errors
/// Returns [`octoplus_health::ServiceError`] on failure.
pub fn init_with_key(
    key_path: &Path,
    ctx: &mut octoplus_health::ServiceContext,
) -> Result<(), octoplus_health::ServiceError> {
    let custom = |msg: String| octoplus_health::ServiceError::Custom(msg);
    let key =
        token::load_key(key_path).map_err(|e| custom(format!("{}: {e}", key_path.display())))?;

    let resolver = SystemResolver::from_system().map_err(|e| custom(format!("dns config: {e}")))?;
    let budget = Duration::from_millis(ctx.config.deadlines.fetch_ms);
    let transport = HttpsTransport::new(transport::TransportConfig {
        resolver: Arc::new(resolver),
        deadline: budget,
        test_root_pem: None,
    })
    .map_err(|e| custom(format!("https client: {e}")))?;

    let handler = FetcherHandler::new(
        Arc::new(key),
        ctx.config.storage.spool_dir.clone(),
        Arc::new(GenericSearchAdapter::default()),
        Arc::new(transport),
    )
    .with_limits(ctx.config.storage.spool_max_bytes, budget);

    ctx.spawn_server("fetch", Arc::new(handler))?;
    ctx.health.set_ready();
    Ok(())
}
