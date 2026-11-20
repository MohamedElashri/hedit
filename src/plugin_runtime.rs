use std::ffi::{OsStr, OsString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

#[cfg(all(
    target_os = "linux",
    target_arch = "x86_64",
    target_pointer_width = "64"
))]
mod linux;

pub(crate) const SUPPORTED: bool = crate::plugin_sandbox::SUPPORTED;
pub(crate) const MAX_SELECTION_BYTES: usize = 64 * 1024;

pub(crate) const HELPER_ARG: &str = "--hedit-plugin-worker";

pub(crate) struct RunningPlugin {
    receiver: mpsc::Receiver<Result<String, String>>,
    cancelled: Arc<AtomicBool>,
}

impl RunningPlugin {
    pub(crate) fn try_result(&mut self) -> Option<Result<String, String>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("The Lua worker stopped.".into())),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_test_result(result: Result<String, String>) -> Self {
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

pub(crate) fn start(
    source: String,
    selection: String,
    language: String,
) -> Result<RunningPlugin, String> {
    #[cfg(all(
        target_os = "linux",
        target_arch = "x86_64",
        target_pointer_width = "64"
    ))]
    return linux::start(source, selection, language);

    #[cfg(not(all(
        target_os = "linux",
        target_arch = "x86_64",
        target_pointer_width = "64"
    )))]
    {
        let _ = (source, selection, language);
        Err("Lua plugins are unavailable on this platform.".into())
    }
}

pub(crate) fn run_if_requested(args: &[OsString]) -> Option<bool> {
    if args != [OsStr::new(HELPER_ARG)] {
        return None;
    }
    // Install the OS boundary before the worker touches its request pipes.
    let isolated = crate::plugin_sandbox::enter().is_ok();
    #[cfg(all(
        target_os = "linux",
        target_arch = "x86_64",
        target_pointer_width = "64"
    ))]
    return Some(linux::run_helper(isolated));

    #[cfg(not(all(
        target_os = "linux",
        target_arch = "x86_64",
        target_pointer_width = "64"
    )))]
    {
        let _ = isolated;
        Some(false)
    }
}
