use super::tests::finish_app_job;
use super::*;

fn settle(app: &mut HeditApp, ctx: &egui::Context) {
    let start = Instant::now();
    while app.busy || app.checkpoint_pending {
        assert!(start.elapsed() < Duration::from_secs(5));
        app.receive(ctx);
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn reload_rejects_stale_edits_and_resets_history_before_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let session = directory.path().join("session");
    let path = directory.path().join("notes.txt");
    std::fs::write(&path, "original\n").unwrap();
    let (workspace, _) = Workspace::restore(session.clone()).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.open(&ctx, Some(path.clone()));
    finish_app_job(&mut app, &ctx);
    let id = app.active().view.id;
    let initial_revision = app.active().page.info.revision;
    let index = app.workspace.active;
    app.workspace.open[index].page.text = "saved edits\n".into();
    app.workspace.edit(index, "original\n").unwrap();
    app.save_to_path(&ctx, id, None);
    finish_app_job(&mut app, &ctx);
    settle(&mut app, &ctx);
    app.workspace.checkpoint().unwrap();
    std::fs::write(&path, "external revision\n").unwrap();
    app.load_page(&ctx, 0, true);
    finish_app_job(&mut app, &ctx);
    settle(&mut app, &ctx);
    assert_eq!(app.active().page.text, "external revision\n");
    let stale_edit = app
        .workspace
        .editor
        .get(id)
        .unwrap()
        .lock()
        .unwrap()
        .replace(0, 0, "stale edit".into(), initial_revision);
    assert!(
        stale_edit.is_err(),
        "a reload must reject edits based on the old text"
    );
    assert!(app.active().cursor_undo.is_empty());
    assert!(app.active().cursor_redo.is_empty());
    app.workspace.checkpoint().unwrap();
    drop(app);

    let (restored, errors) = Workspace::restore(session).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(
        restored.open[restored.active].page.text,
        "external revision\n"
    );
    assert!(!restored.open[restored.active].page.info.dirty);
}

#[test]
fn failed_reload_keeps_the_original_buffer_available_for_save_as() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.txt");
    std::fs::write(&path, "keep this text\n").unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.open(&ctx, Some(path.clone()));
    finish_app_job(&mut app, &ctx);
    settle(&mut app, &ctx);
    let id = app.active().view.id;
    let original_size = app.active().page.info.size;
    let offset = hedit::document::SMALL_LIMIT;
    let mut disk = vec![b'x'; (offset + PAGE_SIZE) as usize];
    disk[offset as usize + 10] = 0xff;
    std::fs::write(&path, disk).unwrap();
    app.load_page(&ctx, offset, true);
    settle(&mut app, &ctx);
    assert!(app.error.is_some());
    assert_eq!(app.active().page.text, "keep this text\n");
    assert_eq!(
        app.workspace
            .editor
            .get(id)
            .unwrap()
            .lock()
            .unwrap()
            .info()
            .size,
        original_size
    );
    app.error = None;
    let rescue = directory.path().join("rescued.txt");
    app.save_to_path(&ctx, id, Some(rescue.clone()));
    finish_app_job(&mut app, &ctx);
    assert_eq!(std::fs::read_to_string(rescue).unwrap(), "keep this text\n");
}

#[test]
fn removing_a_tab_during_a_checkpoint_does_not_break_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    workspace.new_buffer().unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    let first = app
        .workspace
        .editor
        .get(app.workspace.open[0].view.id)
        .unwrap();
    let hold = first.lock().unwrap();
    // Pause snapshot streaming while the UI removes a later document.
    app.checkpoint(&ctx);
    app.workspace.forget(1).unwrap();
    app.changed();
    drop(hold);
    settle(&mut app, &ctx);
    assert!(app.error.is_none(), "{:?}", app.error);
    app.workspace.checkpoint().unwrap();
    drop(app);
    let (restored, errors) = Workspace::restore(directory.path().join("session")).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(restored.open.len(), 1);
}
