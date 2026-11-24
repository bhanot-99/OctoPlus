//! The nudge engine (Phase 6, FR-NUD-01…12, Architecture §10): rules, a pure evaluator, a pure
//! policy gate, and the task that connects them to C3 and C6.
//!
//! This tree must never import the action route, `DispatchRequest` or evidence types: a nudge
//! never acts (R0-6, FR-NUD-10). `tests/nudge_isolation_test.rs` checks the sources.

pub mod boot;
pub mod core;
pub mod engine;
pub mod eval;
pub mod policy;
pub mod replay;
pub mod reply;
pub mod rules;
pub mod signals;
pub mod types;
