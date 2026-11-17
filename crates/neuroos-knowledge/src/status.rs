//! Fixed status sentences for a part that is down (FR-KNO-10, rules §6.6, ADR-0013 decision 9).
//!
//! They are our own text, never built from user or model content, so they carry no taint and
//! need no neutralising. Each is short enough to be spoken in one breath.

/// A part C5 depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// C4, the language model.
    Inference,
    /// C3, storage and memory.
    Storage,
    /// C6, the safety kernel.
    Kernel,
}

/// What kind of turn was being answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// A question about the user's own activity: it cannot be answered without C3.
    Evidence,
    /// Anything else: it can go on without C3 (and without saved facts).
    NoEvidence,
}

/// The status sentence to give instead of an answer, if one is needed. `None` means the turn
/// can continue (a missing C3 on a general question only removes facts and evidence, and the
/// answer is flagged by the caller).
#[must_use]
pub fn status_line(down: Part, need: Need) -> Option<&'static str> {
    match (down, need) {
        (Part::Inference, _) => Some(
            "My thinking part isn't running right now, so I can't answer. Please try again in a moment.",
        ),
        (Part::Kernel, _) => Some(
            "I can't check what's safe right now, so I'm holding off. Please try again in a moment.",
        ),
        (Part::Storage, Need::Evidence) => Some(
            "My memory isn't available right now, so I can't look that up. Please try again in a moment.",
        ),
        (Part::Storage, Need::NoEvidence) => None,
    }
}

/// The fixed answer to "yes, look it up" until the fetcher arrives in Phase 8 (FR-CHT-04).
pub const NO_LOOKUP_YET: &str = "I can't look things up yet.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        for need in [Need::Evidence, Need::NoEvidence] {
            assert!(status_line(Part::Inference, need).is_some());
            assert!(status_line(Part::Kernel, need).is_some());
        }
        assert!(status_line(Part::Storage, Need::Evidence).is_some());
        assert_eq!(status_line(Part::Storage, Need::NoEvidence), None);
    }

    #[test]
    fn lines_are_plain_speech() {
        for l in [
            status_line(Part::Inference, Need::Evidence),
            status_line(Part::Kernel, Need::Evidence),
            status_line(Part::Storage, Need::Evidence),
            Some(NO_LOOKUP_YET),
        ]
        .into_iter()
        .flatten()
        {
            assert!(l.len() < 120, "{l}");
            assert!(!l.contains(['<', '>', '*', '#', '`', '\n']), "{l}");
        }
    }
}
