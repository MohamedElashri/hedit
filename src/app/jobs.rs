//! Always report worker completion, including an unexpected unwind.
use super::*;

pub(super) fn run<T>(work: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
        .unwrap_or_else(|_| Err("The background operation stopped unexpectedly.".into()))
}

impl HeditApp {
    pub(super) fn checkpoint_job(
        &mut self,
        ctx: &egui::Context,
        generation: u64,
        work: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) {
        let sender = self.checkpoint_tx.clone();
        let ctx = ctx.clone();
        self.checkpoint_pending = true;
        self.checkpoint_worker = Some(thread::spawn(move || {
            let result = run(work);
            let _ = sender.send(Checkpoint { generation, result });
            ctx.request_repaint();
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::finish_app_job;

    const TEXT: &str = "Keep this draft: café / 東京 / 😀\n";

    fn app() -> (tempfile::TempDir, egui::Context, HeditApp) {
        let dir = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.workspace.open[0].page.text = TEXT.into();
        app.workspace.edit(0, "").unwrap();
        app.changed();
        (dir, ctx, app)
    }

    #[test]
    fn a_panicked_operation_releases_busy_preserves_text_and_allows_saving() {
        let (dir, ctx, mut app) = app();
        let id = app.active().view.id;
        app.job(&ctx, || panic!("injected background operation failure"));
        app.request_quit();
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.busy {
            assert!(Instant::now() < deadline, "panicked job did not complete");
            app.receive(&ctx);
            thread::sleep(Duration::from_millis(5));
        }
        assert!(!app.busy && !app.closing && !app.exit_ready);
        assert!(app.error.take().unwrap().contains("stopped unexpectedly"));
        assert_eq!(app.active().page.text, TEXT);
        let target = dir.path().join("rescued.txt");
        app.save_to_path(&ctx, id, Some(target.clone()));
        finish_app_job(&mut app, &ctx);
        assert_eq!(std::fs::read_to_string(target).unwrap(), TEXT);
    }

    #[test]
    fn a_panicked_checkpoint_cancels_exit_and_can_be_retried() {
        let (dir, ctx, mut app) = app();
        let persisted = app.persisted;
        app.request_quit();
        // Observe the failure before any automatic checkpoint retry is due.
        app.checkpoint_at = Instant::now() + Duration::from_secs(60);
        app.checkpoint_job(&ctx, app.generation, || {
            panic!("injected checkpoint failure")
        });
        app.checkpoint_worker.take().unwrap().join().unwrap();
        app.receive(&ctx);
        assert!(!app.checkpoint_pending && !app.closing && !app.exit_ready);
        assert_eq!(app.persisted, persisted);
        assert!(app.error.as_deref().unwrap().contains(CHECKPOINT_FAILED));
        assert_eq!(app.active().page.text, TEXT);

        app.checkpoint(&ctx);
        app.checkpoint_worker.take().unwrap().join().unwrap();
        app.receive(&ctx);
        assert_eq!(app.persisted, app.generation);
        drop(app);
        let (restored, errors) = Workspace::restore(dir.path().join("session")).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(restored.open[restored.active].page.text, TEXT);
    }
}
