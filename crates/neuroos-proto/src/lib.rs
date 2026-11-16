//! Generated types for the `neuroos.v1` contracts (`proto/neuroos/v1`).
//!
//! Parts talk only through these types (R0-11). Every frame is one
//! [`v1::Envelope`]; see Architecture §5.3.

pub mod dump;

/// `neuroos.v1` — the only contract version.
#[allow(
    missing_docs,
    missing_debug_implementations,
    clippy::all,
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]
pub mod v1 {
    include!(concat!(env!("OUT_DIR"), "/neuroos.v1.rs"));
}

/// Wire schema version carried in [`v1::Envelope::schema_version`].
pub const SCHEMA_VERSION: u32 = 1;

pub use prost::{DecodeError, Message};
