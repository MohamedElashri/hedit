//! `hedit -w`: signal the waiting terminal once the files are closed.
use super::*;
use std::path::Path;

pub(super) struct Wait {
    /// The files to wait for; empty waits for the window to close.
    paths: Vec<PathBuf>,
    /// Created to release the terminal.
    file: PathBuf,
    /// IDs of opened files, which stay stable through rename and Save As.
    documents: std::collections::HashSet<u64>,
    since: Instant,
}

/// The path part of `file:line:column`, resolved like an open file's path.
fn resolve(path: &Path) -> PathBuf {
    let (path, _, _) = hedit::cli::location(path);
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().map_or_else(|_| path.clone(), |cwd| cwd.join(&path))
    };
    std::fs::canonicalize(&path)
        .ok()
        .or_else(|| {
            // New files have no canonical path yet; match open_or_new, which
            // resolves their parent before assigning the document's path.
            Some(
                std::fs::canonicalize(path.parent()?)
                    .ok()?
                    .join(path.file_name()?),
            )
        })
        .unwrap_or(path)
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
            documents: Default::default(),
            since: Instant::now(),
        });
    }
    /// Release waits whose files are no longer open.
    pub(super) fn track_waits(&mut self) {
        if self.waits.is_empty() {
            return;
        }
        let open: Vec<_> = self
            .workspace
            .open
            .iter()
            .map(|b| (b.view.id, b.page.info.path.as_deref().map(Path::new)))
            .collect();
        let loading = self.busy || self.opens.pending() || !self.pending_paths.is_empty();
        self.waits.retain_mut(|wait| {
            if wait.paths.is_empty() {
                return true; // Released when the window closes.
            }
            for (id, path) in &open {
                if path.is_some_and(|path| wait.paths.iter().any(|p| p == path)) {
                    wait.documents.insert(*id);
                }
            }
            if open.iter().any(|(id, _)| wait.documents.contains(id)) {
                return true;
            }
            // A file that never opened (for example, it was refused) must not
            // keep the terminal waiting forever.
            let gave_up = !loading && wait.since.elapsed() > Duration::from_secs(5);
            if !loading && (!wait.documents.is_empty() || gave_up) {
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
            .any(|w| w.documents.is_empty() && !w.paths.is_empty())
            .then_some(Duration::from_millis(500))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{close::Choice, tests::finish_app_job};

    #[test]
    fn a_slow_open_does_not_release_a_wait_before_the_result_arrives() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("slow.txt");
        std::fs::write(&path, "text").unwrap();
        let released = directory.path().join("released");
        let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.wait_for(&[path], released.clone());
        app.waits[0].since = Instant::now() - Duration::from_secs(6);
        let (tx, rx) = mpsc::channel();
        app.read_open(&ctx, "slow.txt".into(), move || {
            rx.recv().unwrap();
            Ok(Action::Nothing)
        });
        app.track_waits();
        assert!(!released.exists());
        tx.send(()).unwrap();
        finish_app_job(&mut app, &ctx);
        app.track_waits();
        assert!(released.exists());
    }

    #[test]
    fn waiting_for_a_new_relative_path_does_not_release_while_it_is_open() {
        let directory = tempfile::tempdir().unwrap();
        let name = format!(
            "hedit-wait-{}.txt",
            directory.path().file_name().unwrap().to_string_lossy()
        );
        let relative = PathBuf::from(".").join(name);
        let absolute = std::env::current_dir().unwrap().join(&relative);
        assert!(!absolute.exists());
        let released = directory.path().join("released");
        let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.wait_for(std::slice::from_ref(&relative), released.clone());
        app.open(&ctx, Some(relative));
        finish_app_job(&mut app, &ctx);
        app.waits[0].since = Instant::now() - Duration::from_secs(6);
        app.track_waits();
        assert!(!released.exists(), "the new file is still open");
        app.execute(&ctx, Command::Close);
        app.choose_close(&ctx, Choice::Discard);
        finish_app_job(&mut app, &ctx);
        app.track_waits();
        assert!(released.exists(), "closing releases the waiting terminal");
        assert!(!absolute.exists());
    }
}
