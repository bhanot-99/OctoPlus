//! Anti-SSRF Defense-in-Depth Engine (Architecture §8.6, FR-FET-03, security.md §4.1).
//!
//! Validates IP addresses and URLs against strict security policies:
//! - Blocks all non-public IPv4 and IPv6 address ranges (RFC 1918, CGNAT, loopback, link-local, multicast, etc.).
//! - Fully unmaps IPv4-mapped IPv6 addresses (`::ffff:0:0/96`).
//! - Enforces HTTPS only, default port 443 only, and credential-free URLs.
//! - Enforces a maximum redirect depth of 3 with re-validation on every hop.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Maximum allowed character length of a URL (FR-FET-04).
pub const MAX_URL_LEN: usize = 2048;

/// Maximum number of redirects allowed per request (Architecture §8.6).
pub const MAX_REDIRECTS: u32 = 3;

/// Errors returned by the SSRF validator.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SsrError {
    /// URL exceeds length limit.
    #[error("URL exceeds maximum length of {MAX_URL_LEN}")]
    UrlTooLong,
    /// URL has invalid syntax.
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    /// URL scheme is not HTTPS.
    #[error("scheme must be https, got {0}")]
    DisallowedScheme(String),
    /// URL contains userinfo/credentials.
    #[error("user credentials are not permitted in URLs")]
    CredentialsNotAllowed,
    /// URL specifies a non-default port (only 443 permitted).
    #[error("custom ports are not permitted: {0}")]
    CustomPort(u16),
    /// Target IP address falls into a blocked or private range.
    #[error("blocked IP address range: {0}")]
    BlockedIp(IpAddr),
    /// Maximum redirect hops exceeded.
    #[error("maximum redirects ({MAX_REDIRECTS}) exceeded")]
    TooManyRedirects,
}

/// Address policy validator against Architecture §8.6 blocked ranges.
#[derive(Debug, Default, Clone, Copy)]
pub struct AddressPolicy;

impl AddressPolicy {
    /// Checks whether an [`IpAddr`] is safe for outbound egress.
    ///
    /// # Errors
    /// Returns [`SsrError::BlockedIp`] if the IP belongs to any private, loopback, or reserved range.
    pub fn validate_ip(&self, ip: IpAddr) -> Result<(), SsrError> {
        match ip {
            IpAddr::V4(v4) => Self::validate_ipv4(v4),
            IpAddr::V6(v6) => Self::validate_ipv6(v6),
        }
    }

    /// Validates an IPv4 address against Architecture §8.6 rules:
    /// `0.0.0.0/8, 10.0.0.0/8, 100.64.0.0/10, 127.0.0.0/8, 169.254.0.0/16, 172.16.0.0/12,
    /// 192.0.0.0/24, 192.168.0.0/16, 198.18.0.0/15, 224.0.0.0/4, 240.0.0.0/4, 255.255.255.255/32`.
    pub fn validate_ipv4(ip: Ipv4Addr) -> Result<(), SsrError> {
        let octets = ip.octets();
        let u32_val = u32::from_be_bytes(octets);

        // 0.0.0.0/8
        if octets[0] == 0 {
            return Err(SsrError::BlockedIp(IpAddr::V4(ip)));
        }
        // 10.0.0.0/8
        if octets[0] == 10 {
            return Err(SsrError::BlockedIp(IpAddr::V4(ip)));
        }
        // 100.64.0.0/10 (100.64.0.0 to 100.127.255.255)
        if octets[0] == 100 && (octets[1] & 0b1100_0000) == 64 {
            return Err(SsrError::BlockedIp(IpAddr::V4(ip)));
        }
        // 127.0.0.0/8 (Loopback)
        if octets[0] == 127 {
            return Err(SsrError::BlockedIp(IpAddr::V4(ip)));
        }
        // 169.254.0.0/16 (Link-local)
        if octets[0] == 169 && octets[1] == 254 {
            return Err(SsrError::BlockedIp(IpAddr::V4(ip)));
        }
        // 172.16.0.0/12 (172.16.0.0 to 172.31.255.255)
        if octets[0] == 172 && (octets[1] >= 16 && octets[1] <= 31) {
            return Err(SsrError::BlockedIp(IpAddr::V4(ip)));
        }
        // 192.0.0.0/24
        if octets[0] == 192 && octets[1] == 0 && octets[2] == 0 {
            return Err(SsrError::BlockedIp(IpAddr::V4(ip)));
        }
        // 192.168.0.0/16
        if octets[0] == 192 && octets[1] == 168 {
            return Err(SsrError::BlockedIp(IpAddr::V4(ip)));
        }
        // 198.18.0.0/15 (198.18.0.0 to 198.19.255.255)
        if octets[0] == 198 && (octets[1] == 18 || octets[1] == 19) {
            return Err(SsrError::BlockedIp(IpAddr::V4(ip)));
        }
        // 224.0.0.0/4 (Multicast: 224.0.0.0 to 239.255.255.255)
        if (octets[0] & 0b1111_0000) == 224 {
            return Err(SsrError::BlockedIp(IpAddr::V4(ip)));
        }
        // 240.0.0.0/4 (Reserved / Future Use: 240.0.0.0 to 255.255.255.254)
        if (octets[0] & 0b1111_0000) == 240 {
            return Err(SsrError::BlockedIp(IpAddr::V4(ip)));
        }
        // 255.255.255.255/32 (Broadcast)
        if u32_val == 0xFFFF_FFFF {
            return Err(SsrError::BlockedIp(IpAddr::V4(ip)));
        }

        Ok(())
    }

    /// Validates an IPv6 address against Architecture §8.6 rules:
    /// `::/96` (covers `::/128`, `::1/128` and IPv4-compatible), `::ffff:0:0/96` (checked
    /// after unmapping), `64:ff9b::/96, fc00::/7, fe80::/10, ff00::/8, 2001:db8::/32`.
    pub fn validate_ipv6(ip: Ipv6Addr) -> Result<(), SsrError> {
        let segments = ip.segments();

        // ::/128 (Unspecified)
        if ip.is_unspecified() {
            return Err(SsrError::BlockedIp(IpAddr::V6(ip)));
        }
        // ::1/128 (Loopback)
        if ip.is_loopback() {
            return Err(SsrError::BlockedIp(IpAddr::V6(ip)));
        }

        // ::/96 (deprecated IPv4-compatible, e.g. ::7f00:1): a `sit` tunnel can carry it to
        // the embedded IPv4 address, so it is never a public destination (audit A-04).
        if segments[..6].iter().all(|&s| s == 0) {
            return Err(SsrError::BlockedIp(IpAddr::V6(ip)));
        }

        // ::ffff:0:0/96 (IPv4-mapped) -> unmap and validate IPv4
        if segments[0] == 0
            && segments[1] == 0
            && segments[2] == 0
            && segments[3] == 0
            && segments[4] == 0
            && segments[5] == 0xFFFF
        {
            let v4 = Ipv4Addr::new(
                (segments[6] >> 8) as u8,
                segments[6] as u8,
                (segments[7] >> 8) as u8,
                segments[7] as u8,
            );
            Self::validate_ipv4(v4).map_err(|_| SsrError::BlockedIp(IpAddr::V6(ip)))?;
            return Ok(());
        }

        // 64:ff9b::/96 (IPv4-IPv6 translation)
        if segments[0] == 0x0064
            && segments[1] == 0xFF9B
            && segments[2] == 0
            && segments[3] == 0
            && segments[4] == 0
            && segments[5] == 0
        {
            return Err(SsrError::BlockedIp(IpAddr::V6(ip)));
        }

        // fc00::/7 (Unique Local Address: fc00:: to fdff::)
        if (segments[0] & 0xFE00) == 0xFC00 {
            return Err(SsrError::BlockedIp(IpAddr::V6(ip)));
        }

        // fe80::/10 (Link-Local: fe80:: to febf::)
        if (segments[0] & 0xFFC0) == 0xFE80 {
            return Err(SsrError::BlockedIp(IpAddr::V6(ip)));
        }

        // ff00::/8 (Multicast: ff00:: to ffff::)
        if (segments[0] & 0xFF00) == 0xFF00 {
            return Err(SsrError::BlockedIp(IpAddr::V6(ip)));
        }

        // 2001:db8::/32 (Documentation)
        if segments[0] == 0x2001 && segments[1] == 0x0DB8 {
            return Err(SsrError::BlockedIp(IpAddr::V6(ip)));
        }

        Ok(())
    }
}

/// Parsed, validated URL representation safe for network operations.
///
/// Parsing uses the WHATWG parser of the `url` crate, the same one the HTTP client uses, so
/// the host checked here is exactly the host the client connects to (no parser differential,
/// audit A-04). IP-literal hosts are validated here because the client connects to them
/// without asking the resolver; host names are validated after resolution
/// ([`crate::transport`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeUrl {
    /// Host name (lower case) or IP literal (IPv6 without brackets).
    pub host: String,
    /// Path and query string.
    pub path_and_query: String,
    /// Canonical full URL, without fragment: the exact text that is requested.
    pub full_url: String,
    ip: Option<IpAddr>,
}

impl SafeUrl {
    /// Parses and validates a URL string against Scheme, Port, Credential, and Length rules.
    ///
    /// # Errors
    /// Returns [`SsrError`] if the URL is invalid or violates policy.
    pub fn parse(raw: &str) -> Result<Self, SsrError> {
        if raw.len() > MAX_URL_LEN {
            return Err(SsrError::UrlTooLong);
        }
        // Only the exact lower-case prefix: no scheme-relative or mixed-case tricks.
        if !raw.starts_with("https://") {
            return Err(SsrError::DisallowedScheme(scheme_of(raw)));
        }
        let url = url::Url::parse(raw).map_err(|e| SsrError::InvalidUrl(e.to_string()))?;
        Self::from_url(url)
    }

    /// Resolves `location` (a redirect target) against this URL and validates the result.
    ///
    /// # Errors
    /// Returns [`SsrError`] if the target is invalid or violates policy.
    pub fn join(&self, location: &str) -> Result<Self, SsrError> {
        if location.len() > MAX_URL_LEN {
            return Err(SsrError::UrlTooLong);
        }
        let base =
            url::Url::parse(&self.full_url).map_err(|e| SsrError::InvalidUrl(e.to_string()))?;
        let url = base
            .join(location)
            .map_err(|e| SsrError::InvalidUrl(e.to_string()))?;
        Self::from_url(url)
    }

    /// The host as an IP address when the URL names one directly.
    #[must_use]
    pub fn ip_literal(&self) -> Option<IpAddr> {
        self.ip
    }

    fn from_url(mut url: url::Url) -> Result<Self, SsrError> {
        if url.scheme() != "https" {
            return Err(SsrError::DisallowedScheme(url.scheme().to_string()));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(SsrError::CredentialsNotAllowed);
        }
        // The parser drops the default port, so any port left is a custom one.
        if let Some(port) = url.port() {
            return Err(SsrError::CustomPort(port));
        }
        let (host, ip) = match url.host() {
            Some(url::Host::Domain(d)) => {
                // A name made only of digits, dots and hex is an address in some resolvers'
                // eyes even when the WHATWG parser leaves it a name; refuse it outright.
                if d.parse::<IpAddr>().is_ok() || d.is_empty() {
                    return Err(SsrError::InvalidUrl("ambiguous host".to_string()));
                }
                (d.to_string(), None)
            }
            Some(url::Host::Ipv4(v4)) => (v4.to_string(), Some(IpAddr::V4(v4))),
            Some(url::Host::Ipv6(v6)) => (v6.to_string(), Some(IpAddr::V6(v6))),
            None => return Err(SsrError::InvalidUrl("missing host in URL".to_string())),
        };
        if let Some(ip) = ip {
            AddressPolicy.validate_ip(ip)?;
        }
        url.set_fragment(None);
        let full_url = url.to_string();
        if full_url.len() > MAX_URL_LEN {
            return Err(SsrError::UrlTooLong);
        }
        let path_and_query = match url.query() {
            Some(q) => format!("{}?{q}", url.path()),
            None => url.path().to_string(),
        };
        Ok(Self {
            host,
            path_and_query,
            full_url,
            ip,
        })
    }
}

/// The scheme part of `raw` for the error message, bounded so no long input is echoed.
fn scheme_of(raw: &str) -> String {
    raw.split(':')
        .next()
        .unwrap_or_default()
        .chars()
        .take(16)
        .collect()
}

/// Redirect tracking state ensuring at most 3 redirects occur.
#[derive(Debug, Default)]
pub struct RedirectTracker {
    hops: u32,
}

impl RedirectTracker {
    /// Creates a fresh redirect tracker.
    #[must_use]
    pub fn new() -> Self {
        Self { hops: 0 }
    }

    /// Records one redirect hop to `next_url` and validates it against SSRF policy.
    ///
    /// # Errors
    /// Returns [`SsrError::TooManyRedirects`] if exceeded, or any [`SsrError`] if the URL is invalid.
    pub fn step(&mut self, next_url: &str) -> Result<SafeUrl, SsrError> {
        if self.hops >= MAX_REDIRECTS {
            return Err(SsrError::TooManyRedirects);
        }
        self.hops += 1;
        SafeUrl::parse(next_url)
    }

    /// Records one redirect hop from `current` to `location` (relative or absolute) and
    /// validates the target.
    ///
    /// # Errors
    /// Returns [`SsrError::TooManyRedirects`] if exceeded, or any [`SsrError`] if invalid.
    pub fn follow(&mut self, current: &SafeUrl, location: &str) -> Result<SafeUrl, SsrError> {
        if self.hops >= MAX_REDIRECTS {
            return Err(SsrError::TooManyRedirects);
        }
        self.hops += 1;
        current.join(location)
    }

    /// Returns the number of hops taken so far.
    #[must_use]
    pub fn hops(&self) -> u32 {
        self.hops
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_all_prohibited_ipv4_ranges() {
        let policy = AddressPolicy;
        for blocked in [
            "0.0.0.0",
            "0.1.2.3",
            "10.0.0.1",
            "10.254.254.254",
            "100.64.0.1",
            "100.127.255.254",
            "127.0.0.1",
            "127.128.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "172.31.255.255",
            "192.0.0.1",
            "192.168.1.1",
            "198.18.0.1",
            "198.19.255.255",
            "224.0.0.1",
            "239.255.255.255",
            "240.0.0.1",
            "255.255.255.255",
        ] {
            let ip = blocked.parse::<IpAddr>().unwrap();
            assert_eq!(policy.validate_ip(ip), Err(SsrError::BlockedIp(ip)));
        }
    }

    #[test]
    fn allows_public_ipv4() {
        let policy = AddressPolicy;
        for public in ["8.8.8.8", "1.1.1.1", "93.184.216.34", "104.16.132.229"] {
            let ip = public.parse::<IpAddr>().unwrap();
            assert_eq!(policy.validate_ip(ip), Ok(()));
        }
    }

    #[test]
    fn blocks_all_prohibited_ipv6_ranges() {
        let policy = AddressPolicy;
        for blocked in [
            "::",
            "::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "::ffff:192.168.1.1",
            "64:ff9b::1",
            "fc00::1",
            "fd00::1",
            "fe80::1",
            "ff02::1",
            "2001:db8::1",
        ] {
            let ip = blocked.parse::<IpAddr>().unwrap();
            assert_eq!(policy.validate_ip(ip), Err(SsrError::BlockedIp(ip)));
        }
    }

    #[test]
    fn allows_public_ipv6() {
        let policy = AddressPolicy;
        for public in ["2606:4700:4700::1111", "2001:4860:4860::8888"] {
            let ip = public.parse::<IpAddr>().unwrap();
            assert_eq!(policy.validate_ip(ip), Ok(()));
        }
    }

    #[test]
    fn url_validation_rejects_non_https() {
        assert!(matches!(
            SafeUrl::parse("http://example.com"),
            Err(SsrError::DisallowedScheme(_))
        ));
        assert!(matches!(
            SafeUrl::parse("ftp://example.com"),
            Err(SsrError::DisallowedScheme(_))
        ));
    }

    #[test]
    fn url_validation_rejects_credentials() {
        assert_eq!(
            SafeUrl::parse("https://user:pass@example.com"),
            Err(SsrError::CredentialsNotAllowed)
        );
    }

    #[test]
    fn url_validation_rejects_custom_ports() {
        assert_eq!(
            SafeUrl::parse("https://example.com:8080/"),
            Err(SsrError::CustomPort(8080))
        );
        assert_eq!(
            SafeUrl::parse("https://example.com:80/"),
            Err(SsrError::CustomPort(80))
        );
    }

    #[test]
    fn url_validation_accepts_default_port_443_or_no_port() {
        let url1 = SafeUrl::parse("https://example.com/search?q=test").unwrap();
        assert_eq!(url1.host, "example.com");
        assert_eq!(url1.path_and_query, "/search?q=test");

        let url2 = SafeUrl::parse("https://example.com:443/").unwrap();
        assert_eq!(url2.host, "example.com");
    }

    #[test]
    fn url_validation_rejects_private_ip_literal() {
        assert_eq!(
            SafeUrl::parse("https://127.0.0.1/"),
            Err(SsrError::BlockedIp("127.0.0.1".parse().unwrap()))
        );
        assert_eq!(
            SafeUrl::parse("https://[::1]/"),
            Err(SsrError::BlockedIp("::1".parse().unwrap()))
        );
    }

    #[test]
    fn redirect_tracker_enforces_max_3_hops() {
        let mut tracker = RedirectTracker::new();
        assert!(tracker.step("https://a.com/").is_ok());
        assert!(tracker.step("https://b.com/").is_ok());
        assert!(tracker.step("https://c.com/").is_ok());
        assert_eq!(
            tracker.step("https://d.com/"),
            Err(SsrError::TooManyRedirects)
        );
    }
    /// Regression (audit A-04): the URL must be judged by the same parser that sends it.
    /// The old hand-rolled parser read each of these as a harmless host name, while the
    /// WHATWG parser inside the HTTP client connects to a private address.
    #[test]
    fn parser_differentials_cannot_reach_private_addresses() {
        for raw in [
            "https://127.0.0.1\\x.example.com/",
            "https://2130706433/",
            "https://0x7f.0.0.1/",
            "https://0177.0.0.1/",
            "https://127.1/",
            "https://[::ffff:7f00:1]/",
            "https://[::127.0.0.1]/",
            "https://127.0.0.1%2f.example.com/",
        ] {
            let res = SafeUrl::parse(raw);
            assert!(res.is_err(), "{raw:?} must be refused, got {res:?}");
        }
    }

    #[test]
    fn backslash_ends_the_host_like_the_client_parser() {
        let u = SafeUrl::parse("https://example.com\\@127.0.0.1/").unwrap();
        assert_eq!(u.host, "example.com");
        assert_eq!(u.full_url, "https://example.com/@127.0.0.1/");
    }

    #[test]
    fn host_ends_at_query_or_fragment() {
        let u = SafeUrl::parse("https://Example.COM?q=1#frag").unwrap();
        assert_eq!(u.host, "example.com");
        assert_eq!(u.path_and_query, "/?q=1");
        assert_eq!(u.full_url, "https://example.com/?q=1");
    }

    #[test]
    fn ipv6_literal_keeps_brackets_in_full_url() {
        let u = SafeUrl::parse("https://[2606:4700:4700::1111]/dns").unwrap();
        assert_eq!(u.full_url, "https://[2606:4700:4700::1111]/dns");
        assert_eq!(
            u.ip_literal(),
            Some("2606:4700:4700::1111".parse().unwrap())
        );
    }

    #[test]
    fn ipv4_compatible_ipv6_is_blocked() {
        for blocked in ["::7f00:1", "::a00:1", "::808:808"] {
            let ip = blocked.parse::<IpAddr>().unwrap();
            assert!(AddressPolicy.validate_ip(ip).is_err(), "{blocked}");
        }
    }

    #[test]
    fn redirect_targets_resolve_against_the_current_url() {
        let base = SafeUrl::parse("https://example.com/a/b?x=1").unwrap();
        assert_eq!(base.join("/c").unwrap().full_url, "https://example.com/c");
        assert_eq!(base.join("d").unwrap().full_url, "https://example.com/a/d");
        assert!(base.join("http://example.com/").is_err());
        assert!(base.join("https://10.0.0.1/").is_err());
        assert!(base.join("//127.0.0.1/").is_err());
    }
}
