use super::tests::{frame, key};
use super::*;
use hedit::advanced::{self, Selection};

fn tap(key_code: Key, modifiers: Modifiers) -> Vec<egui::Event> {
    let release = egui::Event::Key {
        key: key_code,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers,
    };
    vec![key(key_code, modifiers), release]
}

fn fixture() -> (tempfile::TempDir, egui::Context, HeditApp) {
    let directory = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
    workspace.open[0].page.text =
        "outer:\n  middle:\n    inner:\n      α()\n  tail()\nend\n".into();
    workspace.edit(0, "").unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.settings.terminal_onboarding_complete = true;
    app.workspace.open[0].view.tab_size = Some(2);
    (directory, ctx, app)
}

#[test]
fn fold_levels_replace_old_folds_keep_text_and_move_hidden_cursors() {
    let (_directory, ctx, mut app) = fixture();
    let original = app.active().page.text.clone();
    let revision = app.active().page.info.revision;
    let at = original[..original.find('α').unwrap()].chars().count();
    app.set_selections(vec![Selection::caret(at), Selection::caret(2)]);
    for (command, level) in [
        (Command::FoldLevel1, 1),
        (Command::FoldLevel2, 2),
        (Command::FoldLevel3, 3),
    ] {
        app.execute(&ctx, command);
        assert_eq!(
            app.active().view.folds,
            advanced::indentation_folds_at_level(&original, 2, level)
        );
        assert!(app.active().view.selections.is_empty());
        assert!(!app
            .active()
            .view
            .folds
            .iter()
            .any(|f| f.start < app.active().view.head && app.active().view.head < f.end));
        frame(&ctx, &mut app, vec![]);
        assert_eq!(
            app.active().view.folds.len(),
            1,
            "drawing must not unfold the result"
        );
        assert_eq!(app.active().page.text, original);
        assert_eq!(app.active().page.info.revision, revision);
    }
    app.execute(&ctx, Command::UnfoldAll);
    assert!(app.active().view.folds.is_empty());
}

#[test]
fn fold_level_chords_and_rebinding_work_and_paged_files_are_disabled() {
    let (_directory, ctx, mut app) = fixture();
    for (key_number, level) in [(Key::Num1, 1), (Key::Num2, 2), (Key::Num3, 3)] {
        frame(&ctx, &mut app, tap(Key::K, Modifiers::COMMAND));
        frame(&ctx, &mut app, tap(key_number, Modifiers::COMMAND));
        assert_eq!(
            app.active().view.folds,
            advanced::indentation_folds_at_level(&app.active().page.text, 2, level)
        );
    }
    app.quality.config.keybindings.push(quality::Binding {
        command: Command::FoldLevel2,
        shortcut: "Alt+F8".into(),
        editor_only: true,
    });
    app.execute(&ctx, Command::UnfoldAll);
    frame(&ctx, &mut app, tap(Key::K, Modifiers::COMMAND));
    frame(&ctx, &mut app, tap(Key::Num2, Modifiers::COMMAND));
    assert!(
        app.active().view.folds.is_empty(),
        "overridden default chord must stop working"
    );
    frame(&ctx, &mut app, vec![key(Key::F8, Modifiers::ALT)]);
    assert_eq!(app.active().view.folds.len(), 1);
    app.workspace.open[0].page.info.paged = true;
    for command in [
        Command::FoldLevel1,
        Command::FoldLevel2,
        Command::FoldLevel3,
    ] {
        assert!(!app.command_enabled(command));
    }
}

#[test]
fn fold_level_survives_session_recovery_and_supports_empty_documents() {
    let (directory, ctx, mut app) = fixture();
    app.execute(&ctx, Command::FoldLevel2);
    let folds = app.active().view.folds.clone();
    app.workspace.checkpoint().unwrap();
    drop(app);
    let (workspace, errors) = Workspace::restore(directory.path().into()).unwrap();
    assert!(errors.is_empty());
    assert_eq!(workspace.open[0].view.folds, folds);
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.execute(&ctx, Command::New);
    app.execute(&ctx, Command::FoldLevel3);
    assert!(app.active().view.folds.is_empty());
}
