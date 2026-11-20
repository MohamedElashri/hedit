use hedit::plugin_options::Value;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

mod engine;
#[cfg(unix)]
mod supervisor;
#[cfg(windows)]
#[path = "plugin_runtime/windows.rs"]
mod supervisor;

#[cfg(not(any(unix, windows)))]
mod supervisor {
    use super::engine::Failure;
    use std::{
        fs::File,
        path::{Path, PathBuf},
        sync::atomic::AtomicBool,
        time::Duration,
    };

    pub(super) fn executable() -> Result<PathBuf, Failure> {
        Err(Failure::Sandbox)
    }

    pub(super) fn supervise(
        _: &Path,
        _: Vec<u8>,
        _: &AtomicBool,
        _: Duration,
        _: Option<File>,
    ) -> Result<Vec<u8>, Failure> {
        Err(Failure::Sandbox)
    }
}

pub(crate) const SUPPORTED: bool = crate::plugin_sandbox::SUPPORTED;
pub(crate) const MAX_SELECTION_BYTES: usize = 64 * 1024;
pub(crate) const MAX_SELECTIONS: usize = 16;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Input {
    pub source: String,
    pub selections: Vec<String>,
    pub language: String,
    pub settings: BTreeMap<String, Value>,
}

pub(crate) const HELPER_ARG: &str = "--hedit-plugin-worker";

pub(crate) struct RunningPlugin {
    receiver: mpsc::Receiver<Result<Vec<String>, String>>,
    cancelled: Arc<AtomicBool>,
}

impl RunningPlugin {
    pub(crate) fn try_result(&mut self) -> Option<Result<Vec<String>, String>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("The Lua worker stopped.".into())),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_test_result(result: Result<Vec<String>, String>) -> Self {
        let (sender, receiver) = mpsc::channel();
        sender.send(result).unwrap();
        Self {
            receiver,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Drop for RunningPlugin {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

pub(crate) fn start(input: Input, approval: File) -> Result<RunningPlugin, String> {
    let (sender, receiver) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    let thread_cancelled = cancelled.clone();
    std::thread::Builder::new()
        .name("hedit-lua-worker".into())
        .spawn(move || {
            let result = run(input, Some(approval), &thread_cancelled, false);
            let _ = sender.send(result);
        })
        .map_err(|_| "The Lua worker could not start.")?;
    Ok(RunningPlugin {
        receiver,
        cancelled,
    })
}

pub(crate) fn run_fixture(input: Input) -> Result<String, String> {
    run_one(input, None, false)
}

pub(crate) fn run_save(input: Input, approval: File) -> Result<String, String> {
    run_one(input, Some(approval), true)
}

fn run_one(input: Input, approval: Option<File>, save: bool) -> Result<String, String> {
    let [replacement] = run(input, approval, &AtomicBool::new(false), save)?
        .try_into()
        .map_err(|_| "The Lua worker returned the wrong number of edits.")?;
    Ok(replacement)
}

fn run(
    input: Input,
    approval: Option<File>,
    cancelled: &AtomicBool,
    save: bool,
) -> Result<Vec<String>, String> {
    if !SUPPORTED {
        return Err("Lua plugins are unavailable on this platform.".into());
    }
    engine::run(input, approval, cancelled, save)
}

pub(crate) fn run_if_requested(args: &[OsString]) -> Option<bool> {
    if args != [OsStr::new(HELPER_ARG)] {
        return None;
    }
    if !SUPPORTED {
        return Some(false);
    }
    // Install the OS boundary before the worker touches its request pipes.
    let isolated = crate::plugin_sandbox::enter().is_ok();
    Some(engine::run_helper(isolated))
}
