//! MacDetour core: everything that can be decided without touching the OS.
//!
//! - [`config`]: the daemon's TOML configuration and its validation.
//! - [`net`]: a small CIDR type and the address classes that must never be detoured.
//! - [`blocklist`]: parsing the public list of addresses blocked in Spain.
//! - [`plan`]: which destinations to send through the tunnel.
//! - [`protocol`]: the newline-delimited JSON spoken over the daemon socket.

pub mod blocklist;
pub mod config;
pub mod net;
pub mod plan;
pub mod protocol;

pub use config::Config;
pub use net::Cidr;
