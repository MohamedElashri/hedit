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
        .any(|s| text_rect(&s.shape, "OPEN FILES").is_some()));
    let tabs = egui::containers::panel::PanelState::load(&ctx, egui::Id::new("tabs"))
        .unwrap()
        .rect;
    let tab = output
        .shapes
        .iter()
        .filter_map(|s| text_rect(&s.shape, &name))
        .find(|rect| tabs.contains(rect.center()))
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
    let tabs = egui::containers::panel::PanelState::load(&ctx, egui::Id::new("tabs"))
        .unwrap()
        .rect;
    assert!(output.shapes.iter().filter(|s| matches!(&s.shape,
        egui::Shape::LineSegment { points, stroke } if stroke.width == 1.7 && points.iter().all(|p| tabs.contains(*p))
    )).count() >= 2);
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
fn escape_cancels_tab_close_and_shortcuts_cannot_edit_behind_prompt() {
    let (_directory, ctx, mut app, path, id) = disk_app();
    app.execute(&ctx, Command::Close);
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
fn quit_preserves_all_unsaved_buffers_without_prompting_across_restarts() {
    for window_close in [false, true] {
        let (directory, ctx, mut app, path, _) = disk_app();
        app.workspace.active = 0;
        edit(&mut app, "scratch notes");
        for _ in 0..3 {
            let ids: Vec<_> = app.workspace.open.iter().map(|b| b.view.id).collect();
            if window_close {
                // The native window-close handler uses the same request.
                app.request_quit();
            } else {
                app.execute(&ctx, Command::Quit);
            }
            assert!(app.close_flow.is_none(), "quitting must not ask to save");
            let output = frame(&ctx, &mut app, vec![]);
            for label in ["Save", "Don't Save", "Cancel"] {
                assert!(!output
                    .shapes
                    .iter()
                    .any(|s| text_rect(&s.shape, label).is_some()));
            }
            finish_quit(&mut app, &ctx);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "original\n");
            drop(app);
            let (restored, errors) = Workspace::restore(directory.path().join("session")).unwrap();
            assert!(errors.is_empty(), "{errors:?}");
            assert_eq!(
                restored.open.iter().map(|b| b.view.id).collect::<Vec<_>>(),
                ids
            );
            assert!(restored
                .open
                .iter()
                .any(|b| { b.page.info.path.is_none() && b.page.text == "scratch notes" }));
            assert!(restored.open.iter().any(|b| {
                b.page.info.path.is_some() && b.page.text == "edited\n" && b.page.info.dirty
            }));
            assert!(restored.closed.is_empty());
            app = HeditApp::from_context(&ctx, None, restored, vec![], vec![]);
        }
    }
}

fn finish_quit(app: &mut HeditApp, ctx: &egui::Context) {
    let started = Instant::now();
    while !app.exit_ready {
        assert!(started.elapsed() < Duration::from_secs(5));
        app.receive(ctx);
        assert!(app.error.is_none(), "{:?}", app.error);
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(app.generation, app.persisted);
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
fn missing_files_are_discarded_on_tab_close_but_preserved_on_quit() {
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
            edit(&mut app, "unsaved edits");
            let id = app.active().view.id;
            if saved_before {
                std::fs::remove_file(&path).unwrap();
            }
            app.execute(&ctx, command);
            if command == Command::Close {
                assert!(app.close_flow.is_some());
                app.choose_close(&ctx, Choice::Discard);
                finish_app_job(&mut app, &ctx);
                assert!(!app.workspace.open.iter().any(|b| b.view.id == id));
                assert!(!app.workspace.closed.iter().any(|b| b.id == id));
                assert!(app.workspace.editor.get(id).is_err());
                app.workspace.checkpoint().unwrap();
            } else {
                assert!(app.close_flow.is_none());
                finish_quit(&mut app, &ctx);
            }
            assert!(app.close_flow.is_none());
            assert!(!path.exists());
            drop(app);
            let (restored, errors) = Workspace::restore(session).unwrap();
            assert!(errors.is_empty(), "{errors:?}");
            assert_eq!(restored.open[0].page.text, "keep these notes");
            assert!(restored.closed.is_empty());
            if command == Command::Close {
                assert_eq!(restored.open.len(), 1);
            } else {
                assert_eq!(restored.open.len(), 2);
                assert_eq!(restored.open[1].view.id, id);
                assert_eq!(restored.open[1].page.text, "unsaved edits");
                assert!(restored.open[1].page.info.dirty);
            }
        }
    }
}

#[test]
fn quitting_during_a_tab_close_prompt_preserves_the_tab() {
    let (directory, ctx, mut app, path, id) = disk_app();
    app.execute(&ctx, Command::Close);
    assert!(app.close_flow.is_some());
    app.request_quit();
    assert!(app.close_flow.is_none());
    finish_quit(&mut app, &ctx);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "original\n");
    drop(app);
    let (restored, errors) = Workspace::restore(directory.path().join("session")).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    assert!(restored
        .open
        .iter()
        .any(|b| b.view.id == id && b.page.text == "edited\n"));
}

#[test]
fn quitting_waits_for_inflight_buffers_and_checkpoints_them() {
    let (directory, ctx, mut app, _, _) = disk_app();
    let id = app.workspace.editor.id();
    let mut doc = Document::empty(id);
    doc.replace(
        0,
        0,
        "arrived during shutdown λ".into(),
        doc.info().revision,
    )
    .unwrap();
    let page = doc.page(0).unwrap();
    let (release, wait) = mpsc::channel();
    app.job(&ctx, move || {
        wait.recv().map_err(|e| e.to_string())?;
        Ok(Action::Add(doc, page, 0))
    });
    app.request_quit();
    app.receive(&ctx);
    assert!(!app.exit_ready);
    assert!(app.closing);
    assert!(app.close_flow.is_none());
    release.send(()).unwrap();
    finish_quit(&mut app, &ctx);
    drop(app);
    let (restored, errors) = Workspace::restore(directory.path().join("session")).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    assert!(restored.open.iter().any(|b| {
        b.view.id == id && b.page.text == "arrived during shutdown λ" && b.page.info.dirty
    }));
}

#[test]
fn failed_quit_checkpoint_keeps_the_window_and_unsaved_edits_then_retries() {
    let (directory, ctx, mut app, path, _) = disk_app();
    // Finish any earlier checkpoint before blocking replacement of the manifest.
    let started = Instant::now();
    while app.checkpoint_pending {
        assert!(started.elapsed() < Duration::from_secs(5));
        app.receive(&ctx);
        thread::sleep(Duration::from_millis(5));
    }
    let manifest = directory.path().join("session/session.json");
    if manifest.exists() {
        std::fs::rename(&manifest, manifest.with_extension("before-failure.json")).unwrap();
    }
    std::fs::create_dir(&manifest).unwrap();
    edit(&mut app, "recover after failure\n");
    app.request_quit();
    app.receive(&ctx);
    assert!(app.checkpoint_pending);
    while app.checkpoint_pending {
        assert!(started.elapsed() < Duration::from_secs(5));
        app.receive(&ctx);
        thread::sleep(Duration::from_millis(5));
    }
    assert!(!app.exit_ready);
    assert!(!app.closing);
    assert!(app.error.as_ref().unwrap().starts_with(CHECKPOINT_FAILED));
    assert_eq!(app.active().page.text, "recover after failure\n");
    assert!(app.active().page.info.dirty);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original\n");
    std::fs::remove_dir(&manifest).unwrap();
    app.error = None;
    app.request_quit();
    finish_quit(&mut app, &ctx);
    drop(app);
    let (restored, errors) = Workspace::restore(directory.path().join("session")).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(
        restored.open[restored.active].page.text,
        "recover after failure\n"
    );
}

#[test]
fn clean_files_close_without_prompting_but_nonempty_untitled_tabs_prompt() {
    let (_directory, ctx, mut app, _path, id) = disk_app();
    app.save_to_path(&ctx, id, None);
    finish_app_job(&mut app, &ctx);
    app.execute(&ctx, Command::Close);
    assert!(app.close_flow.is_none());
    assert!(!app.closing);
    edit(&mut app, "scratch notes");
    app.execute(&ctx, Command::Close);
    assert!(app.close_flow.is_some());
    frame(&ctx, &mut app, vec![]);
    let output = frame(&ctx, &mut app, vec![]);
    for label in ["Save", "Don't Save", "Cancel"] {
        assert!(output
            .shapes
            .iter()
            .any(|s| text_rect(&s.shape, label).is_some()));
    }
    click_label(&ctx, &mut app, "Cancel");
    assert_eq!(app.active().page.text, "scratch notes");
    assert!(app.close_flow.is_none());
}

#[test]
fn nonempty_closed_notes_appear_in_closed_files_and_reopen_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let session = directory.path().join("session");
    let (workspace, _) = Workspace::restore(session.clone()).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    edit(&mut app, "recover these notes\nwith another line λ");
    let text = app.active().page.text.clone();
    let id = app.active().view.id;
    app.execute(&ctx, Command::Close);
    frame(&ctx, &mut app, vec![]);
    click_label(&ctx, &mut app, "Don't Save");
    assert!(app.close_flow.is_none());
    assert!(!app.workspace.open.iter().any(|b| b.view.id == id));
    assert_eq!(app.workspace.closed.len(), 1);
    app.workspace.checkpoint().unwrap();
    drop(app);
    let (workspace, errors) = Workspace::restore(session).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.execute(&ctx, Command::Shelf);
    frame(&ctx, &mut app, vec![]);
    let output = frame(&ctx, &mut app, vec![]);
    assert!(output
        .shapes
        .iter()
        .any(|s| text_rect(&s.shape, "recover these notes").is_some()));
    click_label(&ctx, &mut app, "Reopen");
    assert_eq!(app.active().page.text, text);
    assert!(app.active().page.info.path.is_none());
    assert!(app.workspace.closed.is_empty());
}

#[test]
fn cancelling_save_as_cancels_close_and_a_chosen_path_saves_then_shelves_the_note() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("saved-notes.txt");
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    edit(&mut app, "keep these notes\n");
    let id = app.active().view.id;
    app.execute(&ctx, Command::Close);
    // Deliver the native Save As dialog's cancellation/result without opening it in tests.
    app.tx.send(Ok(Action::SaveCancelled)).unwrap();
    app.receive(&ctx);
    assert!(!app.closing);
    assert!(app.close_flow.is_none());
    assert_eq!(app.active().view.id, id);
    assert_eq!(app.active().page.text, "keep these notes\n");
    assert!(app.workspace.closed.is_empty());
    assert!(!path.exists());
    app.execute(&ctx, Command::Close);
    app.tx.send(Ok(Action::SaveTo(id, path.clone()))).unwrap();
    app.receive(&ctx);
    finish_app_job(&mut app, &ctx);
    assert!(!app.closing);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "keep these notes\n");
    assert!(!app.workspace.open.iter().any(|b| b.view.id == id));
    assert!(app.workspace.closed.iter().any(|b| b.id == id));
    app.execute(&ctx, Command::Reopen);
    assert_eq!(app.active().page.text, "keep these notes\n");
    assert!(!app.active().page.info.dirty);
}

#[test]
fn bulk_close_prompts_for_each_nonempty_note_and_keeps_both_on_the_shelf() {
    let directory = tempfile::tempdir().unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    edit(&mut app, "first note");
    let first = app.active().view.id;
    app.execute(&ctx, Command::New);
    edit(&mut app, "second note");
    let second = app.active().view.id;
    app.execute(&ctx, Command::CloseAll);
    for id in [second, first] {
        assert!(app.close_flow.is_some());
        assert_eq!(app.active().view.id, id);
        app.choose_close(&ctx, Choice::Discard);
        assert!(app.workspace.closed.iter().any(|b| b.id == id));
    }
    assert!(app.close_flow.is_none());
    assert_eq!(app.workspace.open.len(), 1);
    assert!(app.active().page.text.is_empty());
    assert_eq!(app.workspace.closed.len(), 2);
}

#[test]
fn cancelling_pending_save_as_during_quit_still_preserves_the_session() {
    let directory = tempfile::tempdir().unwrap();
    let session = directory.path().join("session");
    let (workspace, _) = Workspace::restore(session.clone()).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    edit(&mut app, "pending save λ");
    // Deliver the native dialog's cancellation after the window-close request.
    app.busy = true;
    app.request_quit();
    app.tx.send(Ok(Action::SaveCancelled)).unwrap();
    app.receive(&ctx);
    assert!(
        app.closing,
        "a cancelled Save As must not cancel app shutdown"
    );
    assert!(app.close_flow.is_none());
    finish_quit(&mut app, &ctx);
    drop(app);
    let (restored, errors) = Workspace::restore(session).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(restored.open[restored.active].page.text, "pending save λ");
}

#[test]
fn save_as_formatting_failure_cancels_tab_close_and_keeps_the_note() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join(".editorconfig"),
        "root = true\n[*.txt]\ncharset = latin1\n",
    )
    .unwrap();
    let path = directory.path().join("note.txt");
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    edit(&mut app, "keep this λ");
    let id = app.active().view.id;
    app.execute(&ctx, Command::Close);
    app.tx.send(Ok(Action::SaveTo(id, path.clone()))).unwrap();
    app.receive(&ctx);
    assert!(app.error.is_some(), "Latin-1 cannot encode λ");
    assert!(!app.busy);
    assert!(!app.closing);
    assert!(app.close_flow.is_none());
    assert_eq!(app.active().view.id, id);
    assert_eq!(app.active().page.text, "keep this λ");
    assert!(!path.exists());
}

#[test]
fn dont_save_closes_untitled_tabs_when_the_shelf_is_full_without_losing_notes() {
    let directory = tempfile::tempdir().unwrap();
    let session = directory.path().join("session");
    let (workspace, _) = Workspace::restore(session.clone()).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    for n in 0..50 {
        edit(&mut app, &format!("shelved note {n}"));
        app.workspace.close(app.workspace.active).unwrap();
    }
    let original: Vec<_> = app.workspace.closed.iter().map(|v| v.id).collect();
    edit(&mut app, "first newly closed note λ");
    let first = app.active().view.id;
    app.execute(&ctx, Command::New);
    edit(&mut app, "second newly closed note λ");
    let second = app.active().view.id;
    app.execute(&ctx, Command::CloseAll);
    for _ in 0..2 {
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        click_label(&ctx, &mut app, "Don't Save");
        assert!(app.error.is_none(), "{:?}", app.error);
    }
    assert!(!app.closing);
    assert!(app.close_flow.is_none());
    assert_eq!(app.workspace.open.len(), 1);
    assert!(app.active().page.text.is_empty());
    assert_eq!(app.workspace.closed.len(), 52);
    for id in &original {
        assert!(app.workspace.closed.iter().any(|v| v.id == *id));
    }
    for id in [first, second] {
        assert!(app.workspace.closed.iter().any(|v| v.id == id));
        assert!(!app.workspace.open.iter().any(|b| b.view.id == id));
    }
    app.workspace.checkpoint().unwrap();
    drop(app);
    let (mut workspace, errors) = Workspace::restore(session).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(workspace.closed.len(), 52);
    for text in ["first newly closed note λ", "second newly closed note λ"] {
        let index = workspace
            .closed
            .iter()
            .position(|v| v.name == text)
            .unwrap();
        workspace.reopen_at(index).unwrap();
        assert_eq!(workspace.open[workspace.active].page.text, text);
    }
}
