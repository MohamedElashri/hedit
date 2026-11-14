//! Differential checks against the pre-index cursor implementation. Keep the
//! reference linear so changes to boundary and goal-column semantics are visible.
use super::*;
use egui::epaint::text::{
    cursor::{CCursor, Cursor, PCursor, RCursor},
    Galley, LayoutJob, TextFormat,
};
use egui::{pos2, Rect, Vec2};
mod legacy {
    use super::*;
    pub(super) fn end_pos(galley: &Galley) -> Rect {
        if let Some(row) = galley.rows.last() {
            let x = row.rect.right();
            Rect::from_min_max(pos2(x, row.min_y()), pos2(x, row.max_y()))
        } else {
            // Empty galley
            Rect::from_min_max(pos2(0.0, 0.0), pos2(0.0, 0.0))
        }
    }

    pub(super) fn pos_from_cursor(galley: &Galley, cursor: &Cursor) -> Rect {
        pos_from_pcursor(galley, cursor.pcursor) // pcursor is what TextEdit stores
    }

    pub(super) fn pos_from_pcursor(galley: &Galley, pcursor: PCursor) -> Rect {
        let mut it = PCursor::default();

        for row in &galley.rows {
            if it.paragraph == pcursor.paragraph {
                // Right paragraph, but is it the right row in the paragraph?

                if it.offset <= pcursor.offset
                    && (pcursor.offset <= it.offset + row.char_count_excluding_newline()
                        || row.ends_with_newline)
                {
                    let column = pcursor.offset - it.offset;

                    let select_next_row_instead = pcursor.prefer_next_row
                        && !row.ends_with_newline
                        && column >= row.char_count_excluding_newline();
                    if !select_next_row_instead {
                        let x = row.x_offset(column);
                        return Rect::from_min_max(pos2(x, row.min_y()), pos2(x, row.max_y()));
                    }
                }
            }

            if row.ends_with_newline {
                it.paragraph += 1;
                it.offset = 0;
            } else {
                it.offset += row.char_count_including_newline();
            }
        }

        end_pos(galley)
    }

    pub(super) fn cursor_from_pos(galley: &Galley, pos: Vec2) -> Cursor {
        if let Some(first_row) = galley.rows.first() {
            if pos.y < first_row.min_y() {
                return Cursor::default();
            }
        }
        if let Some(last_row) = galley.rows.last() {
            if last_row.max_y() < pos.y {
                return end(galley);
            }
        }

        let mut best_y_dist = f32::INFINITY;
        let mut cursor = Cursor::default();

        let mut ccursor_index = 0;
        let mut pcursor_it = PCursor::default();

        for (row_nr, row) in galley.rows.iter().enumerate() {
            let is_pos_within_row = row.min_y() <= pos.y && pos.y <= row.max_y();
            let y_dist = (row.min_y() - pos.y).abs().min((row.max_y() - pos.y).abs());
            if is_pos_within_row || y_dist < best_y_dist {
                best_y_dist = y_dist;
                let column = row.char_at(pos.x);
                let prefer_next_row = column < row.char_count_excluding_newline();
                cursor = Cursor {
                    ccursor: CCursor {
                        index: ccursor_index + column,
                        prefer_next_row,
                    },
                    rcursor: RCursor {
                        row: row_nr,
                        column,
                    },
                    pcursor: PCursor {
                        paragraph: pcursor_it.paragraph,
                        offset: pcursor_it.offset + column,
                        prefer_next_row,
                    },
                };

                if is_pos_within_row {
                    return cursor;
                }
            }
            ccursor_index += row.char_count_including_newline();
            if row.ends_with_newline {
                pcursor_it.paragraph += 1;
                pcursor_it.offset = 0;
            } else {
                pcursor_it.offset += row.char_count_including_newline();
            }
        }

        cursor
    }

    pub(super) fn end(galley: &Galley) -> Cursor {
        if galley.rows.is_empty() {
            return Default::default();
        }
        let mut ccursor = CCursor {
            index: 0,
            prefer_next_row: true,
        };
        let mut pcursor = PCursor {
            paragraph: 0,
            offset: 0,
            prefer_next_row: true,
        };
        for row in &galley.rows {
            let row_char_count = row.char_count_including_newline();
            ccursor.index += row_char_count;
            if row.ends_with_newline {
                pcursor.paragraph += 1;
                pcursor.offset = 0;
            } else {
                pcursor.offset += row_char_count;
            }
        }
        Cursor {
            ccursor,
            rcursor: end_rcursor(galley),
            pcursor,
        }
    }

    pub(super) fn end_rcursor(galley: &Galley) -> RCursor {
        if let Some(last_row) = galley.rows.last() {
            RCursor {
                row: galley.rows.len() - 1,
                column: last_row.char_count_including_newline(),
            }
        } else {
            Default::default()
        }
    }

    pub(super) fn from_ccursor(galley: &Galley, ccursor: CCursor) -> Cursor {
        let prefer_next_row = ccursor.prefer_next_row;
        let mut ccursor_it = CCursor {
            index: 0,
            prefer_next_row,
        };
        let mut pcursor_it = PCursor {
            paragraph: 0,
            offset: 0,
            prefer_next_row,
        };

        for (row_nr, row) in galley.rows.iter().enumerate() {
            let row_char_count = row.char_count_excluding_newline();

            if ccursor_it.index <= ccursor.index
                && ccursor.index <= ccursor_it.index + row_char_count
            {
                let column = ccursor.index - ccursor_it.index;

                let select_next_row_instead = prefer_next_row
                    && !row.ends_with_newline
                    && column >= row.char_count_excluding_newline();
                if !select_next_row_instead {
                    pcursor_it.offset += column;
                    return Cursor {
                        ccursor,
                        rcursor: RCursor {
                            row: row_nr,
                            column,
                        },
                        pcursor: pcursor_it,
                    };
                }
            }
            ccursor_it.index += row.char_count_including_newline();
            if row.ends_with_newline {
                pcursor_it.paragraph += 1;
                pcursor_it.offset = 0;
            } else {
                pcursor_it.offset += row.char_count_including_newline();
            }
        }
        debug_assert!(ccursor_it == end(galley).ccursor);
        Cursor {
            ccursor: ccursor_it, // clamp
            rcursor: end_rcursor(galley),
            pcursor: pcursor_it,
        }
    }

    pub(super) fn from_rcursor(galley: &Galley, rcursor: RCursor) -> Cursor {
        if rcursor.row >= galley.rows.len() {
            return end(galley);
        }

        let prefer_next_row =
            rcursor.column < galley.rows[rcursor.row].char_count_excluding_newline();
        let mut ccursor_it = CCursor {
            index: 0,
            prefer_next_row,
        };
        let mut pcursor_it = PCursor {
            paragraph: 0,
            offset: 0,
            prefer_next_row,
        };

        for (row_nr, row) in galley.rows.iter().enumerate() {
            if row_nr == rcursor.row {
                ccursor_it.index += rcursor.column.min(row.char_count_excluding_newline());

                if row.ends_with_newline {
                    // Allow offset to go beyond the end of the paragraph
                    pcursor_it.offset += rcursor.column;
                } else {
                    pcursor_it.offset += rcursor.column.min(row.char_count_excluding_newline());
                }
                return Cursor {
                    ccursor: ccursor_it,
                    rcursor,
                    pcursor: pcursor_it,
                };
            }
            ccursor_it.index += row.char_count_including_newline();
            if row.ends_with_newline {
                pcursor_it.paragraph += 1;
                pcursor_it.offset = 0;
            } else {
                pcursor_it.offset += row.char_count_including_newline();
            }
        }
        Cursor {
            ccursor: ccursor_it,
            rcursor: end_rcursor(galley),
            pcursor: pcursor_it,
        }
    }

    pub(super) fn from_pcursor(galley: &Galley, pcursor: PCursor) -> Cursor {
        let prefer_next_row = pcursor.prefer_next_row;
        let mut ccursor_it = CCursor {
            index: 0,
            prefer_next_row,
        };
        let mut pcursor_it = PCursor {
            paragraph: 0,
            offset: 0,
            prefer_next_row,
        };

        for (row_nr, row) in galley.rows.iter().enumerate() {
            if pcursor_it.paragraph == pcursor.paragraph {
                // Right paragraph, but is it the right row in the paragraph?

                if pcursor_it.offset <= pcursor.offset
                    && (pcursor.offset <= pcursor_it.offset + row.char_count_excluding_newline()
                        || row.ends_with_newline)
                {
                    let column = pcursor.offset - pcursor_it.offset;

                    let select_next_row_instead = pcursor.prefer_next_row
                        && !row.ends_with_newline
                        && column >= row.char_count_excluding_newline();

                    if !select_next_row_instead {
                        ccursor_it.index += column.min(row.char_count_excluding_newline());

                        return Cursor {
                            ccursor: ccursor_it,
                            rcursor: RCursor {
                                row: row_nr,
                                column,
                            },
                            pcursor,
                        };
                    }
                }
            }

            ccursor_it.index += row.char_count_including_newline();
            if row.ends_with_newline {
                pcursor_it.paragraph += 1;
                pcursor_it.offset = 0;
            } else {
                pcursor_it.offset += row.char_count_including_newline();
            }
        }
        Cursor {
            ccursor: ccursor_it,
            rcursor: end_rcursor(galley),
            pcursor,
        }
    }
}
fn layout(text: &str, width: f32, scale: f32, max_rows: usize) -> Arc<Galley> {
    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(scale);
    let mut result = None;
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let mut job = LayoutJob::default();
            job.wrap.max_width = width;
            job.wrap.max_rows = max_rows;
            job.append(
                text,
                0.0,
                TextFormat {
                    font_id: FontId::monospace(14.0),
                    ..Default::default()
                },
            );
            result = Some(ui.fonts(|fonts| fonts.layout_job(job)));
        });
    });
    result.unwrap()
}
fn same(actual: Cursor, expected: Cursor) {
    // Upstream cursor equality intentionally ignores preferred wrap sides.
    // Compare them separately, because preserving those flags is essential.
    assert_eq!(actual, expected);
    assert_eq!(
        actual.ccursor.prefer_next_row,
        expected.ccursor.prefer_next_row
    );
    assert_eq!(
        actual.pcursor.prefer_next_row,
        expected.pcursor.prefer_next_row
    );
}
fn compare(g: &Galley) {
    same(g.end(), legacy::end(g));
    assert_eq!(g.end_rcursor(), legacy::end_rcursor(g));
    let total: usize = g
        .rows
        .iter()
        .map(|r| r.char_count_including_newline())
        .sum();
    for prefer_next_row in [false, true] {
        for index in 0..=total + 3 {
            let c = CCursor {
                index,
                prefer_next_row,
            };
            same(g.from_ccursor(c), legacy::from_ccursor(g, c));
            assert_eq!(
                g.pos_from_ccursor(c),
                legacy::pos_from_cursor(g, &legacy::from_ccursor(g, c))
            );
        }
        let paragraphs = legacy::end(g).pcursor.paragraph + 3;
        for paragraph in 0..paragraphs {
            for offset in 0..=total + 3 {
                let c = PCursor {
                    paragraph,
                    offset,
                    prefer_next_row,
                };
                same(g.from_pcursor(c), legacy::from_pcursor(g, c));
                assert_eq!(g.pos_from_pcursor(c), legacy::pos_from_pcursor(g, c));
            }
        }
    }
    for row in 0..g.rows.len() + 3 {
        for column in 0..total + 3 {
            let c = RCursor { row, column };
            same(g.from_rcursor(c), legacy::from_rcursor(g, c));
            assert_eq!(
                g.pos_from_rcursor(c),
                legacy::pos_from_cursor(g, &legacy::from_rcursor(g, c))
            );
        }
    }
    for y in [
        -100.0,
        -0.0,
        0.0,
        0.25,
        5.0,
        100.0,
        1000.0,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ]
    .into_iter()
    .chain(g.rows.iter().flat_map(|r| {
        [
            r.min_y() - 0.001,
            r.min_y(),
            r.max_y(),
            r.max_y() + 0.001,
            (r.min_y() + r.max_y()) / 2.0,
        ]
    })) {
        for x in [
            -100.0,
            0.0,
            5.0,
            20.0,
            100.0,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NAN,
        ] {
            let pos = Vec2::new(x, y);
            same(g.cursor_from_pos(pos), legacy::cursor_from_pos(g, pos));
        }
        let expected = g
            .rows
            .iter()
            .position(|r| y < r.max_y())
            .unwrap_or(g.rows.len().saturating_sub(1));
        assert_eq!(g.row_at_y(y), expected);
    }
    for row in 0..=g.rows.len() {
        assert_eq!(
            g.row_start(row),
            g.rows[..row]
                .iter()
                .map(|r| r.char_count_including_newline())
                .sum::<usize>()
        );
    }
}
#[test]
fn indexed_cursors_match_linear_reference_at_every_boundary_and_goal_column() {
    for text in [
        "",
        "\n",
        "\n\n",
        "a\n",
        "a\r\nb\r\n",
        "α🦀\tשלום\n\nlast",
        "a long soft wrapped paragraph with spaces and punctuation, then\nnext line\n",
    ] {
        for width in [0.0, 25.0, 90.0, f32::INFINITY] {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                for max_rows in [0, 1, 2, usize::MAX] {
                    compare(&layout(text, width, scale, max_rows));
                }
            }
        }
    }
}
#[test]
fn overlapping_reordered_and_empty_soft_rows_preserve_hit_testing_and_wrapping() {
    for mode in [
        "overlap",
        "reorder",
        "gap",
        "empty-soft",
        "last-newline",
        "zero-height",
    ] {
        let mut g = (*layout("one two\nα🦀\nthree four five", 40.0, 1.25, usize::MAX)).clone();
        match mode {
            "overlap" => {
                for row in &mut g.rows {
                    let row = Arc::make_mut(row);
                    row.rect.min.y = 0.0;
                    row.rect.max.y = 20.0;
                }
            }
            "reorder" => g.rows.reverse(),
            "gap" => {
                for (i, row) in g.rows.iter_mut().enumerate() {
                    let row = Arc::make_mut(row);
                    row.rect = row.rect.translate(egui::vec2(0.0, i as f32 * 11.5));
                }
            }
            "empty-soft" => {
                let mut row = (*g.rows[0]).clone();
                row.glyphs.clear();
                row.ends_with_newline = false;
                g.rows.insert(0, Arc::new(row));
            }
            "zero-height" => {
                let last = g.rows.len() - 1;
                for (i, row) in g.rows.iter_mut().enumerate() {
                    let row = Arc::make_mut(row);
                    row.rect.min.y = if i == last { 10.0 } else { 0.0 };
                    row.rect.max.y = if i == last { 20.0 } else { 0.0 };
                }
            }
            "last-newline" => Arc::make_mut(g.rows.last_mut().unwrap()).ends_with_newline = true,
            _ => unreachable!(),
        }
        g.invalidate_cursor_index();
        compare(&g);
    }
}
#[test]
fn rtl_shaping_preserves_logical_metadata_and_matches_reference_positions() {
    for scale in [1.0, 1.25, 1.5] {
        let g = layout(
            "α English שלום 123\nمرحبا mixed text\nlast line",
            80.0,
            scale,
            usize::MAX,
        );
        let ctx = egui::Context::default();
        let mut shaped = None;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                shaped =
                    Some(ui.fonts(|f| bidi_view::apply(f, g.clone(), bidi_view::Base::Auto).0));
            });
        });
        compare(&shaped.unwrap());
    }
}
#[test]
fn cursor_cache_is_shared_derived_data_and_explicit_mutation_resets_it() {
    let g = layout("alpha\nbeta", 200.0, 1.0, usize::MAX);
    let cold = (*g).clone();
    g.cursor_index();
    assert_eq!(*g, cold, "derived metadata must not affect galley equality");
    let warm = (*g).clone();
    assert!(std::ptr::eq(g.cursor_index(), warm.cursor_index()));
    assert!(g.cursor_cache.bytes() > 0);
    let serialized = serde_json::to_string(&*g).unwrap();
    assert!(!serialized.contains("cursor_cache"));
    let restored: Galley = serde_json::from_str(&serialized).unwrap();
    assert_eq!(restored.cursor_cache.bytes(), 0);
    compare(&restored);
    let mut changed = warm;
    Arc::make_mut(&mut changed.rows[0]).glyphs.pop();
    changed.invalidate_cursor_index();
    compare(&changed);
    let g = g.clone();
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let g = g.clone();
            thread::spawn(move || std::ptr::from_ref(g.cursor_index()) as usize)
        })
        .collect();
    let addresses: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(addresses.iter().all(|a| *a == addresses[0]));
}

#[test]
fn mixed_fonts_leading_space_and_line_spacing_match_the_original_geometry() {
    for scale in [1.0, 1.25, 1.5] {
        for align in [egui::Align::LEFT, egui::Align::Center, egui::Align::RIGHT] {
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(scale);
            let mut g = None;
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut job = LayoutJob::default();
                    job.wrap.max_width = 80.0;
                    job.halign = align;
                    job.first_row_min_height = 31.25;
                    job.round_output_to_gui = false;
                    for (text, size, leading, height) in [
                        ("first ", 14.0, 100.0, 14.0),
                        ("α🦀 words\n", 22.0, 0.0, 8.0),
                        ("\nשלום long words wrap here", 12.0, 0.0, 28.5),
                    ] {
                        job.append(
                            text,
                            leading,
                            TextFormat {
                                font_id: FontId::monospace(size),
                                line_height: Some(height),
                                ..Default::default()
                            },
                        );
                    }
                    g = Some(ui.fonts(|f| f.layout_job(job)));
                });
            });
            compare(&g.unwrap());
        }
    }
}
#[test]
fn cold_cursor_index_initializes_once_across_threads() {
    let g = layout("cold index\nwith several rows", 100.0, 1.0, usize::MAX);
    assert_eq!(g.cursor_cache.bytes(), 0);
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let g = g.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                std::ptr::from_ref(g.cursor_index()) as usize
            })
        })
        .collect();
    let addresses: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(addresses.iter().all(|a| *a == addresses[0]));
}
#[test]
fn incremental_layout_edits_rebuild_cursor_metadata_at_start_middle_and_end() {
    let ctx = egui::Context::default();
    let mut cache = editor_layout::LayoutCache::default();
    let mut source = "\tlet value = compute(item); // שלום\n".repeat(600);
    let theme = hedit::theme::Theme::default();
    for step in 0..12 {
        let mut g = None;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                cache.activate((1, 0));
                g = Some(cache.layout(
                    ui,
                    &source,
                    hedit::syntax::Language::Rust,
                    editor_layout::Font {
                        size: 14.0,
                        line_height: None,
                    },
                    180.0,
                    &theme,
                ));
            });
        });
        let g = g.unwrap();
        same(g.end(), legacy::end(&g));
        let total = legacy::end(&g).ccursor.index;
        for index in [
            0,
            1,
            total / 3,
            total / 2,
            total.saturating_sub(1),
            total,
            total + 1,
        ] {
            for prefer_next_row in [false, true] {
                let c = CCursor {
                    index,
                    prefer_next_row,
                };
                same(g.from_ccursor(c), legacy::from_ccursor(&g, c));
                assert_eq!(
                    g.pos_from_ccursor(c),
                    legacy::pos_from_cursor(&g, &legacy::from_ccursor(&g, c))
                );
            }
        }
        let chars = source.chars().count();
        let at = match step % 3 {
            0 => 0,
            1 => chars / 2,
            _ => chars,
        };
        let byte = hedit::advanced::byte(&source, at);
        source.insert_str(byte, if step % 2 == 0 { "α\n" } else { "x\t" });
    }
}

#[test]
fn pointer_drag_across_frames_selects_text_and_undo_restores_it_in_a_large_editor() {
    use crate::app::tests::{finish_app_job, frame};
    let dir = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
    let source = "    let value = item;\n".repeat(2000);
    workspace.open[0].page.text = source.clone();
    workspace.edit(0, "").unwrap();
    workspace.open[0].view.anchor = source.len() - 200;
    workspace.open[0].view.head = source.len() - 200;
    workspace.open[0].restore_cursor = true;
    workspace.open[0].reveal_cursor = true;
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    ctx.style_mut(|style| style.scroll_animation = egui::style::ScrollAnimation::none());
    // The initial scroll request is applied after layout; wait for its viewport.
    for _ in 0..6 {
        frame(&ctx, &mut app, vec![]);
        if app.caret_screen.is_some() {
            break;
        }
    }
    let start = app
        .caret_screen
        .expect("restored caret must become visible")
        .center();
    let end = start + egui::vec2(20.0, 40.0);
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
        ],
    );
    let anchor = app.active().view.anchor;
    frame(&ctx, &mut app, vec![egui::Event::PointerMoved(end)]);
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert_eq!(app.active().view.anchor, anchor);
    assert!(
        app.active().view.head > anchor,
        "dragging must preserve its anchor across frames"
    );
    let count = app.active().view.head - anchor;
    assert_eq!(app.active().page.text, source);
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Backspace,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert_eq!(
        app.active().page.text.chars().count(),
        source.chars().count() - count
    );
    app.history(&ctx, false);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().page.text, source);
}
