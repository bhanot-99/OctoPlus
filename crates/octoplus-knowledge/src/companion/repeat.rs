//! Repeat detection using a ring buffer of 64-bit FNV-1a sentence hashes.

use std::collections::VecDeque;

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// Computes the 64-bit FNV-1a hash of a byte slice.
#[inline]
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Extracts normalized lowercase alphanumeric words from text.
fn normalized_words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter_map(|w| {
            let clean: String = w
                .chars()
                .filter(|c| c.is_alphanumeric())
                .flat_map(|c| c.to_lowercase())
                .collect();
            if clean.is_empty() { None } else { Some(clean) }
        })
        .collect()
}

/// Ring buffer guard tracking recent companion responses to prevent repeating lines.
#[derive(Debug, Clone)]
pub struct RepeatGuard {
    capacity: usize,
    buffer: VecDeque<u64>,
}

impl RepeatGuard {
    /// Creates a new repeat guard holding up to `capacity` hashed sentences.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            buffer: VecDeque::with_capacity(capacity),
        }
    }

    /// Computes the hash for the sentence if it contains at least 3 words.
    fn hash_sentence(text: &str) -> Option<u64> {
        let words = normalized_words(text);
        if words.len() < 3 {
            return None;
        }
        let normalized = words.join(" ");
        Some(fnv1a_64(normalized.as_bytes()))
    }

    /// Returns true if the sentence has been seen recently in the buffer.
    ///
    /// Sentences with fewer than 3 words always return false.
    pub fn seen(&self, text: &str) -> bool {
        match Self::hash_sentence(text) {
            Some(hash) => self.buffer.contains(&hash),
            None => false,
        }
    }

    /// Records the sentence into the buffer if it contains at least 3 words.
    ///
    /// If capacity is reached, the oldest sentence is dropped.
    pub fn remember(&mut self, text: &str) {
        if self.capacity == 0 {
            return;
        }
        if let Some(hash) = Self::hash_sentence(text) {
            if self.buffer.len() >= self.capacity {
                self.buffer.pop_front();
            }
            self.buffer.push_back(hash);
        }
    }

    /// Checks if the sentence was already seen, and then remembers it.
    ///
    /// Sentences with fewer than 3 words always return false and are not remembered.
    pub fn is_repeat_and_remember(&mut self, text: &str) -> bool {
        let is_repeat = self.seen(text);
        self.remember(text);
        is_repeat
    }
}

/// Computes normalized unique-word overlap ratio `|A ∩ B| / max(|A|, |B|)` in `0.0..=1.0`.
#[must_use]
pub fn word_overlap(a: &str, b: &str) -> f64 {
    use std::collections::HashSet;
    let wa: HashSet<String> = normalized_words(a).into_iter().collect();
    let wb: HashSet<String> = normalized_words(b).into_iter().collect();
    if wa.is_empty() || wb.is_empty() {
        return 0.0;
    }
    let shared = wa.intersection(&wb).count();
    let denom = wa.len().max(wb.len());
    shared as f64 / denom as f64
}

/// Returns true if `candidate` (at least 5 words) shares >= `threshold` of its words with any
/// earlier reply in `prior_replies`.
#[must_use]
pub fn is_near_repeat(candidate: &str, prior_replies: &[String], threshold: f64) -> bool {
    if normalized_words(candidate).len() < 5 {
        return false;
    }
    prior_replies
        .iter()
        .any(|prev| word_overlap(candidate, prev) >= threshold)
}
