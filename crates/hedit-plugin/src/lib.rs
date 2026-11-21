//! Shared Lua engine, bounded invocation protocol, and native process adapters.
mod error;
pub mod options;
mod runtime;
mod sandbox;
pub mod sdk;
pub mod service;
pub mod telemetry;

pub use error::Error;
pub use runtime::{
    run_fixture, run_if_requested, run_save, run_sdk_fixture, run_sdk_fixture_traced, run_sdk_save,
    run_service_fixture, service_admission, service_available, start, start_sdk, start_service,
    start_service_checked, Input, RunningPlugin, RunningSdk, RunningService, ServiceAdmission,
    HELPER_ARG, MAX_SELECTIONS, MAX_SELECTION_BYTES, MAX_SOURCE_BYTES, SUPPORTED,
};

/// Codec-only entry point for the separate coverage-guided fuzz harness.
/// Does not launch a worker, evaluate Lua, read files, or commit effects.
#[cfg(feature = "fuzzing")]
#[doc(hidden)]
pub fn fuzz_protocol(bytes: &[u8]) {
    runtime::fuzz_protocol(bytes);
}
