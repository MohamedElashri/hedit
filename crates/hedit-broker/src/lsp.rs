//! Shared scoped LSP transport/protocol. Document authority is host-owned.
mod connection;
pub use connection::{Connection, Method, Notification, Transport};
