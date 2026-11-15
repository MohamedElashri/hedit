//! Workspace regressions and opt-in interaction profiles using real input paths.
use super::*;
use crate::app::tests::{frame, frame_size, text_rect};
fn app(text: &str, tabs: usize) -> (tempfile::TempDir, egui::Context, HeditApp) {
    let dir = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
    workspace.open[0].page.text = text.into();
    workspace.edit(0, "").unwrap();
    for n in 1..tabs {
        workspace.new_buffer().unwrap();
        workspace
            .name_scratchpad(n, &format!("audit-{n:04}.txt"))
            .unwrap();
    }
    workspace.active = 0;
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.settings.auto_complete = false;
    app.settings.caret_blink = false;
    app.settings.show_sidebar = false;
    ctx.style_mut(|s| s.scroll_animation = egui::style::ScrollAnimation::none());
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    (dir, ctx, app)
}
#[test]
fn keyboard_tab_selection_reveals_the_active_tab_in_a_large_workspace() {
    let (_dir, ctx, mut app) = app("", 100);
    app.select_tab(99);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let out = frame(&ctx, &mut app, vec![]);
    assert!(
        out.shapes
            .iter()
            .any(|shape| text_rect(&shape.shape, "audit-0099.txt")
                .is_some_and(|rect| rect.intersects(shape.clip_rect))),
        "the active tab must be visible after keyboard navigation"
    );
}
#[test]
#[ignore = "manual release CPU audit of tabs, selections, split panes, and long lines"]
fn profile_workspace_interaction() {
    for count in [1, 100, 1000] {
        let (_dir, ctx, mut app) = app("", count);
        let start = Instant::now();
        for _ in 0..30 {
            frame(&ctx, &mut app, vec![]);
        }
        eprintln!(
            "Audit tabs {count}: {:.3} ms/warm frame",
            start.elapsed().as_secs_f64() * 1000.0 / 30.0
        );
        let start = Instant::now();
        for _ in 0..100 {
            app.track_recent_tabs(&ctx);
        }
        eprintln!(
            "Audit MRU {count}: {:.3} ms/call",
            start.elapsed().as_secs_f64() * 10.0
        );
    }
    let source = "    let value = compute(item);\n".repeat(40_000);
    for (label, text) in [
        ("many-lines", source.as_str()),
        ("long-line", &"abcdefghij ".repeat(15_000)),
    ] {
        let (_dir, ctx, mut app) = app(text, 1);
        for selection in [false, true] {
            if selection {
                app.set_selections(vec![hedit::advanced::Selection {
                    anchor: 0,
                    head: text.chars().count(),
                }]);
            }
            for _ in 0..3 {
                frame(&ctx, &mut app, vec![]);
            }
            let start = Instant::now();
            for _ in 0..15 {
                frame(&ctx, &mut app, vec![]);
            }
            eprintln!(
                "Audit {label} selected={selection}: {:.3} ms/warm frame",
                start.elapsed().as_secs_f64() * 1000.0 / 15.0
            );
        }
        app.set_selections(vec![hedit::advanced::Selection::caret(0)]);
        app.split(6);
        for _ in 0..3 {
            frame_size(&ctx, &mut app, vec![], egui::vec2(1280.0, 820.0));
        }
        let start = Instant::now();
        for _ in 0..15 {
            frame(&ctx, &mut app, vec![]);
        }
        eprintln!(
            "Audit {label} four panes: {:.3} ms/warm frame",
            start.elapsed().as_secs_f64() * 1000.0 / 15.0
        );
    }
}

#[test]
fn large_primary_selections_do_not_copy_the_cached_glyph_meshes() {
    let source = "value item\n".repeat(20_000);
    let (_dir, ctx, mut app) = app(&source, 1);
    app.set_selections(vec![hedit::advanced::Selection {
        anchor: 0,
        head: source.chars().count(),
    }]);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let base = app.editor_layout.last().unwrap().0;
    let out = frame(&ctx, &mut app, vec![]);
    let drawn = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == source => Some(&t.galley),
            _ => None,
        })
        .unwrap();
    assert!(
        Arc::ptr_eq(&base, drawn),
        "selection backgrounds must not copy offscreen rows or long-line meshes"
    );
}
#[test]
fn visible_selection_rectangles_match_eguis_original_mesh_rectangles() {
    use egui::text::{CCursor, CursorRange};
    for text in ["", "αβ\r\n\nlong wrapped line with words and spaces\nend"] {
        let galley = row_index::tests::layout(text, 90.0, 1.25);
        let count = text.chars().count();
        for (a, b) in [(0, count), (1.min(count), count), (count, 0), (0, 0)] {
            let range = CursorRange {
                primary: galley.from_ccursor(CCursor::new(b)),
                secondary: galley.from_ccursor(CCursor::new(a)),
            };
            let mut reference = galley.clone();
            let mut indices = vec![];
            egui::text_selection::visuals::paint_text_selection(
                &mut reference,
                &egui::Visuals::dark(),
                &range,
                Some(&mut indices),
            );
            for rows in [
                0..galley.rows.len(),
                1..3,
                galley.rows.len()..galley.rows.len(),
            ] {
                let expected: Vec<_> = indices
                    .iter()
                    .filter(|i| rows.contains(&i.row))
                    .map(|i| {
                        let mesh = &reference.rows[i.row].visuals.mesh;
                        i.vertex_indices.iter().fold(egui::Rect::NOTHING, |r, &v| {
                            r.union(egui::Rect::from_min_max(
                                mesh.vertices[v as usize].pos,
                                mesh.vertices[v as usize].pos,
                            ))
                        })
                    })
                    .collect();
                assert_eq!(
                    editor_interaction::selection_rects(&galley, &range, rows),
                    expected
                );
            }
        }
    }
}

#[test]
#[cfg(target_os = "macos")]
fn accessible_tabs_keep_full_names_selected_state_and_bounded_visible_nodes() {
    let (_dir, ctx, mut app) = app("", 100);
    let name = "A long accessible filename containing שלום and αβγ.txt";
    app.workspace.name_scratchpad(99, name).unwrap();
    ctx.enable_accesskit();
    app.select_tab(99);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let out = frame(&ctx, &mut app, vec![]);
    let tree = out.platform_output.accesskit_update.unwrap();
    let tabs: Vec<_> = tree
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == egui::accesskit::Role::Tab)
        .collect();
    assert!(
        tabs.len() < 20,
        "offscreen tabs should not create accessibility widgets"
    );
    assert!(tabs
        .iter()
        .any(|(_, node)| node.label() == Some(name) && node.is_selected() == Some(true)));
    assert!(tree
        .nodes
        .iter()
        .any(|(_, node)| node.label() == Some(format!("Close {name}").as_str())));
}
#[test]
fn virtual_tab_geometry_tracks_reordering_renames_pin_changes_and_window_resizes() {
    let (_dir, ctx, mut app) = app("", 100);
    for (size, index) in [
        (egui::vec2(720.0, 480.0), 99),
        (egui::vec2(1280.0, 820.0), 50),
        (egui::vec2(640.0, 480.0), 0),
    ] {
        app.select_tab(index);
        let name = format!("Renamed audit tab {index}");
        app.workspace.name_scratchpad(index, &name).unwrap();
        app.workspace.open[index].view.pinned = true;
        for _ in 0..4 {
            frame_size(&ctx, &mut app, vec![], size);
        }
        let out = frame_size(&ctx, &mut app, vec![], size);
        assert!(out
            .shapes
            .iter()
            .any(|s| text_rect(&s.shape, &format!("◆ {name}"))
                .is_some_and(|r| r.intersects(s.clip_rect))));
    }
    let active = app.active().view.id;
    let buffer = app.workspace.open.remove(0);
    app.workspace.open.push(buffer);
    app.workspace.active = 99;
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    assert_eq!(app.active().view.id, active);
    let out = frame(&ctx, &mut app, vec![]);
    assert!(out
        .shapes
        .iter()
        .any(|s| text_rect(&s.shape, "◆ Renamed audit tab 0")
            .is_some_and(|r| r.intersects(s.clip_rect))));
}
#[test]
fn select_all_replace_and_undo_preserve_unicode_crlf_and_shared_pane_cursors() {
    let source = "α🦀 one\r\nשלום two\r\n".repeat(1000);
    let (_dir, ctx, mut app) = app(&source, 1);
    app.workspace.open[0].page.info.eol = "CRLF";
    app.split(1);
    frame(&ctx, &mut app, vec![]);
    app.set_selections(vec![hedit::advanced::Selection {
        anchor: 0,
        head: source.chars().count(),
    }]);
    frame(&ctx, &mut app, vec![]);
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Text("replacement".into())],
    );
    assert_eq!(app.active().page.text, "replacement");
    app.history(&ctx, false);
    crate::app::tests::finish_app_job(&mut app, &ctx);
    frame(&ctx, &mut app, vec![]);
    assert_eq!(app.active().page.text, source);
    app.focus_pane(true);
    frame(&ctx, &mut app, vec![]);
    assert!(app.active().view.head <= source.chars().count());
}

#[test]
fn palette_shortcut_is_visible_and_participates_in_binding_conflict_resolution() {
    let (_dir, ctx, mut app) = app("", 1);
    app.open_preferences(PreferencesPage::Keys);
    app.key_filter = "Command palette".into();
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let out = frame(&ctx, &mut app, vec![]);
    assert!(out
        .shapes
        .iter()
        .any(|s| text_rect(&s.shape, Command::Palette.label()).is_some()));
    app.set_binding(Command::Find, Some("Primary+Shift+P".into()));
    assert_eq!(app.shortcut_text(Command::Palette), "");
    assert!(app.notice.contains("Command palette"));
}
#[test]
fn a_large_workspace_restores_names_active_tab_and_unsaved_unicode() {
    let (dir, ctx, mut app) = app("unsaved α🦀\r\n", 100);
    app.select_tab(99);
    frame(&ctx, &mut app, vec![]);
    app.workspace.checkpoint().unwrap();
    drop(app);
    let (workspace, errors) = Workspace::restore(dir.path().join("session")).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(workspace.open.len(), 100);
    assert_eq!(workspace.active, 99);
    assert_eq!(workspace.open[0].page.text, "unsaved α🦀\r\n");
    assert_eq!(workspace.open[99].view.name, "audit-0099.txt");
}
#[test]
#[ignore = "manual release audit of typing within a single long paragraph"]
fn profile_long_line_typing() {
    let source = "abcdefghij ".repeat(15_000);
    for position in [0, source.len()] {
        let (_dir, ctx, mut app) = app(&source, 1);
        app.set_selections(vec![hedit::advanced::Selection::caret(position)]);
        for _ in 0..4 {
            frame(&ctx, &mut app, vec![]);
        }
        let mut times = vec![];
        for _ in 0..8 {
            let start = Instant::now();
            frame(&ctx, &mut app, vec![egui::Event::Text("x".into())]);
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(f64::total_cmp);
        eprintln!(
            "Audit long-line typing at {position}: p50={:.2} p95={:.2} ms/CPU frame",
            times[4], times[7]
        );
    }
}

#[test]
#[cfg(target_os = "macos")]
fn accessible_word_boundaries_cover_long_identifiers_and_unicode_without_overflow() {
    for text in [
        "x".repeat(600),
        "界".repeat(600),
        format!("{}\r\n next", "word".repeat(100)),
    ] {
        let (_dir, ctx, mut app) = app(&text, 1);
        ctx.enable_accesskit();
        frame(&ctx, &mut app, vec![]);
        let tree = frame(&ctx, &mut app, vec![])
            .platform_output
            .accesskit_update
            .unwrap();
        let mut checked = false;
        for (_, node) in tree.nodes {
            if node.role() == egui::accesskit::Role::TextRun
                && node.value().is_some_and(|v| v.chars().count() > 255)
            {
                let lengths = node.character_lengths();
                assert_eq!(
                    node.word_lengths()
                        .iter()
                        .map(|&n| n as usize)
                        .sum::<usize>(),
                    lengths.len()
                );
                checked = true;
            }
        }
        assert!(checked);
    }
}

#[test]
#[cfg(target_os = "macos")]
fn pure_rtl_accessibility_reports_direction_and_positions_from_the_right_edge() {
    let text = "שלום";
    let (_dir, ctx, mut app) = app(text, 1);
    ctx.enable_accesskit();
    frame(&ctx, &mut app, vec![]);
    let tree = frame(&ctx, &mut app, vec![])
        .platform_output
        .accesskit_update
        .unwrap();
    let (_, node) = tree
        .nodes
        .iter()
        .find(|(_, n)| n.role() == egui::accesskit::Role::TextRun && n.value() == Some(text))
        .unwrap();
    assert_eq!(
        node.text_direction(),
        Some(egui::accesskit::TextDirection::RightToLeft)
    );
    let positions = node.character_positions().unwrap();
    assert_eq!(positions.len(), text.chars().count());
    assert!(positions[0].abs() < 0.001);
    assert!(positions.windows(2).all(|p| p[0] <= p[1]));
}

#[cfg(target_os = "macos")]
fn assert_accessible_focus_is_reachable(out: egui::FullOutput) {
    let tree = out.platform_output.accesskit_update.unwrap();
    let root = tree.tree.unwrap().root;
    let nodes: std::collections::HashMap<_, _> = tree.nodes.into_iter().collect();
    let mut seen = std::collections::HashSet::new();
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        assert!(
            seen.insert(id),
            "accessibility node {id:?} must have exactly one parent"
        );
        let node = nodes
            .get(&id)
            .expect("every accessibility child must exist");
        if node.role() == egui::accesskit::Role::MultilineTextInput {
            for child in node.children() {
                assert_eq!(
                    nodes[child].role(),
                    egui::accesskit::Role::TextRun,
                    "editor text rows must not alias other editors"
                );
            }
        }
        pending.extend_from_slice(node.children());
    }
    assert!(
        seen.contains(&tree.focus),
        "accessibility focus {:?} must be reachable from the current root",
        tree.focus
    );
}
#[test]
#[cfg(target_os = "macos")]
fn a_focus_request_for_a_future_widget_cannot_export_a_dangling_accessibility_id() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let out = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label("Current view");
        });
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("next-frame-widget")));
    });
    assert_accessible_focus_is_reachable(out);
    assert!(ctx.memory(|m| m.has_focus(egui::Id::new("next-frame-widget"))));
    let mut text = String::new();
    let out = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add(egui::TextEdit::singleline(&mut text).id(egui::Id::new("next-frame-widget")));
        });
    });
    assert_accessible_focus_is_reachable(out);
}
#[test]
#[cfg(target_os = "macos")]
fn accessibility_focus_stays_reachable_through_split_undo_busy_and_hidden_editor_states() {
    let (_dir, ctx, mut app) = app("α🦀 one\r\nשלום two\r\n", 100);
    ctx.enable_accesskit();
    assert_accessible_focus_is_reachable(frame(&ctx, &mut app, vec![]));
    app.split(1);
    app.focus_group(1, false);
    assert_accessible_focus_is_reachable(frame(&ctx, &mut app, vec![]));
    app.set_selections(vec![hedit::advanced::Selection {
        anchor: 0,
        head: app.active().page.text.chars().count(),
    }]);
    assert_accessible_focus_is_reachable(frame(&ctx, &mut app, vec![]));
    assert_accessible_focus_is_reachable(frame(
        &ctx,
        &mut app,
        vec![egui::Event::Text("replacement".into())],
    ));
    app.history(&ctx, false);
    assert_accessible_focus_is_reachable(frame(&ctx, &mut app, vec![]));
    crate::app::tests::finish_app_job(&mut app, &ctx);
    assert_accessible_focus_is_reachable(frame(&ctx, &mut app, vec![]));
    app.source_control.open = true;
    assert_accessible_focus_is_reachable(frame(&ctx, &mut app, vec![]));
}

#[test]
fn plain_arrows_collapse_rtl_selections_to_the_visual_edge_before_moving() {
    for (anchor, head) in [(0, 2), (2, 0), (1, 3), (3, 1)] {
        for (key, expected) in [
            (Key::ArrowLeft, anchor.max(head)),
            (Key::ArrowRight, anchor.min(head)),
        ] {
            let (_dir, ctx, mut app) = app("שלום", 1);
            app.set_selections(vec![hedit::advanced::Selection { anchor, head }]);
            frame(&ctx, &mut app, vec![]);
            frame(
                &ctx,
                &mut app,
                vec![crate::app::tests::key(key, Modifiers::NONE)],
            );
            assert_eq!(
                (app.active().view.anchor, app.active().view.head),
                (expected, expected)
            );
            assert_eq!(app.active().page.text, "שלום");
        }
    }
}

#[test]
#[cfg(target_os = "macos")]
fn native_accessibility_consumer_accepts_successive_split_undo_trees() {
    for _ in 0..20 {
        let (_dir, ctx, mut app) = app("α🦀 one\r\nשלום two\r\n", 100);
        ctx.enable_accesskit();
        let mut tree = accesskit_consumer::Tree::new(
            frame(&ctx, &mut app, vec![])
                .platform_output
                .accesskit_update
                .unwrap(),
            true,
        );
        let mut update = |app: &mut HeditApp, events| {
            app.receive(&ctx);
            let output = frame(&ctx, app, events);
            assert_accessible_focus_is_reachable(output.clone());
            tree.update(output.platform_output.accesskit_update.unwrap());
        };
        app.split(1);
        app.focus_group(1, false);
        update(&mut app, vec![]);
        app.set_selections(vec![hedit::advanced::Selection {
            anchor: 0,
            head: app.active().page.text.chars().count(),
        }]);
        update(&mut app, vec![]);
        update(&mut app, vec![egui::Event::Text("replacement".into())]);
        app.history(&ctx, false);
        for _ in 0..100 {
            update(&mut app, vec![]);
            if !app.busy {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(!app.busy);
        update(&mut app, vec![]);
    }
}

#[test]
#[cfg(target_os = "macos")]
fn split_editor_ids_cannot_alias_the_primary_editors_accessibility_rows() {
    let (_dir, ctx, mut app) = app("first\nsecond\nthird\n", 1);
    ctx.enable_accesskit();
    let primary = app.editor_id();
    app.split(1);
    app.focus_group(1, false);
    let secondary = app.editor_id();
    assert_ne!(secondary, primary.with(1usize));
    let out = frame(&ctx, &mut app, vec![]);
    let nodes = &out.platform_output.accesskit_update.as_ref().unwrap().nodes;
    assert_eq!(
        nodes
            .iter()
            .filter(|(_, node)| node.role() == egui::accesskit::Role::MultilineTextInput)
            .count(),
        2
    );
    assert_accessible_focus_is_reachable(out);
}
