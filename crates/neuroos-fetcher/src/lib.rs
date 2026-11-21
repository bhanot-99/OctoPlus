//! Semantic-NeuroOS Network Fetcher (C7).
//!
//! Serves `/run/neuroos-fetcher/fetch/fetch.sock`:
//! - Accepts [`FetchRequest`] and [`SearchRequest`] bound by valid [`ApprovalToken`].
//! - Anti-SSRF, request hygiene, HTML text extraction, Format v1 spooling, and egress tracking.

pub mod extract;
pub mod search;
pub mod spool;
pub mod ssrf;
pub mod token;
pub mod transport;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

use neuroos_ipc::{Handler, HandlerError, Request, StreamSink};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{EgressEntry, ErrorCode, FetchRequest, FetchResult, SearchRequest};

use crate::search::{GenericSearchAdapter, SearchAdapter};
use crate::ssrf::SafeUrl;
use crate::token::{Approval, NonceTracker, SecretKey, Verdict};
use crate::transport::{MockTransport, Transport};

/// Handles network fetch and search requests (Architecture §8.4, FR-FET-01..10).
pub struct FetcherHandler {
    key: Arc<SecretKey>,
    spool_dir: PathBuf,
    tracker: Arc<Mutex<NonceTracker>>,
    search_adapter: Arc<dyn SearchAdapter>,
    transport: Arc<dyn Transport>,
}

impl std::fmt::Debug for FetcherHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FetcherHandler")
            .field("spool_dir", &self.spool_dir)
            .finish_non_exhaustive()
    }
}

impl FetcherHandler {
    /// Creates a new `FetcherHandler` with explicit parameters.
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
            tracker: Arc::new(Mutex::new(NonceTracker::new())),
            search_adapter,
            transport,
        }
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

    async fn handle_fetch(&self, req: FetchRequest) -> Result<Body, HandlerError> {
        let token_proto = req.token.ok_or_else(|| {
            HandlerError::new(ErrorCode::PermissionDenied, "missing approval token", false)
        })?;

        let mut nonce = [0u8; 16];
        if token_proto.nonce.len() != 16 {
            return Err(HandlerError::new(
                ErrorCode::PermissionDenied,
                "invalid token nonce length",
                false,
            ));
        }
        nonce.copy_from_slice(&token_proto.nonce);

        let mut action_id = [0u8; 16];
        if token_proto.action_id.len() == 16 {
            action_id.copy_from_slice(&token_proto.action_id);
        }

        // Canonical JSON argument representation for web.fetch
        let canonical_args = format!(r#"{{"url":"{}"}}"#, req.url);
        let args_hash = token::args_hash(&canonical_args);

        let approval = Approval {
            action_id,
            nonce,
            capability: "web.fetch".to_string(),
            args_hash,
            verdict: Verdict::Allow,
            expires_at_ns: token_proto.expires_at_ns,
        };

        let now_ns = neuroos_common::time::now_ns()
            .map_err(|_| HandlerError::new(ErrorCode::Internal, "clock unavailable", false))?;

        {
            let mut tracker = self.tracker.lock().await;
            tracker
                .verify(&self.key, &approval, &token_proto.mac, now_ns)
                .map_err(|_| {
                    HandlerError::new(
                        ErrorCode::PermissionDenied,
                        "approval token verification failed",
                        false,
                    )
                })?;
        }

        let safe_url = SafeUrl::parse(&req.url).map_err(|_| {
            HandlerError::new(
                ErrorCode::InvalidRequest,
                "url failed SSRF validation",
                false,
            )
        })?;

        let resp =
            self.transport.fetch(&safe_url).await.map_err(|_| {
                HandlerError::new(ErrorCode::Unavailable, "network fetch failed", true)
            })?;

        let extracted = extract::extract_html(&resp.body, &safe_url.host).map_err(|_| {
            HandlerError::new(ErrorCode::Internal, "content extraction failed", false)
        })?;

        // Create unique spool filename
        let filename = format!("fetch_{:016x}.json", now_ns);
        if let Err(e) = spool::write_spool(
            &self.spool_dir,
            &filename,
            &safe_url.full_url,
            &extracted.title,
            now_ns,
            &extracted.text,
        ) {
            tracing::warn!("spool write failed: {e}");
        }

        let entry = EgressEntry {
            host: safe_url.host,
            sent_text: safe_url.full_url,
            bytes_out: resp.bytes_out,
            bytes_in: resp.bytes_in,
            at_ns: now_ns,
        };

        Ok(Body::FetchResult(FetchResult {
            egress: vec![entry],
            spool_files: vec![filename],
        }))
    }

    async fn handle_search(&self, req: SearchRequest) -> Result<Body, HandlerError> {
        let token_proto = req.token.ok_or_else(|| {
            HandlerError::new(ErrorCode::PermissionDenied, "missing approval token", false)
        })?;

        let mut nonce = [0u8; 16];
        if token_proto.nonce.len() != 16 {
            return Err(HandlerError::new(
                ErrorCode::PermissionDenied,
                "invalid token nonce length",
                false,
            ));
        }
        nonce.copy_from_slice(&token_proto.nonce);

        let mut action_id = [0u8; 16];
        if token_proto.action_id.len() == 16 {
            action_id.copy_from_slice(&token_proto.action_id);
        }

        let canonical_args = format!(r#"{{"query":"{}"}}"#, req.query);
        let args_hash = token::args_hash(&canonical_args);

        let approval = Approval {
            action_id,
            nonce,
            capability: "web.search".to_string(),
            args_hash,
            verdict: Verdict::Allow,
            expires_at_ns: token_proto.expires_at_ns,
        };

        let now_ns = neuroos_common::time::now_ns()
            .map_err(|_| HandlerError::new(ErrorCode::Internal, "clock unavailable", false))?;

        {
            let mut tracker = self.tracker.lock().await;
            tracker
                .verify(&self.key, &approval, &token_proto.mac, now_ns)
                .map_err(|_| {
                    HandlerError::new(
                        ErrorCode::PermissionDenied,
                        "approval token verification failed",
                        false,
                    )
                })?;
        }

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

        let search_resp =
            self.transport.fetch(&safe_search_url).await.map_err(|_| {
                HandlerError::new(ErrorCode::Unavailable, "search fetch failed", true)
            })?;

        let mut egress_list = vec![EgressEntry {
            host: safe_search_url.host,
            sent_text: req.query.clone(),
            bytes_out: search_resp.bytes_out,
            bytes_in: search_resp.bytes_in,
            at_ns: now_ns,
        }];

        let mut spool_files = Vec::new();
        let max_pages = req.max_pages.clamp(1, 5) as usize;
        let results = self
            .search_adapter
            .parse_results(&search_resp.body, max_pages)
            .unwrap_or_default();

        for (idx, item) in results.into_iter().enumerate() {
            let Ok(item_url) = SafeUrl::parse(&item.url) else {
                continue;
            };
            let Ok(item_resp) = self.transport.fetch(&item_url).await else {
                continue;
            };

            egress_list.push(EgressEntry {
                host: item_url.host.clone(),
                sent_text: item_url.full_url.clone(),
                bytes_out: item_resp.bytes_out,
                bytes_in: item_resp.bytes_in,
                at_ns: now_ns.saturating_add(idx as u64 + 1),
            });

            if let Ok(doc) = extract::extract_html(&item_resp.body, &item_url.host) {
                let filename = format!("search_{:016x}_{idx}.json", now_ns);
                if spool::write_spool(
                    &self.spool_dir,
                    &filename,
                    &item_url.full_url,
                    &doc.title,
                    now_ns,
                    &doc.text,
                )
                .is_ok()
                {
                    spool_files.push(filename);
                }
            }
        }

        Ok(Body::FetchResult(FetchResult {
            egress: egress_list,
            spool_files,
        }))
    }
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

/// Initializes C7 fetcher services and registers them with `ServiceContext`.
///
/// # Errors
/// Returns [`neuroos_health::ServiceError`] on failure.
pub fn init(ctx: &mut neuroos_health::ServiceContext) -> Result<(), neuroos_health::ServiceError> {
    let key_path = Path::new("/etc/neuroos/fetch.key");
    let key = if key_path.exists() {
        token::load_key(key_path)
            .map_err(|e| neuroos_health::ServiceError::Custom(e.to_string()))?
    } else {
        // Fallback for dev / uninstalled environments
        SecretKey::from_bytes([0x5A; token::KEY_LEN])
    };

    let spool_dir = PathBuf::from("/var/spool/neuroos-fetcher");
    let handler =
        FetcherHandler::with_mock_transport(Arc::new(key), spool_dir, MockTransport::new());

    ctx.spawn_server("fetch", Arc::new(handler))?;
    ctx.health.set_ready();
    Ok(())
}
