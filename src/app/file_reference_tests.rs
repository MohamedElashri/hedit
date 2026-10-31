use super::tests::{finish_app_job, frame, key};
use super::*;

fn fixture(root: &std::path::Path) -> (egui::Context, HeditApp) {
    let (workspace, _) = Workspace::restore(root.join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.settings.terminal_onboarding_complete = true;
    (ctx, app)
}

#[test]
fn file_reference_shortcut_opens_at_location_and_preserves_dirty_buffer() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("data file.txt");
    let source = directory.path().join("notes.txt");
    std::fs::write(&target, "first\nsecond\nthird\n").unwrap();
    std::fs::write(&source, "\"data file.txt:2:3\"\n").unwrap();
    let (ctx, mut app) = fixture(directory.path());
    app.open(&ctx, Some(target.clone()));
    finish_app_job(&mut app, &ctx);
    let old = app.active().page.text.clone();
    app.workspace.open[app.workspace.active]
        .page
        .text
        .push_str("unsaved\n");
    app.workspace.edit(app.workspace.active, &old).unwrap();
    let target_id = app.active().view.id;
    app.open(&ctx, Some(source));
    finish_app_job(&mut app, &ctx);
    app.workspace.open[app.workspace.active].view.anchor = 4;
    app.workspace.open[app.workspace.active].view.head = 4;
    frame(&ctx, &mut app, vec![]);
    let count = app.workspace.open.len();
    frame(&ctx, &mut app, vec![key(Key::D, Modifiers::ALT)]);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().view.id, target_id);
    assert_eq!(app.active().view.head, 8);
    assert!(app.active().page.text.ends_with("unsaved\n"));
    assert!(app.active().page.info.dirty);
    assert_eq!(app.workspace.open.len(), count);
}

#[test]
fn file_reference_missing_and_stale_results_leave_the_document_alone() {
    let directory = tempfile::tempdir().unwrap();
    let (ctx, mut app) = fixture(directory.path());
    app.workspace.open[0].page.text = "missing.txt".into();
    app.workspace.edit(0, "").unwrap();
    app.execute(&ctx, Command::OpenFileReference);
    finish_app_job(&mut app, &ctx);
    assert!(app.notice.contains("No existing file"));
    assert_eq!(app.workspace.open.len(), 1);
    let id = app.active().view.id;
    app.execute(&ctx, Command::New);
    app.quality_event(
        &ctx,
        quality::Event::FileReferences(
            id,
            None,
            "missing.txt".into(),
            vec![directory.path().join("missing.txt")],
        ),
    );
    assert!(!app.busy);
    assert!(!directory.path().join("missing.txt").exists());
}

#[test]
fn file_reference_picker_selects_project_match_and_removed_targets_do_not_make_buffers() {
    let directory = tempfile::tempdir().unwrap();
    let (ctx, mut app) = fixture(directory.path());
    for name in ["first", "second"] {
        let root = directory.path().join(name);
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("common.h"), name).unwrap();
        app.open(&ctx, Some(root));
        finish_app_job(&mut app, &ctx);
    }
    app.workspace.open[app.workspace.active].page.text = "common.h".into();
    app.workspace.edit(app.workspace.active, "").unwrap();
    app.execute(&ctx, Command::OpenFileReference);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.pick_list.as_ref().unwrap().items.len(), 2);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![egui::Event::Text("second".into())]);
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().page.text, "second");
    let removed = directory.path().join("removed.txt");
    std::fs::write(&removed, "old").unwrap();
    let found = hedit::file_reference::resolve(removed.to_str().unwrap(), None, &[]);
    std::fs::remove_file(&removed).unwrap();
    let count = app.workspace.open.len();
    app.open_existing(&ctx, found[0].clone());
    let until = Instant::now() + Duration::from_secs(5);
    while app.busy {
        assert!(Instant::now() < until);
        app.receive(&ctx);
        thread::sleep(Duration::from_millis(5));
    }
    assert!(app.error.is_some());
    assert_eq!(app.workspace.open.len(), count);
    assert!(!removed.exists());
}
