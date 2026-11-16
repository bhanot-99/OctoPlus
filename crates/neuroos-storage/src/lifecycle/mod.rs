//! Lifecycle jobs (FR-STO-06, -11, -12, -13; Architecture §7.5): garbage collection and the chunk
//! cap, health roll-up, backup, re-embedding, forget and purge. SQL here is pure functions over a
//! connection and a time argument, so every boundary is testable with a fake "now" (AB-13).

pub mod backup;
pub mod forget;
pub mod gc;
pub mod rollup;
