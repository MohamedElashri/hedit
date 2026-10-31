use super::tests::{finish_app_job, frame, key};
use super::*;
use hedit::advanced::Selection;

fn fixture() -> (tempfile::TempDir, egui::Context, HeditApp) {
    let directory = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
    workspace.open[0].page.text = "<root> before <item>مرحبا</item> after </root>".into();
    workspace.edit(0, "").unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.settings.terminal_onboarding_complete = true;
    app.set_syntax(Some(syntax::Language::Html));
    let text = &app.active().page.text;
    let at = text[..text.find("مرحبا").unwrap()].chars().count() + 2;
    app.set_selections(vec![Selection::caret(at)]);
    (directory, ctx, app)
}

fn text_selected(app: &HeditApp) -> &str {
    let text = &app.active().page.text;
    let range = app.selections()[0];
    &text[hedit::advanced::byte(text, range.start())..hedit::advanced::byte(text, range.end())]
}

#[test]
fn markup_selection_shortcut_expands_and_soft_undo_restores_without_editing() {
    let (_directory, ctx, mut app) = fixture();
    let original = app.active().page.text.clone();
    let revision = app.active().page.info.revision;
    let before = app.selections();
    app.quality.config.keybindings.push(quality::Binding {
        command: Command::SelectAll,
        shortcut: "Alt+F5".into(),
        editor_only: true,
    });
    frame(&ctx, &mut app, vec![]);
    frame(
        &ctx,
        &mut app,
        vec![key(Key::A, Modifiers::COMMAND | Modifiers::SHIFT)],
    );
    finish_app_job(&mut app, &ctx);
    assert_eq!(text_selected(&app), "مرحبا");
    frame(&ctx, &mut app, vec![]);
    app.execute(&ctx, Command::ExpandToTag);
    finish_app_job(&mut app, &ctx);
    assert_eq!(text_selected(&app), "<item>مرحبا</item>");
    frame(&ctx, &mut app, vec![]);
    app.execute(&ctx, Command::SoftUndo);
    assert_eq!(text_selected(&app), "مرحبا");
    app.execute(&ctx, Command::SoftUndo);
    assert_eq!(app.selections(), before);
    assert_eq!(app.active().page.text, original);
    assert_eq!(app.active().page.info.revision, revision);
}

#[test]
fn markup_selection_availability_and_rebinding_respect_editor_focus() {
    let (_directory, ctx, mut app) = fixture();
    app.set_syntax(Some(syntax::Language::PlainText));
    assert!(!app.command_enabled(Command::ExpandToTag));
    app.set_syntax(Some(syntax::Language::Xml));
    assert!(app.command_enabled(Command::ExpandToTag));
    app.workspace.open[0].page.info.paged = true;
    assert!(!app.command_enabled(Command::ExpandToTag));
    app.workspace.open[0].page.info.paged = false;
    app.quality.config.keybindings.push(quality::Binding {
        command: Command::ExpandToTag,
        shortcut: "Alt+F7".into(),
        editor_only: true,
    });
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::F7, Modifiers::ALT)]);
    finish_app_job(&mut app, &ctx);
    assert_eq!(text_selected(&app), "مرحبا");
    app.execute(&ctx, Command::Find);
    frame(&ctx, &mut app, vec![]);
    let before = app.selections();
    // An editor-only custom shortcut must not act on the document from Find.
    frame(&ctx, &mut app, vec![key(Key::F7, Modifiers::ALT)]);
    assert!(!app.busy);
    assert_eq!(app.selections(), before);
}

#[test]
fn markup_selection_discards_stale_revision_view_and_selection_results() {
    let (_directory, ctx, mut app) = fixture();
    let id = app.active().view.id;
    let revision = app.active().page.info.revision;
    let before = app.selections();
    let after = vec![Selection {
        anchor: 0,
        head: app.active().page.text.chars().count(),
    }];
    app.quality_event(
        &ctx,
        quality::Event::TagSelections(id, revision + 1, before.clone(), after.clone()),
    );
    assert_eq!(app.selections(), before);
    app.quality_event(
        &ctx,
        quality::Event::TagSelections(id, revision, vec![Selection::caret(0)], after.clone()),
    );
    assert_eq!(app.selections(), before);
    app.execute(&ctx, Command::New);
    app.quality_event(
        &ctx,
        quality::Event::TagSelections(id, revision, before, after),
    );
    assert_eq!(app.selections(), [Selection::caret(0)]);
}
