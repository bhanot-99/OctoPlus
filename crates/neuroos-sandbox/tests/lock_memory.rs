#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `lock_memory` (security.md §4.7): a small boxed secret can be locked, and an
//! empty slice is a no-op. The SAFETY argument is checked by reading the
//! bytes back unchanged.

#[test]
fn locks_a_boxed_secret_without_changing_it() {
    let mut secret = Box::new([0x5a_u8; 32]);
    neuroos_sandbox::lock_memory(&mut secret[..]).unwrap();
    assert!(secret.iter().all(|b| *b == 0x5a));
}

#[test]
fn empty_is_a_no_op() {
    neuroos_sandbox::lock_memory(&mut []).unwrap();
}
