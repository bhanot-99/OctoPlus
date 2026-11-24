//! Search Provider Adapter Interface (FR-FET-05, OQ-02, Risk R-14).
//!
//! Provides a decoupled abstraction over search providers, converting query strings into
//! validated HTTPS requests and extracting top search result items (title, link, snippet).

use serde::{Deserialize, Serialize};

/// A single search result entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResult {
    /// Result page title.
    pub title: String,
    /// Destination URL.
    pub url: String,
    /// Text snippet / description.
    pub snippet: String,
}

/// Interface for search provider adapters.
pub trait SearchAdapter: Send + Sync {
    /// Formulates the exact HTTPS request URL to send for `query`.
    fn format_query_url(&self, query: &str) -> Result<String, String>;

    /// Parses search results from the provider response payload.
    fn parse_results(&self, body: &str, max_results: usize) -> Result<Vec<SearchResult>, String>;
}

/// Generic HTML search adapter suitable for standard privacy-preserving HTML search endpoints.
#[derive(Debug, Clone)]
pub struct GenericSearchAdapter {
    /// Base URL endpoint, e.g. "https://html.duckduckgo.com/html/" or configured local mock.
    pub endpoint: String,
}

impl GenericSearchAdapter {
    /// Creates a new generic search adapter with `endpoint`.
    #[must_use]
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }
}

impl Default for GenericSearchAdapter {
    fn default() -> Self {
        Self {
            endpoint: octoplus_common::search::SEARCH_ENDPOINT.to_string(),
        }
    }
}

impl GenericSearchAdapter {
    /// The provider's base domain, the last two labels of its host (the endpoint may be
    /// `html.duckduckgo.com` while its links use `duckduckgo.com`).
    fn provider_domain(&self) -> Option<String> {
        let url = url::Url::parse(&self.endpoint).ok()?;
        let host = url.host_str()?.to_ascii_lowercase();
        let labels: Vec<&str> = host.split('.').collect();
        let base = labels[labels.len().saturating_sub(2)..].join(".");
        Some(base)
    }

    /// Whether `host` is the provider's own site (its pages are never results).
    fn is_provider(&self, host: Option<&str>) -> bool {
        match (self.provider_domain(), host) {
            (Some(base), Some(h)) => {
                let h = h.to_ascii_lowercase();
                h == base || h.ends_with(&format!(".{base}"))
            }
            _ => false,
        }
    }

    /// The real target of a result link: direct `https://` links, or the `uddg` parameter of
    /// the provider's redirect wrapper. Anything else (ads, settings, other schemes) is `None`.
    fn result_target(&self, href: &str) -> Option<String> {
        let href = decode_entities(href);
        let absolute = if href.starts_with("//") {
            format!("https:{href}")
        } else {
            href
        };
        let url = url::Url::parse(&absolute).ok()?;
        let target = if self.is_provider(url.host_str()) {
            if url.path() != "/l/" {
                return None;
            }
            url.query_pairs()
                .find(|(k, _)| k == "uddg")
                .map(|(_, v)| v.into_owned())?
        } else {
            absolute
        };
        let parsed = url::Url::parse(&target).ok()?;
        if parsed.scheme() != "https" || self.is_provider(parsed.host_str()) {
            return None;
        }
        Some(parsed.to_string())
    }
}

impl SearchAdapter for GenericSearchAdapter {
    fn format_query_url(&self, query: &str) -> Result<String, String> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Err("search query is empty".to_string());
        }
        // `application/x-www-form-urlencoded`: UTF-8 bytes, space as '+' (audit A-07).
        let encoded: String = url::form_urlencoded::byte_serialize(trimmed.as_bytes()).collect();
        let separator = if self.endpoint.contains('?') {
            '&'
        } else {
            '?'
        };
        Ok(format!("{}{separator}q={encoded}", self.endpoint))
    }

    fn parse_results(&self, body: &str, max_results: usize) -> Result<Vec<SearchResult>, String> {
        let mut results: Vec<SearchResult> = Vec::new();
        let mut cursor = body;
        while let Some(a_start) = cursor.find("<a ") {
            let Some(a_len) = cursor[a_start..].find("</a>") else {
                break;
            };
            let anchor = &cursor[a_start..a_start + a_len];
            cursor = &cursor[a_start + a_len + 4..];

            let Some(target) = href_of(anchor).and_then(|h| self.result_target(h)) else {
                continue;
            };
            let inner = anchor.find('>').map_or("", |i| &anchor[i + 1..]);
            let text = plain_text(inner);
            if text.is_empty() {
                continue;
            }
            // A second anchor to a result already seen is its snippet.
            if let Some(seen) = results.iter_mut().find(|r| r.url == target) {
                if seen.snippet.is_empty() && anchor.contains("result__snippet") {
                    seen.snippet = text;
                }
                continue;
            }
            if results.len() >= max_results {
                continue;
            }
            results.push(SearchResult {
                title: text,
                url: target,
                snippet: String::new(),
            });
        }
        Ok(results)
    }
}

/// The `href="..."` value of an anchor's opening tag.
fn href_of(anchor: &str) -> Option<&str> {
    let open_tag = &anchor[..anchor.find('>')?];
    let start = open_tag.find("href=\"")? + 6;
    let len = open_tag[start..].find('"')?;
    Some(&open_tag[start..start + len])
}

/// Text of an HTML fragment: tags removed, the common entities decoded, spaces collapsed.
fn plain_text(fragment: &str) -> String {
    let mut out = String::with_capacity(fragment.len());
    let mut in_tag = false;
    for c in fragment.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    decode_entities(&out)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Decodes the entities search pages use in links and titles.
fn decode_entities(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_query_encodes_spaces_and_special_chars() {
        let adapter = GenericSearchAdapter::new("https://search.example/find");
        let url = adapter.format_query_url("shampoo for dry scalp").unwrap();
        assert_eq!(url, "https://search.example/find?q=shampoo+for+dry+scalp");
    }

    #[test]
    fn parse_html_search_results() {
        let adapter = GenericSearchAdapter::default();
        let html = r#"
            <div>
                <a href="https://example.com/item1">First Product</a>
                <span>Description 1</span>
            </div>
            <div>
                <a href="https://example.org/item2">Second Product</a>
                <span>Description 2</span>
            </div>
        "#;

        let results = adapter.parse_results(html, 5).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "First Product");
        assert_eq!(results[0].url, "https://example.com/item1");
        assert_eq!(results[1].title, "Second Product");
        assert_eq!(results[1].url, "https://example.org/item2");
    }

    /// Regression (audit A-07): non-ASCII text is sent as UTF-8 percent-encoding.
    #[test]
    fn format_query_percent_encodes_utf8_bytes() {
        let adapter = GenericSearchAdapter::new("https://search.example/find");
        let url = adapter.format_query_url("café & crème?").unwrap();
        assert_eq!(
            url,
            "https://search.example/find?q=caf%C3%A9+%26+cr%C3%A8me%3F"
        );
    }

    /// Regression (audit A-07): the provider's own result links are redirect wrappers
    /// (`//duckduckgo.com/l/?uddg=...`); the real target is unwrapped, provider pages,
    /// ads and duplicates are skipped, and tags in titles are removed.
    #[test]
    fn parse_provider_redirect_links() {
        let adapter = GenericSearchAdapter::default();
        let html = r#"
            <a href="https://duckduckgo.com/settings">Settings</a>
            <h2 class="result__title"><a rel="nofollow" class="result__a"
               href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.example.com%2Fdry%2Dscalp%3Fa%3D1&amp;rut=abc">Best <b>shampoo</b> for dry scalp</a></h2>
            <a class="result__snippet" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.example.com%2Fdry%2Dscalp%3Fa%3D1&amp;rut=abc">Gentle &amp; <b>moisturising</b> options.</a>
            <a class="result__a" href="https://duckduckgo.com/y.js?ad_domain=ads.example&amp;u3=x">Ad</a>
            <h2 class="result__title"><a class="result__a" href="https://second.example.org/page">Second</a></h2>
            <a class="result__a" href="//duckduckgo.com/l/?uddg=http%3A%2F%2Fplain.example%2F">Not https</a>
        "#;
        let results = adapter.parse_results(html, 5).unwrap();
        let urls: Vec<&str> = results.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(
            urls,
            [
                "https://www.example.com/dry-scalp?a=1",
                "https://second.example.org/page"
            ]
        );
        assert_eq!(results[0].title, "Best shampoo for dry scalp");
        assert_eq!(results[0].snippet, "Gentle & moisturising options.");
    }
}
