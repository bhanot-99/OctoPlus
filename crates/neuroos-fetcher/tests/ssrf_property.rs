//! Property test of the URL gate (FR-FET-03, security.md §8 "SSRF policy"). Not the ≥ 1 h
//! coverage-guided fuzz run the table asks for (cargo-fuzz is not installed; owner item), but
//! a wide random search over hostile URL text:
//! - `SafeUrl::parse` never panics;
//! - an accepted URL is judged on the host the HTTP client will use (same WHATWG parser);
//! - no accepted URL names a non-public IP literal.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::IpAddr;

use neuroos_fetcher::ssrf::{AddressPolicy, SafeUrl};
use proptest::prelude::*;

/// Characters that matter to URL parsers, plus ordinary ones.
fn piece() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("127.0.0.1".to_string()),
        Just("10.0.0.1".to_string()),
        Just("0x7f".to_string()),
        Just("0177".to_string()),
        Just("2130706433".to_string()),
        Just("[::1]".to_string()),
        Just("[::ffff:7f00:1]".to_string()),
        Just("[::7f00:1]".to_string()),
        Just("localhost".to_string()),
        Just("example.com".to_string()),
        Just("%2e".to_string()),
        Just("%40".to_string()),
        Just("%00".to_string()),
        "[a-z0-9]{1,6}",
        prop::sample::select(vec![
            "@", ":", "/", "\\", "?", "#", ".", "[", "]", "%", ":443", ":80", "..", "\t", " ",
            "ａ", "。", "\u{2024}",
        ])
        .prop_map(str::to_string),
    ]
}

fn url_text() -> impl Strategy<Value = String> {
    (
        prop::sample::select(vec![
            "https://",
            "HTTPS://",
            "https:/",
            "https:\\\\",
            "http://",
            "",
        ]),
        prop::collection::vec(piece(), 0..10),
    )
        .prop_map(|(scheme, parts)| format!("{scheme}{}", parts.concat()))
}

/// 20 000 cases in the gate (about 10 s in a debug build); `NEUROOS_SSRF_CASES` sets more
/// for a long run (200 000 passed on 2026-11-22).
fn cases() -> u32 {
    std::env::var("NEUROOS_SSRF_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20_000)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), .. ProptestConfig::default() })]

    #[test]
    fn the_gate_judges_the_host_the_client_uses(raw in url_text()) {
        let Ok(safe) = SafeUrl::parse(&raw) else { return Ok(()); };
        // The client parses the canonical text the gate returns.
        let client = url::Url::parse(&safe.full_url).expect("canonical URL parses");
        prop_assert_eq!(client.scheme(), "https");
        prop_assert!(client.port().is_none());
        prop_assert!(client.username().is_empty() && client.password().is_none());
        let host = client.host_str().expect("host").trim_matches(|c| c == '[' || c == ']');
        prop_assert_eq!(host, safe.host.as_str());
        // A literal address must be public; a name is checked after resolution (transport).
        if let Ok(ip) = host.parse::<IpAddr>() {
            prop_assert!(AddressPolicy.validate_ip(ip).is_ok(), "{raw:?} -> {ip}");
        }
        // And the raw text, parsed by the client directly, names the same host.
        let direct = url::Url::parse(&raw).expect("raw parses when the gate accepted it");
        prop_assert_eq!(direct.host_str(), client.host_str());
    }
}
