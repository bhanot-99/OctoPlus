#![no_main]
//! Any text given to `SafeUrl::parse`: never panics; whatever it accepts meets `common::check`,
//! and the client parsing the raw text directly names the same host.

use libfuzzer_sys::fuzz_target;
use octoplus_fetcher::ssrf::SafeUrl;

mod common;

fuzz_target!(|data: &[u8]| {
    let Ok(raw) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(safe) = SafeUrl::parse(raw) else {
        return;
    };
    common::check(&safe);
    let direct = url::Url::parse(raw).expect("the raw text parses when the gate accepted it");
    let canonical = url::Url::parse(&safe.full_url).expect("canonical parses");
    assert_eq!(direct.host_str(), canonical.host_str());
});
