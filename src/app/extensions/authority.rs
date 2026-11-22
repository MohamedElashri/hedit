//! A short receipt lease prevents an unpumped UI from retaining a policy lock.
use std::{
    fs::File,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};
const LIFETIME: Duration = Duration::from_millis(200);
struct State {
    file: Mutex<Option<File>>,
    changed: Condvar,
    expires: Instant,
}
pub(super) struct Receipt {
    state: Arc<State>,
}
pub(super) struct Expiry {
    state: Arc<State>,
}
impl Receipt {
    pub(super) fn new(file: File) -> (Self, Expiry) {
        let state = Arc::new(State {
            file: Mutex::new(Some(file)),
            changed: Condvar::new(),
            expires: Instant::now() + LIFETIME,
        });
        (
            Self {
                state: state.clone(),
            },
            Expiry { state },
        )
    }
    /// Never wait for the I/O thread on the UI. The returned guard is consumed
    /// immediately by the authorized delivery/publication/effect in this frame.
    pub(super) fn take(self) -> Option<File> {
        let mut file = self.state.file.try_lock().ok()?;
        let result = if Instant::now() < self.state.expires {
            file.take()
        } else {
            file.take();
            None
        };
        self.state.changed.notify_one();
        result
    }
}
impl Drop for Receipt {
    fn drop(&mut self) {
        if let Ok(mut file) = self.state.file.try_lock() {
            file.take();
        }
        self.state.changed.notify_one();
    }
}
impl Expiry {
    /// Runs on the existing bounded authorization thread, never a new timer.
    pub(super) fn wait(self) {
        let mut file = self
            .state
            .file
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        while file.is_some() {
            let remaining = self.state.expires.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                file.take();
                break;
            }
            let (next, _) = self
                .state
                .changed
                .wait_timeout(file, remaining)
                .unwrap_or_else(|error| error.into_inner());
            file = next;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unconsumed_receipts_expire_and_dropped_receipts_release_immediately() {
        let path = tempfile::NamedTempFile::new().unwrap();
        let locked = File::options()
            .read(true)
            .write(true)
            .open(path.path())
            .unwrap();
        locked.lock().unwrap();
        let contender = File::options()
            .read(true)
            .write(true)
            .open(path.path())
            .unwrap();
        let (receipt, expiry) = Receipt::new(locked);
        let worker = std::thread::spawn(move || expiry.wait());
        worker.join().unwrap();
        assert!(receipt.take().is_none());
        hedit_core::file_io::lock_with_timeout(&contender, Duration::from_secs(2)).unwrap();
        contender.unlock().unwrap();
        let locked = File::options()
            .read(true)
            .write(true)
            .open(path.path())
            .unwrap();
        locked.lock().unwrap();
        let (receipt, expiry) = Receipt::new(locked);
        drop(receipt);
        expiry.wait();
        hedit_core::file_io::lock_with_timeout(&contender, Duration::from_secs(2)).unwrap();
    }
    #[test]
    fn a_consumed_guard_retains_authority_until_the_authorized_operation_finishes() {
        let path = tempfile::NamedTempFile::new().unwrap();
        let locked = File::options()
            .read(true)
            .write(true)
            .open(path.path())
            .unwrap();
        locked.lock().unwrap();
        let contender = File::options()
            .read(true)
            .write(true)
            .open(path.path())
            .unwrap();
        let (receipt, expiry) = Receipt::new(locked);
        let guard = receipt.take().unwrap();
        expiry.wait();
        assert!(matches!(
            contender.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        drop(guard);
        // Parallel native launches can briefly inherit the open description.
        // Keep the receipt deadline strict; only wait for OS lock cleanup here.
        hedit_core::file_io::lock_with_timeout(&contender, Duration::from_secs(2)).unwrap();
    }
}
