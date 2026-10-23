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
    assert!(!app.quality.completion_open, "not Complete word");
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
    frame(&ctx, &mut app, vec![key(Key::M, Modifiers::COMMAND)]);
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
    assert_eq!(app.shortcut_text(Command::Goto), "Ctrl+K, Ctrl+G");
    app.preferences_open = false;
    frame(&ctx, &mut app, ctrl(Key::K));
    frame(&ctx, &mut app, ctrl(Key::G));
    assert!(app.goto_open, "the recorded chord runs Go to line");
}
