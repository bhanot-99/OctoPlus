//! The invariants every accepted URL must meet (the same ones as `tests/ssrf_property.rs`).

use std::net::IpAddr;

use octoplus_fetcher::ssrf::{AddressPolicy, SafeUrl};

/// An accepted URL is HTTPS on the default port, carries no credentials, is judged on the
/// host the HTTP client will use, and never names a non-public IP literal.
pub fn check(safe: &SafeUrl) {
    let client = url::Url::parse(&safe.full_url).expect("the canonical URL parses");
    assert_eq!(client.scheme(), "https");
    assert!(client.port().is_none(), "custom port accepted: {}", safe.full_url);
    assert!(client.username().is_empty() && client.password().is_none());
    let host = client
        .host_str()
        .expect("an accepted URL has a host")
        .trim_matches(|c| c == '[' || c == ']');
    // The same address may be written two ways ("::ffff:6f00:1" and "::ffff:111.0.0.1"),
    // so literals are compared as addresses, names as text.
    match (host.parse::<IpAddr>(), safe.host.parse::<IpAddr>()) {
        (Ok(a), Ok(b)) => assert_eq!(a, b, "gate and client disagree on the address"),
        _ => assert_eq!(host, safe.host, "gate and client disagree on the host"),
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        assert!(
            AddressPolicy.validate_ip(ip).is_ok(),
            "non-public literal accepted: {ip}"
        );
    }
}
