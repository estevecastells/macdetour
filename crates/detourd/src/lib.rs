//! MacDetour daemon library. `main.rs` wires these together; tests drive the
//! engine with a fake platform and the socket with a fake engine.

pub mod engine;
pub mod fetch;
pub mod logger;
pub mod platform;
pub mod probe;
pub mod server;
pub mod system;
pub mod tunnel;
