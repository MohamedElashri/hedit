//! Switching tabs from the keyboard.
use super::tests::{frame, key};
use super::*;

fn app_with_tabs(count: usize) -> (tempfile::TempDir, egui::Context, HeditApp) {
    let directory = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
    for _ in 1..count {
        workspace.new_buffer().unwrap();
    }
    workspace.active = 0;
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    frame(&ctx, &mut app, vec![]);
    (directory, ctx, app)
}

/// One frame with `held` modifiers down, as when Ctrl stays pressed.
fn held(ctx: &egui::Context, app: &mut HeditApp, held: Modifiers, events: Vec<egui::Event>) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 820.0),
        )),
        focused: true,
        modifiers: held,
        events,
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| {
        app.shortcuts(ctx);
        app.editor(ctx);
    });
}

fn control() -> Modifiers {
    if cfg!(target_os = "macos") {
        Modifiers::CTRL
    } else {
        Modifiers::COMMAND
    }
}

#[test]
fn page_keys_step_through_tabs_in_order_and_wrap() {
    let (_directory, ctx, mut app) = app_with_tabs(3);
    frame(&ctx, &mut app, vec![key(Key::PageDown, control())]);
    assert_eq!(app.workspace.active, 1);
    frame(&ctx, &mut app, vec![key(Key::PageDown, control())]);
    frame(&ctx, &mut app, vec![key(Key::PageDown, control())]);
    assert_eq!(app.workspace.active, 0, "wraps to the first tab");
    frame(&ctx, &mut app, vec![key(Key::PageUp, control())]);
    assert_eq!(app.workspace.active, 2, "wraps to the last tab");
}

#[test]
fn number_keys_choose_a_tab_by_position_without_typing() {
    let (_directory, ctx, mut app) = app_with_tabs(3);
    let position = if cfg!(target_os = "macos") {
        Modifiers::COMMAND
    } else {
        Modifiers::ALT
    };
    app.focus_editor = true;
    frame(&ctx, &mut app, vec![]);
    frame(
        &ctx,
        &mut app,
        vec![key(Key::Num3, position), egui::Event::Text("3".into())],
    );
    assert_eq!(app.workspace.active, 2);
    assert!(app.workspace.open.iter().all(|b| b.page.text.is_empty()));
    frame(&ctx, &mut app, vec![key(Key::Num9, position)]);
    assert_eq!(app.workspace.active, 2, "a missing tab changes nothing");
    // Alt+Shift+1 is still a layout, not tab 1.
    if !cfg!(target_os = "macos") {
        frame(
            &ctx,
            &mut app,
            vec![key(Key::Num2, Modifiers::ALT | Modifiers::SHIFT)],
        );
        assert_eq!(app.workspace.active, 2);
        assert_eq!(app.panes.mode, 1);
    }
}

#[test]
fn control_tab_walks_recently_used_tabs_while_held() {
    let (_directory, ctx, mut app) = app_with_tabs(4);
    // Use tabs in the order 0, 2, 3: most recent first is 3, 2, 0, 1.
    for index in [2, 3] {
        app.select_tab(index);
        frame(&ctx, &mut app, vec![]);
    }
    held(&ctx, &mut app, control(), vec![key(Key::Tab, control())]);
    assert_eq!(app.workspace.active, 2);
    held(&ctx, &mut app, control(), vec![key(Key::Tab, control())]);
    assert_eq!(app.workspace.active, 0, "holding Ctrl goes further back");
    // Releasing Ctrl makes tab 0 the most recent; Ctrl+Tab then returns to 3.
    held(&ctx, &mut app, Modifiers::NONE, vec![]);
    held(&ctx, &mut app, control(), vec![key(Key::Tab, control())]);
    assert_eq!(app.workspace.active, 3);
    held(&ctx, &mut app, Modifiers::NONE, vec![]);
    let back = Modifiers {
        shift: true,
        ..control()
    };
    held(&ctx, &mut app, back, vec![key(Key::Tab, back)]);
    assert_eq!(
        app.workspace.active, 1,
        "Ctrl+Shift+Tab starts from the least recent"
    );
}
