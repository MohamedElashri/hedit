use super::*;
use egui::epaint::text::cursor::LayoutCursor;
use egui::{
    text::{CCursor, CharIndex, Galley, LayoutJob, TextFormat},
    FontId, Vec2,
};

fn linear_start(galley: &Galley, row: usize) -> usize {
    galley
        .rows
        .iter()
        .take(row)
        .map(|row| row.char_count_including_newline().0)
        .sum()
}

fn linear_layout(galley: &Galley, cursor: CCursor) -> LayoutCursor {
    let mut start = 0;
    for (row, placed) in galley.rows.iter().enumerate() {
        let end = start + placed.glyphs.len();
        if start <= cursor.index.0
            && (cursor.index.0 < end
                || (cursor.index.0 == end && (!cursor.prefer_next_row || placed.ends_with_newline)))
        {
            return LayoutCursor {
                row,
                column: CharIndex(cursor.index.0 - start),
            };
        }
        start += placed.char_count_including_newline().0;
    }
    galley
        .rows
        .last()
        .map_or(LayoutCursor::default(), |row| LayoutCursor {
            row: galley.rows.len() - 1,
            column: row.char_count_including_newline(),
        })
}

fn linear_hit(galley: &Galley, pos: Vec2) -> CCursor {
    if galley
        .rows
        .first()
        .is_some_and(|row| pos.y < row.min_y() - 5.0)
    {
        return CCursor::default();
    }
    if galley
        .rows
        .last()
        .is_some_and(|row| pos.y > row.max_y() + 5.0)
    {
        return CCursor {
            index: CharIndex(linear_start(galley, galley.rows.len())),
            prefer_next_row: true,
        };
    }
    let mut best = f32::INFINITY;
    let mut cursor = CCursor::default();
    for (i, row) in galley.rows.iter().enumerate() {
        let within = row.min_y() <= pos.y && pos.y <= row.max_y();
        let distance = (row.min_y() - pos.y).abs().min((row.max_y() - pos.y).abs());
        if within || distance < best {
            best = distance;
            let column = row.char_at(pos.x - row.pos.x).0;
            cursor = CCursor {
                index: CharIndex(linear_start(galley, i) + column),
                prefer_next_row: column < row.glyphs.len(),
            };
            if within {
                break;
            }
        }
    }
    cursor
}

fn layout(text: &str, width: f32, scale: f32, max_rows: usize) -> Arc<Galley> {
    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(scale);
    let mut result = None;
    let _ = crate::app::tests::run_pass(&ctx, egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show_in_context(ctx, |ui| {
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
            result = Some(ui.fonts_mut(|fonts| fonts.layout_job(job)));
        });
    });
    result.unwrap()
}

fn same(actual: CCursor, expected: CCursor) {
    assert_eq!(actual.index, expected.index);
    assert_eq!(actual.prefer_next_row, expected.prefer_next_row);
}

fn compare(galley: &Galley) {
    let end = linear_start(galley, galley.rows.len());
    assert_eq!(galley.end().index.0, end);
    for at in 0..=end + 1 {
        for prefer_next_row in [false, true] {
            let cursor = CCursor {
                index: CharIndex(at),
                prefer_next_row,
            };
            let expected = linear_layout(galley, cursor);
            assert_eq!(galley.layout_from_cursor(cursor), expected);
            assert_eq!(
                galley.pos_from_cursor(cursor),
                galley.pos_from_layout_cursor(&expected)
            );
        }
    }
    for row in 0..=galley.rows.len() {
        assert_eq!(galley.row_start(row), linear_start(galley, row));
    }
    let ys = [
        -100.0,
        0.0,
        1000.0,
        f32::NAN,
        f32::NEG_INFINITY,
        f32::INFINITY,
    ]
    .into_iter()
    .chain(galley.rows.iter().flat_map(|row| {
        [
            row.min_y() - 0.001,
            row.min_y(),
            row.rect().center().y,
            row.max_y(),
            row.max_y() + 0.001,
        ]
    }));
    for y in ys {
        for x in [
            -100.0,
            0.0,
            5.0,
            100.0,
            f32::NAN,
            f32::NEG_INFINITY,
            f32::INFINITY,
        ] {
            let pos = Vec2::new(x, y);
            let indexed = galley.cursor_from_pos(pos);
            let linear = linear_hit(galley, pos);
            assert_eq!(
                indexed.index,
                linear.index,
                "pos={pos:?}, rows={:?}",
                galley.rows.iter().map(|r| r.rect()).collect::<Vec<_>>()
            );
            same(indexed, linear);
        }
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
                    row.pos.y = 0.0;
                    Arc::make_mut(&mut row.row).size.y = 20.0;
                }
            }
            "reorder" => g.rows.reverse(),
            "gap" => {
                for (i, row) in g.rows.iter_mut().enumerate() {
                    row.pos.y += i as f32 * 11.5;
                }
            }
            "empty-soft" => {
                let mut row = g.rows[0].clone();
                Arc::make_mut(&mut row.row).glyphs.clear();
                row.ends_with_newline = false;
                g.rows.insert(0, row);
            }
            "zero-height" => {
                let last = g.rows.len() - 1;
                for (i, row) in g.rows.iter_mut().enumerate() {
                    row.pos.y = if i == last { 10.0 } else { 0.0 };
                    Arc::make_mut(&mut row.row).size.y = if i == last { 10.0 } else { 0.0 };
                }
            }
            "last-newline" => g.rows.last_mut().unwrap().ends_with_newline = true,
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
        let _ = crate::app::tests::run_pass(&ctx, egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show_in_context(ctx, |ui| {
                shaped =
                    Some(ui.fonts_mut(|f| bidi_view::apply(f, g.clone(), bidi_view::Base::Auto).0));
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
    Arc::make_mut(&mut changed.rows[0].row).glyphs.pop();
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
            let _ = crate::app::tests::run_pass(&ctx, egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show_in_context(ctx, |ui| {
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
                    g = Some(ui.fonts_mut(|f| f.layout_job(job)));
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
        let _ = crate::app::tests::run_pass(&ctx, egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show_in_context(ctx, |ui| {
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
        let total = linear_start(&g, g.rows.len());
        assert_eq!(g.end().index.0, total);
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
                    index: CharIndex(index),
                    prefer_next_row,
                };
                assert_eq!(g.layout_from_cursor(c), linear_layout(&g, c));
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
    ctx.global_style_mut(|style| style.scroll_animation = egui::style::ScrollAnimation::none());
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

#[test]
fn long_row_pointer_lookup_matches_the_original_walk_at_centers_and_irregular_geometry() {
    let source = "αe\u{301}🦀 varied-width text ".repeat(12);
    for scale in [1.0, 1.25, 1.5, 2.0] {
        for mode in [
            "ordered", "equal", "reversed", "negative", "nan", "infinite",
        ] {
            let mut g = (*layout(&source, f32::INFINITY, scale, usize::MAX)).clone();
            let row = Arc::make_mut(&mut g.rows[0].row);
            match mode {
                "equal" => {
                    let x = row.glyphs[100].pos.x;
                    for glyph in &mut row.glyphs[100..150] {
                        glyph.pos.x = x;
                        glyph.advance_width = 0.0;
                    }
                }
                "reversed" => row.glyphs.reverse(),
                "negative" => {
                    row.glyphs[100].advance_width = -500.0;
                }
                "nan" => row.glyphs[100].pos.x = f32::NAN,
                "infinite" => row.glyphs[100].pos.x = f32::INFINITY,
                _ => {}
            }
            g.invalidate_cursor_index();
            let row = &g.rows[0];
            let xs = row
                .glyphs
                .iter()
                .flat_map(|glyph| {
                    let x = glyph.logical_rect().center().x;
                    [x - 0.001, x, x + 0.001]
                })
                .chain([f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.0]);
            for x in xs {
                let pos = egui::vec2(x, row.rect().center().y);
                same(g.cursor_from_pos(pos), linear_hit(&g, pos));
            }
        }
    }
}

#[test]
fn warmed_long_row_indexes_are_invalidated_by_rtl_shaping() {
    for source in ["שלום ".repeat(100), "English שלום 123 ".repeat(100)] {
        let g = layout(&source, f32::INFINITY, 1.25, usize::MAX);
        g.cursor_index();
        let ctx = egui::Context::default();
        let mut shaped = None;
        let _ = crate::app::tests::run_pass(&ctx, Default::default(), |ctx| {
            egui::CentralPanel::default().show_in_context(ctx, |ui| {
                shaped =
                    Some(ui.fonts_mut(|f| bidi_view::apply(f, g.clone(), bidi_view::Base::Auto).0));
            });
        });
        let shaped = shaped.unwrap();
        for glyph in &shaped.rows[0].glyphs {
            let pos = egui::vec2(
                glyph.logical_rect().center().x,
                shaped.rows[0].rect().center().y,
            );
            same(shaped.cursor_from_pos(pos), linear_hit(&shaped, pos));
        }
    }
}

#[test]
fn vertical_navigation_between_long_rows_keeps_the_original_goal_column() {
    let g = layout(
        &format!(
            "{}\n{}\n{}",
            "varied width ".repeat(100),
            "α🦀 ".repeat(300),
            "last ".repeat(200)
        ),
        f32::INFINITY,
        1.25,
        usize::MAX,
    );
    for row in 0..g.rows.len() {
        for column in [0, 127, 255, 899, 1200, 2000] {
            let cursor = g.cursor_from_layout(LayoutCursor {
                row,
                column: CharIndex(column),
            });
            let x = g.pos_from_cursor(cursor).left();
            for down in [false, true] {
                let Some(next) = (if down {
                    (row + 1 < g.rows.len()).then_some(row + 1)
                } else {
                    row.checked_sub(1)
                }) else {
                    continue;
                };
                let (actual, goal) = if down {
                    g.cursor_down_one_row(&cursor, Some(x))
                } else {
                    g.cursor_up_one_row(&cursor, Some(x))
                };
                let expected = g.rows[next].char_at(x - g.rows[next].pos.x);
                assert_eq!(actual.index.0, linear_start(&g, next) + expected.0);
                assert_eq!(goal, Some(x));
            }
        }
    }
}
