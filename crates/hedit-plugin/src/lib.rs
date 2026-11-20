//! Shared Lua engine, bounded invocation protocol, and native process adapters.
pub mod options;
mod plugin_runtime;
mod plugin_sandbox;

pub use plugin_runtime::{
    run_fixture, run_if_requested, run_save, start, Input, RunningPlugin, HELPER_ARG,
    MAX_SELECTIONS, MAX_SELECTION_BYTES, SUPPORTED,
};
