//! Regression coverage for the redesigned chrome using actual pointer/key events.
use super::tests::{click_at, frame_size, text_rect};
use super::*;

const SIZE: egui::Vec2 = egui::vec2(720.0, 480.0);

fn app() -> (tempfile::TempDir, egui::Context, HeditApp) {
    let directory = tempfile::tempdir().unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    (directory, ctx, app)
}

#[test]
fn redesign_tabs_display_rtl_filenames_and_switch_to_the_correct_buffer() {
    for name in ["ملاحظات.txt", "שלום.txt", "notes-مرحبا.rs"] {
        let (_directory, ctx, mut app) = app();
        app.workspace.name_scratchpad(0, name).unwrap();
        app.workspace.new_buffer().unwrap();
        for _ in 0..3 {
            frame_size(&ctx, &mut app, vec![], SIZE);
        }
        let output = frame_size(&ctx, &mut app, vec![], SIZE);
        let visual = hedit::bidi::display(name);
        let row = output
            .shapes
            .iter()
            .find_map(|s| text_rect(&s.shape, &visual))
            .unwrap();
        click_at(&ctx, &mut app, row.center(), SIZE);
        assert_eq!(app.workspace.active, 0);
        assert_eq!(app.active().view.name, name);
        assert!(!output
            .shapes
            .iter()
            .any(|s| text_rect(&s.shape, "OPEN FILES").is_some()));
    }
}

#[test]
fn redesign_status_actions_fit_with_follow_multicursor_and_a_long_notice() {
    let (_directory, ctx, mut app) = app();
    let language = syntax::Language::ALL
        .into_iter()
        .max_by_key(|l| l.label().len())
        .unwrap();
    app.set_syntax(Some(language));
    app.workspace.open[0].follow = true;
    app.workspace.open[0].view.selections = vec![hedit::advanced::Selection::caret(0); 12];
    app.notice = "An external file changed in a very long project path. ".repeat(30);
    for _ in 0..3 {
        frame_size(&ctx, &mut app, vec![], SIZE);
    }
    let output = frame_size(&ctx, &mut app, vec![], SIZE);
    let status = egui::containers::panel::PanelState::load(&ctx, egui::Id::new("status"))
        .unwrap()
        .rect;
    for label in [
        format!("{} ⏷", language.label()),
        "UTF-8".into(),
        "LF".into(),
        "Spaces: 4".into(),
    ] {
        let rect = output
            .shapes
            .iter()
            .find_map(|s| text_rect(&s.shape, &label))
            .unwrap_or_else(|| panic!("missing {label}"));
        assert!(
            status.contains_rect(rect),
            "{label} was clipped: {rect:?}, status {status:?}"
        );
    }
}

#[test]
fn redesign_find_and_replace_remain_clickable_beside_a_wide_sidebar() {
    use super::tests::finish_app_job;
    for theme in [Theme::default(), Theme::paper()] {
        let (_directory, ctx, mut app) = app();
        app.set_theme(&ctx, theme);
        app.workspace.open[0].page.text = "cat cat dog".into();
        app.workspace.edit(0, "").unwrap();
        for _ in 0..3 {
            frame_size(&ctx, &mut app, vec![], SIZE);
        }
        // Resize the sidebar through the same pointer gesture a user makes.
        let side = egui::containers::panel::PanelState::load(&ctx, egui::Id::new("buffers"))
            .unwrap()
            .rect;
        let from = egui::pos2(side.right() - 1.0, side.center().y);
        let to = egui::pos2(359.0, side.center().y);
        frame_size(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(from),
                egui::Event::PointerButton {
                    pos: from,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ],
            SIZE,
        );
        frame_size(&ctx, &mut app, vec![egui::Event::PointerMoved(to)], SIZE);
        frame_size(
            &ctx,
            &mut app,
            vec![egui::Event::PointerButton {
                pos: to,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
            SIZE,
        );
        let side = egui::containers::panel::PanelState::load(&ctx, egui::Id::new("buffers"))
            .unwrap()
            .rect;
        assert!(side.width() >= 350.0, "sidebar did not resize: {side:?}");
        app.execute(&ctx, Command::Replace);
        app.query = "cat".into();
        app.replacement = "fox".into();
        for _ in 0..3 {
            frame_size(&ctx, &mut app, vec![], SIZE);
        }
        let output = frame_size(&ctx, &mut app, vec![], SIZE);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, SIZE);
        let mut replace = None;
        for label in ["Find next", "Replace", "Replace selected", "Replace all"] {
            let (shape, rect) = output
                .shapes
                .iter()
                .find_map(|s| text_rect(&s.shape, label).map(|r| (s, r)))
                .unwrap();
            assert!(
                screen.contains_rect(rect) && shape.clip_rect.contains_rect(rect),
                "{label} clipped: {rect:?}, clip {:?}",
                shape.clip_rect
            );
            if label == "Replace all" {
                replace = Some(rect.center());
            }
        }
        click_at(&ctx, &mut app, replace.unwrap(), SIZE);
        assert_eq!(app.active().page.text, "fox fox dog");
        app.history(&ctx, false);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, "cat cat dog");
    }
}
