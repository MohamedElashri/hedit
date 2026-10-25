use super::tests::{finish_app_job, frame, frame_size, key, text_rect};
use super::*;

#[test]
fn projects_reopen_clean_files_in_their_selected_encoding() {
    for (encoding, text) in [("windows-1252", "café €\n"), ("UTF-16LE", "café λ\n")] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("notes.txt");
        let project = directory.path().join("work.hedit-project");
        let bytes = hedit::text_encoding::encode(text, encoding, false).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let (mut ws, _) = Workspace::restore(directory.path().join("session")).unwrap();
        let mut doc = Document::open_with_encoding(ws.editor.id(), &path, encoding).unwrap();
        let page = doc.page(0).unwrap();
        ws.add(doc, page).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, ws, vec![], vec![]);
        app.quality.project_path = Some(project.clone());
        app.save_project(&ctx, false);
        finish_app_job(&mut app, &ctx);
        drop(app);

        let (ws, _) = Workspace::restore(directory.path().join("fresh-session")).unwrap();
        let mut app = HeditApp::from_context(&ctx, None, ws, vec![], vec![]);
        app.load_project(&ctx, Some(project));
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, text);
        assert_eq!(app.active().page.info.encoding, encoding);
        assert!(!app.active().page.info.dirty);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn advanced_find_replace_all_and_editorconfig_save_keep_backend_and_undo_together() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join(".editorconfig"),"root = true\n[*.py]\nindent_size = 2\ntrim_trailing_whitespace = true\ninsert_final_newline = true\nend_of_line = crlf\n").unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().join("session")).unwrap();
    ws.open[0].page.text = "λ_one λ_two  ".into();
    ws.edit(0, "").unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, ws, vec![], vec![]);
    app.query = r"λ_(\w+)".into();
    app.quality.options.regex = true;
    app.replacement = "α_${1}".into();
    app.replace_advanced(true);
    assert_eq!(app.active().page.text, "α_one α_two  ");
    assert!(app.quality.match_status.contains('2'));
    app.history(&ctx, false);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().page.text, "λ_one λ_two  ");
    let target = directory.path().join("first.py");
    app.save_to_path(&ctx, app.active().view.id, Some(target.clone()));
    finish_app_job(&mut app, &ctx);
    assert_eq!(std::fs::read_to_string(target).unwrap(), "λ_one λ_two\r\n");
    assert_eq!(app.active().page.text, "λ_one λ_two\r\n");
    assert_eq!(app.active().view.tab_size, Some(2));
    app.history(&ctx, false);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().page.text, "λ_one λ_two  ");
    assert!(app.active().page.info.dirty);
}

#[test]
fn projects_restore_unsaved_text_pin_cursor_and_split_and_preserve_other_work() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("src");
    std::fs::create_dir(&root).unwrap();
    let path = root.join("main.rs");
    std::fs::write(&path, "fn main() {}\n").unwrap();
    let project = directory.path().join("demo.hedit-project");
    let ctx = egui::Context::default();
    let (ws, _) = Workspace::restore(directory.path().join("first-session")).unwrap();
    let mut app = HeditApp::from_context(&ctx, None, ws, vec![], vec![]);
    app.open(&ctx, Some(root));
    finish_app_job(&mut app, &ctx);
    app.open(&ctx, Some(path));
    finish_app_job(&mut app, &ctx);
    let index = app.workspace.active;
    let old = app.active().page.text.clone();
    app.workspace.open[index].page.text = "fn main() { /* unsaved λ */ }\n".into();
    app.workspace.edit(index, &old).unwrap();
    app.workspace.open[index].view.head = 8;
    app.workspace.open[index].view.anchor = 8;
    app.workspace.open[index].view.pinned = true;
    app.split(1);
    frame(&ctx, &mut app, vec![]);
    app.quality.project_path = Some(project.clone());
    app.save_project(&ctx, false);
    finish_app_job(&mut app, &ctx);
    drop(app);
    let (mut ws, _) = Workspace::restore(directory.path().join("second-session")).unwrap();
    ws.open[0].page.text = "unrelated scratch".into();
    ws.edit(0, "").unwrap();
    let mut app = HeditApp::from_context(&ctx, None, ws, vec![], vec![]);
    app.load_project(&ctx, Some(project));
    finish_app_job(&mut app, &ctx);
    assert!(app.active().page.text.contains("unsaved λ"));
    assert!(app.active().view.pinned);
    assert_eq!(app.active().view.head, 8);
    assert_eq!(app.panes.mode, 1);
    assert_eq!(app.workspace.folders.len(), 1);
    let closed = app
        .workspace
        .closed
        .iter()
        .find(|v| v.name.contains("unrelated"))
        .unwrap();
    assert_eq!(
        app.workspace
            .editor
            .get(closed.id)
            .unwrap()
            .lock()
            .unwrap()
            .text(1024)
            .unwrap(),
        "unrelated scratch"
    );
}

#[test]
fn previews_replace_only_clean_unpinned_tabs_and_group_close_keeps_pins() {
    let directory = tempfile::tempdir().unwrap();
    let (ws, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, ws, vec![], vec![]);
    for name in ["first.txt", "second.txt", "third.txt"] {
        std::fs::write(directory.path().join(name), name).unwrap();
    }
    app.quality.preview_request = true;
    app.open(&ctx, Some(directory.path().join("first.txt")));
    finish_app_job(&mut app, &ctx);
    let first = app.active().view.id;
    assert_eq!(app.quality.preview, Some(first));
    app.quality.preview_request = true;
    app.open(&ctx, Some(directory.path().join("second.txt")));
    finish_app_job(&mut app, &ctx);
    assert!(!app.workspace.open.iter().any(|b| b.view.id == first));
    app.execute(&ctx, Command::PinTab);
    let pinned = app.active().view.id;
    app.quality.preview_request = true;
    app.open(&ctx, Some(directory.path().join("third.txt")));
    finish_app_job(&mut app, &ctx);
    app.execute(&ctx, Command::CloseOthers);
    assert_eq!(app.workspace.open.len(), 2);
    assert!(app.workspace.open.iter().any(|b| b.view.id == pinned));
}

#[test]
fn keybindings_override_defaults_and_snippet_triggers_use_normal_undo() {
    let directory = tempfile::tempdir().unwrap();
    let (ws, _) = Workspace::restore(directory.path().into()).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, ws, vec![], vec![]);
    app.quality.config.keybindings.push(quality::Binding {
        command: Command::New,
        shortcut: "Primary+Shift+N".into(),
        editor_only: false,
    });
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::N, Modifiers::COMMAND)]);
    assert_eq!(app.workspace.open.len(), 1);
    frame(
        &ctx,
        &mut app,
        vec![key(
            Key::N,
            Modifiers {
                shift: true,
                ..Modifiers::COMMAND
            },
        )],
    );
    assert_eq!(app.workspace.open.len(), 2);
    let index = app.workspace.active;
    app.workspace.open[index].page.text = "todo".into();
    app.workspace.edit(index, "").unwrap();
    app.workspace.open[index].view.anchor = 4;
    app.workspace.open[index].view.head = 4;
    assert!(app.expand_trigger());
    assert_eq!(app.active().page.text, "TODO: ");
    app.history(&ctx, false);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().page.text, "todo");
}

#[test]
fn rename_retargets_open_and_closed_buffers_and_retains_external_save_guard() {
    let directory = tempfile::tempdir().unwrap();
    let old = directory.path().join("old.txt");
    std::fs::write(&old, "original").unwrap();
    let (ws, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, ws, vec![], vec![]);
    app.open(&ctx, Some(old.clone()));
    finish_app_job(&mut app, &ctx);
    std::fs::write(&old, "external version").unwrap();
    app.begin_file_action(old, quality::FileOperation::Rename);
    app.quality.file_name = "renamed.txt".into();
    app.perform_file_action(&ctx);
    finish_app_job(&mut app, &ctx);
    assert!(app
        .active()
        .page
        .info
        .path
        .as_ref()
        .unwrap()
        .ends_with("renamed.txt"));
    let shared = app.workspace.editor.get(app.active().view.id).unwrap();
    assert!(shared.lock().unwrap().save(None).is_err());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("renamed.txt")).unwrap(),
        "external version"
    );
}

#[cfg(unix)]
#[test]
fn rename_through_symlinked_parent_retargets_folder_and_open_and_closed_buffers() {
    use std::path::Path;

    let directory = tempfile::tempdir().unwrap();
    let real = directory.path().join("real");
    let alias = directory.path().join("alias");
    let root = real.join("src");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("open.txt"), "open text").unwrap();
    std::fs::write(root.join("closed.txt"), "closed text").unwrap();
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    let (ws, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, ws, vec![], vec![]);
    app.open(&ctx, Some(alias.join("src")));
    finish_app_job(&mut app, &ctx);
    app.open(&ctx, Some(alias.join("src/closed.txt")));
    finish_app_job(&mut app, &ctx);
    let closed_id = app.active().view.id;
    app.workspace.close(app.workspace.active).unwrap();
    app.open(&ctx, Some(alias.join("src/open.txt")));
    finish_app_job(&mut app, &ctx);
    app.begin_file_action(alias.join("src"), quality::FileOperation::Rename);
    app.quality.file_name = "renamed".into();
    app.perform_file_action(&ctx);
    finish_app_job(&mut app, &ctx);

    let renamed = real.join("renamed").canonicalize().unwrap();
    assert_eq!(
        app.active().page.info.path.as_deref().map(Path::new),
        Some(renamed.join("open.txt").as_path())
    );
    assert_eq!(Path::new(&app.workspace.folders[0].path), renamed);
    assert!(app.workspace.closed.iter().any(|view| view.id == closed_id));
    let closed = app.workspace.editor.get(closed_id).unwrap();
    assert_eq!(
        closed.lock().unwrap().info().path.as_deref().map(Path::new),
        Some(renamed.join("closed.txt").as_path())
    );
    assert!(!root.exists());
}

#[test]
fn project_replacement_edits_open_buffers_unsaved_and_keeps_newer_text() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("src");
    std::fs::create_dir(&root).unwrap();
    let path = root.join("file.txt");
    std::fs::write(&path, "needle\n").unwrap();
    let (ws, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, ws, vec![], vec![]);
    app.open(&ctx, Some(root));
    finish_app_job(&mut app, &ctx);
    app.open(&ctx, Some(path.clone()));
    finish_app_job(&mut app, &ctx);
    app.query = "needle".into();
    app.replacement = "changed".into();
    app.folder_search(&ctx, true);
    finish_app_job(&mut app, &ctx);
    app.apply_folder_replacements(&ctx);
    finish_app_job(&mut app, &ctx);
    // The open file changes in the editor and waits to be saved.
    assert_eq!(app.active().page.text, "changed\n");
    assert!(app.active().page.info.dirty);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "needle\n");
    app.query = "changed".into();
    app.replacement = "disk edit".into();
    app.folder_search(&ctx, true);
    finish_app_job(&mut app, &ctx);
    // Typing after the preview keeps the newer text.
    let index = app.workspace.active;
    app.workspace.open[index].page.text = "my unsaved text".into();
    app.workspace.edit(index, "changed\n").unwrap();
    app.apply_folder_replacements(&ctx);
    while app.busy {
        app.receive(&ctx);
        thread::sleep(Duration::from_millis(5));
    }
    assert!(app
        .error
        .as_ref()
        .unwrap()
        .contains("changed since the preview"));
    assert_eq!(app.active().page.text, "my unsaved text");
    assert_eq!(std::fs::read_to_string(path).unwrap(), "needle\n");
}

#[test]
fn quality_dialogs_render_at_minimum_window_size_without_editing_the_buffer() {
    for command in [
        Command::FindFiles,
        Command::KeyBindings,
        Command::ProjectSettings,
        Command::Encoding,
        Command::Complete,
        Command::Snippets,
        Command::RecentProjects,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let (mut ws, _) = Workspace::restore(directory.path().into()).unwrap();
        ws.open[0].page.text = "keep this λ".into();
        ws.edit(0, "").unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, ws, vec![], vec![]);
        app.execute(&ctx, command);
        for _ in 0..3 {
            frame_size(&ctx, &mut app, vec![], egui::vec2(720.0, 480.0));
        }
        if command == Command::KeyBindings {
            // Key bindings are a Settings page now; its footer stays reachable.
            assert!(app.preferences_open);
            let output = frame_size(&ctx, &mut app, vec![], egui::vec2(720.0, 480.0));
            assert!(output.shapes.iter().any(|shape| text_rect(
                &shape.shape,
                "Open settings.json"
            )
            .is_some()));
        }
        if command == Command::ProjectSettings {
            let output = frame_size(&ctx, &mut app, vec![], egui::vec2(720.0, 480.0));
            assert!(
                output
                    .shapes
                    .iter()
                    .any(
                        |shape| text_rect(&shape.shape, "Save settings")
                            .is_some_and(
                                |rect| shape.clip_rect.contains_rect(rect) && rect.bottom() < 480.0
                            )
                    ),
                "Save settings must remain visible at minimum size"
            );
        }
        assert_eq!(app.active().page.text, "keep this λ");
        frame_size(
            &ctx,
            &mut app,
            vec![key(Key::Escape, Modifiers::NONE)],
            egui::vec2(720.0, 480.0),
        );
    }
}

#[test]
fn dragging_tabs_reorders_them_without_changing_the_active_document() {
    let directory = tempfile::tempdir().unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().into()).unwrap();
    ws.name_scratchpad(0, "Alpha tab").unwrap();
    ws.new_buffer().unwrap();
    ws.name_scratchpad(1, "Beta tab").unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, ws, vec![], vec![]);
    frame(&ctx, &mut app, vec![]);
    let output = frame(&ctx, &mut app, vec![]);
    let position = |label| {
        output
            .shapes
            .iter()
            .find_map(|s| text_rect(&s.shape, label))
            .unwrap()
            .center()
    };
    let source = position("Alpha tab");
    let target = position("Beta tab");
    let active = app.active().view.id;
    frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerMoved(source),
            egui::Event::PointerButton {
                pos: source,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    frame(&ctx, &mut app, vec![egui::Event::PointerMoved(target)]);
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::PointerButton {
            pos: target,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert_eq!(app.workspace.open[0].view.name, "Beta tab");
    assert_eq!(app.active().view.id, active);
}

#[test]
#[ignore = "writes quality dialog SVG previews to HEDIT_PREVIEW_DIR"]
fn export_quality_previews() {
    let output = PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    for (name, command) in [
        ("search", Command::FindFiles),
        ("settings", Command::KeyBindings),
        ("encoding", Command::Encoding),
        ("snippets", Command::Snippets),
    ] {
        for size in [egui::vec2(1280.0, 820.0), egui::vec2(720.0, 480.0)] {
            let directory = tempfile::tempdir().unwrap();
            let (mut ws, _) = Workspace::restore(directory.path().into()).unwrap();
            ws.open[0].page.text = "fn main() {\n    // workspace notes λ\n}\n".into();
            ws.edit(0, "").unwrap();
            let ctx = egui::Context::default();
            let mut app = HeditApp::from_context(&ctx, None, ws, vec![], vec![]);
            app.query = "workspace".into();
            app.execute(&ctx, command);
            for _ in 0..3 {
                frame_size(&ctx, &mut app, vec![], size);
            }
            let rendered = frame_size(&ctx, &mut app, vec![], size);
            let svg = crate::preview::export(
                &rendered.shapes,
                [size.x, size.y],
                app.theme.background.color(),
            );
            std::fs::write(output.join(format!("quality-{name}-{}.svg", size.x)), svg).unwrap();
        }
    }
}

#[test]
fn keyboard_completion_selects_a_word_without_inserting_a_newline() {
    let directory = tempfile::tempdir().unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().into()).unwrap();
    let text = "alpha alpine al";
    ws.open[0].page.text = text.into();
    ws.edit(0, "").unwrap();
    ws.open[0].view.anchor = text.chars().count();
    ws.open[0].view.head = text.chars().count();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, ws, vec![], vec![]);
    frame(&ctx, &mut app, vec![]);
    app.open_completion(&ctx);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::ArrowDown, Modifiers::NONE)]);
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    assert_eq!(app.active().page.text, "alpha alpine alpine");
    assert!(app.auto.popup.is_none());
    app.history(&ctx, false);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().page.text, text);
}

#[test]
fn zero_width_find_starts_at_the_cursor_and_navigation_replays_unicode_offsets() {
    let directory = tempfile::tempdir().unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().join("session")).unwrap();
    ws.open[0].page.text = "α\nβ\n".into();
    ws.edit(0, "").unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, ws, vec![], vec![]);
    let first = app.active().view.id;
    app.query = "^".into();
    app.quality.options.regex = true;
    app.find(&ctx);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().view.head, 0);
    app.find(&ctx);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().view.head, 2);
    let path = directory.path().join("second.txt");
    std::fs::write(&path, "second").unwrap();
    app.open(&ctx, Some(path));
    finish_app_job(&mut app, &ctx);
    let second = app.active().view.id;
    app.navigate(&ctx, false);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().view.id, first);
    assert_eq!(app.active().view.head, 2);
    app.navigate(&ctx, true);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().view.id, second);
}
