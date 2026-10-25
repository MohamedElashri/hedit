//! Overlay dismissal, Escape order, and keyboard focus.
use super::tests::{click_at, click_label, finish_app_job, frame, key, text_rect};
use super::*;

const SIZE: egui::Vec2 = egui::vec2(1280.0, 820.0);

fn app_with(text: &str) -> (tempfile::TempDir, egui::Context, HeditApp) {
    let directory = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
    workspace.open[0].page.text = text.into();
    workspace.edit(0, "").unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    frame(&ctx, &mut app, vec![]);
    (directory, ctx, app)
}

fn escape(ctx: &egui::Context, app: &mut HeditApp) {
    frame(ctx, app, vec![key(Key::Escape, Modifiers::NONE)]);
}

#[test]
fn quick_pickers_close_when_clicking_elsewhere_but_survive_the_opening_click() {
    let (_directory, ctx, mut app) = app_with("notes");
    let away = egui::pos2(900.0, 600.0);
    // Opening from the menu: the click that runs the command must not close it.
    click_label(&ctx, &mut app, "Tools");
    click_label(&ctx, &mut app, "Command palette…");
    frame(&ctx, &mut app, vec![]);
    assert!(app.palette);
    click_at(&ctx, &mut app, away, SIZE);
    assert!(!app.palette);
    // The theme switcher restores the previous theme when dismissed this way.
    let original = app.theme.clone();
    app.execute(&ctx, Command::Appearance);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::ArrowDown, Modifiers::NONE)]);
    assert_ne!(app.theme, original);
    click_at(&ctx, &mut app, away, SIZE);
    assert!(!app.palette);
    assert_eq!(app.theme, original);
    for command in [Command::Goto, Command::Bookmarks, Command::Shelf] {
        app.execute(&ctx, command);
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![]);
        click_at(&ctx, &mut app, away, SIZE);
        assert!(
            !app.goto_open && !app.bookmarks_open && !app.shelf_open,
            "{command:?}"
        );
    }
    assert_eq!(app.active().page.text, "notes");
}

#[test]
fn opening_a_window_closes_the_previous_one() {
    let (_directory, ctx, mut app) = app_with("");
    app.execute(&ctx, Command::Settings);
    assert!(app.preferences_open);
    app.execute(&ctx, Command::Palette);
    assert!(app.palette && !app.preferences_open);
    app.execute(&ctx, Command::Goto);
    assert!(app.goto_open && !app.palette);
    // The Find bar is a panel and stays beside windows.
    app.execute(&ctx, Command::Find);
    app.execute(&ctx, Command::Symbols);
    assert!(app.find_open && app.quality.symbols_open);
}

#[test]
fn escape_closes_only_the_topmost_window_then_extra_cursors() {
    let (_directory, ctx, mut app) = app_with("one two one");
    app.execute(&ctx, Command::Find);
    app.execute(&ctx, Command::Goto);
    frame(&ctx, &mut app, vec![]);
    escape(&ctx, &mut app);
    assert!(!app.goto_open);
    assert!(app.find_open, "one Escape closes one window");
    escape(&ctx, &mut app);
    assert!(!app.find_open);
    app.set_selections(vec![
        hedit::advanced::Selection::caret(0),
        hedit::advanced::Selection::caret(8),
    ]);
    app.execute(&ctx, Command::Goto);
    frame(&ctx, &mut app, vec![]);
    escape(&ctx, &mut app);
    assert!(!app.goto_open);
    assert_eq!(app.selections().len(), 2, "closing a window keeps cursors");
    app.error = Some("Something failed".into());
    escape(&ctx, &mut app);
    assert_eq!(app.error, None);
    assert_eq!(app.selections().len(), 2);
    escape(&ctx, &mut app);
    assert_eq!(app.selections().len(), 1);
}

#[test]
fn escape_closes_windows_while_a_background_job_runs() {
    let (_directory, ctx, mut app) = app_with("");
    app.execute(&ctx, Command::Palette);
    app.busy = true;
    escape(&ctx, &mut app);
    assert!(!app.palette);
    app.busy = false;
}

#[test]
fn enter_in_the_find_bar_steps_through_matches_without_editing() {
    let text = "alpha beta alpha beta alpha";
    let (_directory, ctx, mut app) = app_with(text);
    frame(&ctx, &mut app, vec![key(Key::F, Modifiers::COMMAND)]);
    frame(&ctx, &mut app, vec![egui::Event::Text("beta".into())]);
    // Typing already selects the first match, as in Sublime.
    assert_eq!(app.active().view.anchor.min(app.active().view.head), 6);
    let mut found = vec![];
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
        finish_app_job(&mut app, &ctx);
        frame(&ctx, &mut app, vec![]);
        assert!(app.find_input_focused(&ctx), "focus stays in Find");
        found.push(app.active().view.anchor.min(app.active().view.head));
    }
    assert_eq!(app.active().page.text, text);
    assert_eq!(found, vec![17, 6, 17]);
    // Shift+Enter goes back.
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::SHIFT)]);
    finish_app_job(&mut app, &ctx);
    frame(&ctx, &mut app, vec![]);
    assert_eq!(app.active().page.text, text);
    assert_eq!(app.active().view.anchor.min(app.active().view.head), 6);
}

#[test]
fn palette_find_command_focuses_the_find_field() {
    let (_directory, ctx, mut app) = app_with("text");
    app.execute(&ctx, Command::Palette);
    frame(&ctx, &mut app, vec![]);
    frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::Text("Find…".into()),
            key(Key::Enter, Modifiers::NONE),
        ],
    );
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![]);
    assert!(app.find_input_focused(&ctx));
    frame(&ctx, &mut app, vec![egui::Event::Text("x".into())]);
    assert_eq!(app.query, "x");
    assert_eq!(app.active().page.text, "text");
}

#[test]
fn enter_types_a_newline_beside_a_non_modal_window() {
    let (_directory, ctx, mut app) = app_with("ab");
    app.focus_editor = true;
    frame(&ctx, &mut app, vec![]);
    app.execute(&ctx, Command::FindFiles);
    frame(&ctx, &mut app, vec![]);
    // The window focuses its own field; move back to the editor as a click would.
    app.focus_editor = true;
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    assert!(
        !app.busy,
        "Enter outside the window does not start a search"
    );
    assert_eq!(app.active().page.text.matches('\n').count(), 1);
}

#[test]
fn routine_search_outcomes_are_notices_not_errors() {
    let (_directory, ctx, mut app) = app_with("nothing here");
    app.query = "absent".into();
    app.find(&ctx);
    let started = Instant::now();
    while app.busy {
        assert!(started.elapsed() < Duration::from_secs(5));
        app.receive(&ctx);
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(app.error, None);
    assert_eq!(app.notice, "No matches found.");
}

fn open_binary(ctx: &egui::Context, app: &mut HeditApp, path: &std::path::Path) {
    std::fs::write(path, [0, 1, 2, 3]).unwrap();
    app.open(ctx, Some(path.to_owned()));
    let started = Instant::now();
    while app.busy || app.checkpoint_pending || app.generation != app.persisted {
        assert!(started.elapsed() < Duration::from_secs(5));
        app.receive(ctx);
        thread::sleep(Duration::from_millis(5));
    }
    assert!(app.error.as_deref().unwrap().contains("binary file"));
}

#[test]
fn binary_file_error_expires_and_wakes_an_idle_window() {
    let (directory, ctx, mut app) = app_with("keep my notes");
    let id = app.active().view.id;
    open_binary(&ctx, &mut app, &directory.path().join("image.bin"));
    let (message, shown) = app.transient_error.clone().unwrap();
    let output = frame(&ctx, &mut app, vec![]);
    assert!(output
        .shapes
        .iter()
        .any(|s| text_rect(&s.shape, &message).is_some()));
    let wake = app
        .next_wake(&ctx)
        .expect("repaint when the banner expires");
    assert!(wake > Duration::ZERO && wake <= FILE_ERROR_TIME);
    app.expire_error(shown + FILE_ERROR_TIME - Duration::from_millis(1));
    assert_eq!(app.error.as_ref(), Some(&message));
    app.expire_error(shown + FILE_ERROR_TIME);
    assert!(app.error.is_none());
    assert!(app.transient_error.is_none());
    let output = frame(&ctx, &mut app, vec![]);
    assert!(!output
        .shapes
        .iter()
        .any(|s| text_rect(&s.shape, &message).is_some()));
    assert_eq!(app.next_wake(&ctx), None, "no polling after expiry");
    assert_eq!(app.active().view.id, id);
    assert_eq!(app.active().page.text, "keep my notes");
}

#[test]
fn binary_error_timeout_does_not_dismiss_newer_save_or_recovery_errors() {
    for error in [
        "Could not save notes.txt: permission denied.",
        "RECOVERY CHECKPOINT FAILED\nDisk full.",
    ] {
        let (directory, ctx, mut app) = app_with("keep my notes");
        open_binary(&ctx, &mut app, &directory.path().join("image.bin"));
        let shown = app.transient_error.as_ref().unwrap().1;
        app.error = Some(error.into());
        app.expire_error(shown + FILE_ERROR_TIME);
        assert_eq!(app.error.as_deref(), Some(error));
        assert!(app.transient_error.is_none());
    }
}

#[test]
fn another_binary_error_gets_a_fresh_timeout_and_can_still_be_dismissed() {
    let (directory, ctx, mut app) = app_with("notes");
    let path = directory.path().join("image.bin");
    open_binary(&ctx, &mut app, &path);
    app.transient_error.as_mut().unwrap().1 = Instant::now() - Duration::from_secs(4);
    let old_deadline = app.transient_error.as_ref().unwrap().1 + FILE_ERROR_TIME;
    open_binary(&ctx, &mut app, &path);
    app.expire_error(old_deadline);
    assert!(app.error.is_some(), "the new message gets all five seconds");
    escape(&ctx, &mut app);
    assert!(app.error.is_none());
    assert!(app.transient_error.is_none());
    assert_eq!(app.active().page.text, "notes");
}

#[test]
fn an_idle_window_schedules_no_redraws() {
    let (_directory, ctx, mut app) = app_with("");
    // Let the startup checkpoint finish.
    let started = Instant::now();
    while app.generation != app.persisted || app.checkpoint_pending {
        assert!(started.elapsed() < Duration::from_secs(5));
        app.checkpoint(&ctx);
        app.receive(&ctx);
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(app.next_wake(&ctx), None, "idle: wait for input");
    app.generation = app.generation.wrapping_add(1);
    assert!(app.next_wake(&ctx).unwrap() <= Duration::from_secs(2));
    app.generation = app.persisted;
    app.busy = true;
    assert_eq!(app.next_wake(&ctx), Some(Duration::from_millis(50)));
    app.busy = false;
    app.workspace.open[0].follow = true;
    assert!(app.next_wake(&ctx).unwrap() <= Duration::from_secs(1));
}

#[test]
fn a_request_from_another_hedit_process_wakes_the_window() {
    let (directory, ctx, _app) = app_with("");
    let requests = directory.path().join("requests");
    std::fs::create_dir_all(&requests).unwrap();
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    assert!(!ctx.has_requested_repaint());
    std::fs::write(requests.join("1.request"), "{}").unwrap();
    let started = Instant::now();
    while !ctx.has_requested_repaint() {
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "watcher did not wake the window"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn dialogs_offer_cancel_and_confirm_with_enter() {
    let (_directory, ctx, mut app) = app_with("one\ntwo\nthree\n");
    app.execute(&ctx, Command::Goto);
    frame(&ctx, &mut app, vec![]);
    click_label(&ctx, &mut app, "Cancel");
    assert!(!app.goto_open);

    app.execute(&ctx, Command::Goto);
    frame(&ctx, &mut app, vec![]);
    // Enter does nothing until the field holds a line number.
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    assert!(app.goto_open);
    frame(&ctx, &mut app, vec![egui::Event::Text("3".into())]);
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    finish_app_job(&mut app, &ctx);
    assert!(!app.goto_open);
    assert_eq!(app.active().view.head, 8);
}

#[test]
fn panels_and_banners_share_one_close_button() {
    let (_directory, ctx, mut app) = app_with("notes");
    app.error = Some("Something failed.".into());
    app.find_open = true;
    app.build.open = true;
    let output = frame(&ctx, &mut app, vec![]);
    // The old text buttons are gone; each surface draws the same ×.
    for label in ["Dismiss", "×", "Close panel", "Done"] {
        assert!(
            !output
                .shapes
                .iter()
                .any(|shape| text_rect(&shape.shape, label).is_some()),
            "{label}"
        );
    }
}
