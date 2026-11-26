use super::*;
use crate::app::tests::{frame, key};
fn app(text: &str) -> (tempfile::TempDir, egui::Context, HeditApp) {
    let dir = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
    workspace.open[0].page.text = text.into();
    workspace.edit(0, "").unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    ctx.global_style_mut(|style| style.scroll_animation = egui::style::ScrollAnimation::none());
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![]);
    (dir, ctx, app)
}
#[test]
fn vertical_navigation_keeps_its_column_across_frames_and_short_lines() {
    let (_dir, ctx, mut app) = app("abcdefghij\nx\nabcdefghij\n");
    app.set_selections(vec![hedit::advanced::Selection::caret(8)]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::ArrowDown, Modifiers::NONE)]);
    assert_eq!(app.active().view.head, 12);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::ArrowDown, Modifiers::NONE)]);
    assert_eq!(
        app.active().view.head,
        21,
        "the original column should survive the short line"
    );
}
#[test]
fn pointer_drag_with_press_move_and_release_in_one_frame_keeps_the_press_anchor() {
    let (_dir, ctx, mut app) = app("abcdefghij\nklmnopqrst\nuvwxyz\n");
    let start = app.caret_screen.unwrap().center() + egui::vec2(10.0, 0.0);
    let end = start + egui::vec2(32.0, 20.0);
    frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
            egui::Event::PointerMoved(end),
            egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    assert!(
        app.active().view.head > app.active().view.anchor,
        "a complete drag must retain its starting anchor"
    );
}
#[test]
#[ignore = "manual release CPU profile of minimap frames"]
fn profile_minimap_interaction() {
    let source: String = (0..40_000)
        .map(|i| format!("    let value_{i} = compute_{}(item);\n", i % 997))
        .collect();
    let (_dir, ctx, mut app) = app(&source);
    app.settings.auto_complete = false;
    for enabled in [false, true] {
        app.settings.show_minimap = enabled;
        for _ in 0..4 {
            frame(&ctx, &mut app, vec![]);
        }
        let start = Instant::now();
        for _ in 0..60 {
            frame(&ctx, &mut app, vec![]);
        }
        eprintln!(
            "Minimap {enabled}, {} bytes: {:.3} ms/warm CPU frame",
            source.len(),
            start.elapsed().as_secs_f64() * 1000.0 / 60.0
        );
    }
}

#[test]
fn explicit_navigation_to_same_position_resets_preferred_column() {
    let (_dir, ctx, mut app) = app("abcdefghij\nx\nabcdefghij\n");
    app.set_selections(vec![hedit::advanced::Selection::caret(8)]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::ArrowDown, Modifiers::NONE)]);
    app.set_selections(vec![hedit::advanced::Selection::caret(12)]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::ArrowDown, Modifiers::NONE)]);
    assert_eq!(app.active().view.head, 14);
}

#[test]
fn shift_vertical_navigation_preserves_anchor_and_goal_through_blank_unicode_lines() {
    let (_dir, ctx, mut app) = app("abcdefghij\n\nαβγδεζηθικ\n");
    app.set_selections(vec![hedit::advanced::Selection::caret(8)]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::ArrowDown, Modifiers::SHIFT)]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::ArrowDown, Modifiers::SHIFT)]);
    assert_eq!((app.active().view.anchor, app.active().view.head), (8, 20));
    frame(&ctx, &mut app, vec![key(Key::ArrowUp, Modifiers::NONE)]);
    frame(&ctx, &mut app, vec![key(Key::ArrowUp, Modifiers::NONE)]);
    assert_eq!(app.active().view.head, 8);
}

fn drag(start: egui::Pos2, end: egui::Pos2, modifiers: Modifiers) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(start),
        egui::Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers,
        },
        egui::Event::PointerMoved(end),
        egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers,
        },
    ]
}
#[test]
fn coalesced_backward_drag_and_shift_extension_survive_idle_repaint() {
    for shift in [false, true] {
        let (_dir, ctx, mut app) = app("abcdefghij\nklmnopqrst\nuvwxyz\n");
        let origin = app.caret_screen.unwrap().center();
        let start = origin + egui::vec2(50.0, 20.0);
        let end = origin + egui::vec2(10.0, 0.0);
        frame(
            &ctx,
            &mut app,
            drag(
                start,
                end,
                if shift {
                    Modifiers::SHIFT
                } else {
                    Modifiers::NONE
                },
            ),
        );
        let selection = (app.active().view.anchor, app.active().view.head);
        if shift {
            assert_eq!(selection.0, 0);
            assert!(selection.1 > 0);
        } else {
            assert!(selection.0 > selection.1);
        }
        frame(&ctx, &mut app, vec![]);
        assert_eq!(
            (app.active().view.anchor, app.active().view.head),
            selection
        );
    }
}
#[test]
fn coalesced_drag_cannot_select_through_a_modal_or_from_outside_editor() {
    for modal in [false, true] {
        let (_dir, ctx, mut app) = app("abcdefghij\nklmnopqrst\nuvwxyz\n");
        let origin = app.caret_screen.unwrap().center();
        if modal {
            app.about_open = true;
            frame(&ctx, &mut app, vec![]);
        }
        let start = if modal {
            origin + egui::vec2(20.0, 0.0)
        } else {
            egui::pos2(0.0, 0.0)
        };
        frame(
            &ctx,
            &mut app,
            drag(start, origin + egui::vec2(50.0, 20.0), Modifiers::NONE),
        );
        assert_eq!(app.active().view.anchor, app.active().view.head);
    }
}

#[test]
fn same_length_external_text_change_cannot_reuse_old_paragraph_coordinates() {
    let (_dir, ctx, mut app) = app("abc\ndef\nghi\n");
    app.set_selections(vec![hedit::advanced::Selection::caret(6)]);
    frame(&ctx, &mut app, vec![]);
    // Replace a newline without moving the public cursor or setting reveal.
    let old = app.active().page.text.clone();
    app.workspace.open[0].page.text = "abc def\nghi\n".into();
    app.workspace.edit(0, &old).unwrap();
    app.workspace.open[0].reveal_cursor = false;
    frame(&ctx, &mut app, vec![]);
    assert_eq!(app.active().view.head, 6);
    frame(&ctx, &mut app, vec![key(Key::ArrowRight, Modifiers::NONE)]);
    assert_eq!(app.active().view.head, 7);
}

#[test]
fn soft_wrap_affinity_survives_idle_repaints_and_relayout() {
    use egui::text::CCursor;
    let (_dir, ctx, mut app) = app(&"abcdefghij ".repeat(20));
    app.settings.word_wrap = true;
    app.settings.wrap_width = 12;
    frame(&ctx, &mut app, vec![]);
    let (galley, _) = app.editor_layout.last().unwrap();
    assert!(galley.rows.len() > 2);
    let at = galley.row_start(1);
    app.set_selections(vec![hedit::advanced::Selection::caret(at)]);
    frame(&ctx, &mut app, vec![]);
    let id = egui::Id::unique(("editor", app.active().page.info.id, app.active().page.start));
    let mut state = egui::TextEdit::load_state(&ctx, id).unwrap();
    let cursor = CCursor::new(at);
    assert_eq!(galley.layout_from_cursor(cursor).row, 0);
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(cursor)));
    state.store(&ctx, id);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let state = egui::TextEdit::load_state(&ctx, id).unwrap();
    assert!(!state.cursor.char_range().unwrap().primary.prefer_next_row);
    app.settings.font_size += 1.0;
    frame(&ctx, &mut app, vec![]);
    assert_eq!(app.active().view.head, at);
    assert!(
        !egui::TextEdit::load_state(&ctx, id)
            .unwrap()
            .cursor
            .char_range()
            .unwrap()
            .primary
            .prefer_next_row
    );
}

#[test]
fn coalesced_selection_replacement_and_undo_preserve_crlf_and_clear_secondary_cursors() {
    let source = "abcdefghij\r\nklmnopqrst\r\nuvwxyz\r\n";
    let (_dir, ctx, mut app) = app(source);
    app.workspace.open[0].page.info.eol = "CRLF";
    app.set_selections(vec![
        hedit::advanced::Selection::caret(0),
        hedit::advanced::Selection::caret(2),
    ]);
    frame(&ctx, &mut app, vec![]);
    let origin = app.caret_screen.unwrap().center();
    frame(
        &ctx,
        &mut app,
        drag(
            origin + egui::vec2(10.0, 0.0),
            origin + egui::vec2(50.0, 20.0),
            Modifiers::NONE,
        ),
    );
    assert!(app.active().view.selections.is_empty());
    assert_ne!(app.active().view.anchor, app.active().view.head);
    frame(&ctx, &mut app, vec![egui::Event::Text("X".into())]);
    assert_ne!(app.active().page.text, source);
    app.execute(&ctx, Command::Undo);
    crate::app::tests::finish_app_job(&mut app, &ctx);
    frame(&ctx, &mut app, vec![]);
    assert_eq!(app.active().page.text, source);
}

#[test]
fn coalesced_rtl_drag_maps_expanded_tabs_back_to_source_characters() {
    let (_dir, ctx, mut app) = app("\tשלום α🦀\r\nnext\r\n");
    settings_store::fallback_fonts();
    settings_store::set_editor_font(&ctx, None);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let (galley, bidi) = app.editor_layout.last().unwrap();
    assert!(bidi.any());
    let origin = app.caret_screen.unwrap().center()
        - bidi_view::caret_rect(&galley, &bidi, 0).center().to_vec2();
    let row = &galley.rows[0];
    let first = &row.glyphs[4];
    let last = &row.glyphs[7];
    assert!(first.pos.x > last.pos.x);
    let start = origin
        + egui::vec2(
            first.pos.x + first.advance_width - 1.0,
            row.rect().center().y,
        );
    let end = origin + egui::vec2(last.pos.x + 1.0, row.rect().center().y);
    frame(&ctx, &mut app, drag(start, end, Modifiers::NONE));
    assert_eq!((app.active().view.anchor, app.active().view.head), (1, 5));
    frame(&ctx, &mut app, vec![]);
    assert_eq!((app.active().view.anchor, app.active().view.head), (1, 5));
}
