//! The sensors. Each is a [`crate::sensor::Sensor`]; none knows about privacy or the bus.

pub mod battery;
pub mod cosmic;
pub mod disk;
pub mod idle;
pub mod proc;
pub mod resources;
pub mod thermal;
pub mod wl;
