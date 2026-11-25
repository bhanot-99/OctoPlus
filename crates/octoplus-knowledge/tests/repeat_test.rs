#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use octoplus_knowledge::companion::repeat::{RepeatGuard, fnv1a_64};

#[test]
fn test_fnv1a_64_deterministic() {
    let h1 = fnv1a_64(b"hello world");
    let h2 = fnv1a_64(b"hello world");
    let h3 = fnv1a_64(b"hello world!");
    assert_eq!(h1, h2);
    assert_ne!(h1, h3);
}

#[test]
fn test_repeat_detected_after_punctuation_and_case() {
    let mut guard = RepeatGuard::new(10);

    let s1 = "How are you doing today?";
    let s2 = "how are you doing today!";
    let s3 = "HOW ARE YOU DOING TODAY.";

    assert!(!guard.seen(s1));
    guard.remember(s1);

    assert!(guard.seen(s2));
    assert!(guard.seen(s3));
}

#[test]
fn test_repeat_ring_buffer_capacity_eviction() {
    let mut guard = RepeatGuard::new(3);

    let line1 = "first sentence spoken here";
    let line2 = "second sentence spoken here";
    let line3 = "third sentence spoken here";
    let line4 = "fourth sentence spoken here";

    guard.remember(line1);
    guard.remember(line2);
    guard.remember(line3);

    assert!(guard.seen(line1));
    assert!(guard.seen(line2));
    assert!(guard.seen(line3));

    // Adding 4th drops line1 from capacity 3 buffer
    guard.remember(line4);

    assert!(
        !guard.seen(line1),
        "Oldest sentence line1 should be evicted"
    );
    assert!(guard.seen(line2));
    assert!(guard.seen(line3));
    assert!(guard.seen(line4));
}

#[test]
fn test_short_sentences_ignored() {
    let mut guard = RepeatGuard::new(10);

    let short1 = "Yes.";
    let short2 = "No way.";
    let short3 = "Okay bye.";

    // All have fewer than 3 words
    assert!(!guard.seen(short1));
    guard.remember(short1);
    assert!(!guard.seen(short1));

    assert!(!guard.is_repeat_and_remember(short2));
    assert!(!guard.seen(short2));

    assert!(!guard.is_repeat_and_remember(short3));
    assert!(!guard.seen(short3));
}

#[test]
fn test_is_repeat_and_remember() {
    let mut guard = RepeatGuard::new(5);

    let line = "We should grab lunch together today";

    // First time: not a repeat, then remembered
    assert!(!guard.is_repeat_and_remember(line));

    // Second time: repeat!
    assert!(guard.is_repeat_and_remember(line));
}
