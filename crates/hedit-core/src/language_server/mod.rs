//! Bounded, native language-server management and workspace sessions.
mod client;
pub mod manager;
mod service;
pub mod text;
pub mod wire;
pub mod workspace_edit;
pub use service::{Document, Event, Feature, Operation, Reply, Service, Status};
pub const MAX_DOCUMENT: usize = 4 * 1024 * 1024;
pub const MAX_MESSAGE: usize = 8 * 1024 * 1024;
pub(crate) fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
#[cfg(test)]
mod tests;
