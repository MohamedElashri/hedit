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
