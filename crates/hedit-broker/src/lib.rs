//! Host-owned extension capabilities. No broker authority is deserialized from Lua.
mod handle;
pub mod workspace;

/// Parked watches and previews must leave four contexts for foreground commands.
pub const MAX_PROVIDER_CONTEXTS: usize = 12;
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
pub mod lsp;
pub mod recovery_store;
pub mod state;
pub mod tool;
