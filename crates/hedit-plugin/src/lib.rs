//! Shared Lua engine, bounded invocation protocol, and native process adapters.
mod error;
pub mod options;
mod runtime;
mod sandbox;

pub use error::Error;
pub use runtime::{
    run_fixture, run_if_requested, run_save, start, Input, RunningPlugin, HELPER_ARG,
    MAX_SELECTIONS, MAX_SELECTION_BYTES, SUPPORTED,
};
