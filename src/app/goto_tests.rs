//! Go to Anything: @symbol, :line, and #text in Go to file.
use super::goto_anything::{parse, Target};
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
    app.preview_line(&ctx, 2, Some(4));
    assert!(app.active().view.selections.is_empty());
    // A second preview must still restore the original view, not the first preview.
    app.preview_line(&ctx, 3, None);
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
        .is_some_and(|error| error.contains("outside the readable text")));
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
