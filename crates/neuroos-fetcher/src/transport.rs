//! Network Transport Layer for C7 Fetcher (FR-FET-04, FR-FET-06, rules.md §8.4).
//!
//! Abstracts the physical HTTPS transfer to ensure:
//! - Offline testing compliance (rules.md §8.4: zero test traffic to the live internet).
//! - Accurate byte accounting for the egress ledger (bytes out and bytes in).
//! - Statelessness (no cookies, no session cache).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use crate::ssrf::SafeUrl;

/// Response returned by a transport execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportResponse {
    /// HTTP status code.
    pub status: u16,
    /// Extracted response body text.
    pub body: String,
    /// Outbound bytes transmitted on the wire (request line + headers + query).
    pub bytes_out: u64,
    /// Inbound bytes received on the wire (status line + headers + body).
    pub bytes_in: u64,
}

/// Interface for executing GET requests against validated URLs (dyn-compatible).
pub trait Transport: Send + Sync {
    /// Fetches the given [`SafeUrl`].
    fn fetch<'a>(
        &'a self,
        url: &'a SafeUrl,
    ) -> Pin<Box<dyn Future<Output = Result<TransportResponse, String>> + Send + 'a>>;
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
    fn fetch<'a>(
        &'a self,
        url: &'a SafeUrl,
    ) -> Pin<Box<dyn Future<Output = Result<TransportResponse, String>> + Send + 'a>> {
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

            let bytes_out = (full_url.len() + 120) as u64;
            let bytes_in = (body.len() + 80) as u64;

            Ok(TransportResponse {
                status: 200,
                body,
                bytes_out,
                bytes_in,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_transport_records_and_returns_registered_response() {
        let transport = MockTransport::new();
        let safe = SafeUrl::parse("https://example.com/test-doc").unwrap();
        transport.register("https://example.com/test-doc", "<p>Special Test Data</p>");

        let resp = transport.fetch(&safe).await.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, "<p>Special Test Data</p>");
        assert!(resp.bytes_out > 0);
        assert!(resp.bytes_in > 0);

        let reqs = transport.requests();
        assert_eq!(reqs, vec!["https://example.com/test-doc"]);
    }
}
