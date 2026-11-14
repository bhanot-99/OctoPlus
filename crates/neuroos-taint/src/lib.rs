//! Taint model (Architecture §7.4, rule R0-4, NFR-SEC-07).
//!
//! * `taint(output) = union(taint(inputs))` — taint is never cleared.
//! * A **missing** taint value is an error, never "clean".
//! * Unknown bits on the wire are an error, so a newer sender cannot have its
//!   flags silently dropped by an older receiver.
//!
//! Security-critical (NFR-QA-01): every decision outcome here is covered by a
//! table-driven or property test.

use bitflags::bitflags;

bitflags! {
    /// Where a piece of data came from.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct TaintFlags: u32 {
        /// Came through C7 from the internet.
        const EXTERNAL_UNTRUSTED = 1 << 0;
        /// Spoken or typed by the user.
        const USER_PROVIDED      = 1 << 1;
        /// Produced by C4.
        const MODEL_GENERATED    = 1 << 2;
        /// Embedded with an old model.
        const STALE_INDEX        = 1 << 3;
    }
}

/// Errors decoding taint from the wire or storage.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TaintError {
    /// No taint value was present. Never treated as clean (R0-4).
    #[error("taint value is missing")]
    Missing,
    /// Bits this build does not know.
    #[error("taint value has unknown bits {0:#x}")]
    UnknownBits(u32),
}

impl TaintFlags {
    /// Decodes a taint value that may be absent.
    ///
    /// # Errors
    /// [`TaintError::Missing`] for `None`; [`TaintError::UnknownBits`] for
    /// bits outside the known set.
    pub fn decode(raw: Option<u32>) -> Result<Self, TaintError> {
        let bits = raw.ok_or(TaintError::Missing)?;
        Self::from_bits(bits).ok_or(TaintError::UnknownBits(bits & !Self::all().bits()))
    }

    /// Union of `self` and `other` — the only way to combine taint.
    #[must_use]
    pub const fn merge(self, other: Self) -> Self {
        self.union(other)
    }

    /// Union of every input (empty input gives no flags).
    #[must_use]
    pub fn merge_all<I: IntoIterator<Item = Self>>(inputs: I) -> Self {
        inputs.into_iter().fold(Self::empty(), Self::merge)
    }

    /// True when the data came from the internet; the only flag that changes
    /// tier decisions in v1.0.
    #[must_use]
    pub const fn is_external(self) -> bool {
        self.contains(Self::EXTERNAL_UNTRUSTED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn flags() -> impl Strategy<Value = TaintFlags> {
        (0u32..16).prop_map(TaintFlags::from_bits_truncate)
    }

    #[test]
    fn decode_table() {
        let cases: [(Option<u32>, Result<TaintFlags, TaintError>); 6] = [
            (None, Err(TaintError::Missing)),
            (Some(0), Ok(TaintFlags::empty())),
            (Some(1), Ok(TaintFlags::EXTERNAL_UNTRUSTED)),
            (Some(0b1111), Ok(TaintFlags::all())),
            (Some(0b1_0000), Err(TaintError::UnknownBits(0b1_0000))),
            (Some(u32::MAX), Err(TaintError::UnknownBits(!0b1111))),
        ];
        for (raw, want) in cases {
            assert_eq!(TaintFlags::decode(raw), want, "{raw:?}");
        }
    }

    #[test]
    fn external_flag_table() {
        assert!(!TaintFlags::empty().is_external());
        assert!(!TaintFlags::USER_PROVIDED.is_external());
        assert!(TaintFlags::EXTERNAL_UNTRUSTED.is_external());
        assert!((TaintFlags::EXTERNAL_UNTRUSTED | TaintFlags::MODEL_GENERATED).is_external());
    }

    #[test]
    fn merge_all_of_nothing_is_empty() {
        assert_eq!(TaintFlags::merge_all([]), TaintFlags::empty());
    }

    proptest! {
        #[test]
        fn union_is_associative(a in flags(), b in flags(), c in flags()) {
            prop_assert_eq!(a.merge(b).merge(c), a.merge(b.merge(c)));
        }

        #[test]
        fn union_is_commutative(a in flags(), b in flags()) {
            prop_assert_eq!(a.merge(b), b.merge(a));
        }

        #[test]
        fn union_is_idempotent(a in flags()) {
            prop_assert_eq!(a.merge(a), a);
        }

        #[test]
        fn taint_only_grows(a in flags(), b in flags()) {
            let m = a.merge(b);
            prop_assert!(m.contains(a) && m.contains(b));
        }

        #[test]
        fn merge_all_matches_pairwise(v in proptest::collection::vec(flags(), 0..8)) {
            let mut acc = TaintFlags::empty();
            for f in &v { acc = acc.merge(*f); }
            prop_assert_eq!(TaintFlags::merge_all(v), acc);
        }

        #[test]
        fn decode_round_trips_known_bits(a in flags()) {
            prop_assert_eq!(TaintFlags::decode(Some(a.bits())), Ok(a));
        }
    }
}
