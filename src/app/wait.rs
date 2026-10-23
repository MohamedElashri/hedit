//! `hedit -w`: signal the waiting terminal once the files are closed.
use super::*;
use std::path::Path;

pub(super) struct Wait {
    /// The files to wait for; empty waits for the window to close.
    paths: Vec<PathBuf>,
    /// Created to release the terminal.
    file: PathBuf,
    /// Whether any of the files has been open, so closing them counts.
    seen: bool,
    since: Instant,
}

/// The path part of `file:line:column`, resolved like an open file's path.
fn resolve(path: &Path) -> PathBuf {
    let (path, _, _) = hedit::cli::location(path);
    std::fs::canonicalize(&path).unwrap_or(path)
}

fn release(file: &Path) {
    let _ = std::fs::write(file, b"closed");
}

impl HeditApp {
    /// Release `file` once every one of `paths` has been opened and closed.
    pub fn wait_for(&mut self, paths: &[PathBuf], file: PathBuf) {
        self.waits.push(Wait {
            paths: paths.iter().map(|p| resolve(p)).collect(),
            file,
            seen: false,
            since: Instant::now(),
        });
    }
    /// Release waits whose files are no longer open.
    pub(super) fn track_waits(&mut self) {
        if self.waits.is_empty() {
            return;
        }
        let open: Vec<PathBuf> = self
            .workspace
            .open
            .iter()
            .filter_map(|b| b.page.info.path.as_deref())
            .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p)))
            .collect();
        let loading = self.busy || !self.pending_paths.is_empty();
        self.waits.retain_mut(|wait| {
            if wait.paths.is_empty() {
                return true; // Released when the window closes.
            }
            if wait.paths.iter().any(|p| open.contains(p)) {
                wait.seen = true;
                return true;
            }
            // A file that never opened (for example, it was refused) must not
            // keep the terminal waiting forever.
            let gave_up = !loading && wait.since.elapsed() > Duration::from_secs(5);
            if wait.seen || gave_up {
                release(&wait.file);
                return false;
            }
            true
        });
    }
    /// Release every wait, as the window closes.
    pub(super) fn release_waits(&mut self) {
        for wait in self.waits.drain(..) {
            release(&wait.file);
        }
    }
    /// When to check again for files that have not opened yet.
    pub(super) fn wait_due(&self) -> Option<Duration> {
        self.waits
            .iter()
            .any(|w| !w.seen && !w.paths.is_empty())
            .then_some(Duration::from_millis(500))
    }
}
