//! The one configured search provider (FR-FET-05, OQ-02, ADR-0016).
//!
//! C7 sends approved queries here and C6's dialog names it, so both read it from this one
//! place: the user is told exactly where the text goes and how many pages are read.

/// The provider's HTML search endpoint; the query is appended as `q=`.
pub const SEARCH_ENDPOINT: &str = "https://html.duckduckgo.com/html/";

/// The host the query goes to, as the approval dialog shows it. It must be a lower-case
/// host name: the dialog refuses anything else (BUG-116), and it is the host of
/// [`SEARCH_ENDPOINT`], so the user is shown where the text really goes.
pub const SEARCH_PROVIDER: &str = "html.duckduckgo.com";

/// Result pages one approved search reads (UJ-3, design.md §6: "up to 2 result pages").
pub const SEARCH_RESULT_PAGES: u32 = 2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_provider_is_the_endpoints_host() {
        let rest = SEARCH_ENDPOINT.strip_prefix("https://").unwrap();
        assert_eq!(rest.split('/').next(), Some(SEARCH_PROVIDER));
    }
}
