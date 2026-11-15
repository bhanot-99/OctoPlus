//! Shared building blocks for every Semantic-NeuroOS part: configuration,
//! filesystem paths, boundary time and logging setup.
//!
//! Nothing in this crate opens a network socket (R0-1).

pub mod config;
pub mod logging;
pub mod paths;
pub mod socketmap;
pub mod time;

pub use config::{Config, ConfigError};
pub use paths::{Part, PathError};
pub use socketmap::{Client, SOCKET_MAP, SocketEntry};
pub use time::TimeError;
