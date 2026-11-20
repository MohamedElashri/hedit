use crate::{options::Value, Error};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

mod engine;
#[cfg(unix)]
#[path = "runtime/unix.rs"]
mod supervisor;
#[cfg(windows)]
#[path = "runtime/windows.rs"]
mod supervisor;

#[cfg(not(any(unix, windows)))]
mod supervisor {
    use crate::Error;
    use std::{
        fs::File,
        path::{Path, PathBuf},
        sync::atomic::AtomicBool,
        time::Duration,
    };

    pub(super) fn executable() -> Result<PathBuf, Error> {
        Err(Error::Sandbox)
    }

    pub(super) fn supervise(
        _: &Path,
        _: Vec<u8>,
        _: &AtomicBool,
        _: Duration,
        _: Option<File>,
    ) -> Result<Vec<u8>, Error> {
        Err(Error::Sandbox)
    }
}

pub const SUPPORTED: bool = crate::sandbox::SUPPORTED;
pub const MAX_SELECTION_BYTES: usize = 64 * 1024;
pub const MAX_SELECTIONS: usize = 16;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub source: String,
    pub selections: Vec<String>,
    pub language: String,
    pub settings: BTreeMap<String, Value>,
}

pub const HELPER_ARG: &str = "--hedit-plugin-worker";

pub struct RunningPlugin {
    receiver: mpsc::Receiver<Result<Vec<String>, Error>>,
    cancelled: Arc<AtomicBool>,
}

impl RunningPlugin {
    pub fn try_result(&mut self) -> Option<Result<Vec<String>, Error>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(Error::Stopped)),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn from_test_result(result: Result<Vec<String>, Error>) -> Self {
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

pub fn start(input: Input, approval: File) -> Result<RunningPlugin, Error> {
    let (sender, receiver) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    let thread_cancelled = cancelled.clone();
    std::thread::Builder::new()
        .name("hedit-lua-worker".into())
        .spawn(move || {
            let result = run(input, Some(approval), &thread_cancelled, false);
            let _ = sender.send(result);
        })
        .map_err(|_| Error::Startup)?;
    Ok(RunningPlugin {
        receiver,
        cancelled,
    })
}

pub fn run_fixture(input: Input) -> Result<String, Error> {
    run_one(input, None, false)
}

pub fn run_save(input: Input, approval: File) -> Result<String, Error> {
    run_one(input, Some(approval), true)
}

fn run_one(input: Input, approval: Option<File>, save: bool) -> Result<String, Error> {
    let [replacement] = run(input, approval, &AtomicBool::new(false), save)?
        .try_into()
        .map_err(|_| Error::EditCount)?;
    Ok(replacement)
}

fn run(
    input: Input,
    approval: Option<File>,
    cancelled: &AtomicBool,
    save: bool,
) -> Result<Vec<String>, Error> {
    if !SUPPORTED {
        return Err(Error::Unavailable);
    }
    engine::run(input, approval, cancelled, save)
}

pub fn run_if_requested(args: &[OsString]) -> Option<bool> {
    if args != [OsStr::new(HELPER_ARG)] {
        return None;
    }
    if !SUPPORTED {
        return Some(false);
    }
    // Install the OS boundary before the worker touches its request pipes.
    let isolated = crate::sandbox::enter().is_ok();
    Some(engine::run_helper(isolated))
}
