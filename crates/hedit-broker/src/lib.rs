//! Host-owned extension capabilities. No broker authority is deserialized from Lua.
mod handle;
pub mod workspace;
pub use handle::Handle;
pub mod cancel;
pub mod journal;
pub mod network;
pub mod plan;
pub mod resources;
pub mod search;
pub mod watch;

pub mod session;

pub mod declaration;
pub mod host;
pub mod recovery_store;
