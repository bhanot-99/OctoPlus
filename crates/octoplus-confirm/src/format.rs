//! Pure formatting helpers for the dialog (design.md §4 rules 2 and 4).

/// Human size with a space and a binary unit: `512 B`, `12 KiB`, `610 MiB`, `4.2 GiB`.
#[must_use]
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut unit = 0;
    #[allow(clippy::cast_precision_loss)] // display only
    let mut value = bytes as f64;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    match unit {
        0 => format!("{bytes} B"),
        1 | 2 => format!("{value:.0} {}", UNITS[unit]),
        _ => format!("{value:.1} {}", UNITS[unit]),
    }
}

/// Countdown text `m:ss`, rounding up so it never shows 0:00 while time remains.
#[must_use]
pub fn countdown(remaining_ms: u64) -> String {
    let secs = remaining_ms.div_ceil(1000);
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Truncates in the middle with `…` so both the start and the end of a path stay visible.
#[must_use]
pub fn middle_truncate(s: &str, max_chars: usize) -> String {
    let n = s.chars().count();
    if n <= max_chars || max_chars < 3 {
        return s.to_owned();
    }
    let keep = max_chars - 1;
    let head = keep.div_ceil(2);
    let tail = keep - head;
    let start: String = s.chars().take(head).collect();
    let end: String = s.chars().skip(n - tail).collect();
    format!("{start}…{end}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(1023), "1023 B");
        assert_eq!(size(1024), "1 KiB");
        assert_eq!(size(610 << 20), "610 MiB");
        assert_eq!(size(4_509_715_660), "4.2 GiB");
        assert_eq!(size(u64::MAX), "16777216.0 TiB");
    }

    #[test]
    fn countdowns() {
        assert_eq!(countdown(120_000), "2:00");
        assert_eq!(countdown(114_001), "1:55");
        assert_eq!(countdown(1), "0:01");
        assert_eq!(countdown(0), "0:00");
    }

    #[test]
    fn truncation() {
        assert_eq!(middle_truncate("short", 10), "short");
        let t = middle_truncate("~/projects/very/long/path/target", 12);
        assert_eq!(t.chars().count(), 12);
        assert!(t.starts_with("~/pro") && t.ends_with("arget") && t.contains('…'));
        assert_eq!(middle_truncate("ééééééééé", 5).chars().count(), 5);
    }
}
