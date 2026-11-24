#![no_main]
//! A redirect `Location` (any text) joined to a public page: whatever is accepted meets
//! `common::check`, so a redirect can never lead to a private address or another port.

use libfuzzer_sys::fuzz_target;
use octoplus_fetcher::ssrf::SafeUrl;

mod common;

fuzz_target!(|data: &[u8]| {
    let Ok(location) = std::str::from_utf8(data) else {
        return;
    };
    let base = SafeUrl::parse("https://example.com/a/b?q=1").expect("base");
    if let Ok(next) = base.join(location) {
        common::check(&next);
    }
});
