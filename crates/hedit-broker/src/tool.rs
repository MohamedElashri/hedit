//! Host-owned native images. No executable path is accepted from worker intents.
pub mod format;
mod source;
pub use source::{Copied, Source};
mod store;
mod trust;
pub use store::{Binding, Image, Record, Store};
