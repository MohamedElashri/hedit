//! Experimental remote document recovery. No GUI, key storage, or local cache.
#[cfg(all(feature = "test-faults", not(debug_assertions)))]
compile_error!("test-faults must never be enabled in a release helper");
pub mod client;
#[cfg(unix)]
pub mod files;
#[cfg(unix)]
pub mod host;
#[cfg(unix)]
pub mod journal;
pub mod protocol;
#[cfg(unix)]
mod runtime;
#[cfg(unix)]
pub mod server;
