//! Network Transport Layer for C7 Fetcher (FR-FET-03, FR-FET-04, FR-FET-06, rules.md §8.4).
//!
//! [`HttpsTransport`] is the only code in the product that opens an outbound connection
//! (R0-1). One stateless client is built at start:
//! - **Resolve once, validate every address, pin** (FR-FET-03, Architecture §8.6): host names
//!   go through [`ValidatingResolver`], which looks the name up once and refuses the whole
//!   answer if any address is non-public; the connection can only use the addresses it just
//!   validated. IP literals are validated by [`SafeUrl`] (the client does not resolve them).
//! - **Redirects** are never followed by the client: each `Location` is re-validated as a new
//!   [`SafeUrl`] and counted, at most [`crate::ssrf::MAX_REDIRECTS`].
//! - **Limits** (FR-FET-04): `GET` only, text content types only, body ≤ [`MAX_BODY_BYTES`],
//!   one deadline for the whole request including redirects.
//! - **Hygiene** (FR-FET-06): no cookies, no connection reuse between requests, no proxy from
//!   the environment, no referrer, no compression, one fixed generic user agent.
//!
//! [`MockTransport`] serves tests that must not touch any network (rules.md §8.4).

use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::ssrf::{AddressPolicy, RedirectTracker, SafeUrl};

/// Largest response body accepted (FR-FET-04).
pub const MAX_BODY_BYTES: u64 = 5 * 1024 * 1024;

/// The fixed, generic user agent every request carries (FR-FET-06).
pub const USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";

/// The `Accept` header every request carries: text only (FR-FET-04).
const ACCEPT: &str = "text/html,application/xhtml+xml,text/plain;q=0.9";

/// The page a transfer returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response body (UTF-8, invalid sequences replaced).
    pub body: String,
}

/// One request that left the machine, for the egress ledger (FR-FET-09, R0-2). A redirect
/// is a second hop, with its own host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hop {
    /// Host the request went to.
    pub host: String,
    /// The exact URL requested.
    pub url: String,
    /// Request line and headers at the HTTP layer (TLS framing is not counted); 0 when the
    /// connection was never made.
    pub bytes_out: u64,
    /// Status line, headers and body read.
    pub bytes_in: u64,
}

/// Everything one fetch did: every hop attempted, success or not, and the outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    /// Requests attempted, in order. Reported to the ledger even when the fetch failed.
    pub hops: Vec<Hop>,
    /// The page, or why there is none.
    pub outcome: Result<TransportResponse, TransportError>,
}

/// Why a transfer failed. The text never contains page content.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    /// A redirect target failed URL or address policy, or there were too many.
    #[error("redirect refused: {0}")]
    Redirect(String),
    /// The server answered with a non-success status.
    #[error("http status {0}")]
    Status(u16),
    /// The content type is not text (FR-FET-04).
    #[error("content type is not text")]
    NotText,
    /// The body is larger than [`MAX_BODY_BYTES`].
    #[error("body too large")]
    TooLarge,
    /// The whole request took longer than its deadline.
    #[error("deadline exceeded")]
    Timeout,
    /// Connection, TLS, DNS or address-policy failure.
    #[error("connection failed: {0}")]
    Connect(String),
}

/// Interface for executing GET requests against validated URLs (dyn-compatible).
pub trait Transport: Send + Sync {
    /// Fetches the given [`SafeUrl`], following validated redirects.
    fn fetch<'a>(&'a self, url: &'a SafeUrl) -> Pin<Box<dyn Future<Output = Attempt> + Send + 'a>>;
}

/// Looks up the addresses of a host name.
pub trait HostResolver: Send + Sync {
    /// Every address the name resolves to.
    fn lookup(
        &self,
        host: String,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<IpAddr>, String>> + Send + '_>>;
}

/// The system's DNS configuration (`/etc/resolv.conf`, `/etc/hosts`), read once at start.
pub struct SystemResolver(hickory_resolver::TokioResolver);

impl SystemResolver {
    /// Reads the system configuration and builds the resolver.
    ///
    /// # Errors
    /// Returns the reason when the configuration cannot be read.
    pub fn from_system() -> Result<Self, String> {
        let resolver = hickory_resolver::TokioResolver::builder_tokio()
            .map_err(|e| e.to_string())?
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self(resolver))
    }
}

impl std::fmt::Debug for SystemResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SystemResolver")
    }
}

impl HostResolver for SystemResolver {
    fn lookup(
        &self,
        host: String,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<IpAddr>, String>> + Send + '_>> {
        Box::pin(async move {
            let found = self
                .0
                .lookup_ip(host.as_str())
                .await
                .map_err(|e| e.to_string())?;
            Ok(found.iter().collect())
        })
    }
}

/// A fixed name → addresses table: for tests in a private network namespace, where no
/// DNS server exists (rules.md §8.4).
#[derive(Debug, Default, Clone)]
pub struct StaticResolver(HashMap<String, Vec<IpAddr>>);

impl StaticResolver {
    /// Creates a resolver that knows only `entries`.
    #[must_use]
    pub fn new(entries: impl IntoIterator<Item = (String, Vec<IpAddr>)>) -> Self {
        Self(
            entries
                .into_iter()
                .map(|(h, a)| (h.to_ascii_lowercase(), a))
                .collect(),
        )
    }
}

impl HostResolver for StaticResolver {
    fn lookup(
        &self,
        host: String,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<IpAddr>, String>> + Send + '_>> {
        let found = self.0.get(&host.to_ascii_lowercase()).cloned();
        Box::pin(async move { found.ok_or_else(|| "no such host".to_string()) })
    }
}

/// Validates every resolved address (FR-FET-03). All or nothing: one non-public address
/// refuses the whole answer, so a name cannot mix a public and a private address.
///
/// # Errors
/// Returns the reason when the answer is empty or any address is blocked.
pub fn validated_addrs(ips: &[IpAddr]) -> Result<Vec<SocketAddr>, String> {
    if ips.is_empty() {
        return Err("no addresses".to_string());
    }
    for ip in ips {
        AddressPolicy
            .validate_ip(*ip)
            .map_err(|_| "resolved to a blocked address".to_string())?;
    }
    // Port 0: the client puts the URL's port (always 443) on resolved addresses.
    Ok(ips.iter().map(|ip| SocketAddr::new(*ip, 0)).collect())
}

/// The client's resolver: resolve once with `inner`, then [`validated_addrs`].
struct ValidatingResolver {
    inner: Arc<dyn HostResolver>,
}

impl reqwest::dns::Resolve for ValidatingResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let inner = Arc::clone(&self.inner);
        let host = name.as_str().to_string();
        Box::pin(async move {
            let ips = inner.lookup(host).await?;
            let addrs = validated_addrs(&ips)?;
            let iter: reqwest::dns::Addrs = Box::new(addrs.into_iter());
            Ok(iter)
        })
    }
}

/// Settings for [`HttpsTransport`].
pub struct TransportConfig {
    /// Name resolution; [`SystemResolver`] in the product.
    pub resolver: Arc<dyn HostResolver>,
    /// Deadline for one whole request including redirects (FR-FET-04, `deadlines.fetch_ms`).
    pub deadline: Duration,
    /// Extra trusted roots (PEM), for tests with a private certificate authority only. When
    /// set, the system roots are not trusted.
    pub test_root_pem: Option<Vec<u8>>,
}

impl std::fmt::Debug for TransportConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransportConfig")
            .field("deadline", &self.deadline)
            .field("test_root", &self.test_root_pem.is_some())
            .finish_non_exhaustive()
    }
}

/// The real HTTPS transport (see the module documentation).
#[derive(Debug)]
pub struct HttpsTransport {
    client: reqwest::Client,
    deadline: Duration,
}

impl HttpsTransport {
    /// Builds the one client. With the system roots this reads the CA store, so the
    /// sandbox must allow it (see `main.rs`).
    ///
    /// # Errors
    /// Returns the reason when the TLS roots cannot be loaded or the client cannot be built.
    pub fn new(cfg: TransportConfig) -> Result<Self, TransportError> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static(ACCEPT),
        );
        let mut builder = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .no_proxy()
            .https_only(true)
            .http1_only()
            .pool_max_idle_per_host(0)
            .connect_timeout(cfg.deadline)
            .timeout(cfg.deadline)
            .min_tls_version(reqwest::tls::Version::TLS_1_2)
            .dns_resolver(Arc::new(ValidatingResolver {
                inner: cfg.resolver,
            }));
        if let Some(pem) = &cfg.test_root_pem {
            let cert = reqwest::Certificate::from_pem(pem)
                .map_err(|e| TransportError::Connect(format!("test root: {e}")))?;
            builder = builder
                .tls_built_in_root_certs(false)
                .add_root_certificate(cert);
        }
        let client = builder
            .build()
            .map_err(|e| TransportError::Connect(format!("client: {e}")))?;
        Ok(Self {
            client,
            deadline: cfg.deadline,
        })
    }

    async fn fetch_inner(
        &self,
        url: &SafeUrl,
        hops: &mut Vec<Hop>,
    ) -> Result<TransportResponse, TransportError> {
        let mut tracker = RedirectTracker::new();
        let mut current = url.clone();
        loop {
            hops.push(Hop {
                host: current.host.clone(),
                url: current.full_url.clone(),
                bytes_out: request_bytes(&current),
                bytes_in: 0,
            });
            let hop = hops.len() - 1;
            let resp = match self.client.get(current.full_url.as_str()).send().await {
                Ok(r) => r,
                Err(e) => {
                    // No connection: the request itself never left (the name lookup did).
                    if e.is_connect() {
                        hops[hop].bytes_out = 0;
                    }
                    return Err(connect_error(&e));
                }
            };
            hops[hop].bytes_in = head_bytes(&resp);
            let status = resp.status();
            if status.is_redirection() {
                let location = resp
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| TransportError::Redirect("no location".to_string()))?
                    .to_string();
                current = tracker
                    .follow(&current, &location)
                    .map_err(|e| TransportError::Redirect(e.to_string()))?;
                continue;
            }
            if !status.is_success() {
                return Err(TransportError::Status(status.as_u16()));
            }
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if !is_text(content_type) {
                return Err(TransportError::NotText);
            }
            let body = read_capped(resp, MAX_BODY_BYTES, &mut hops[hop].bytes_in).await?;
            return Ok(TransportResponse {
                status: status.as_u16(),
                body: String::from_utf8_lossy(&body).into_owned(),
            });
        }
    }
}

impl Transport for HttpsTransport {
    fn fetch<'a>(&'a self, url: &'a SafeUrl) -> Pin<Box<dyn Future<Output = Attempt> + Send + 'a>> {
        Box::pin(async move {
            let mut hops = Vec::new();
            let outcome = tokio::time::timeout(self.deadline, self.fetch_inner(url, &mut hops))
                .await
                .unwrap_or(Err(TransportError::Timeout));
            Attempt { hops, outcome }
        })
    }
}

/// Text types only (FR-FET-04); parameters such as `charset` are ignored.
#[must_use]
pub fn is_text(content_type: &str) -> bool {
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    matches!(
        essence.as_str(),
        "text/html" | "text/plain" | "application/xhtml+xml"
    )
}

/// Reads the body, refusing it as soon as it passes `cap` bytes; counts what was read.
async fn read_capped(
    mut resp: reqwest::Response,
    cap: u64,
    bytes_in: &mut u64,
) -> Result<Vec<u8>, TransportError> {
    if resp.content_length().is_some_and(|n| n > cap) {
        return Err(TransportError::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| connect_error(&e))? {
        *bytes_in = bytes_in.saturating_add(chunk.len() as u64);
        if (body.len() + chunk.len()) as u64 > cap {
            return Err(TransportError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Request line plus the headers this client sends, as HTTP/1.1 text.
fn request_bytes(url: &SafeUrl) -> u64 {
    let lines = [
        format!("GET {} HTTP/1.1\r\n", url.path_and_query),
        format!("host: {}\r\n", url.host),
        format!("user-agent: {USER_AGENT}\r\n"),
        format!("accept: {ACCEPT}\r\n"),
        "\r\n".to_string(),
    ];
    lines.iter().map(|l| l.len() as u64).sum()
}

/// Status line plus received headers, as HTTP/1.1 text.
fn head_bytes(resp: &reqwest::Response) -> u64 {
    let status_line = 17_u64; // "HTTP/1.1 200 OK\r\n" is the usual length.
    resp.headers().iter().fold(status_line + 2, |acc, (k, v)| {
        acc + (k.as_str().len() + 2 + v.len() + 2) as u64
    })
}

/// Connection error text without URL or content (rules.md §6.3).
fn connect_error(e: &reqwest::Error) -> TransportError {
    if e.is_timeout() {
        return TransportError::Timeout;
    }
    let mut kind = if e.is_connect() {
        "connect"
    } else if e.is_body() || e.is_decode() {
        "body"
    } else {
        "request"
    };
    // The resolver's refusal surfaces as a connect error; name it so the ledger and the
    // user see why nothing was fetched.
    let mut source: Option<&dyn std::error::Error> = std::error::Error::source(e);
    while let Some(s) = source {
        if s.to_string().contains("blocked address") {
            kind = "blocked address";
            break;
        }
        source = s.source();
    }
    TransportError::Connect(kind.to_string())
}

/// In-memory mock transport for unit and integration testing without live internet.
#[derive(Debug, Default, Clone)]
pub struct MockTransport {
    responses: Arc<Mutex<HashMap<String, String>>>,
    recorded_requests: Arc<Mutex<Vec<String>>>,
}

impl MockTransport {
    /// Creates a new empty mock transport.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a mock HTML response for a specific exact full URL.
    pub fn register(&self, url: impl Into<String>, body: impl Into<String>) {
        let mut map = self.responses.lock().unwrap_or_else(|e| e.into_inner());
        map.insert(url.into(), body.into());
    }

    /// Returns the list of URLs requested through this transport.
    #[must_use]
    pub fn requests(&self) -> Vec<String> {
        self.recorded_requests
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl Transport for MockTransport {
    fn fetch<'a>(&'a self, url: &'a SafeUrl) -> Pin<Box<dyn Future<Output = Attempt> + Send + 'a>> {
        Box::pin(async move {
            let full_url = url.full_url.clone();
            {
                let mut reqs = self
                    .recorded_requests
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                reqs.push(full_url.clone());
            }

            let map = self.responses.lock().unwrap_or_else(|e| e.into_inner());
            let body = if let Some(resp) = map.get(&full_url) {
                resp.clone()
            } else {
                format!(
                    "<!DOCTYPE html><html><head><title>Mock Page for {}</title></head><body><p>Content for {}</p></body></html>",
                    url.host, url.path_and_query
                )
            };

            Attempt {
                hops: vec![Hop {
                    host: url.host.clone(),
                    url: full_url,
                    bytes_out: request_bytes(url),
                    bytes_in: body.len() as u64,
                }],
                outcome: Ok(TransportResponse { status: 200, body }),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_text_content_types_are_accepted() {
        for ok in [
            "text/html",
            "text/html; charset=utf-8",
            "TEXT/PLAIN",
            "application/xhtml+xml",
        ] {
            assert!(is_text(ok), "{ok}");
        }
        for bad in [
            "",
            "application/json",
            "image/png",
            "text/htmlx",
            "application/octet-stream",
        ] {
            assert!(!is_text(bad), "{bad}");
        }
    }

    #[test]
    fn one_blocked_address_refuses_the_whole_answer() {
        let public: IpAddr = "93.184.216.34".parse().unwrap();
        let private: IpAddr = "10.0.0.5".parse().unwrap();
        assert_eq!(validated_addrs(&[public]).unwrap().len(), 1);
        assert!(validated_addrs(&[public, private]).is_err());
        assert!(validated_addrs(&[]).is_err());
        assert!(validated_addrs(&["::1".parse().unwrap()]).is_err());
    }

    #[tokio::test]
    async fn static_resolver_is_case_insensitive_and_refuses_unknown_names() {
        let r = StaticResolver::new([(
            "Example.COM".to_string(),
            vec!["93.184.216.34".parse().unwrap()],
        )]);
        assert_eq!(r.lookup("example.com".to_string()).await.unwrap().len(), 1);
        assert!(r.lookup("other.example".to_string()).await.is_err());
    }

    #[tokio::test]
    async fn mock_transport_records_and_returns_registered_response() {
        let transport = MockTransport::new();
        let safe = SafeUrl::parse("https://example.com/test-doc").unwrap();
        transport.register("https://example.com/test-doc", "<p>Special Test Data</p>");

        let attempt = transport.fetch(&safe).await;
        let resp = attempt.outcome.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, "<p>Special Test Data</p>");
        assert_eq!(attempt.hops.len(), 1);
        assert!(attempt.hops[0].bytes_out > 0);
        assert!(attempt.hops[0].bytes_in > 0);

        let reqs = transport.requests();
        assert_eq!(reqs, vec!["https://example.com/test-doc"]);
    }
}
