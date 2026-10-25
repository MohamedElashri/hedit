use super::close::Choice;
use super::tests::{click_label, finish_app_job, frame, key, text_rect};
use super::*;

fn disk_app() -> (tempfile::TempDir, egui::Context, HeditApp, PathBuf, u64) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.txt");
    std::fs::write(&path, "original\n").unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.open(&ctx, Some(path.clone()));
    finish_app_job(&mut app, &ctx);
    let id = app.active().view.id;
    edit(&mut app, "edited\n");
    (directory, ctx, app, path, id)
}

fn edit(app: &mut HeditApp, text: &str) {
    let old = app.active().page.text.clone();
    app.workspace.open[app.workspace.active].page.text = text.into();
    app.workspace.edit(app.workspace.active, &old).unwrap();
    app.changed();
}

#[test]
fn last_blank_tab_has_no_close_button_and_ignores_close_actions() {
    let directory = tempfile::tempdir().unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    let id = app.active().view.id;
    let name = app.active().view.name.clone();
    frame(&ctx, &mut app, vec![]);
    let output = frame(&ctx, &mut app, vec![]);
    assert!(!app.command_enabled(Command::Close));
    assert!(!output
        .shapes
        .iter()
        .any(|s| text_rect(&s.shape, "×").is_some()));
    assert!(!output
        .shapes
        .iter()
        .any(|s| text_rect(&s.shape, "OPEN FILES").is_some()));
    let tab = output
        .shapes
        .iter()
        .find_map(|s| text_rect(&s.shape, &name))
        .unwrap();
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(tab.center()),
                egui::Event::PointerButton {
                    pos: tab.center(),
                    button: egui::PointerButton::Middle,
                    pressed,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
    }
    frame(&ctx, &mut app, vec![key(Key::W, Modifiers::COMMAND)]);
    for command in [Command::Close, Command::CloseAll, Command::CloseUnmodified] {
        app.execute(&ctx, command);
        assert_eq!(app.workspace.open.len(), 1);
        assert_eq!(app.active().view.id, id);
        assert_eq!(app.active().view.name, name);
        assert!(app.workspace.closed.is_empty());
        assert!(!app.closing);
    }
    app.execute(&ctx, Command::New);
    assert!(app.command_enabled(Command::Close));
    let output = frame(&ctx, &mut app, vec![]);
    assert!(output
        .shapes
        .iter()
        .any(|s| text_rect(&s.shape, "×").is_some()));
    app.execute(&ctx, Command::CloseAll);
    assert_eq!(app.workspace.open.len(), 1);
    assert_eq!(app.active().view.id, id);
    assert_eq!(app.workspace.closed.len(), 1);
    app.execute(&ctx, Command::Quit);
    assert!(app.closing, "the empty tab must not prevent quitting");
}

#[test]
fn close_prompts_without_writing_and_cancel_keeps_edits() {
    let (_directory, ctx, mut app, path, id) = disk_app();
    app.execute(&ctx, Command::Close);
    app.receive(&ctx);
    assert!(app.close_flow.is_some());
    assert!(!app.exit_ready);
    assert!(!app.busy);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original\n");
    assert_eq!(app.active().view.id, id);
    frame(&ctx, &mut app, vec![]);
    let output = frame(&ctx, &mut app, vec![]);
    for label in ["Save", "Don't Save", "Cancel"] {
        assert!(output
            .shapes
            .iter()
            .any(|s| text_rect(&s.shape, label).is_some()));
    }
    click_label(&ctx, &mut app, "Cancel");
    assert!(app.close_flow.is_none());
    assert!(!app.closing);
    assert_eq!(app.active().page.text, "edited\n");
    assert!(app.active().page.info.dirty);
}

#[test]
fn save_button_writes_before_closing_and_reopen_is_clean() {
    let (_directory, ctx, mut app, path, id) = disk_app();
    app.execute(&ctx, Command::Close);
    frame(&ctx, &mut app, vec![]);
    click_label(&ctx, &mut app, "Save");
    finish_app_job(&mut app, &ctx);
    assert!(!app.closing);
    assert!(app.close_flow.is_none());
    assert!(!app.workspace.open.iter().any(|b| b.view.id == id));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "edited\n");
    app.execute(&ctx, Command::Reopen);
    assert_eq!(app.active().page.text, "edited\n");
    assert!(!app.active().page.info.dirty);
}

#[test]
fn dont_save_discards_from_shelf_undo_and_recovery_without_writing() {
    let (directory, ctx, mut app, path, id) = disk_app();
    app.execute(&ctx, Command::Close);
    frame(&ctx, &mut app, vec![]);
    click_label(&ctx, &mut app, "Don't Save");
    finish_app_job(&mut app, &ctx);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original\n");
    assert!(!app.workspace.open.iter().any(|b| b.view.id == id));
    app.execute(&ctx, Command::Reopen);
    assert_eq!(app.active().page.text, "original\n");
    assert!(!app.active().page.info.dirty);
    app.execute(&ctx, Command::Undo);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().page.text, "original\n");
    app.workspace.checkpoint().unwrap();
    drop(app);
    let (restored, errors) = Workspace::restore(directory.path().join("session")).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(restored.open[restored.active].page.text, "original\n");
    assert!(!restored.open[restored.active].page.info.dirty);
}

#[test]
fn escape_cancels_quit_and_shortcuts_cannot_edit_behind_prompt() {
    let (_directory, ctx, mut app, path, id) = disk_app();
    app.execute(&ctx, Command::Quit);
    frame(&ctx, &mut app, vec![key(Key::W, Modifiers::COMMAND)]);
    assert!(app.close_flow.is_some());
    assert_eq!(app.active().view.id, id);
    frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
    assert!(app.close_flow.is_none());
    assert!(!app.closing);
    assert_eq!(app.active().page.text, "edited\n");
    assert_eq!(std::fs::read_to_string(path).unwrap(), "original\n");
}

#[test]
fn quit_resolves_disk_edits_and_keeps_untitled_notes_in_recovery() {
    for choice in [Choice::Save, Choice::Discard] {
        let (directory, ctx, mut app, path, _) = disk_app();
        app.workspace.active = 0;
        edit(&mut app, "scratch notes");
        app.execute(&ctx, Command::Quit);
        app.receive(&ctx);
        assert!(app.close_flow.is_some());
        assert!(!app.exit_ready);
        app.choose_close(&ctx, choice);
        finish_app_job(&mut app, &ctx);
        let started = Instant::now();
        while !app.exit_ready {
            assert!(started.elapsed() < Duration::from_secs(5));
            app.receive(&ctx);
            thread::sleep(Duration::from_millis(5));
        }
        let expected = if matches!(choice, Choice::Save) {
            "edited\n"
        } else {
            "original\n"
        };
        assert_eq!(std::fs::read_to_string(path).unwrap(), expected);
        drop(app);
        let (restored, errors) = Workspace::restore(directory.path().join("session")).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert!(restored
            .open
            .iter()
            .any(|b| b.page.info.path.is_none() && b.page.text == "scratch notes"));
        assert!(restored
            .open
            .iter()
            .any(|b| b.page.info.path.is_some() && b.page.text == expected && !b.page.info.dirty));
    }
}

#[test]
fn bulk_closes_prompt_for_each_dirty_disk_file_and_respect_pins() {
    for command in [Command::CloseAll, Command::CloseOthers, Command::CloseRight] {
        let (directory, ctx, mut app, path, first) = disk_app();
        let other_path = directory.path().join("other.txt");
        std::fs::write(&other_path, "other\n").unwrap();
        app.open(&ctx, Some(other_path.clone()));
        finish_app_job(&mut app, &ctx);
        let second = app.active().view.id;
        edit(&mut app, "other edited\n");
        app.workspace.active = 0;
        app.workspace.open[0].view.pinned = true;
        app.execute(&ctx, command);
        assert_eq!(app.active().view.id, second);
        app.choose_close(&ctx, Choice::Discard);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().view.id, first);
        assert!(app.close_flow.is_some());
        assert!(!app.workspace.open.iter().any(|b| b.view.id == second));
        app.choose_close(&ctx, Choice::Cancel);
        assert!(!app.closing);
        assert!(app.workspace.open.iter().any(|b| b.view.pinned));
        assert!(app.active().page.info.dirty);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "original\n");
        assert_eq!(std::fs::read_to_string(&other_path).unwrap(), "other\n");
    }
}

#[test]
fn failed_save_aborts_close_and_retains_edits() {
    let (_directory, ctx, mut app, path, id) = disk_app();
    std::fs::write(&path, "external changes with a different size\n").unwrap();
    app.execute(&ctx, Command::Close);
    app.choose_close(&ctx, Choice::Save);
    let started = Instant::now();
    while app.busy {
        assert!(started.elapsed() < Duration::from_secs(5));
        app.receive(&ctx);
        thread::sleep(Duration::from_millis(5));
    }
    assert!(app.error.is_some());
    assert!(!app.closing);
    assert!(app.close_flow.is_none());
    assert_eq!(app.active().view.id, id);
    assert_eq!(app.active().page.text, "edited\n");
    assert!(app.active().page.info.dirty);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "external changes with a different size\n"
    );
}

#[test]
fn enter_saves_and_closes_from_the_prompt() {
    let (_directory, ctx, mut app, path, id) = disk_app();
    app.execute(&ctx, Command::Close);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    finish_app_job(&mut app, &ctx);
    assert!(!app.workspace.open.iter().any(|b| b.view.id == id));
    assert_eq!(std::fs::read_to_string(path).unwrap(), "edited\n");
}

#[test]
fn dont_save_closes_missing_files_without_recreating_or_recovering_them() {
    for saved_before in [false, true] {
        for command in [Command::Close, Command::Quit] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("missing.txt");
            if saved_before {
                std::fs::write(&path, "original\n").unwrap();
            }
            let session = directory.path().join("session");
            let (workspace, _) = Workspace::restore(session.clone()).unwrap();
            let ctx = egui::Context::default();
            let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
            edit(&mut app, "keep these notes");
            app.open(&ctx, Some(path.clone()));
            finish_app_job(&mut app, &ctx);
            edit(&mut app, "discard these edits");
            let id = app.active().view.id;
            if saved_before {
                std::fs::remove_file(&path).unwrap();
            }
            app.execute(&ctx, command);
            assert!(app.close_flow.is_some());
            app.choose_close(&ctx, Choice::Discard);
            finish_app_job(&mut app, &ctx);
            assert!(app.close_flow.is_none());
            assert!(!path.exists());
            assert!(!app.workspace.open.iter().any(|b| b.view.id == id));
            assert!(!app.workspace.closed.iter().any(|b| b.id == id));
            assert!(app.workspace.editor.get(id).is_err());
            if command == Command::Quit {
                let started = Instant::now();
                while !app.exit_ready {
                    assert!(started.elapsed() < Duration::from_secs(5));
                    app.receive(&ctx);
                    assert!(app.error.is_none(), "{:?}", app.error);
                    thread::sleep(Duration::from_millis(5));
                }
            }
            app.workspace.checkpoint().unwrap();
            drop(app);
            let (restored, errors) = Workspace::restore(session).unwrap();
            assert!(errors.is_empty(), "{errors:?}");
            assert_eq!(restored.open.len(), 1);
            assert_eq!(restored.open[0].page.text, "keep these notes");
            assert!(restored.closed.is_empty());
        }
    }
}

#[test]
fn clean_files_and_untitled_notes_close_without_prompting() {
    let (_directory, ctx, mut app, _path, id) = disk_app();
    app.save_to_path(&ctx, id, None);
    finish_app_job(&mut app, &ctx);
    app.execute(&ctx, Command::Close);
    assert!(app.close_flow.is_none());
    assert!(!app.closing);
    edit(&mut app, "scratch notes");
    app.execute(&ctx, Command::Close);
    assert!(app.close_flow.is_none());
    app.execute(&ctx, Command::Reopen);
    assert_eq!(app.active().page.text, "scratch notes");
    assert!(app.active().page.info.path.is_none());
}
