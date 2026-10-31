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
fn counterpart_shortcut_reuses_dirty_buffers_and_preserves_navigation() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("solver.cpp");
    let header = directory.path().join("solver.hpp");
    std::fs::write(&source, "first\nsecond\n").unwrap();
    std::fs::write(&header, "header\n").unwrap();
    let (ctx, mut app) = fixture(directory.path());
    app.open(&ctx, Some(header.clone()));
    finish_app_job(&mut app, &ctx);
    app.workspace.open[app.workspace.active].page.text = "unsaved header\n".into();
    app.workspace
        .edit(app.workspace.active, "header\n")
        .unwrap();
    assert!(app.active().page.info.dirty);
    let header_id = app.active().view.id;
    app.open(&ctx, Some(source.clone()));
    finish_app_job(&mut app, &ctx);
    app.workspace.open[app.workspace.active].view.anchor = 6;
    app.workspace.open[app.workspace.active].view.head = 6;
    let count = app.workspace.open.len();
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::O, Modifiers::ALT)]);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().view.id, header_id);
    assert_eq!(app.active().page.text, "unsaved header\n");
    assert!(app.active().page.info.dirty);
    assert_eq!(app.workspace.open.len(), count);
    app.execute(&ctx, Command::NavBack);
    finish_app_job(&mut app, &ctx);
    assert_eq!(
        app.active().page.info.path.as_deref(),
        source.canonicalize().unwrap().to_str()
    );
    assert_eq!(app.active().view.head, 6);
}

#[test]
fn counterpart_picker_can_cancel_or_choose_without_changing_source() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("solver.cpp");
    let header = directory.path().join("solver.hpp");
    for path in [&source, &header, &directory.path().join("solver.h")] {
        std::fs::write(path, "keep source\n").unwrap();
    }
    let (ctx, mut app) = fixture(directory.path());
    app.open(&ctx, Some(source.clone()));
    finish_app_job(&mut app, &ctx);
    let id = app.active().view.id;
    app.execute(&ctx, Command::SwitchCounterpart);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.pick_list.as_ref().unwrap().items.len(), 2);
    assert_eq!(app.active().view.id, id);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
    assert!(!app.palette);
    assert_eq!(app.active().view.id, id);
    app.execute(&ctx, Command::SwitchCounterpart);
    finish_app_job(&mut app, &ctx);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![egui::Event::Text("hpp".into())]);
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    finish_app_job(&mut app, &ctx);
    assert_eq!(
        app.active().page.info.path.as_deref(),
        header.canonicalize().unwrap().to_str()
    );
    assert_eq!(std::fs::read_to_string(source).unwrap(), "keep source\n");
}

#[test]
fn counterpart_availability_no_match_and_stale_replies() {
    let directory = tempfile::tempdir().unwrap();
    let (ctx, mut app) = fixture(directory.path());
    assert!(!app.command_enabled(Command::SwitchCounterpart));
    let source = directory.path().join("main.cu");
    std::fs::write(&source, "kernel\n").unwrap();
    app.open(&ctx, Some(source.clone()));
    finish_app_job(&mut app, &ctx);
    assert!(app.command_enabled(Command::SwitchCounterpart));
    app.execute(&ctx, Command::SwitchCounterpart);
    finish_app_job(&mut app, &ctx);
    assert!(app.notice.contains("No header or implementation"));
    let id = app.active().view.id;
    app.execute(&ctx, Command::New);
    app.quality_event(
        &ctx,
        quality::Event::Counterparts(id, source.clone(), vec![source]),
    );
    assert!(!app.busy);
    assert!(!app.palette);
    assert!(app.active().page.info.path.is_none());
}

#[test]
fn counterpart_removed_after_lookup_does_not_create_an_empty_buffer() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("main.cpp");
    let target = directory.path().join("main.h");
    std::fs::write(&source, "source").unwrap();
    std::fs::write(&target, "header").unwrap();
    let (ctx, mut app) = fixture(directory.path());
    app.open(&ctx, Some(source.clone()));
    finish_app_job(&mut app, &ctx);
    let found =
        hedit::counterpart::find(&source, &[], &std::sync::atomic::AtomicBool::new(false)).unwrap();
    std::fs::remove_file(target).unwrap();
    let id = app.active().view.id;
    let count = app.workspace.open.len();
    app.show_counterparts(&ctx, id, source.canonicalize().unwrap(), found);
    let until = Instant::now() + Duration::from_secs(5);
    while app.busy {
        assert!(Instant::now() < until);
        app.receive(&ctx);
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(app.workspace.open.len(), count);
    assert_eq!(app.active().view.id, id);
}
