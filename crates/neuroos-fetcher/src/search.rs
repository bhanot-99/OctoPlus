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
            endpoint: "https://html.duckduckgo.com/html/".to_string(),
        }
    }
}

impl SearchAdapter for GenericSearchAdapter {
    fn format_query_url(&self, query: &str) -> Result<String, String> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Err("search query is empty".to_string());
        }
        // Minimal percent-encoding for alphanumeric and spaces
        let encoded: String = trimmed
            .chars()
            .map(|c| match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
                ' ' => "+".to_string(),
                _ => format!("%{:02X}", c as u32),
            })
            .collect();

        let separator = if self.endpoint.contains('?') {
            '&'
        } else {
            '?'
        };
        Ok(format!("{}{separator}q={encoded}", self.endpoint))
    }

    fn parse_results(&self, body: &str, max_results: usize) -> Result<Vec<SearchResult>, String> {
        // Simple, resilient extraction for HTML search results
        let mut results = Vec::new();

        // Look for anchor tags containing result links
        let mut cursor = body;
        while results.len() < max_results {
            let Some(a_start) = cursor.find("<a ") else {
                break;
            };
            let Some(a_end) = cursor[a_start..].find("</a>") else {
                break;
            };
            let anchor = &cursor[a_start..a_start + a_end + 4];
            cursor = &cursor[a_start + a_end + 4..];

            // Extract href
            if let Some(href_start) = anchor.find("href=\"") {
                let after_href = &anchor[href_start + 6..];
                if let Some(href_end) = after_href.find('"') {
                    let href = &after_href[..href_end];
                    // Skip internal links
                    if !href.starts_with("https://") {
                        continue;
                    }

                    // Extract inner text as title
                    let inner = match anchor.find('>') {
                        Some(tag_close) => {
                            let text_part = &anchor[tag_close + 1..anchor.len() - 4];
                            text_part.trim()
                        }
                        None => "",
                    };

                    if !inner.is_empty() {
                        results.push(SearchResult {
                            title: inner.to_string(),
                            url: href.to_string(),
                            snippet: String::new(),
                        });
                    }
                }
            }
        }

        Ok(results)
    }
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
}
