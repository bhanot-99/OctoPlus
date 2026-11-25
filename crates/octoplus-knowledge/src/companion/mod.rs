//! Companion persona, mode machine, and RNG core (Phase 12).
//!
//! Provides pure components for Laila:
//! - [`persona`]: configuration file structures, validation, and defaults.
//! - [`rng`]: deterministic splitmix64 PRNG for controlled switching and chance rules.
//! - [`mode`]: two-mode switching state machine (logic and heart).

pub mod mode;
pub mod persona;
pub mod rng;
