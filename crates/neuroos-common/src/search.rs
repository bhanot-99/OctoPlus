//! The one configured search provider (FR-FET-05, OQ-02, ADR-0016).
//!
//! C7 sends approved queries here and C6's dialog names it, so both read it from this one
//! place: the user is told exactly where the text goes and how many pages are read.

/// The provider's HTML search endpoint; the query is appended as `q=`.
pub const SEARCH_ENDPOINT: &str = "https://html.duckduckgo.com/html/";

/// The provider's name as the approval dialog shows it.
pub const SEARCH_PROVIDER: &str = "DuckDuckGo";

/// Result pages one approved search reads (UJ-3, design.md §6: "up to 2 result pages").
pub const SEARCH_RESULT_PAGES: u32 = 2;
