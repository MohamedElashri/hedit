//! Go to Anything: @symbol, :line, and #text in Go to file.
use super::goto_anything::{parse, PickerLine, Target};
use super::tests::{click_label, finish_app_job, frame, key};
use super::*;

#[test]
fn queries_split_into_a_file_and_a_target() {
    assert_eq!(parse("main"), ("main", None));
    assert_eq!(
        parse("app.rs@save"),
        ("app.rs", Some(Target::Symbol("save".into())))
    );
    assert_eq!(parse(":42:7"), ("", Some(Target::Line(Some(42), Some(7)))));
    assert_eq!(
        parse("kernel.cu:12"),
        ("kernel.cu", Some(Target::Line(Some(12), None)))
    );
    assert_eq!(parse(":x"), ("", Some(Target::Line(None, None))));
    assert_eq!(
        parse("#todo item"),
        ("", Some(Target::Text("todo item".into())))
    );
}

fn line_of(app: &HeditApp) -> usize {
    let b = app.active();
    b.page
        .text
        .chars()
        .take(b.view.head)
        .filter(|&c| c == '\n')
        .count()
        + 1
}

fn palette(ctx: &egui::Context, app: &mut HeditApp, query: &str) {
    app.execute(ctx, Command::Switcher);
    frame(ctx, app, vec![]);
    frame(ctx, app, vec![egui::Event::Text(query.into())]);
}

#[test]
fn go_to_file_restores_the_focused_groups_cursor() {
    use hedit::advanced::Selection;

    let directory = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    workspace.open[0].page.text = "abcdefghijk".into();
    workspace.edit(0, "").unwrap();
    workspace.name_scratchpad(0, "first").unwrap();
    workspace.new_buffer().unwrap();
    workspace.name_scratchpad(1, "second").unwrap();
    workspace.active = 0;
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.set_selections(vec![Selection::caret(1)]);
    app.split(1);
    app.focus_group(1, false);
    app.set_selections(vec![Selection::caret(4)]);
    app.select_tab(1);
    app.focus_group(0, false);
    assert_eq!(app.active().view.head, 1);
    app.focus_group(1, false);

    palette(&ctx, &mut app, "first");
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    assert_eq!(app.active().view.name, "first");
    assert_eq!(app.active().view.head, 4);
}

#[test]
fn cancelling_a_goto_preview_restores_extra_selections_and_horizontal_scroll() {
    let directory = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    workspace.open[0].page.text = "alpha beta\ngamma delta\nlast line\n".into();
    workspace.edit(0, "").unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.set_selections(vec![
        hedit::advanced::Selection { anchor: 5, head: 0 },
        hedit::advanced::Selection {
            anchor: 11,
            head: 16,
        },
    ]);
    app.workspace.open[0].view.scroll_top = 20.0;
    app.workspace.open[0].view.scroll_left = 150.0;
    let selections = app.selections();
    app.preview_line(&ctx, PickerLine::File(2, Some(4)));
    assert!(app.active().view.selections.is_empty());
    // A second preview must still restore the original view, not the first preview.
    app.preview_line(&ctx, PickerLine::File(3, None));
    app.workspace.open[0].view.scroll_top = 40.0;
    app.workspace.open[0].view.scroll_left = 0.0;
    app.cancel_goto_preview();
    assert_eq!(app.selections(), selections);
    assert_eq!(app.active().view.scroll_top, 20.0);
    assert_eq!(app.active().view.scroll_left, 150.0);
    assert!(!app.workspace.open[0].reveal_cursor);
}

#[test]
fn goto_in_a_paged_file_honors_the_requested_column() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("large.txt");
    let line = "αβγδεζηθ abcdefghijklmnopqrstuvwxyz\n";
    std::fs::write(&path, line.repeat(70_000)).unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.open(&ctx, Some(path));
    finish_app_job(&mut app, &ctx);
    assert!(app.active().page.info.paged);
    app.jump_to_line(&ctx, 60_000, Some(6));
    finish_app_job(&mut app, &ctx);
    let buffer = app.active();
    let byte = buffer
        .page
        .text
        .chars()
        .take(buffer.view.head)
        .map(char::len_utf8)
        .sum::<usize>();
    assert!(buffer.page.start > 0);
    assert_eq!(
        buffer.page.start + byte as u64,
        59_999 * line.len() as u64 + "αβγδε".len() as u64
    );
    assert_eq!(buffer.page.text[byte..].chars().next(), Some('ζ'));
}

#[test]
fn go_to_text_previews_and_selects_matches_on_a_later_page() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("large.txt");
    let line = "filler abcdefghijklmnopqrstuvwxyz\n";
    let before = line.repeat(80_000);
    let expected = before.len() as u64;
    std::fs::write(&path, before + "unique_marker\n" + &line.repeat(50_000)).unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.open(&ctx, Some(path));
    finish_app_job(&mut app, &ctx);
    app.jump_to_line(&ctx, 79_999, None);
    finish_app_job(&mut app, &ctx);
    let page_start = app.active().page.start;
    assert!(page_start > 0 && app.active().page.info.paged);

    app.preview_line(&ctx, PickerLine::File(125_000, None));
    wait_for_preview(&ctx, &mut app);
    assert_ne!(app.active().page.start, page_start);

    palette(&ctx, &mut app, "#unique_marker");
    let started = Instant::now();
    while app.goto_origin.is_none() {
        assert!(started.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(5));
        frame(&ctx, &mut app, vec![]);
    }
    let offset = |app: &HeditApp| {
        let buffer = app.active();
        buffer.page.start + hedit::advanced::byte(&buffer.page.text, buffer.view.head) as u64
    };
    assert_eq!(offset(&app), expected);
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    assert_eq!(offset(&app), expected);
    assert_eq!(app.active().page.start, page_start);
}

#[test]
fn opening_a_malformed_paged_file_finishes_without_a_worker_panic() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("partial.txt");
    let mut bytes = vec![b'x'; hedit::document::SMALL_LIMIT as usize + 1];
    bytes[0] = 0x80;
    std::fs::write(&path, bytes).unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.open(&ctx, Some(path));
    let started = Instant::now();
    while app.opens.pending() {
        assert!(started.elapsed() < Duration::from_secs(5));
        app.receive(&ctx);
        thread::sleep(Duration::from_millis(5));
    }
    assert!(app
        .error
        .take()
        .is_some_and(|error| error.contains("not valid UTF-8")));
    app.transient_error = None;

    let valid = directory.path().join("next.txt");
    std::fs::write(&valid, "next file").unwrap();
    app.open(&ctx, Some(valid));
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().page.text, "next file");
}

#[test]
fn symbols_lines_and_text_are_previewed_then_chosen_or_cancelled() {
    let directory = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
    workspace.open[0].page.text =
        "fn alpha() {\n}\n\nfn beta_value() {\n    // needle here\n}\n".into();
    workspace.edit(0, "").unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    frame(&ctx, &mut app, vec![]);
    palette(&ctx, &mut app, "@beta");
    assert_eq!(line_of(&app), 4, "the first match is previewed");
    frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
    assert!(!app.palette);
    assert_eq!(line_of(&app), 1, "Escape returns to where you were");
    palette(&ctx, &mut app, "#needle");
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    assert!(!app.palette);
    assert_eq!(line_of(&app), 5);
    palette(&ctx, &mut app, ":2");
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    assert_eq!(line_of(&app), 2);
    // Back returns to the line before the jump.
    app.execute(&ctx, Command::NavBack);
    finish_app_job(&mut app, &ctx);
    assert_eq!(line_of(&app), 5);
}

#[test]
fn a_file_name_before_the_target_opens_it_there() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("src");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("other.rs"), "// top\n\nfn gamma() {}\n").unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.set_folder(hedit::folder::read_directory(source.clone()).unwrap());
    frame(&ctx, &mut app, vec![]);
    palette(&ctx, &mut app, "other@gamma");
    // Wait for the folder search to list the file.
    let started = Instant::now();
    while !app.project.search.as_ref().is_some_and(|r| r.is_ok()) {
        assert!(started.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(10));
        frame(&ctx, &mut app, vec![]);
    }
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    finish_app_job(&mut app, &ctx);
    finish_app_job(&mut app, &ctx);
    assert!(app
        .active()
        .page
        .info
        .path
        .as_deref()
        .unwrap()
        .ends_with("other.rs"));
    assert_eq!(line_of(&app), 3);
}

#[test]
fn find_selects_matches_while_typing_and_highlights_them_all() {
    let text = "one two\nthree two\nfour\n";
    let directory = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
    workspace.open[0].page.text = text.into();
    workspace.edit(0, "").unwrap();
    // Find starts searching from the cursor.
    workspace.open[0].view.anchor = 5;
    workspace.open[0].view.head = 5;
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::F, Modifiers::COMMAND)]);
    frame(&ctx, &mut app, vec![egui::Event::Text("t".into())]);
    let selected = |app: &HeditApp| (app.active().view.anchor, app.active().view.head);
    assert_eq!(selected(&app), (8, 9), "first “t” after the cursor");
    frame(&ctx, &mut app, vec![egui::Event::Text("wo".into())]);
    assert_eq!(selected(&app), (14, 17), "“two” after the cursor");
    assert_eq!(app.quality.match_status, "2 of 2");
    assert_eq!(
        app.find_highlights(&ctx).unwrap().as_slice(),
        [(4, 7), (14, 17)]
    );
    frame(&ctx, &mut app, vec![egui::Event::Text("x".into())]);
    assert_eq!(selected(&app), (5, 5), "no match returns to the start");
    assert_eq!(app.quality.match_status, "No matches");
    assert_eq!(app.active().page.text, text);
    // Without Find, selecting a word outlines its other occurrences.
    frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
    app.set_selections(vec![hedit::advanced::Selection { anchor: 4, head: 7 }]);
    assert_eq!(
        app.selection_highlights(&ctx).unwrap().as_slice(),
        [(4, 7), (14, 17)]
    );
    app.set_selections(vec![hedit::advanced::Selection { anchor: 4, head: 6 }]);
    assert!(app.selection_highlights(&ctx).is_none(), "only whole words");
    app.settings.match_selection = false;
    app.set_selections(vec![hedit::advanced::Selection { anchor: 4, head: 7 }]);
    assert!(app.selection_highlights(&ctx).is_none());
}

#[test]
fn find_reveals_offscreen_matches_without_leaving_the_search_field() {
    for (query, wrap) in [("needle", true), ("a café sentence", false)] {
        let padding = if wrap {
            String::new()
        } else {
            "λ ".repeat(150)
        };
        let gap = "another line\n".repeat(120);
        let text = format!("{gap}{padding}{query}\n{gap}{query}\n{gap}");
        let directory = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        workspace.open[0].page.text = text.clone();
        workspace.edit(0, "").unwrap();
        let ctx = egui::Context::default();
        ctx.style_mut(|style| style.scroll_animation = egui::style::ScrollAnimation::none());
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.settings.word_wrap = wrap;
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![key(Key::F, Modifiers::COMMAND)]);
        frame(&ctx, &mut app, vec![egui::Event::Text(query.into())]);

        let assert_visible = |app: &mut HeditApp, occurrence: usize| {
            // Let the scroll offset and scrollbar layout settle.
            frame(&ctx, app, vec![]);
            frame(&ctx, app, vec![]);
            let output = frame(&ctx, app, vec![]);
            assert!(app.find_input_focused(&ctx));
            let start = text.match_indices(query).nth(occurrence).unwrap().0;
            let anchor = text[..start].chars().count();
            let head = anchor + query.chars().count();
            assert_eq!(
                (app.active().view.anchor, app.active().view.head),
                (anchor, head)
            );
            let (_, bidi) = app.editor_layout.last().unwrap();
            fn visible(
                shape: &egui::Shape,
                clip: egui::Rect,
                text: &str,
                bidi: &bidi_view::BidiRows,
                anchor: usize,
                head: usize,
            ) -> bool {
                match shape {
                    egui::Shape::Vec(shapes) => shapes
                        .iter()
                        .any(|shape| visible(shape, clip, text, bidi, anchor, head)),
                    egui::Shape::Text(shape) if shape.galley.text() == text => {
                        [anchor, head].into_iter().all(|at| {
                            clip.contains_rect(
                                bidi_view::caret_rect(&shape.galley, bidi, at)
                                    .translate(shape.pos.to_vec2()),
                            )
                        })
                    }
                    _ => false,
                }
            }
            assert!(
                output.shapes.iter().any(|shape| visible(
                    &shape.shape,
                    shape.clip_rect,
                    &text,
                    &bidi,
                    anchor,
                    head,
                )),
                "the selected match must be visible (wrap={wrap}, occurrence={occurrence}, top={})",
                app.active().view.scroll_top
            );
        };
        assert_visible(&mut app, 0);
        assert!(app.active().view.scroll_top > 0.0);
        if !wrap {
            assert!(app.active().view.scroll_left > 0.0);
        }
        // Enter follows later matches and wraps to the first; Shift+Enter
        // follows previous matches and wraps to the last.
        for (modifiers, occurrence) in [
            (Modifiers::NONE, 1),
            (Modifiers::NONE, 0),
            (Modifiers::SHIFT, 1),
            (Modifiers::SHIFT, 0),
        ] {
            frame(&ctx, &mut app, vec![key(Key::Enter, modifiers)]);
            finish_app_job(&mut app, &ctx);
            frame(&ctx, &mut app, vec![]);
            assert_visible(&mut app, occurrence);
        }
        assert_eq!(app.active().page.text, text);
    }
}

#[test]
fn live_find_respects_selection_only_and_updates_when_toggled() {
    let text = "hit α hit β hit";
    let directory = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
    workspace.open[0].page.text = text.into();
    workspace.edit(0, "").unwrap();
    workspace.open[0].view.anchor = 4;
    workspace.open[0].view.head = 11;
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::F, Modifiers::COMMAND)]);
    frame(&ctx, &mut app, vec![egui::Event::Text("hit".into())]);
    assert_eq!(app.find_highlights(&ctx).unwrap().len(), 3);
    click_label(&ctx, &mut app, "Selection only");
    assert_eq!(app.quality.match_status, "1 of 1");
    assert_eq!(app.find_highlights(&ctx).unwrap().as_slice(), [(6, 9)]);
    // Narrowing the query must not jump to a match outside the scope.
    app.query = "hit β hit".into();
    app.find_as_you_type(&ctx);
    assert_eq!(app.quality.match_status, "No matches");
    assert_eq!((app.active().view.anchor, app.active().view.head), (4, 4));
    click_label(&ctx, &mut app, "Selection only");
    assert_eq!(app.quality.match_status, "1 of 1");
    assert_eq!((app.active().view.anchor, app.active().view.head), (6, 15));
    assert_eq!(app.active().page.text, text);
}

#[test]
fn live_find_counts_zero_width_regex_matches_and_enter_advances() {
    let directory = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
    workspace.open[0].page.text = "α\nβ\nγ".into();
    workspace.edit(0, "").unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.quality.options.regex = true;
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::F, Modifiers::COMMAND)]);
    frame(&ctx, &mut app, vec![egui::Event::Text("^".into())]);
    assert_eq!(app.quality.match_status, "1 of 3");
    assert_eq!((app.active().view.anchor, app.active().view.head), (0, 0));
    for head in [2, 4, 0] {
        frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
        finish_app_job(&mut app, &ctx);
        assert_eq!(
            (app.active().view.anchor, app.active().view.head),
            (head, head)
        );
    }
    assert_eq!(app.active().page.text, "α\nβ\nγ");
}

#[test]
#[ignore = "manual release CPU profile of Go to Anything over a large source buffer"]
fn profile_goto_navigation() {
    let source: String = (0..20000)
        .map(|i| format!("fn compute_{i}() {{ let value = {i}; }}\n"))
        .collect();
    let directory = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    workspace.open[0].page.text = source.clone();
    workspace.edit(0, "").unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    for query in ["@compute", "#absent_query"] {
        app.palette = true;
        app.palette_query = query.into();
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        let until = Instant::now() + Duration::from_secs(5);
        while app.goto_matches.pending() {
            frame(&ctx, &mut app, vec![]);
            assert!(Instant::now() < until);
            thread::sleep(Duration::from_millis(1));
        }
        let start = Instant::now();
        for _ in 0..20 {
            frame(&ctx, &mut app, vec![]);
        }
        eprintln!(
            "Go to Anything {query}, {} bytes: {:.2} ms/frame (20 unchanged frames)",
            source.len(),
            start.elapsed().as_secs_f64() * 1000.0 / 20.0
        );
    }
}

#[test]
fn file_symbol_navigation_reads_decoded_utf16_documents() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("symbols.rs");
    let source = "fn start() {}\n\n// unicode 🦀\nfn pick_me() {}\n";
    let bytes: Vec<_> = [0xff, 0xfe]
        .into_iter()
        .chain(source.encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    std::fs::write(&path, bytes).unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.go_to_target(
        &ctx,
        Some(path.to_string_lossy().into_owned()),
        Target::Symbol("pick_me".into()),
    );
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().page.text, source);
    assert_eq!(app.active().page.info.encoding, "UTF-16LE");
    assert_eq!(line_of(&app), 4);
}

fn app_with(text: &str) -> (tempfile::TempDir, egui::Context, HeditApp) {
    let directory = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    workspace.open[0].page.text = text.into();
    workspace.edit(0, "").unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.settings.auto_complete = false;
    app.focus_editor = true;
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![]);
    (directory, ctx, app)
}

#[test]
fn selection_only_skips_a_different_tab() {
    let (_directory, ctx, mut app) = app_with("needle needle");
    let original = app.active().view.id;
    app.quality.selection_only = true;
    app.quality.scope = Some((original, 0, 6));
    app.query = "needle".into();
    app.replacement = "changed".into();
    app.execute(&ctx, Command::New);
    let second = app.active().view.id;
    assert_ne!(second, original);
    assert!(app.apply_edit(
        hedit::editing::Edit {
            text: "needle needle".into(),
            anchor: 0,
            head: 0,
        },
        None
    ));
    assert!(app.quality.selection_only);
    app.replace_advanced(true);
    assert_eq!(app.active().page.text, "needle needle");
}

#[test]
fn folder_search_includes_named_unsaved_files_from_the_editor() {
    let (directory, ctx, mut app) = app_with("");
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let path = root.canonicalize().unwrap().join("not-on-disk.txt");
    app.set_folder(hedit::folder::read_directory(root).unwrap());
    app.open(&ctx, Some(path.clone()));
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().page.info.name, "not-on-disk.txt");
    assert!(app.apply_edit(
        hedit::editing::Edit {
            text: "unique_marker\n".into(),
            anchor: 0,
            head: 0,
        },
        None
    ));
    assert!(!path.exists());
    app.quality.folder_query = "unique_marker".into();
    app.folder_search(&ctx, false);
    finish_app_job(&mut app, &ctx);
    assert!(app.quality.results.hits.iter().any(|hit| hit.path == path));
}

#[test]
fn git_partial_stage_uses_header_records_not_filename_fragments() {
    for name in ["mode notes.txt", "rename plan.txt"] {
        let patch = format!("diff --git a/{name} b/{name}\nindex 1111111..2222222 100644\n--- a/{name}\t\n+++ b/{name}\t\n@@ -1 +1 @@\n-old\n+new\n");
        let diff = hedit::git::Diff::parse(patch, true);
        assert!(diff.supports_partial(), "{name}");
        assert!(diff.hunk_patch(0).is_some());
    }
}

#[test]
fn cancelling_a_line_preview_restores_the_old_page() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("large.txt");
    std::fs::write(&path, "filler abcdefghijklmnopqrstuvwxyz\n".repeat(130_000)).unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.open(&ctx, Some(path));
    finish_app_job(&mut app, &ctx);
    app.jump_to_line(&ctx, 60_000, None);
    finish_app_job(&mut app, &ctx);
    let original = app.active().page.start;
    assert!(original > 0 && app.active().page.info.paged);
    app.preview_line(&ctx, PickerLine::File(100_000, None));
    wait_for_preview(&ctx, &mut app);
    assert_ne!(app.active().page.start, original);
    app.cancel_goto_preview();
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().page.start, original);
    app.preview_line(&ctx, PickerLine::File(100_000, None));
    app.cancel_goto_preview();
    wait_for_preview(&ctx, &mut app);
    assert_eq!(app.active().page.start, original);
}

#[test]
fn go_to_file_prefers_a_literal_filename_with_a_hash() {
    let (directory, ctx, mut app) = app_with("");
    let first = directory.path().join("notes");
    let second = directory.path().join("notes#draft.md");
    std::fs::write(&first, "a plain note\n").unwrap();
    std::fs::write(&second, "# draft\n").unwrap();
    for path in [second, first] {
        app.open(&ctx, Some(path));
        finish_app_job(&mut app, &ctx);
    }
    palette(&ctx, &mut app, "notes#draft.md");
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    assert_eq!(app.active().page.info.name, "notes#draft.md");
}

#[test]
fn selection_scope_does_not_invent_whole_word_boundaries() {
    use hedit::search::{self, Options};
    let text = "bobcat cat";
    let options = Options {
        whole_word: true,
        ..Default::default()
    };
    let found = search::matches(text, "cat", &options, 3..6).unwrap();
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn column_coordinates_match_file_open_after_an_emoji() {
    let (_directory, ctx, mut app) = app_with("a😀b\n");
    let mut doc = Document::empty(1);
    doc.replace(0, 0, "a😀b\n".into(), 0).unwrap();
    let expected = doc.location_offset(1, 4).unwrap();
    app.jump_to_line(&ctx, 1, Some(4));
    assert_eq!(
        hedit::advanced::byte(&app.active().page.text, app.active().view.head) as u64,
        expected
    );
}

fn wait_for_preview(ctx: &egui::Context, app: &mut HeditApp) {
    let started = Instant::now();
    while app.goto_preview_running {
        assert!(started.elapsed() < Duration::from_secs(5));
        app.receive(ctx);
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn quoted_file_reference_keeps_the_location_suffix() {
    use hedit::{advanced::Selection, file_reference};
    let reference = r#""notes folder/file.txt":3:7,"#;
    assert_eq!(
        file_reference::at(reference, Selection::caret(10)).as_deref(),
        Some("notes folder/file.txt:3:7")
    );
}

#[test]
fn git_partial_stage_supports_quoted_paths() {
    let patch = "diff --git \"a/notes\\tfile.txt\" \"b/notes\\tfile.txt\"\n\
        --- \"a/notes\\tfile.txt\"\n+++ \"b/notes\\tfile.txt\"\n@@ -1 +1 @@\n-old\n+new\n";
    let diff = hedit::git::Diff::parse(patch.into(), true);
    assert!(diff.supports_partial());
    assert!(diff.hunk_patch(0).is_some());
}
