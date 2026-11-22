//! Shared scoped LSP transport/protocol. Document authority is host-owned.
mod connection;
pub use connection::{Connection, Method, Notification, Transport};
mod document;
pub use document::Document;
mod owner;
pub use owner::Owner;
