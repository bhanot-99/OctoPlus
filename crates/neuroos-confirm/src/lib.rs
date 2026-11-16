//! `neuroos-confirm` — the confirmation dialog C6 starts as its own child
//! (FR-KER-04, FR-KER-09, design.md §6, Architecture §8.4).
//!
//! The library part (always built) is the stdin/stdout contract, the fixed
//! wording and the formatting helpers, so C6 can use the same types without
//! linking GTK. The window itself is the `gui` feature, used only by the binary.

pub mod format;
pub mod protocol;
pub mod wording;

#[cfg(feature = "gui")]
pub mod ui;
