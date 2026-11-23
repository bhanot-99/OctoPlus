//! Pure formatting helpers for OctoPit dashboard.
//!
//! All functions are pure, allocation-conscious, and safe against overflow.
//! ASCII-safe output except the ellipsis-free dash "—" which is allowed.

/// Formats a byte count into IEC binary prefix notation (e.g. `0 B`, `1.5 KiB`, `3.0 GiB`).
///
/// Produces exact integer count for bytes (`B`), and one decimal place for units
/// above bytes (`KiB` through `EiB`).
#[must_use]
pub fn bytes_iec(b: u64) -> String {
    const UNITS: [&str; 6] = ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    if b < 1024 {
        return format!("{b} B");
    }
    #[allow(clippy::cast_precision_loss)]
    let mut val = (b as f64) / 1024.0;
    let mut idx = 0;
    while val >= 1024.0 && idx < UNITS.len() - 1 {
        val /= 1024.0;
        idx += 1;
    }
    format!("{val:.1} {}", UNITS[idx])
}

/// Formats duration in seconds into human-readable uptime (e.g. `3d 04h`, `12m 30s`).
///
/// Output matches `neuroosctl status` format.
#[must_use]
pub fn uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;

    if days > 0 {
        format!("{days}d {hours:02}h")
    } else if hours > 0 {
        format!("{hours}h {mins:02}m")
    } else if mins > 0 {
        format!("{mins}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

/// Formats latency in nanoseconds for human display.
///
/// - `0` returns `"—"`
/// - `< 1_000_000` returns integer microseconds (e.g. `"812 µs"`)
/// - `< 1_000_000_000` returns milliseconds with 1 decimal place (e.g. `"4.0 ms"`)
/// - `>= 1_000_000_000` returns seconds with 2 decimal places (e.g. `"4.00 s"`)
#[must_use]
pub fn latency(ns: u64) -> String {
    if ns == 0 {
        return "—".to_string();
    }
    #[allow(clippy::cast_precision_loss)]
    if ns < 1_000_000 {
        let us = (ns as f64) / 1_000.0;
        format!("{us:.0} µs")
    } else if ns < 1_000_000_000 {
        let ms = (ns as f64) / 1_000_000.0;
        format!("{ms:.1} ms")
    } else {
        let s = (ns as f64) / 1_000_000_000.0;
        format!("{s:.2} s")
    }
}

/// Formats a fraction into a percentage string (e.g. `0.415` -> `"42%"`).
///
/// Rounds half away from zero. Returns `"0%"` for `NaN` or negative values.
/// Values greater than 1.0 are allowed (e.g. `1.20` -> `"120%"`).
#[must_use]
pub fn percent(fraction: f64) -> String {
    if fraction.is_nan() || fraction <= 0.0 || fraction.is_infinite() {
        return "0%".to_string();
    }
    let pct = (fraction * 100.0).round();
    #[allow(clippy::cast_precision_loss)]
    if pct >= (u64::MAX as f64) {
        format!("{}%", u64::MAX)
    } else {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let val = pct as u64;
        format!("{val}%")
    }
}

/// Formats elapsed time since a given timestamp in nanoseconds into a friendly relative label.
///
/// - `< 5 s` or `then_ns > now_ns` -> `"just now"`
/// - `< 60 s` -> `"{diff_secs} s ago"`
/// - `< 3600 s` -> `"{diff_mins} min ago"`
/// - `< 86400 s` -> `"{diff_hours} h ago"`
/// - `>= 86400 s` -> `"{diff_days} d ago"`
#[must_use]
pub fn age_label(now_ns: u64, then_ns: u64) -> String {
    if then_ns > now_ns {
        return "just now".to_string();
    }
    let diff_ns = now_ns - then_ns;
    let diff_secs = diff_ns / 1_000_000_000;
    if diff_secs < 5 {
        "just now".to_string()
    } else if diff_secs < 60 {
        format!("{diff_secs} s ago")
    } else if diff_secs < 3600 {
        let mins = diff_secs / 60;
        format!("{mins} min ago")
    } else if diff_secs < 86400 {
        let hours = diff_secs / 3600;
        format!("{hours} h ago")
    } else {
        let days = diff_secs / 86400;
        format!("{days} d ago")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_bytes_iec() {
        assert_eq!(bytes_iec(0), "0 B");
        assert_eq!(bytes_iec(1), "1 B");
        assert_eq!(bytes_iec(1023), "1023 B");
        assert_eq!(bytes_iec(1024), "1.0 KiB");
        assert_eq!(bytes_iec(1536), "1.5 KiB");
        assert_eq!(bytes_iec(3_221_225_472), "3.0 GiB");
        assert_eq!(bytes_iec(u64::MAX), "16.0 EiB");
    }

    #[test]
    fn test_uptime() {
        assert_eq!(uptime(0), "0s");
        assert_eq!(uptime(45), "45s");
        assert_eq!(uptime(60), "1m 00s");
        assert_eq!(uptime(75), "1m 15s");
        assert_eq!(uptime(3600), "1h 00m");
        assert_eq!(uptime(3665), "1h 01m");
        assert_eq!(uptime(86400), "1d 00h");
        assert_eq!(uptime(90000), "1d 01h");
        assert!(!uptime(u64::MAX).is_empty());
    }

    #[test]
    fn test_latency() {
        assert_eq!(latency(0), "—");
        assert_eq!(latency(1_000), "1 µs");
        assert_eq!(latency(812_000), "812 µs");
        assert_eq!(latency(1_000_000), "1.0 ms");
        assert_eq!(latency(4_000_000), "4.0 ms");
        assert_eq!(latency(1_000_000_000), "1.00 s");
        assert_eq!(latency(4_000_000_000), "4.00 s");
        assert!(!latency(u64::MAX).is_empty());
    }

    #[test]
    fn test_percent() {
        assert_eq!(percent(0.415), "42%");
        assert_eq!(percent(0.0), "0%");
        assert_eq!(percent(-0.5), "0%");
        assert_eq!(percent(f64::NAN), "0%");
        assert_eq!(percent(f64::INFINITY), "0%");
        assert_eq!(percent(f64::NEG_INFINITY), "0%");
        assert_eq!(percent(1.0), "100%");
        assert_eq!(percent(1.20), "120%");
    }

    #[test]
    fn test_age_label() {
        assert_eq!(age_label(100, 200), "just now");
        assert_eq!(age_label(10_000_000_000, 10_000_000_000), "just now");
        assert_eq!(age_label(14_000_000_000, 10_000_000_000), "just now");
        assert_eq!(age_label(22_000_000_000, 10_000_000_000), "12 s ago");
        assert_eq!(age_label(190_000_000_000, 10_000_000_000), "3 min ago");
        assert_eq!(age_label(7_210_000_000_000, 10_000_000_000), "2 h ago");
        assert_eq!(age_label(86_410_000_000_000, 10_000_000_000), "1 d ago");
        assert_eq!(age_label(500_000_000_000_000, 10_000_000_000), "5 d ago");
        assert_eq!(age_label(0, u64::MAX), "just now");
        assert!(!age_label(u64::MAX, 0).is_empty());
    }

    proptest! {
        #[test]
        fn proptest_bytes_iec_never_panics(b: u64) {
            let res = bytes_iec(b);
            prop_assert!(!res.is_empty());
        }

        #[test]
        fn proptest_age_label_never_panics(now: u64, then: u64) {
            let res = age_label(now, then);
            prop_assert!(!res.is_empty());
        }
    }
}
