//! Sublime-style typing aids: bracket pairing, indentation detection, syntax.
use super::tests::{finish_app_job, frame, key};
use super::*;

fn app_with(text: &str) -> (tempfile::TempDir, egui::Context, HeditApp) {
    let directory = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
    workspace.open[0].page.text = text.into();
    workspace.edit(0, "").unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.focus_editor = true;
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![]);
    (directory, ctx, app)
}

fn type_text(ctx: &egui::Context, app: &mut HeditApp, text: &str) {
    for c in text.chars() {
        frame(ctx, app, vec![egui::Event::Text(c.into())]);
    }
}

#[test]
fn typing_pairs_brackets_and_backspace_removes_both() {
    let (_directory, ctx, mut app) = app_with("");
    app.set_syntax(Some(syntax::Language::Python));
    type_text(&ctx, &mut app, "f(\"x");
    assert_eq!(app.active().page.text, "f(\"x\")");
    type_text(&ctx, &mut app, "\")");
    assert_eq!(
        app.active().page.text,
        "f(\"x\")",
        "closers are stepped over"
    );
    assert_eq!(app.active().view.head, 6);
    type_text(&ctx, &mut app, "[");
    assert_eq!(app.active().page.text, "f(\"x\")[]");
    frame(&ctx, &mut app, vec![key(Key::Backspace, Modifiers::NONE)]);
    assert_eq!(app.active().page.text, "f(\"x\")");
    // Turning the setting off types characters as they are.
    app.settings.auto_match_enabled = false;
    type_text(&ctx, &mut app, "(");
    assert_eq!(app.active().page.text, "f(\"x\")(");
}

#[test]
fn opening_a_file_uses_its_own_indentation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tabs.c");
    std::fs::write(&path, "int main() {\n\treturn 0;\n}\n").unwrap();
    let spaces = directory.path().join("two.py");
    std::fs::write(&spaces, "def f():\n  if x:\n    pass\n").unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.settings.format.translate_tabs_to_spaces = Some(true);
    app.settings.format.tab_size = Some(4);
    app.open(&ctx, Some(path));
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().view.indent.as_deref(), Some("tabs"));
    app.open(&ctx, Some(spaces));
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().view.indent.as_deref(), Some("spaces"));
    assert_eq!(app.active().view.tab_size, Some(2));
    // Without detection, the settings apply.
    app.settings.detect_indentation = false;
    app.apply_format_preferences();
    assert_eq!(app.active().view.tab_size, Some(4));
    // New files use the settings.
    app.settings.detect_indentation = true;
    app.settings.format.tab_size = Some(3);
    app.execute(&ctx, Command::New);
    assert_eq!(app.active().view.tab_size, Some(3));
    assert_eq!(app.active().view.indent.as_deref(), Some("spaces"));
}

#[test]
fn set_syntax_from_the_palette_applies_its_formatting() {
    let (_directory, ctx, mut app) = app_with("x");
    app.settings.syntax_specific.insert(
        "makefile".into(),
        hedit::settings::FormatSettings {
            translate_tabs_to_spaces: Some(false),
            ..Default::default()
        },
    );
    app.execute(&ctx, Command::Palette);
    frame(&ctx, &mut app, vec![]);
    frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::Text("set syntax: makefile".into()),
            key(Key::Enter, Modifiers::NONE),
        ],
    );
    assert_eq!(app.active().language(), syntax::Language::Makefile);
    assert_eq!(app.active().view.indent.as_deref(), Some("tabs"));
}

#[test]
fn display_settings_change_cursor_and_line_spacing() {
    let text = (1..=40).map(|n| format!("line {n}\n")).collect::<String>();
    let (_directory, ctx, mut app) = app_with(&text);
    app.settings.caret_width = 3.0;
    app.settings.caret_blink = false;
    frame(&ctx, &mut app, vec![]);
    let cursor = ctx.style().visuals.text_cursor.clone();
    assert_eq!(cursor.stroke.width, 3.0);
    assert!(!cursor.blink);
    // Line spacing makes each laid-out row taller.
    frame(&ctx, &mut app, vec![]);
    let tight = app.editor_layout.row_height().unwrap();
    app.settings.line_padding = 6.0;
    frame(&ctx, &mut app, vec![]);
    let loose = app.editor_layout.row_height().unwrap();
    assert!(loose >= tight + 5.0, "{tight} → {loose}");
}

#[test]
fn tab_keeps_focus_in_a_long_file() {
    let text = (1..=200).map(|n| format!("line {n}\n")).collect::<String>();
    let (_directory, ctx, mut app) = app_with(&text);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::Tab, Modifiers::NONE)]);
    frame(&ctx, &mut app, vec![key(Key::Tab, Modifiers::NONE)]);
    assert!(
        app.active().page.text.starts_with("        line 1"),
        "{:?}",
        &app.active().page.text[..20]
    );
}

#[test]
fn quit_and_zoom_shortcuts_follow_the_platform() {
    let (_directory, ctx, mut app) = app_with("text");
    let zoom = ctx.zoom_factor();
    frame(&ctx, &mut app, vec![key(Key::Plus, Modifiers::COMMAND)]);
    frame(&ctx, &mut app, vec![key(Key::Equals, Modifiers::COMMAND)]);
    assert_eq!(app.settings.font_size, 16.0);
    assert_eq!(ctx.zoom_factor(), zoom, "the interface does not zoom");
    frame(&ctx, &mut app, vec![key(Key::Num0, Modifiers::COMMAND)]);
    assert_eq!(app.settings.font_size, 14.0);
    assert_eq!(ctx.zoom_factor(), zoom);
    assert_eq!(
        Command::Quit.shortcut(),
        if cfg!(target_os = "macos") {
            "Cmd+Q"
        } else {
            "Ctrl+Q"
        }
    );
    assert_eq!(
        Command::Replace.shortcut(),
        if cfg!(target_os = "macos") {
            "Cmd+Option+F"
        } else {
            "Ctrl+H"
        }
    );
    frame(&ctx, &mut app, vec![key(Key::Q, Modifiers::COMMAND)]);
    assert!(
        app.closing,
        "Ctrl/Cmd+Q quits through the recovery checkpoint"
    );
}

#[test]
fn selection_shortcuts_follow_sublime() {
    let (_directory, ctx, mut app) = app_with("call(one, two)\nnext line\n");
    let shift = Modifiers {
        shift: true,
        ..Modifiers::COMMAND
    };
    let ranges = |app: &HeditApp| -> Vec<(usize, usize)> {
        app.selections()
            .iter()
            .map(|s| (s.start(), s.end()))
            .collect()
    };
    app.set_selections(vec![hedit::advanced::Selection::caret(6)]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::Space, shift)]);
    assert_eq!(
        ranges(&app),
        [(5, 8)],
        "Ctrl+Shift+Space expands to the word"
    );
    assert!(app.auto.popup.is_none(), "not Complete word");
    frame(&ctx, &mut app, vec![key(Key::M, shift)]);
    assert_eq!(
        ranges(&app),
        [(5, 13)],
        "Ctrl+Shift+M selects inside brackets"
    );
    // Ctrl+U steps back through selection changes.
    frame(&ctx, &mut app, vec![key(Key::U, Modifiers::COMMAND)]);
    assert_eq!(ranges(&app), [(5, 8)]);
    frame(&ctx, &mut app, vec![key(Key::U, Modifiers::COMMAND)]);
    assert_eq!(ranges(&app), [(6, 6)]);
    frame(&ctx, &mut app, vec![key(Key::U, shift)]);
    assert_eq!(ranges(&app), [(5, 8)], "Ctrl+Shift+U steps forward");
    // Ctrl+M jumps between brackets.
    app.set_selections(vec![hedit::advanced::Selection::caret(5)]);
    // Control+M on macOS, where Cmd+M minimizes the window.
    let jump = if cfg!(target_os = "macos") {
        Modifiers::CTRL
    } else {
        Modifiers::COMMAND
    };
    frame(&ctx, &mut app, vec![key(Key::M, jump)]);
    assert_eq!(ranges(&app), [(13, 13)]);
    // Ctrl+Shift+L splits a selection into one cursor per line.
    app.set_selections(vec![hedit::advanced::Selection {
        anchor: 0,
        head: 19,
    }]);
    frame(&ctx, &mut app, vec![key(Key::L, shift)]);
    assert_eq!(ranges(&app), [(0, 14), (15, 19)]);
    // Alt+F3 selects every occurrence.
    app.set_selections(vec![hedit::advanced::Selection {
        anchor: 15,
        head: 19,
    }]);
    frame(&ctx, &mut app, vec![key(Key::F3, Modifiers::ALT)]);
    assert_eq!(ranges(&app), [(15, 19)]);
    app.set_selections(vec![hedit::advanced::Selection {
        anchor: 10,
        head: 11,
    }]);
    frame(&ctx, &mut app, vec![key(Key::F3, Modifiers::ALT)]);
    assert_eq!(ranges(&app), [(10, 11), (18, 19)]);
    assert_eq!(app.active().page.text, "call(one, two)\nnext line\n");
}

#[test]
fn two_key_shortcuts_run_commands_and_can_be_recorded() {
    let (_directory, ctx, mut app) = app_with("abc def\nsecond\n");
    // Press and release, as a keyboard does; a held key repeats instead.
    let ctrl = |key| {
        let press = super::tests::key(key, Modifiers::COMMAND);
        let egui::Event::Key { key, modifiers, .. } = press.clone() else {
            unreachable!()
        };
        vec![
            press,
            egui::Event::Key {
                key,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers,
            },
        ]
    };
    app.set_selections(vec![hedit::advanced::Selection { anchor: 0, head: 3 }]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, ctrl(Key::K));
    assert!(app.chord.is_some());
    assert!(app.notice.contains("Waiting for the second key"));
    frame(&ctx, &mut app, ctrl(Key::U));
    assert_eq!(app.active().page.text, "ABC def\nsecond\n");
    assert!(app.chord.is_none());
    finish_app_job(&mut app, &ctx);
    // Escape cancels, and an unknown second key is reported, not typed.
    frame(&ctx, &mut app, ctrl(Key::K));
    frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
    assert!(app.chord.is_none());
    assert_eq!(app.notice, "Shortcut cancelled.");
    frame(&ctx, &mut app, ctrl(Key::K));
    frame(&ctx, &mut app, ctrl(Key::X));
    assert!(app.notice.contains("not followed by that key"));
    assert_eq!(app.active().page.text, "ABC def\nsecond\n");
    // Ctrl+Shift+K is not mistaken for the start of a chord.
    frame(&ctx, &mut app, vec![]);
    frame(
        &ctx,
        &mut app,
        vec![key(
            Key::K,
            Modifiers {
                shift: true,
                ..Modifiers::COMMAND
            },
        )],
    );
    assert!(app.chord.is_none());
    assert_eq!(app.active().page.text, "second\n");
    let release = egui::Event::Key {
        key: Key::K,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    frame(&ctx, &mut app, vec![release]);
    // Recording: a second key soon after the first makes a chord.
    app.open_preferences(PreferencesPage::Keys);
    app.key_capture = Some(Command::Goto);
    frame(&ctx, &mut app, ctrl(Key::K));
    frame(&ctx, &mut app, ctrl(Key::G));
    assert_eq!(app.key_capture, None);
    let primary = if cfg!(target_os = "macos") {
        "Cmd"
    } else {
        "Ctrl"
    };
    assert_eq!(
        app.shortcut_text(Command::Goto),
        format!("{primary}+K, {primary}+G")
    );
    app.preferences_open = false;
    frame(&ctx, &mut app, ctrl(Key::K));
    frame(&ctx, &mut app, ctrl(Key::G));
    assert!(app.goto_open, "the recorded chord runs Go to line");
}

#[test]
fn ctrl_b_saves_builds_and_f4_visits_errors() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("tool.py");
    std::fs::write(&file, "line one\nline two\nline three\n").unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.settings.build_systems = vec![hedit::build::BuildSystem {
        name: "Check".into(),
        shell_cmd: r#"cat "$file_name" > /dev/null; echo "$file_name:2:4: warning: two"; echo "$file_name:3:1: error: three"; exit 2"#.into(),
        file_regex: r"^(..[^:\n]*):(\d+):(\d+): (.*)$".into(),
        selector: vec!["python".into()],
        ..Default::default()
    }];
    app.open(&ctx, Some(file.clone()));
    finish_app_job(&mut app, &ctx);
    // An unsaved change is saved before building.
    let index = app.workspace.active;
    let previous = std::mem::replace(
        &mut app.workspace.open[index].page.text,
        "line one\nline two!\nline three\n".into(),
    );
    app.workspace.edit(index, &previous).unwrap();
    app.execute(&ctx, Command::Build);
    finish_app_job(&mut app, &ctx);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "line one\nline two!\nline three\n"
    );
    let started = Instant::now();
    while app.build.running() {
        assert!(started.elapsed() < Duration::from_secs(10));
        thread::sleep(Duration::from_millis(10));
        app.poll_build();
    }
    assert!(app.build.open);
    assert!(
        app.build.output.contains("tool.py:2:4: warning: two"),
        "{:?} {:?} {:?} pending",
        app.build.output,
        app.build.status,
        app.error
    );
    assert!(app.build.output.ends_with("with exit code 2]"));
    assert_eq!(app.build.locations.len(), 2);
    assert_eq!(
        app.build.locations[0].file,
        std::fs::canonicalize(directory.path().join("tool.py")).unwrap()
    );
    // F4 goes to each location in turn.
    frame(&ctx, &mut app, vec![key(Key::F4, Modifiers::NONE)]);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.build.current, Some(0));
    assert_eq!(app.notice, "warning: two");
    let b = app.active();
    let line = b
        .page
        .text
        .chars()
        .take(b.view.head)
        .filter(|&c| c == '\n')
        .count()
        + 1;
    assert_eq!(line, 2);
    frame(&ctx, &mut app, vec![key(Key::F4, Modifiers::SHIFT)]);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.build.current, Some(1));
    // Escape hides the panel.
    frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
    assert!(!app.build.open);
}

#[test]
fn layouts_have_up_to_four_groups_and_ctrl_n_focuses_one() {
    let (_directory, ctx, mut app) = app_with("first file");
    let both = Modifiers {
        alt: true,
        shift: true,
        ..Modifiers::NONE
    };
    for (key_number, mode, groups) in [
        (Key::Num3, 3, 3),
        (Key::Num4, 4, 4),
        (Key::Num5, 6, 4),
        (Key::Num9, 5, 3),
        (Key::Num8, 2, 2),
        (Key::Num1, 0, 1),
    ] {
        if cfg!(target_os = "macos") {
            break; // macOS uses Cmd+Option+N, checked by the command labels.
        }
        frame(&ctx, &mut app, vec![key(key_number, both)]);
        assert_eq!(app.panes.mode, mode, "{key_number:?}");
        assert_eq!(app.panes.groups(), groups);
    }
    // Three columns, each group showing its own file.
    app.execute(&ctx, Command::Columns3);
    app.execute(&ctx, Command::New);
    frame(&ctx, &mut app, vec![]);
    let second = app.active().page.info.id;
    // Ctrl+N focuses a group; Control+N on macOS, where Cmd+N is New file.
    let focus = if cfg!(target_os = "macos") {
        Modifiers::CTRL
    } else {
        Modifiers::COMMAND
    };
    frame(&ctx, &mut app, vec![key(Key::Num3, focus)]);
    frame(&ctx, &mut app, vec![]);
    assert_eq!(app.panes.focused, 2);
    frame(&ctx, &mut app, vec![key(Key::Num2, focus)]);
    frame(&ctx, &mut app, vec![]);
    assert_eq!(app.panes.focused, 1);
    let _ = second;
    // A layout saved by the two-group version still loads.
    let mut old: panes::Panes =
        serde_json::from_str(r#"{"mode":2,"focused":1,"views":[{},{}]}"#).unwrap();
    old.normalize();
    assert_eq!((old.mode, old.focused, old.groups()), (2, 1, 2));
    let mut bad: panes::Panes = serde_json::from_str(r#"{"mode":40,"focused":9}"#).unwrap();
    bad.normalize();
    assert_eq!((bad.mode, bad.focused), (0, 0));
}

#[test]
fn hedit_w_waits_until_the_files_are_closed() {
    assert!(
        hedit::cli::Options::parse(["-w".into(), "a.txt".into()])
            .unwrap()
            .wait
    );
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("COMMIT_EDITMSG");
    std::fs::write(&file, "message\n").unwrap();
    let closed = directory.path().join("closed");
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![file.clone()]);
    app.wait_for(
        &[PathBuf::from(format!("{}:1", file.display()))],
        closed.clone(),
    );
    finish_app_job(&mut app, &ctx);
    app.track_waits();
    assert!(!closed.exists(), "still open");
    app.execute(&ctx, Command::Close);
    app.track_waits();
    assert!(closed.exists(), "released once the file is closed");
    // A request from another `hedit -w` waits the same way.
    let other = directory.path().join("other.txt");
    std::fs::write(&other, "x").unwrap();
    let released = directory.path().join("released");
    let requests = directory.path().join("session/requests");
    std::fs::create_dir_all(&requests).unwrap();
    let request = hedit::ipc::Request {
        files: vec![hedit::ipc::WirePath::new(&other)],
        stdin: None,
        new_buffer: false,
        wait: Some(hedit::ipc::WirePath::new(&released)),
    };
    std::fs::write(
        requests.join("1.request"),
        serde_json::to_vec(&request).unwrap(),
    )
    .unwrap();
    app.poll_incoming(&ctx);
    finish_app_job(&mut app, &ctx);
    app.track_waits();
    assert!(!released.exists());
    // Closing the window releases every wait.
    app.release_waits();
    assert!(released.exists());
}

#[test]
fn an_empty_extra_window_removes_its_session_on_close() {
    let data = tempfile::tempdir().unwrap();
    for (text, kept) in [("", false), ("draft", true)] {
        let session = hedit::windows::new_session(&data.path().join("session"));
        let (mut workspace, _) = Workspace::restore(session.clone()).unwrap();
        workspace.open[0].page.text = text.into();
        workspace.edit(0, "").unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        eframe::App::on_exit(&mut app, None);
        drop(app);
        assert_eq!(session.exists(), kept, "{text:?}");
    }
    // The main window's session is never removed.
    let main = data.path().join("session");
    let (workspace, _) = Workspace::restore(main.clone()).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    eframe::App::on_exit(&mut app, None);
    drop(app);
    assert!(main.exists());
}

#[test]
fn add_cursor_below_keeps_its_column_and_types_on_every_line() {
    let (_directory, ctx, mut app) = app_with("abcdef\nab\nabcdef\n");
    app.set_selections(vec![hedit::advanced::Selection::caret(4)]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![]);
    let add = if cfg!(target_os = "macos") {
        Modifiers::CTRL | Modifiers::SHIFT
    } else {
        Modifiers::ALT | Modifiers::SHIFT
    };
    frame(&ctx, &mut app, vec![key(Key::ArrowDown, add)]);
    frame(&ctx, &mut app, vec![key(Key::ArrowDown, add)]);
    type_text(&ctx, &mut app, "|");
    assert_eq!(app.active().page.text, "abcd|ef\nab|\nabcd|ef\n");
    // Above, from a single cursor, the new cursor becomes the one in view.
    frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
    app.execute(&ctx, Command::AddCursorAbove);
    assert_eq!(app.selections().len(), 2);
    assert!(app.active().view.head < app.active().view.selections[0].head);
}

#[test]
fn repeated_arrow_shortcuts_keep_the_editor_focused() {
    let (_directory, ctx, mut app) = app_with("one\ntwo\nthree\nfour\n");
    app.set_selections(vec![hedit::advanced::Selection::caret(0)]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![]);
    let shift = Modifiers {
        shift: true,
        ..Modifiers::COMMAND
    };
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![key(Key::ArrowDown, shift)]);
    }
    assert_eq!(app.active().page.text, "two\nthree\nfour\none\n");
}

#[test]
fn text_command_shortcuts_edit_the_file_as_one_undo_step() {
    let (_directory, ctx, mut app) = app_with("int a = 1;\nab\n");
    app.set_syntax(Some(syntax::Language::C));
    app.set_selections(vec![hedit::advanced::Selection { anchor: 4, head: 9 }]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![]);
    if cfg!(target_os = "macos") {
        app.execute(&ctx, Command::ToggleBlockComment);
    } else {
        // Ctrl+Shift+/ arrives as ?, and must not toggle the line comment.
        frame(
            &ctx,
            &mut app,
            vec![key(
                Key::Questionmark,
                Modifiers::COMMAND | Modifiers::SHIFT,
            )],
        );
    }
    assert_eq!(app.active().page.text, "int /*a = 1*/;\nab\n");
    app.execute(&ctx, Command::Undo);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().page.text, "int a = 1;\nab\n");
    // Ctrl+T swaps the letters around the cursor.
    app.set_selections(vec![hedit::advanced::Selection::caret(12)]);
    frame(&ctx, &mut app, vec![]);
    let transpose = if cfg!(target_os = "macos") {
        Modifiers::CTRL
    } else {
        Modifiers::COMMAND
    };
    frame(&ctx, &mut app, vec![key(Key::T, transpose)]);
    assert_eq!(app.active().page.text, "int a = 1;\nba\n");
    // Ctrl+K, Ctrl+K deletes to the end of the line.
    app.set_selections(vec![hedit::advanced::Selection::caret(3)]);
    frame(&ctx, &mut app, vec![]);
    let release = egui::Event::Key {
        key: Key::K,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    };
    frame(
        &ctx,
        &mut app,
        vec![key(Key::K, Modifiers::COMMAND), release.clone()],
    );
    frame(
        &ctx,
        &mut app,
        vec![key(Key::K, Modifiers::COMMAND), release],
    );
    assert_eq!(app.active().page.text, "int\nba\n");
    // Alt+Q wraps without typing q.
    app.settings.wrap_width = 2;
    app.set_selections(vec![hedit::advanced::Selection::caret(0)]);
    frame(&ctx, &mut app, vec![]);
    if cfg!(target_os = "macos") {
        app.execute(&ctx, Command::WrapLines);
    } else {
        frame(
            &ctx,
            &mut app,
            vec![key(Key::Q, Modifiers::ALT), egui::Event::Text("q".into())],
        );
    }
    assert_eq!(app.active().page.text, "int\nba\n");
    assert!(app.command_enabled(Command::Reindent));
    app.set_syntax(Some(syntax::Language::Python));
    assert!(!app.command_enabled(Command::Reindent));
}

#[test]
fn paste_history_lists_copies_and_pastes_at_every_cursor() {
    let (_directory, ctx, mut app) = app_with("alpha beta\n\n");
    assert!(!app.command_enabled(Command::PasteHistory));
    for range in [(0, 5), (6, 10), (0, 5)] {
        app.set_selections(vec![hedit::advanced::Selection {
            anchor: range.0,
            head: range.1,
        }]);
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![egui::Event::Copy]);
    }
    assert_eq!(
        app.paste_history,
        ["alpha", "beta"],
        "newest first, no repeats"
    );
    app.set_selections(vec![
        hedit::advanced::Selection::caret(11),
        hedit::advanced::Selection::caret(12),
    ]);
    frame(&ctx, &mut app, vec![]);
    app.execute(&ctx, Command::PasteHistory);
    assert!(app.palette && app.pick_list.is_some());
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![egui::Event::Text("bet".into())]);
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    assert!(!app.palette);
    assert_eq!(app.active().page.text, "alpha beta\nbeta\nbeta");
    assert_eq!(app.paste_history[0], "beta", "the pasted entry is newest");
    // The next palette is the ordinary one again.
    app.execute(&ctx, Command::Palette);
    frame(&ctx, &mut app, vec![]);
    assert!(app.pick_list.is_none());
}

#[test]
fn tab_moves_through_snippet_fields_and_mirrors_follow_typing() {
    let (_directory, ctx, mut app) = app_with("    fori");
    app.set_syntax(Some(syntax::Language::C));
    app.workspace.open[0].view.indent = Some("spaces".into());
    app.quality.config.snippets = vec![hedit::intelligence::Snippet {
        trigger: "fori".into(),
        description: "for loop".into(),
        body: "for (${1:i} = 0; $1 < ${2:n}; $1++) {\n\t$0\n}".into(),
        language: None,
    }];
    app.set_selections(vec![hedit::advanced::Selection::caret(8)]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::Tab, Modifiers::NONE)]);
    assert_eq!(
        app.active().page.text,
        "    for (i = 0; i < n; i++) {\n        \n    }"
    );
    assert_eq!(app.selections().len(), 3, "field 1 and its mirrors");
    type_text(&ctx, &mut app, "k");
    assert_eq!(
        app.active().page.text,
        "    for (k = 0; k < n; k++) {\n        \n    }"
    );
    frame(&ctx, &mut app, vec![key(Key::Tab, Modifiers::NONE)]);
    type_text(&ctx, &mut app, "len");
    assert_eq!(
        app.active().page.text,
        "    for (k = 0; k < len; k++) {\n        \n    }"
    );
    // Shift+Tab goes back to the first field.
    frame(&ctx, &mut app, vec![key(Key::Tab, Modifiers::SHIFT)]);
    assert_eq!(app.selections().len(), 3);
    frame(&ctx, &mut app, vec![key(Key::Tab, Modifiers::NONE)]);
    frame(&ctx, &mut app, vec![key(Key::Tab, Modifiers::NONE)]);
    // The last Tab ends at $0, and the next one indents as usual.
    assert_eq!(app.selections(), [hedit::advanced::Selection::caret(40)]);
    assert!(app.snippet.is_none());
    frame(&ctx, &mut app, vec![key(Key::Tab, Modifiers::NONE)]);
    assert_eq!(
        app.active().page.text,
        "    for (k = 0; k < len; k++) {\n            \n    }"
    );
}

#[test]
fn escape_or_moving_away_ends_snippet_fields() {
    let (_directory, ctx, mut app) = app_with("");
    app.insert_snippet(
        hedit::intelligence::Snippet {
            trigger: String::new(),
            description: String::new(),
            body: "${1:a}, ${2:b}".into(),
            language: None,
        },
        0,
    );
    frame(&ctx, &mut app, vec![]);
    assert!(app.snippet.is_some());
    frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
    assert!(app.snippet.is_none());
    frame(&ctx, &mut app, vec![key(Key::Tab, Modifiers::NONE)]);
    assert_eq!(
        app.active().page.text,
        "    a, b",
        "Tab indents again after Escape"
    );
    app.insert_snippet(
        hedit::intelligence::Snippet {
            trigger: String::new(),
            description: String::new(),
            body: "${1:x}${2:y}".into(),
            language: None,
        },
        0,
    );
    frame(&ctx, &mut app, vec![]);
    assert!(app.snippet.is_some());
    app.set_selections(vec![hedit::advanced::Selection::caret(2)]);
    frame(&ctx, &mut app, vec![]);
    assert!(app.snippet.is_none(), "the cursor left the field");
}

#[test]
fn escape_keeps_typing_in_the_editor() {
    let (_directory, ctx, mut app) = app_with("");
    type_text(&ctx, &mut app, "a");
    frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
    type_text(&ctx, &mut app, "b");
    assert_eq!(app.active().page.text, "ab");
}

/// Run frames until the background word index is ready.
fn wait_for_index(ctx: &egui::Context, app: &mut HeditApp) {
    let started = Instant::now();
    app.auto.wanted = true;
    frame(ctx, app, vec![]);
    while app.auto.building.is_some() {
        assert!(started.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(5));
        frame(ctx, app, vec![]);
    }
}

#[test]
fn completions_appear_while_typing_and_tab_accepts() {
    let (_directory, ctx, mut app) = app_with("render_frame frame_count\n");
    app.settings.auto_complete_delay = 0;
    app.set_selections(vec![hedit::advanced::Selection::caret(25)]);
    wait_for_index(&ctx, &mut app);
    type_text(&ctx, &mut app, "fr");
    frame(&ctx, &mut app, vec![]);
    let popup = app
        .auto
        .popup
        .as_ref()
        .expect("the list opens after two letters");
    assert!(matches!(&popup.choices[0], auto_complete::Choice::Word(w) if w == "frame_count"));
    // Typing narrows the list; Tab accepts the first entry.
    type_text(&ctx, &mut app, "c");
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::Tab, Modifiers::NONE)]);
    assert_eq!(
        app.active().page.text,
        "render_frame frame_count\nframe_count"
    );
    assert!(app.auto.popup.is_none());
    // A space closes the list without accepting anything.
    type_text(&ctx, &mut app, " ren");
    frame(&ctx, &mut app, vec![]);
    assert!(app.auto.popup.is_some());
    type_text(&ctx, &mut app, " ");
    frame(&ctx, &mut app, vec![]);
    assert!(app.auto.popup.is_none());
    assert_eq!(
        app.active().page.text,
        "render_frame frame_count\nframe_count ren "
    );
}

#[test]
fn completion_keys_follow_the_settings() {
    let (_directory, ctx, mut app) = app_with("alpha alpine\n");
    app.settings.auto_complete_delay = 0;
    app.set_selections(vec![hedit::advanced::Selection::caret(13)]);
    wait_for_index(&ctx, &mut app);
    // With commit on Tab, Enter starts a new line and closes the list.
    app.settings.auto_complete_commit_on_tab = true;
    type_text(&ctx, &mut app, "al");
    frame(&ctx, &mut app, vec![]);
    assert!(app.auto.popup.is_some());
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    assert!(app.auto.popup.is_none());
    assert_eq!(app.active().page.text, "alpha alpine\nal\n");
    // Escape closes the list, keeping the typed text.
    type_text(&ctx, &mut app, "alp");
    frame(&ctx, &mut app, vec![]);
    assert!(app.auto.popup.is_some());
    frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
    assert!(app.auto.popup.is_none());
    assert_eq!(app.active().page.text, "alpha alpine\nal\nalp");
    // Turned off, typing shows nothing.
    app.settings.auto_complete = false;
    type_text(&ctx, &mut app, "h");
    frame(&ctx, &mut app, vec![]);
    assert!(app.auto.popup.is_none());
}

#[test]
#[ignore = "manual headless CPU timing for typing with completions in a large file"]
fn profile_completion_typing() {
    // About 1 MB with many distinct words.
    let source: String = (0..40_000)
        .map(|i| format!("let value_{i} = compute_{}(item);\n", i % 997))
        .collect();
    let (_directory, ctx, mut app) = app_with(&source);
    app.settings.auto_complete_delay = 0;
    let end = source.chars().count();
    app.set_selections(vec![hedit::advanced::Selection::caret(end)]);
    wait_for_index(&ctx, &mut app);
    for (label, auto) in [("off", false), ("on", true)] {
        app.settings.auto_complete = auto;
        let start = Instant::now();
        for c in "value_12".chars() {
            frame(&ctx, &mut app, vec![egui::Event::Text(c.into())]);
        }
        eprintln!(
            "Typing with completions {label} in {} bytes: {:.2} ms/frame ({} words indexed)",
            source.len(),
            start.elapsed().as_secs_f64() * 1000.0 / 8.0,
            app.auto.words_len()
        );
        frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
        type_text(&ctx, &mut app, "\n");
    }
}

#[test]
fn nothing_is_indexed_until_a_word_is_typed() {
    let (_directory, ctx, mut app) = app_with("some words here\n");
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    assert!(app.auto.building.is_none() && app.auto.words_len() == 0);
    type_text(&ctx, &mut app, "s");
    frame(&ctx, &mut app, vec![]);
    assert!(app.auto.wanted);
}

#[test]
fn f12_jumps_to_a_definition_and_shift_f12_lists_references() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("lib.c"),
        "int helper(int x) {\n    return x;\n}\n",
    )
    .unwrap();
    std::fs::write(root.join("blob.bin"), b"helper\0helper").unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    workspace.open[0].page.text = "int main() {\n    return helper(1) + helper(2);\n}\n".into();
    workspace.edit(0, "").unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.set_folder(hedit::folder::read_directory(root.clone()).unwrap());
    app.set_selections(vec![hedit::advanced::Selection::caret(26)]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::F12, Modifiers::NONE)]);
    finish_app_job(&mut app, &ctx);
    frame(&ctx, &mut app, vec![]);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().view.name, "lib.c", "the only definition opens");
    // Back returns to the call.
    app.execute(&ctx, Command::NavBack);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().view.head, 26);
    app.execute(&ctx, Command::GotoReferences);
    finish_app_job(&mut app, &ctx);
    let list = app.pick_list.as_ref().expect("references are listed");
    assert!(list.title.contains("(2)"), "{}", list.title);
    // The cursor on nothing gives a notice instead.
    app.palette = false;
    frame(&ctx, &mut app, vec![]);
    app.set_selections(vec![hedit::advanced::Selection::caret(12)]);
    app.execute(&ctx, Command::GotoDefinition);
    assert!(app.notice.contains("cursor on a name"), "{}", app.notice);
}

#[test]
fn resting_on_a_name_lists_its_definitions_from_open_files() {
    let (_directory, ctx, mut app) = app_with("fn total() {}\nlet x = total();\n");
    // `total` in the call, on line 2.
    app.hover_target = Some((app.active().view.id, (22, 27), egui::pos2(100.0, 100.0)));
    frame(&ctx, &mut app, vec![]);
    let hover = app.hover.as_ref().expect("a definition is listed");
    assert_eq!(hover.found_lines(), [1]);
    // Moving the pointer away closes it; the definition itself lists nothing.
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(egui::pos2(600.0, 600.0))],
    );
    assert!(app.hover.is_none());
    app.hover_target = Some((app.active().view.id, (3, 8), egui::pos2(100.0, 100.0)));
    frame(&ctx, &mut app, vec![]);
    assert!(app.hover.is_none());
    app.settings.show_definitions = false;
    app.hover_target = Some((app.active().view.id, (22, 27), egui::pos2(100.0, 100.0)));
    frame(&ctx, &mut app, vec![]);
    assert!(app.hover.is_none());
}

#[test]
fn replace_in_folders_edits_open_files_unsaved_and_undoes_disk_files() {
    let directory = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(directory.path())
        .unwrap()
        .join("project");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("open.txt"), "old on disk\n").unwrap();
    std::fs::write(root.join("closed.txt"), "old closed\n").unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.set_folder(hedit::folder::read_directory(root.clone()).unwrap());
    app.open(&ctx, Some(root.join("open.txt")));
    finish_app_job(&mut app, &ctx);
    // Unsaved text in the open file is what gets searched and replaced.
    app.set_selections(vec![hedit::advanced::Selection::caret(0)]);
    type_text(&ctx, &mut app, "old ");
    assert!(app.active().page.info.dirty);
    app.query = "old".into();
    app.replacement = "new".into();
    app.folder_search(&ctx, true);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.quality.results.hits.len(), 3);
    assert_eq!(app.quality.results.plans.len(), 2);
    app.apply_folder_replacements(&ctx);
    finish_app_job(&mut app, &ctx);
    let open = app
        .workspace
        .open
        .iter()
        .find(|b| b.view.name == "open.txt")
        .unwrap();
    assert_eq!(open.page.text, "new new on disk\n");
    assert!(open.page.info.dirty, "open files stay unsaved");
    assert_eq!(
        std::fs::read_to_string(root.join("open.txt")).unwrap(),
        "old on disk\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("closed.txt")).unwrap(),
        "new closed\n"
    );
    assert!(app.notice.contains("1 open files"), "{}", app.notice);
    // Undo in the open file brings its text back.
    app.history(&ctx, false);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().page.text, "old old on disk\n");
    // Undo replacements puts the closed file back.
    app.undo_folder_replacements(&ctx);
    finish_app_job(&mut app, &ctx);
    assert_eq!(
        std::fs::read_to_string(root.join("closed.txt")).unwrap(),
        "old closed\n"
    );
    assert!(app.quality.replace_undo.is_empty());
}

#[test]
fn f4_goes_through_folder_matches_after_a_search() {
    let directory = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(directory.path())
        .unwrap()
        .join("project");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.txt"), "x\nneedle\n").unwrap();
    std::fs::write(root.join("b.txt"), "needle\n").unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.set_folder(hedit::folder::read_directory(root.clone()).unwrap());
    app.query = "needle".into();
    app.folder_search(&ctx, false);
    finish_app_job(&mut app, &ctx);
    assert!(app.command_enabled(Command::NextResult));
    app.execute(&ctx, Command::NextResult);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().view.name, "a.txt");
    assert_eq!(app.active().view.head, 2, "line 2, column 1");
    app.execute(&ctx, Command::NextResult);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().view.name, "b.txt");
    app.execute(&ctx, Command::PreviousResult);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().view.name, "a.txt");
}

/// The editor text's rectangle on screen, from last frame.
fn editor_rect(ctx: &egui::Context, app: &HeditApp) -> egui::Rect {
    let id = egui::Id::new(("editor", app.active().page.info.id, app.active().page.start));
    ctx.read_response(id).expect("editor drawn").rect
}

#[test]
fn fold_arrows_in_the_gutter_fold_and_unfold_blocks() {
    let (_directory, ctx, mut app) = app_with("fn a() {\n    one();\n    two();\n}\nfn b() {}\n");
    app.set_selections(vec![hedit::advanced::Selection::caret(0)]);
    frame(&ctx, &mut app, vec![]);
    let rect = editor_rect(&ctx, &app);
    let row = app.settings.font_size * 1.3;
    let arrow = egui::pos2(rect.left() - 6.0, rect.top() + row / 2.0);
    // Arrows appear while the pointer is over the gutter.
    frame(&ctx, &mut app, vec![egui::Event::PointerMoved(arrow)]);
    frame(&ctx, &mut app, vec![egui::Event::PointerMoved(arrow)]);
    let click = |pressed| egui::Event::PointerButton {
        pos: arrow,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    frame(&ctx, &mut app, vec![click(true)]);
    frame(&ctx, &mut app, vec![click(false)]);
    assert_eq!(
        app.active().view.folds.len(),
        1,
        "the block under line 1 folds"
    );
    frame(&ctx, &mut app, vec![click(true)]);
    frame(&ctx, &mut app, vec![click(false)]);
    assert!(
        app.active().view.folds.is_empty(),
        "the folded arrow unfolds it"
    );
}

#[test]
fn editor_menu_and_tab_menu_commands() {
    let directory = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(directory.path())
        .unwrap()
        .join("project");
    std::fs::create_dir_all(root.join("src/deep")).unwrap();
    let file = root.join("src/deep/main.rs");
    std::fs::write(&file, "fn main() {}\n").unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    assert!(
        !app.command_enabled(Command::CopyPath),
        "unsaved files have no path"
    );
    app.set_folder(hedit::folder::read_directory(root.clone()).unwrap());
    app.open(&ctx, Some(file.clone()));
    finish_app_job(&mut app, &ctx);
    app.settings.show_sidebar = false;
    app.execute(&ctx, Command::RevealInSidebar);
    assert!(app.settings.show_sidebar && app.settings.show_folders);
    let src = root.join("src").to_string_lossy().into_owned();
    let deep = root.join("src/deep").to_string_lossy().into_owned();
    assert!(app.project.expanded.contains(&src) && app.project.expanded.contains(&deep));
    let output = {
        app.execute(&ctx, Command::CopyPath);
        frame(&ctx, &mut app, vec![])
    };
    let _ = output;
    assert_eq!(
        app.paste_history.first().map(String::as_str),
        Some(file.to_str().unwrap())
    );
    // Right-clicking moves the cursor to the click and opens the menu.
    frame(&ctx, &mut app, vec![]);
    let rect = editor_rect(&ctx, &app);
    let at = egui::pos2(rect.left() + 40.0, rect.top() + 6.0);
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    frame(&ctx, &mut app, vec![egui::Event::PointerMoved(at)]);
    frame(&ctx, &mut app, vec![press(true)]);
    frame(&ctx, &mut app, vec![press(false)]);
    assert!(app.active().view.head > 0, "the cursor moved to the click");
    let output = frame(&ctx, &mut app, vec![]);
    let texts: Vec<String> = output
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
            _ => None,
        })
        .collect();
    assert!(
        texts.iter().any(|t| t == "Go to definition"),
        "the menu is open"
    );
}

#[test]
fn hovering_a_git_marker_shows_its_change() {
    let (_directory, ctx, mut app) = app_with("one\ntwo\n");
    let id = app.active().view.id;
    app.quality.git_lines.insert(id, [1].into_iter().collect());
    app.quality
        .git_cache
        .insert(id, "@@ -1 +1 @@\n-uno\n+one\n".into());
    frame(&ctx, &mut app, vec![]);
    let rect = editor_rect(&ctx, &app);
    let gutter = rect.left() - editor_gutter_width(&app);
    let at = egui::pos2(gutter + 4.0, rect.top() + app.settings.font_size * 0.6);
    let mut seen = false;
    for _ in 0..12 {
        let output = frame(&ctx, &mut app, vec![egui::Event::PointerMoved(at)]);
        thread::sleep(Duration::from_millis(60));
        seen |= output
            .shapes
            .iter()
            .any(|c| matches!(&c.shape, egui::Shape::Text(t) if t.galley.text().contains("+one")));
    }
    assert!(seen, "the hunk shows on hover");
}

fn editor_gutter_width(app: &HeditApp) -> f32 {
    let digits = (app.active().page.text.matches('\n').count() + 1)
        .to_string()
        .len();
    (digits.max(3) as f32 * app.settings.font_size * 0.65 + 16.0).max(42.0)
}

/// Run frames until spelling results for the active file are in.
fn wait_for_spelling(ctx: &egui::Context, app: &mut HeditApp) {
    let started = Instant::now();
    loop {
        frame(ctx, app, vec![]);
        let view = app.active().view.id;
        let revision = app.active().page.info.revision;
        if app.spell.pending.is_none() && app.spell_checked(view) == Some(revision) {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "spelling timed out"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn f6_checks_spelling_and_the_menu_fixes_words() {
    let (_directory, ctx, mut app) = app_with("A smal tset of wrods.\n");
    assert!(app.misspellings(app.active().view.id).is_none());
    frame(&ctx, &mut app, vec![key(Key::F6, Modifiers::NONE)]);
    assert!(app.settings.spell_check);
    wait_for_spelling(&ctx, &mut app);
    let view = app.active().view.id;
    assert_eq!(app.misspellings(view).unwrap(), [2..6, 7..11, 15..20]);
    // Ctrl+F6 selects the next misspelled word.
    app.set_selections(vec![hedit::advanced::Selection::caret(0)]);
    frame(&ctx, &mut app, vec![key(Key::F6, Modifiers::COMMAND)]);
    assert_eq!(
        app.selections(),
        [hedit::advanced::Selection { anchor: 2, head: 6 }]
    );
    // A suggestion replaces the word; ranges after it follow along.
    app.word_menu_at(view, 3);
    let menu = app.spell.menu.as_ref().unwrap();
    assert_eq!(menu.word, "smal");
    assert!(menu.suggestions.iter().any(|w| w == "small"));
    app.spell_action(spell_check::SpellAction::Replace("small".into()));
    assert_eq!(app.active().page.text, "A small tset of wrods.\n");
    assert_eq!(app.misspellings(view).unwrap(), [8..12, 16..21]);
    // Adding a word to the dictionary saves it and checks again.
    app.word_menu_at(view, 9);
    app.spell_action(spell_check::SpellAction::Add);
    assert_eq!(app.settings.added_words, ["tset"]);
    wait_for_spelling(&ctx, &mut app);
    assert_eq!(app.misspellings(view).unwrap().to_vec(), vec![16..21; 1]);
    // Turned off, nothing is underlined.
    app.execute(&ctx, Command::ToggleSpellCheck);
    frame(&ctx, &mut app, vec![]);
    assert!(app.misspellings(view).is_none());
}

#[test]
fn resting_the_pointer_on_a_call_finds_its_definition() {
    let (_directory, ctx, mut app) = app_with("fn total() {}\ntotal();\n");
    frame(&ctx, &mut app, vec![]);
    let rect = editor_rect(&ctx, &app);
    // The middle of `total` on line 2.
    let row = app.settings.font_size * 1.3;
    let at = egui::pos2(rect.left() + 20.0, rect.top() + row * 1.5);
    let run = |app: &mut HeditApp, time: f64, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 820.0),
            )),
            time: Some(time),
            focused: true,
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            app.shortcuts(ctx);
            app.chrome(ctx);
            app.search_panel(ctx);
            app.build_panel(ctx);
            app.editor(ctx);
            app.quality_windows(ctx);
        });
    };
    run(&mut app, 10.0, vec![egui::Event::PointerMoved(at)]);
    run(&mut app, 11.0, vec![]);
    run(&mut app, 11.1, vec![]);
    let hover = app.hover.as_ref().expect("the definition is listed");
    assert_eq!(hover.found_lines(), [1]);
}

#[test]
fn typing_in_find_does_not_open_editor_completions() {
    let (_directory, ctx, mut app) = app_with("alpha alpine al");
    app.settings.auto_complete_delay = 0;
    app.set_selections(vec![hedit::advanced::Selection::caret(15)]);
    wait_for_index(&ctx, &mut app);
    app.execute(&ctx, Command::Find);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![egui::Event::Text("p".into())]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![]);
    assert_eq!(app.query, "p");
    assert!(app.auto.popup.is_none());
}

#[test]
fn undo_replacements_leaves_files_edited_since_alone() {
    let directory = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(directory.path())
        .unwrap()
        .join("project");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.txt"), "old\n").unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.set_folder(hedit::folder::read_directory(root.clone()).unwrap());
    app.query = "old".into();
    app.replacement = "new".into();
    app.folder_search(&ctx, true);
    finish_app_job(&mut app, &ctx);
    app.apply_folder_replacements(&ctx);
    finish_app_job(&mut app, &ctx);
    // Open the replaced file and edit it without saving.
    app.open(&ctx, Some(root.join("a.txt")));
    finish_app_job(&mut app, &ctx);
    app.set_selections(vec![hedit::advanced::Selection::caret(0)]);
    type_text(&ctx, &mut app, "x");
    app.undo_folder_replacements(&ctx);
    while app.busy {
        app.receive(&ctx);
        thread::sleep(Duration::from_millis(5));
    }
    assert!(app.error.as_ref().unwrap().contains("unsaved changes"));
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "new\n"
    );
    assert_eq!(app.active().page.text, "xnew\n");
}

#[test]
fn full_screen_and_distraction_free_turn_on_and_off_separately() {
    let (_directory, ctx, mut app) = app_with("text");
    app.execute(&ctx, Command::DistractionFree);
    assert!(
        app.distraction_free && !app.fullscreen,
        "it leaves full screen alone"
    );
    app.execute(&ctx, Command::Fullscreen);
    assert!(app.distraction_free && app.fullscreen);
    app.execute(&ctx, Command::DistractionFree);
    assert!(
        !app.distraction_free && app.fullscreen,
        "full screen stays on"
    );
    app.execute(&ctx, Command::Fullscreen);
    assert!(!app.distraction_free && !app.fullscreen);
}

#[test]
fn the_palette_matches_letters_in_order_best_first() {
    let (_directory, ctx, mut app) = app_with("text");
    app.set_syntax(Some(syntax::Language::Python));
    app.execute(&ctx, Command::Palette);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![egui::Event::Text("tglc".into())]);
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    assert_eq!(app.active().page.text, "# text");
}

#[test]
fn find_shortcuts_select_matches_and_reuse_the_selection() {
    let (_directory, ctx, mut app) = app_with("λ cat, cat;\ncat\n");
    // Ctrl+E takes the word at the cursor as the Find text.
    app.set_selections(vec![hedit::advanced::Selection::caret(3)]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::E, Modifiers::COMMAND)]);
    assert_eq!(app.query, "cat");
    // Alt+Enter puts a cursor on every match, in characters.
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::ALT)]);
    let ranges: Vec<(usize, usize)> = app
        .selections()
        .iter()
        .map(|s| (s.start(), s.end()))
        .collect();
    assert_eq!(ranges, [(2, 5), (7, 10), (12, 15)]);
    assert_eq!(
        app.active().page.text,
        "λ cat, cat;\ncat\n",
        "nothing typed"
    );
    // Ctrl+Shift+E fills Replace.
    app.set_selections(vec![hedit::advanced::Selection { anchor: 5, head: 6 }]);
    frame(&ctx, &mut app, vec![]);
    frame(
        &ctx,
        &mut app,
        vec![key(Key::E, Modifiers::COMMAND | Modifiers::SHIFT)],
    );
    assert_eq!(app.replacement, ",");
    // Ctrl+F3 selects the word, then its next copies, wrapping around.
    app.set_selections(vec![hedit::advanced::Selection::caret(8)]);
    frame(&ctx, &mut app, vec![]);
    let next = |app: &mut HeditApp, ctx: &egui::Context| {
        frame(ctx, app, vec![key(Key::F3, Modifiers::COMMAND)]);
        app.selections()[0]
    };
    assert_eq!(
        next(&mut app, &ctx),
        hedit::advanced::Selection {
            anchor: 7,
            head: 10
        }
    );
    assert_eq!(
        next(&mut app, &ctx),
        hedit::advanced::Selection {
            anchor: 12,
            head: 15
        }
    );
    assert_eq!(
        next(&mut app, &ctx),
        hedit::advanced::Selection { anchor: 2, head: 5 }
    );
    frame(
        &ctx,
        &mut app,
        vec![key(Key::F3, Modifiers::COMMAND | Modifiers::SHIFT)],
    );
    assert_eq!(
        app.selections()[0],
        hedit::advanced::Selection {
            anchor: 12,
            head: 15
        }
    );
}

#[test]
fn change_markers_navigate_and_revert_against_the_saved_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.txt");
    std::fs::write(&path, "one\ntwo\nthree\nfour\nfive\n").unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.settings.mini_diff = "saved".into();
    app.open(&ctx, Some(path.clone()));
    finish_app_job(&mut app, &ctx);
    // Change line 2 and delete line 4.
    let index = app.workspace.active;
    let old = app.workspace.open[index].page.text.clone();
    app.workspace.open[index].page.text = "one\nTWO\nthree\nfive\n".into();
    app.workspace.edit(index, &old).unwrap();
    // Wait for the markers.
    let started = Instant::now();
    while app
        .quality
        .git_lines
        .get(&app.active().view.id)
        .is_none_or(|l| l.is_empty())
    {
        assert!(started.elapsed() < Duration::from_secs(5));
        app.quality.git_at = None;
        frame(&ctx, &mut app, vec![]);
        app.poll_git(&ctx);
        thread::sleep(Duration::from_millis(10));
        app.receive(&ctx);
    }
    let line = |app: &HeditApp| {
        let b = app.active();
        b.page
            .text
            .chars()
            .take(b.view.head)
            .filter(|c| *c == '\n')
            .count()
            + 1
    };
    app.set_selections(vec![hedit::advanced::Selection::caret(0)]);
    app.execute(&ctx, Command::NextChange);
    assert_eq!(line(&app), 2);
    app.execute(&ctx, Command::NextChange);
    assert_eq!(line(&app), 4, "where line four was deleted");
    app.execute(&ctx, Command::NextChange);
    assert_eq!(line(&app), 2, "wraps around");
    app.execute(&ctx, Command::RevertChange);
    assert_eq!(app.active().page.text, "one\ntwo\nthree\nfive\n");
    app.execute(&ctx, Command::Undo);
    finish_app_job(&mut app, &ctx);
    assert_eq!(
        app.active().page.text,
        "one\nTWO\nthree\nfive\n",
        "reverting is undoable"
    );
    // The two-key shortcut reaches the next change.
    let release = |key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    };
    app.set_selections(vec![hedit::advanced::Selection::caret(0)]);
    frame(&ctx, &mut app, vec![]);
    frame(
        &ctx,
        &mut app,
        vec![key(Key::K, Modifiers::COMMAND), release(Key::K)],
    );
    frame(
        &ctx,
        &mut app,
        vec![key(Key::Period, Modifiers::COMMAND), release(Key::Period)],
    );
    assert_eq!(line(&app), 2);
}
