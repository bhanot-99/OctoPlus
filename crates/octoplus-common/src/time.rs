//! Boundary time: every timestamp that crosses a part boundary is a `u64`
//! count of nanoseconds since the UTC Unix epoch (rules.md AB-8).
//!
//! Durations and latencies use `std::time::Instant`, never wall-clock
//! arithmetic (rules.md §5).

use jiff::Timestamp;

/// Errors converting between wall-clock time and boundary nanoseconds.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TimeError {
    /// The instant is before 1970-01-01T00:00:00Z and has no `u64` form.
    #[error("timestamp is before the Unix epoch")]
    BeforeEpoch,
    /// The instant is past the year 2554, the largest `u64` nanosecond value.
    #[error("timestamp does not fit in u64 nanoseconds")]
    Overflow,
    /// The nanosecond value is outside the range a timestamp can hold.
    #[error("nanosecond value is out of the representable timestamp range")]
    OutOfRange,
}

/// Current UTC time as epoch nanoseconds (AB-8).
///
/// # Errors
/// Fails only if the system clock is set before 1970 or after 2554.
pub fn now_ns() -> Result<u64, TimeError> {
    to_ns(Timestamp::now())
}

/// Converts a timestamp to boundary nanoseconds.
///
/// # Errors
/// [`TimeError::BeforeEpoch`] for instants before 1970, [`TimeError::Overflow`]
/// for instants that do not fit in `u64`.
pub fn to_ns(ts: Timestamp) -> Result<u64, TimeError> {
    let ns = ts.as_nanosecond();
    if ns < 0 {
        return Err(TimeError::BeforeEpoch);
    }
    u64::try_from(ns).map_err(|_| TimeError::Overflow)
}

/// Converts boundary nanoseconds back to a timestamp.
///
/// # Errors
/// [`TimeError::OutOfRange`] if the value is past the largest timestamp jiff
/// can represent.
pub fn from_ns(ns: u64) -> Result<Timestamp, TimeError> {
    Timestamp::from_nanosecond(i128::from(ns)).map_err(|_| TimeError::OutOfRange)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn epoch_is_zero() {
        assert_eq!(to_ns(Timestamp::UNIX_EPOCH), Ok(0));
        assert_eq!(from_ns(0), Ok(Timestamp::UNIX_EPOCH));
    }

    #[test]
    fn known_instant_round_trips() {
        let ts: Timestamp = "2026-11-14T12:34:56.123456789Z".parse().unwrap();
        let ns = to_ns(ts).unwrap();
        assert_eq!(ns, 1_794_659_696_123_456_789);
        assert_eq!(from_ns(ns), Ok(ts));
    }

    #[test]
    fn before_epoch_is_rejected() {
        let ts: Timestamp = "1969-12-31T23:59:59Z".parse().unwrap();
        assert_eq!(to_ns(ts), Err(TimeError::BeforeEpoch));
    }

    #[test]
    fn u64_max_round_trips() {
        // jiff's maximum (year 9999) is above u64::MAX ns (year 2554),
        // so every u64 value must convert back.
        let ts = from_ns(u64::MAX).unwrap();
        assert_eq!(to_ns(ts), Ok(u64::MAX));
    }

    #[test]
    fn now_is_after_2026() {
        let ns = now_ns().unwrap();
        assert!(ns > 1_767_225_600_000_000_000, "clock reads before 2026");
    }

    proptest! {
        #[test]
        fn every_u64_round_trips(ns in any::<u64>()) {
            prop_assert_eq!(to_ns(from_ns(ns).unwrap()), Ok(ns));
        }
    }
}
