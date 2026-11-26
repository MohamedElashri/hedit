//! Long-line coordinates: compare independent scalar walks with indexed lookups.
use super::*;
#[test]
#[ignore = "manual optimized profile of long-line coordinate lookup"]
fn profile_long_line_coordinates() {
    for (name, text) in [
        ("165k ASCII", "abcdefghij ".repeat(15_000)),
        ("1M ASCII", "abcdefghij".repeat(100_000)),
        ("Unicode tabs", "α🦀\tאב\r".repeat(20_000)),
    ] {
        let index = editor_layout::TextIndex::new(&text);
        let g = row_index::tests::layout(&text, f32::INFINITY, 1.0);
        let row = &g.rows[0];
        let position = egui::vec2(row.rect().right() - 17.0, row.rect().center().y);
        g.cursor_from_pos(position);
        for operation in ["byte", "column", "pointer"] {
            let start = Instant::now();
            for n in 0..1000 {
                let cursor = std::hint::black_box(index.chars.saturating_sub(1 + n % 32));
                match operation {
                    "byte" => {
                        std::hint::black_box(index.byte(&text, cursor));
                    }
                    "column" => {
                        std::hint::black_box(index.line_column(&text, cursor, 4));
                    }
                    _ => {
                        std::hint::black_box(g.cursor_from_pos(std::hint::black_box(position)));
                    }
                }
            }
            eprintln!(
                "Long-line {name} {operation}: {:.3} us/query",
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
}

#[test]
fn long_line_text_buffer_tracks_multiple_same_frame_edits_and_undo_in_split_groups() {
    use crate::app::tests::{finish_app_job, frame};
    for unit in ["α🦀value", "α🦀\tvalue"] {
        let dir = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
        let source = format!("{}\r\nlast", unit.repeat(2000));
        workspace.open[0].page.text = source.clone();
        workspace.edit_with_line_ending(0, "", Some(true)).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        assert_eq!(app.active().page.info.eol, "CRLF");
        app.settings.auto_complete = false;
        app.settings.caret_blink = false;
        frame(&ctx, &mut app, vec![]);
        app.split(1);
        app.focus_group(1, false);
        let position = 7_999;
        app.set_selections(vec![hedit::advanced::Selection::caret(position)]);
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::Text("a".into()),
                egui::Event::Text("🦀".into()),
                crate::app::tests::key(Key::Backspace, Modifiers::NONE),
            ],
        );
        let mut expected = source.clone();
        expected.insert(hedit::advanced::byte(&source, position), 'a');
        assert!(
            app.active().page.text == expected,
            "text must match the independent byte-range edit"
        );
        assert_eq!(
            (app.active().view.anchor, app.active().view.head),
            (position + 1, position + 1)
        );
        // Replacing a range across sparse checkpoints exercises both byte endpoints
        // and a CRLF insertion through the actual TextBuffer path.
        app.set_selections(vec![hedit::advanced::Selection {
            anchor: 255,
            head: 1025,
        }]);
        frame(&ctx, &mut app, vec![]);
        frame(
            &ctx,
            &mut app,
            vec![egui::Event::Paste("new🦀\nline".into())],
        );
        let before_paste = expected.clone();
        let a = hedit::advanced::byte(&expected, 255);
        let b = hedit::advanced::byte(&expected, 1025);
        expected.replace_range(a..b, "new🦀\r\nline");
        assert!(
            app.active().page.text == expected,
            "text must match the independent byte-range edit"
        );
        app.history(&ctx, false);
        finish_app_job(&mut app, &ctx);
        assert!(
            app.active().page.text == before_paste,
            "Undo and pane changes must preserve the original text"
        );
        frame(&ctx, &mut app, vec![]);
        app.focus_group(0, false);
        frame(&ctx, &mut app, vec![]);
        assert!(
            app.active().page.text == before_paste,
            "Undo and pane changes must preserve the original text"
        );
    }
}
