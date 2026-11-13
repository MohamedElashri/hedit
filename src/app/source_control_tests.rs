use super::super::tests::{click_label, finish_app_job, frame, frame_size, key, text_rect};
use super::*;

fn fixture() -> (tempfile::TempDir, egui::Context, HeditApp, Git) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let git = Git::default();
    git.command(&root, &["init", "-b", "main"]).unwrap();
    for (key, value) in [
        ("user.name", "Test Author"),
        ("user.email", "test@example.invalid"),
        ("commit.gpgsign", "false"),
        ("core.hooksPath", "/nonexistent/hedit-test-hooks"),
        ("core.autocrlf", "false"),
    ] {
        git.command(&root, &["config", key, value]).unwrap();
    }
    let base = "use std::path::Path;\n\nfn load(path: &Path) -> String {\n    std::fs::read_to_string(path).unwrap()\n}\n";
    std::fs::write(root.join("main.rs"), base).unwrap();
    git.execute(&root, &Operation::Stage(vec!["main.rs".into()]))
        .unwrap();
    git.execute(
        &root,
        &Operation::Commit {
            message: "Read project notes".into(),
            amend: false,
        },
    )
    .unwrap();
    std::fs::write(
        root.join("main.rs"),
        base.replace("unwrap()", "unwrap_or_default()"),
    )
    .unwrap();
    let (mut workspace, errors) = Workspace::restore(directory.path().join("session")).unwrap();
    workspace.folders = vec![hedit::session::FolderView {
        path: root.to_string_lossy().into_owned(),
        visible: true,
        collapsed: false,
        expanded: vec![],
    }];
    let ctx = egui::Context::default();
    ctx.style_mut(|s| s.scroll_animation = egui::style::ScrollAnimation::none());
    let mut app = HeditApp::from_context(&ctx, None, workspace, errors, vec![]);
    app.open_source_control(&ctx);
    finish_git(&ctx, &mut app);
    (directory, ctx, app, git)
}
fn finish_git(ctx: &egui::Context, app: &mut HeditApp) {
    let start = Instant::now();
    loop {
        app.poll_source_control(ctx);
        if !app.source_control.pending {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "Git job did not complete"
        );
        thread::sleep(Duration::from_millis(5));
    }
    frame(ctx, app, vec![]);
}
fn visible(output: &egui::FullOutput, label: &str) -> bool {
    output
        .shapes
        .iter()
        .any(|s| text_rect(&s.shape, label).is_some_and(|r| s.clip_rect.contains_rect(r)))
}

#[cfg(target_os = "macos")]
fn accessible_node<'a>(output: &'a egui::FullOutput, label: &str) -> &'a egui::accesskit::Node {
    &output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("Accessibility tree")
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some(label))
        .unwrap_or_else(|| panic!("Missing control {label}"))
        .1
}

#[cfg(target_os = "macos")]
#[test]
fn compact_diff_layout_controls_report_the_actual_layout_and_restore_the_preference() {
    let (_directory, ctx, mut app, _git) = fixture();
    ctx.enable_accesskit();
    app.source_control.select(
        &ctx,
        Selection {
            path: "main.rs".into(),
            kind: DiffKind::Working,
        },
    );
    finish_git(&ctx, &mut app);
    app.source_control.split_diff = true;
    let output = frame_size(&ctx, &mut app, vec![], egui::vec2(800.0, 600.0));
    assert_eq!(
        accessible_node(&output, "Unified").toggled(),
        Some(egui::accesskit::Toggled::True)
    );
    assert_eq!(
        accessible_node(&output, "Side by side").toggled(),
        Some(egui::accesskit::Toggled::False)
    );
    assert!(accessible_node(&output, "Side by side").is_disabled());
    assert!(
        app.source_control.split_diff,
        "Resizing must keep the chosen layout preference"
    );
    let output = frame(&ctx, &mut app, vec![]);
    assert_eq!(
        accessible_node(&output, "Side by side").toggled(),
        Some(egui::accesskit::Toggled::True)
    );
    assert!(!accessible_node(&output, "Side by side").is_disabled());
}

#[cfg(target_os = "macos")]
#[test]
fn pending_diff_actions_stay_visible_disabled_and_keep_the_selection_highlight() {
    let (_directory, ctx, mut app, _git) = fixture();
    ctx.enable_accesskit();
    app.source_control.select(
        &ctx,
        Selection {
            path: "main.rs".into(),
            kind: DiffKind::Working,
        },
    );
    finish_git(&ctx, &mut app);
    let index = app
        .source_control
        .diff
        .as_ref()
        .unwrap()
        .lines
        .iter()
        .position(|l| l.kind == '-')
        .unwrap();
    let text = app.source_control.diff.as_ref().unwrap().lines[index].text[1..].to_owned();
    app.source_control.selected_lines.insert(index);
    app.source_control.pending = true;
    for split in [false, true] {
        app.source_control.split_diff = split;
        let output = frame(&ctx, &mut app, vec![]);
        assert!(visible(&output, "Stage hunk"));
        assert!(accessible_node(&output, "Stage hunk").is_disabled());
        assert!(accessible_node(&output, "Stage selected (1)").is_disabled());
        assert!(!accessible_node(&output, "Copy diff").is_disabled());
        let galley = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(shape) if shape.galley.job.text == text => Some(&shape.galley),
                _ => None,
            })
            .unwrap();
        assert!(galley
            .job
            .sections
            .iter()
            .all(|s| s.format.color == app.theme.selection_text.color()));
    }
    assert_eq!(app.source_control.selected_lines.len(), 1);
}

#[test]
fn horizontal_diff_scrolling_keeps_line_numbers_fixed_in_both_layouts() {
    let (_directory, ctx, mut app, _git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    let source = format!("let long = \"{}\";", "x".repeat(1200));
    std::fs::write(root.join("main.rs"), format!("{source}\n")).unwrap();
    app.source_control.select(
        &ctx,
        Selection {
            path: "main.rs".into(),
            kind: DiffKind::Working,
        },
    );
    finish_git(&ctx, &mut app);
    let x = |output: &egui::FullOutput, value: &str| {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == value => Some(text.pos.x),
                _ => None,
            })
            .unwrap_or_else(|| panic!("Missing painted text {value}"))
    };
    for split in [false, true] {
        app.source_control.split_diff = split;
        app.source_control.last_refresh = Instant::now();
        let before = frame(&ctx, &mut app, vec![]);
        let number = x(&before, "1");
        let code = x(&before, &source);
        frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(egui::pos2(700.0, 350.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(-220.0, 0.0),
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        let after = frame(&ctx, &mut app, vec![]);
        assert!(
            x(&after, &source) < code - 10.0,
            "The code must scroll horizontally"
        );
        assert!(
            (x(&after, "1") - number).abs() < 0.1,
            "Line numbers must remain fixed, split={split}"
        );
    }
}

#[test]
fn diff_context_menus_copy_the_same_source_and_patch_in_both_layouts() {
    let (_directory, ctx, mut app, _git) = fixture();
    app.source_control.select(
        &ctx,
        Selection {
            path: "main.rs".into(),
            kind: DiffKind::Working,
        },
    );
    finish_git(&ctx, &mut app);
    let patch = app.source_control.diff.as_ref().unwrap().patch.clone();
    let source = "    std::fs::read_to_string(path).unwrap()";
    let text_position = |output: &egui::FullOutput, label: &str| {
        output
            .shapes
            .iter()
            .filter_map(|shape| text_rect(&shape.shape, label))
            .next_back()
            .unwrap_or_else(|| panic!("Missing {label}"))
            .center()
    };
    let button = |pos, button, pressed| egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    };
    for (split, pending) in [(false, false), (true, false), (false, true), (true, true)] {
        app.source_control.split_diff = split;
        app.source_control.pending = pending;
        app.source_control.last_refresh = Instant::now();
        for (label, expected) in [("Copy line", source), ("Copy diff", patch.as_str())] {
            let output = frame(&ctx, &mut app, vec![]);
            let pos = text_position(&output, source);
            frame(
                &ctx,
                &mut app,
                vec![
                    egui::Event::PointerMoved(pos),
                    button(pos, egui::PointerButton::Secondary, true),
                ],
            );
            frame(
                &ctx,
                &mut app,
                vec![button(pos, egui::PointerButton::Secondary, false)],
            );
            let output = frame(&ctx, &mut app, vec![]);
            assert!(visible(&output, "Copy line"));
            assert!(visible(&output, "Copy diff"));
            let pos = text_position(&output, label);
            frame(
                &ctx,
                &mut app,
                vec![
                    egui::Event::PointerMoved(pos),
                    button(pos, egui::PointerButton::Primary, true),
                ],
            );
            let output = frame(
                &ctx,
                &mut app,
                vec![button(pos, egui::PointerButton::Primary, false)],
            );
            assert!(output.platform_output.commands.iter().any(|command|
                matches!(command, egui::OutputCommand::CopyText(text) if text == expected)),
                "Clipboard action must match in both layouts, split={split}, pending={pending}, action={label}");
        }
    }
}

#[test]
fn shared_display_commands_preserve_git_review_and_editor_state() {
    let (_directory, ctx, mut app, _git) = fixture();
    app.source_control.select(
        &ctx,
        Selection {
            path: "main.rs".into(),
            kind: DiffKind::Working,
        },
    );
    finish_git(&ctx, &mut app);
    app.source_control.draft = "Keep this draft while changing appearance".into();
    let selection = app.source_control.selection.clone();
    let patch = app.source_control.diff.as_ref().unwrap().patch.clone();
    let editor = app.active().page.text.clone();
    let revision = app.active().page.info.revision;
    for command in [
        Command::ZoomIn,
        Command::ZoomOut,
        Command::ResetZoom,
        Command::LineNumbers,
        Command::LineNumbers,
        Command::Sidebar,
        Command::Sidebar,
        Command::StatusBar,
        Command::StatusBar,
    ] {
        app.execute(&ctx, command);
        frame(&ctx, &mut app, vec![]);
        assert!(
            app.source_control.open,
            "Display command {command:?} must preserve Git review"
        );
        assert_eq!(app.source_control.selection, selection);
        assert_eq!(app.source_control.diff.as_ref().unwrap().patch, patch);
        assert_eq!(
            app.source_control.draft,
            "Keep this draft while changing appearance"
        );
    }
    app.font_families = Some(vec![]);
    app.execute(&ctx, Command::ChooseFont);
    assert!(app.source_control.open);
    assert!(app.palette);
    frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
    assert!(app.source_control.open);
    assert_eq!(app.active().page.text, editor);
    assert_eq!(app.active().page.info.revision, revision);
}

#[test]
fn review_controls_remain_visible_across_themes_fonts_gutters_and_window_sizes() {
    let (_directory, ctx, mut app, _git) = fixture();
    app.source_control.select(
        &ctx,
        Selection {
            path: "main.rs".into(),
            kind: DiffKind::Working,
        },
    );
    finish_git(&ctx, &mut app);
    let index = app
        .source_control
        .diff
        .as_ref()
        .unwrap()
        .lines
        .iter()
        .position(|l| l.kind == '-')
        .unwrap();
    app.source_control.selected_lines.insert(index);
    for theme in ["Hedit Ink", "Hedit Paper"] {
        app.set_theme(&ctx, hedit::theme::builtin(theme).unwrap());
        for (size, padding) in [(10.0, 0.0), (14.0, 0.0), (28.0, 8.0)] {
            app.settings.font_size = size;
            app.settings.line_padding = padding;
            for numbers in [false, true] {
                app.settings.line_numbers = numbers;
                for width in [800.0, 1280.0] {
                    for split in [false, true] {
                        app.source_control.split_diff = split;
                        app.source_control.last_refresh = Instant::now();
                        let output = frame_size(&ctx, &mut app, vec![], egui::vec2(width, 600.0));
                        for label in [
                            "Stage file",
                            "Copy diff",
                            "Stage selected (1)",
                            "Clear selection",
                            "Stage hunk",
                        ] {
                            assert!(visible(&output, label), "Hidden {label}: theme={theme}, font={size}, padding={padding}, numbers={numbers}, width={width}, split={split}");
                        }
                        let has_numbers = output
                            .shapes
                            .iter()
                            .any(|shape| text_rect(&shape.shape, "1").is_some());
                        assert_eq!(
                            has_numbers, numbers,
                            "Line-number setting must apply in both layouts"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn changed_lines_can_be_selected_and_staged_in_both_layouts() {
    for split in [false, true] {
        let (_directory, ctx, mut app, git) = fixture();
        let root = app.source_control.root.clone().unwrap();
        app.source_control.split_diff = split;
        click_label(&ctx, &mut app, "main.rs");
        finish_git(&ctx, &mut app);
        let before = "    std::fs::read_to_string(path).unwrap()";
        click_label(&ctx, &mut app, before);
        assert_eq!(app.source_control.selected_lines.len(), 1);
        // An unchanged background refresh preserves the review selection.
        app.source_control.request(&ctx, None);
        finish_git(&ctx, &mut app);
        assert_eq!(app.source_control.selected_lines.len(), 1);
        click_label(&ctx, &mut app, "Stage selected (1)");
        finish_git(&ctx, &mut app);
        assert!(app.source_control.selected_lines.is_empty());
        let indexed = git.command(&root, &["show", ":main.rs"]).unwrap();
        assert!(!indexed.contains("read_to_string"));
        assert!(std::fs::read_to_string(root.join("main.rs"))
            .unwrap()
            .contains("unwrap_or_default"));
        app.source_control.select(
            &ctx,
            Selection {
                path: "main.rs".into(),
                kind: DiffKind::Staged,
            },
        );
        finish_git(&ctx, &mut app);
        click_label(&ctx, &mut app, before);
        click_label(&ctx, &mut app, "Unstage selected (1)");
        finish_git(&ctx, &mut app);
        assert!(git
            .command(&root, &["show", ":main.rs"])
            .unwrap()
            .contains("unwrap()"));
    }
}

#[test]
fn diff_layouts_shape_and_reorder_rtl_without_changing_reviewed_text() {
    let (_directory, ctx, mut app, _git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    let source = "let title = \"עברית العربية\";";
    std::fs::write(root.join("main.rs"), format!("{source}\n")).unwrap();
    app.source_control.select(
        &ctx,
        Selection {
            path: "main.rs".into(),
            kind: DiffKind::Working,
        },
    );
    finish_git(&ctx, &mut app);
    for split in [false, true] {
        app.source_control.split_diff = split;
        let output = frame(&ctx, &mut app, vec![]);
        let galley = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == source => Some(&text.galley),
                _ => None,
            })
            .expect("The diff must render the reviewed source line");
        let glyphs = &galley.rows[0].glyphs;
        let first = source.chars().position(|c| c == 'ע').unwrap();
        assert!(
            glyphs[first].pos.x > glyphs[first + 1].pos.x,
            "Hebrew glyphs must read right to left"
        );
        let arabic = source.chars().position(|c| c == 'ا').unwrap();
        assert!(
            glyphs[arabic].pos.x > glyphs[arabic + 1].pos.x,
            "Arabic glyphs must read right to left"
        );
        assert!(app
            .source_control
            .diff
            .as_ref()
            .unwrap()
            .patch
            .contains(source));
    }
    assert_eq!(
        std::fs::read_to_string(root.join("main.rs")).unwrap(),
        format!("{source}\n")
    );
}

#[test]
fn diff_code_matches_editor_typography_syntax_and_selection_for_custom_themes() {
    let (_directory, ctx, mut app, _git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    let source = "\tfn main(){\tlet n = 42;}\n";
    std::fs::write(root.join("main.rs"), source).unwrap();
    app.source_control.open = false;
    app.open(&ctx, Some(root.join("main.rs")));
    finish_app_job(&mut app, &ctx);
    app.workspace.open[app.workspace.active].view.tab_size = Some(8);
    let display = hedit::advanced::Projection::new(source, 8, &[]).text;
    let line_text = display.trim_end_matches('\n');
    let find = |output: &egui::FullOutput, text: &str| {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(shape) if shape.galley.job.text == text => {
                    Some(shape.galley.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("Missing code layout: {text:?}"))
    };
    for name in ["Hedit Ink", "Hedit Paper"] {
        let mut theme = hedit::theme::builtin(name).unwrap();
        // Nonstandard status and selection colors expose a separate Git palette.
        theme.success = hedit::theme::Rgb::new(40, 110, 210);
        theme.error = hedit::theme::Rgb::new(180, 55, 155);
        theme.selection = hedit::theme::Rgb::new(65, 75, 100);
        theme.selection_text = hedit::theme::Rgb::new(245, 246, 247);
        app.set_theme(&ctx, theme.clone());
        assert_eq!(status_color('A', &theme), theme.success.color());
        assert_eq!(status_color('D', &theme), theme.error.color());
        assert_eq!(status_color('M', &theme), theme.warning.color());
        for (size, padding) in [(14.0, 0.0), (24.0, 7.0)] {
            app.settings.font_size = size;
            app.settings.line_padding = padding;
            app.source_control.open = false;
            let output = frame(&ctx, &mut app, vec![]);
            let editor = find(&output, &display);
            app.open_source_control(&ctx);
            finish_git(&ctx, &mut app);
            app.source_control.select(
                &ctx,
                Selection {
                    path: "main.rs".into(),
                    kind: DiffKind::Working,
                },
            );
            finish_git(&ctx, &mut app);
            for split in [false, true] {
                app.source_control.split_diff = split;
                app.source_control.clear_line_selection();
                app.source_control.last_refresh = Instant::now();
                let output = frame(&ctx, &mut app, vec![]);
                let diff = find(&output, line_text);
                assert_eq!(editor.rows[0].rect.height(), diff.rows[0].rect.height());
                for byte in line_text.char_indices().map(|(i, _)| i) {
                    let a = editor
                        .job
                        .sections
                        .iter()
                        .find(|s| s.byte_range.contains(&byte))
                        .unwrap();
                    let b = diff
                        .job
                        .sections
                        .iter()
                        .find(|s| s.byte_range.contains(&byte))
                        .unwrap();
                    assert_eq!(a.format.font_id, b.format.font_id);
                    assert_eq!(
                        a.format.color, b.format.color,
                        "Syntax color at byte {byte}, {name}"
                    );
                }
                assert!(output.shapes.iter().any(|shape| match &shape.shape {
                    egui::Shape::Rect(rect) =>
                        rect.fill == theme.success.color().gamma_multiply(0.10),
                    _ => false,
                }));
                let index = app
                    .source_control
                    .diff
                    .as_ref()
                    .unwrap()
                    .lines
                    .iter()
                    .position(|l| l.kind == '+')
                    .unwrap();
                app.source_control.selected_lines.insert(index);
                let output = frame(&ctx, &mut app, vec![]);
                let selected = find(&output, line_text);
                assert!(selected
                    .job
                    .sections
                    .iter()
                    .all(|s| s.format.color == theme.selection_text.color()));
                assert!(output.shapes.iter().any(|shape| match &shape.shape {
                    egui::Shape::Rect(rect) => rect.fill == theme.selection.color(),
                    _ => false,
                }));
            }
        }
    }
    assert_eq!(
        std::fs::read_to_string(root.join("main.rs")).unwrap(),
        source
    );
}

#[test]
fn diff_font_changes_and_zoom_stay_in_sync_with_the_editor() {
    let (_directory, ctx, mut app, git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    let source = "fn main() { let title = \"AVATAR\"; }\n";
    std::fs::write(root.join("main.rs"), source.replace("AVATAR", "AxATAR")).unwrap();
    git.execute(&root, &Operation::Stage(vec!["main.rs".into()]))
        .unwrap();
    git.execute(
        &root,
        &Operation::Commit {
            message: "Prepare font comparison".into(),
            amend: false,
        },
    )
    .unwrap();
    std::fs::write(root.join("main.rs"), source).unwrap();
    app.source_control.open = false;
    app.open(&ctx, Some(root.join("main.rs")));
    finish_app_job(&mut app, &ctx);
    app.open_source_control(&ctx);
    finish_git(&ctx, &mut app);
    app.source_control.select(
        &ctx,
        Selection {
            path: "main.rs".into(),
            kind: DiffKind::Working,
        },
    );
    finish_git(&ctx, &mut app);
    assert!(app
        .source_control
        .diff
        .as_ref()
        .unwrap()
        .lines
        .iter()
        .any(|line| line.kind == '+' && !line.emphasis.is_empty()));
    let find = |output: &egui::FullOutput, text: &str| {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(shape) if shape.galley.job.text == text => {
                    Some(shape.galley.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("Missing code layout: {text:?}"))
    };
    // A proportional face makes a stale family detectable through actual glyph
    // advances, even though both layouts' FontIds resolve to Monospace.
    let custom = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets/fonts/NotoSans-Regular.ttf")
        .to_string_lossy()
        .into_owned();
    let mut widths = vec![];
    for face in [String::new(), custom, String::new()] {
        app.settings.font_face = face.clone();
        frame(&ctx, &mut app, vec![]);
        let start = Instant::now();
        while !face.is_empty() && app.editor_font.is_none() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "Font did not load"
            );
            app.receive(&ctx);
            frame(&ctx, &mut app, vec![]);
            thread::sleep(Duration::from_millis(5));
        }
        for (scale, padding) in [(1.0, 0.0), (1.5, 6.0), (2.0, 0.0)] {
            ctx.set_pixels_per_point(scale);
            app.settings.line_padding = padding;
            for (size, event) in [
                (10.0, None),
                (14.0, None),
                (15.0, Some(key(Key::Plus, Modifiers::COMMAND))),
                (14.0, Some(key(Key::Minus, Modifiers::COMMAND))),
                (28.0, None),
            ] {
                let measure = event.is_none() && size == 14.0 && scale == 1.0;
                if event.is_none() {
                    app.settings.font_size = size;
                }
                let output = frame(&ctx, &mut app, event.into_iter().collect());
                assert!(app.source_control.open);
                assert_eq!(app.settings.font_size, size);
                let first = find(&output, source.trim_end());
                for split in [false, true] {
                    app.source_control.split_diff = split;
                    let output = frame(&ctx, &mut app, vec![]);
                    let diff = find(&output, source.trim_end());
                    app.source_control.open = false;
                    let output = frame(&ctx, &mut app, vec![]);
                    let editor = find(&output, source);
                    // Compare resolved glyph metrics, not just the requested size
                    // or family: both surfaces must actually render the same face.
                    let metrics = |galley: &egui::Galley| {
                        galley.rows[0]
                            .glyphs
                            .iter()
                            .map(|glyph| egui::epaint::text::Glyph {
                                // Intraline highlights split syntax sections.
                                section_index: 0,
                                ..*glyph
                            })
                            .collect::<Vec<_>>()
                    };
                    assert_eq!(
                        metrics(&diff),
                        metrics(&editor),
                        "face={face}, size={size}, scale={scale}, padding={padding}, split={split}"
                    );
                    assert_eq!(diff.rows[0].rect, editor.rows[0].rect);
                    assert!(diff
                        .job
                        .sections
                        .iter()
                        .all(|section| { section.format.font_id == FontId::monospace(size) }));
                    app.source_control.open = true;
                }
                if measure {
                    widths.push(first.rows[0].rect.width());
                }
            }
        }
    }
    assert_ne!(
        widths[0], widths[1],
        "The custom font must change glyph metrics"
    );
    assert_eq!(
        widths[0], widths[2],
        "Restoring the built-in font must restore glyph metrics"
    );
}

#[test]
fn conflict_previews_share_editor_font_spacing_and_syntax_colors() {
    let (_directory, ctx, mut app, _git) = fixture();
    app.settings.font_size = 24.0;
    app.settings.line_padding = 6.0;
    let current = "let current = 12;";
    let incoming = "let incoming = 34;";
    app.source_control.selection = Some(Selection {
        path: "main.rs".into(),
        kind: DiffKind::Conflict,
    });
    app.source_control.diff = Some(Arc::new(Diff {
        conflict: Some(git::Conflict {
            base: None,
            ours: Some(current.into()),
            theirs: Some(incoming.into()),
            expected: String::new(),
        }),
        ..Default::default()
    }));
    for name in ["Hedit Ink", "Hedit Paper"] {
        app.set_theme(&ctx, hedit::theme::builtin(name).unwrap());
        app.source_control.last_refresh = Instant::now();
        let output = frame(&ctx, &mut app, vec![]);
        for source in [current, incoming] {
            let galley = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == source => Some(&text.galley),
                    _ => None,
                })
                .expect("Conflict sides must render with the shared code layout");
            let font = galley.job.sections[0].format.font_id.clone();
            assert_eq!(font, FontId::monospace(24.0));
            let expected_height = ctx.fonts(|f| f.row_height(&font)) + 6.0;
            assert!((galley.rows[0].rect.height() - expected_height).abs() < 0.1);
            let sample = hedit::syntax::highlight_sample(
                source,
                24.0,
                f32::INFINITY,
                hedit::syntax::Language::Rust,
                &app.theme,
            );
            for byte in source.char_indices().map(|(i, _)| i) {
                let expected = sample
                    .sections
                    .iter()
                    .find(|s| s.byte_range.contains(&byte))
                    .unwrap();
                let actual = galley
                    .job
                    .sections
                    .iter()
                    .find(|s| s.byte_range.contains(&byte))
                    .unwrap();
                assert_eq!(actual.format.color, expected.format.color);
            }
        }
    }
}

#[test]
fn line_selection_clears_after_a_changed_diff_and_unsaved_text_blocks_staging() {
    let (_directory, ctx, mut app, git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    app.source_control.select(
        &ctx,
        Selection {
            path: "main.rs".into(),
            kind: DiffKind::Working,
        },
    );
    finish_git(&ctx, &mut app);
    let diff = app.source_control.diff.clone().unwrap();
    let changed: Vec<_> = diff
        .lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| matches!(l.kind, '+' | '-').then_some(i))
        .collect();
    app.source_control.select_line(changed[0], &diff, false);
    app.source_control
        .select_line(*changed.last().unwrap(), &diff, true);
    assert_eq!(app.source_control.selected_lines.len(), changed.len());
    app.open(&ctx, Some(root.join("main.rs")));
    finish_app_job(&mut app, &ctx);
    let before = app.active().page.text.clone();
    let index = app.workspace.active;
    app.workspace.open[index]
        .page
        .text
        .push_str("\n// keep unsaved\n");
    app.workspace.edit(index, &before).unwrap();
    app.git_action(
        &ctx,
        Operation::Lines {
            selection: app.source_control.selection.clone().unwrap(),
            expected: diff.patch.clone(),
            lines: changed,
        },
    );
    assert!(!app.source_control.pending);
    assert!(app
        .source_control
        .error
        .as_ref()
        .unwrap()
        .contains("Save or close"));
    assert!(!git.snapshot(&root, 200).unwrap().files[0].staged());
    std::fs::write(root.join("main.rs"), "new saved text\n").unwrap();
    app.source_control.request(&ctx, None);
    finish_git(&ctx, &mut app);
    assert!(app.source_control.selected_lines.is_empty());
    assert!(app.active().page.text.contains("keep unsaved"));
    git.execute(&root, &Operation::Stage(vec!["main.rs".into()]))
        .unwrap();
    app.source_control.select(
        &ctx,
        Selection {
            path: "main.rs".into(),
            kind: DiffKind::Staged,
        },
    );
    finish_git(&ctx, &mut app);
    let diff = app.source_control.diff.clone().unwrap();
    let lines = diff
        .lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| matches!(l.kind, '+' | '-').then_some(i))
        .collect();
    app.git_action(
        &ctx,
        Operation::Lines {
            selection: app.source_control.selection.clone().unwrap(),
            expected: diff.patch.clone(),
            lines,
        },
    );
    assert!(
        app.source_control.pending,
        "Index-only line unstaging allows unsaved editor text"
    );
    finish_git(&ctx, &mut app);
    assert!(!git.snapshot(&root, 200).unwrap().files[0].staged());
    assert!(app.active().page.text.contains("keep unsaved"));
}

#[test]
fn file_tree_collapses_filters_and_keeps_duplicate_names_distinct() {
    let (_directory, ctx, mut app, _git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    for (folder, text) in [("src/nested", "source\n"), ("tests/nested", "test\n")] {
        std::fs::create_dir_all(root.join(folder)).unwrap();
        std::fs::write(root.join(folder).join("same.rs"), text).unwrap();
    }
    app.source_control.request(&ctx, None);
    finish_git(&ctx, &mut app);
    click_label(&ctx, &mut app, "src/nested  (1)");
    let output = frame(&ctx, &mut app, vec![]);
    assert!(visible(&output, "tests/nested  (1)"));
    app.source_control.filter = "src/nested".into();
    let output = frame(&ctx, &mut app, vec![]);
    assert!(!visible(&output, "tests/nested  (1)"));
    click_label(&ctx, &mut app, "List");
    app.source_control.filter.clear();
    let output = frame(&ctx, &mut app, vec![]);
    assert!(visible(&output, "src/nested/same.rs"));
    assert!(visible(&output, "tests/nested/same.rs"));
    click_label(&ctx, &mut app, "src/nested/same.rs");
    finish_git(&ctx, &mut app);
    assert_eq!(
        app.source_control.selection.as_ref().unwrap().path,
        PathBuf::from("src/nested/same.rs")
    );
}

#[test]
fn large_changed_file_trees_only_paint_visible_rows_and_filter_hidden_files() {
    let (_directory, ctx, mut app, _git) = fixture();
    let mut snapshot = (**app.source_control.snapshot.as_ref().unwrap()).clone();
    snapshot.files = (0..10_000)
        .map(|i| FileStatus {
            path: format!("generated/file-{i:05}.rs").into(),
            original: None,
            index: '.',
            worktree: 'M',
            conflict: false,
        })
        .collect();
    app.source_control.snapshot = Some(Arc::new(snapshot));
    let output = frame(&ctx, &mut app, vec![]);
    let painted = output
        .shapes
        .iter()
        .filter(|shape| match &shape.shape {
            egui::Shape::Text(text) => text.galley.job.text.starts_with("file-"),
            _ => false,
        })
        .count();
    assert!(
        painted > 0 && painted < 50,
        "Only visible rows should be painted, got {painted}"
    );
    app.source_control.filter = "file-09999".into();
    let output = frame(&ctx, &mut app, vec![]);
    assert!(visible(&output, "file-09999.rs"));
    assert!(!visible(&output, "file-00000.rs"));
}

#[test]
fn sidebar_stages_and_commits_saved_files_and_preserves_the_editor() {
    let (_directory, ctx, mut app, git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    let revision = app.active().page.info.revision;
    let output = frame(&ctx, &mut app, vec![]);
    assert!(visible(&output, "Source Control"));
    assert!(!visible(&output, "OPEN FILES"));
    click_label(&ctx, &mut app, "main.rs");
    finish_git(&ctx, &mut app);
    assert!(app
        .source_control
        .diff
        .as_ref()
        .unwrap()
        .patch
        .contains("+    std::fs::read_to_string(path).unwrap_or_default()"));
    click_label(&ctx, &mut app, "Stage file");
    finish_git(&ctx, &mut app);
    assert!(git.snapshot(&root, 200).unwrap().files[0].staged());
    app.source_control.draft = "Avoid a panic while loading notes".into();
    click_label(&ctx, &mut app, "Commit staged (1)");
    finish_git(&ctx, &mut app);
    assert!(git.snapshot(&root, 200).unwrap().files.is_empty());
    assert_eq!(
        git.history(&root, 200).unwrap()[0].subject,
        "Avoid a panic while loading notes"
    );
    assert!(app.source_control.draft.is_empty());
    assert_eq!(app.active().page.info.revision, revision);
    click_label(&ctx, &mut app, "Files");
    assert!(!app.source_control.open);
    assert!(
        app.workspace.closed.is_empty(),
        "Git view must not create Closed Files entries"
    );
}

#[test]
fn unsaved_buffers_block_staging_and_worktree_changes_without_losing_text() {
    let (_directory, ctx, mut app, git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    app.open(&ctx, Some(root.join("main.rs")));
    finish_app_job(&mut app, &ctx);
    let index = app.workspace.active;
    let before = app.active().page.text.clone();
    app.workspace.open[index]
        .page
        .text
        .push_str("\n// unsaved note\n");
    app.workspace.edit(index, &before).unwrap();
    let text = app.active().page.text.clone();
    app.git_action(&ctx, Operation::Stage(vec!["main.rs".into()]));
    assert!(!app.source_control.pending);
    assert!(app
        .source_control
        .error
        .as_ref()
        .unwrap()
        .contains("Save or close"));
    assert!(!git.snapshot(&root, 200).unwrap().files[0].staged());
    app.git_action(&ctx, Operation::CreateBranch("feature".into()));
    assert_eq!(git.snapshot(&root, 200).unwrap().branch, "main");
    assert_eq!(app.active().page.text, text);
    assert!(app.active().page.info.dirty);
}

#[test]
fn branch_prompt_cancel_escape_and_create_have_distinct_outcomes() {
    let (_directory, ctx, mut app, git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    app.source_control.view = View::Branches;
    app.source_control.prompt = Some(Prompt::Branch("cancelled".into()));
    frame(&ctx, &mut app, vec![]);
    click_label(&ctx, &mut app, "Cancel");
    assert!(app.source_control.prompt.is_none());
    assert!(!app.source_control.pending);
    app.source_control.prompt = Some(Prompt::Branch("escaped".into()));
    frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
    assert!(app.source_control.prompt.is_none());
    app.source_control.prompt = Some(Prompt::Branch("feature".into()));
    frame(&ctx, &mut app, vec![]);
    click_label(&ctx, &mut app, "Create");
    finish_git(&ctx, &mut app);
    assert_eq!(git.snapshot(&root, 200).unwrap().branch, "feature");
}

#[test]
fn source_control_history_and_compact_layout_keep_actions_visible() {
    let (_directory, ctx, mut app, _git) = fixture();
    for name in ["Hedit Ink", "Hedit Paper"] {
        app.set_theme(&ctx, hedit::theme::builtin(name).unwrap());
        for size in [egui::vec2(1280.0, 820.0), egui::vec2(800.0, 600.0)] {
            let output = frame_size(&ctx, &mut app, vec![], size);
            assert!(visible(&output, "Commit staged (0)"));
            assert!(visible(&output, "Source Control"));
        }
    }
    app.source_control.view = View::History;
    let commit = app.source_control.snapshot.as_ref().unwrap().commits[0]
        .oid
        .clone();
    app.source_control.commit = Some(commit);
    app.source_control.request(&ctx, None);
    finish_git(&ctx, &mut app);
    assert_eq!(app.source_control.files.len(), 1);
    assert!(app.source_control.body.contains("Read project notes"));
    let output = frame(&ctx, &mut app, vec![]);
    assert!(
        visible(&output, "A"),
        "Commit review must show the file's added status"
    );
    click_label(&ctx, &mut app, "main.rs");
    finish_git(&ctx, &mut app);
    assert!(app
        .source_control
        .diff
        .as_ref()
        .unwrap()
        .patch
        .contains("+use std::path::Path;"));
}

#[test]
fn shortcut_close_and_commit_drafts_preserve_editor_state() {
    #[derive(Default)]
    struct Storage(BTreeMap<String, String>);
    impl eframe::Storage for Storage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.into(), value);
        }
        fn flush(&mut self) {}
    }
    let (_directory, ctx, mut app, _git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    app.source_control.draft = "Keep this message across restarts".into();
    let mut storage = Storage::default();
    app.source_control.tree_view = false;
    app.source_control.save(&mut storage);
    assert!(!SourceControl::load(Some(&storage)).tree_view);
    assert_eq!(
        SourceControl::load(Some(&storage))
            .drafts
            .get(&root)
            .unwrap(),
        "Keep this message across restarts"
    );
    let count = app.workspace.open.len();
    frame(&ctx, &mut app, vec![key(Key::W, Modifiers::COMMAND)]);
    assert!(!app.source_control.open);
    assert_eq!(app.workspace.open.len(), count);
    frame(
        &ctx,
        &mut app,
        vec![key(Key::G, Modifiers::COMMAND | Modifiers::SHIFT)],
    );
    assert!(app.source_control.open);
    assert!(!app.goto_open);
    finish_git(&ctx, &mut app);
    app.source_control.draft.clear();
    app.source_control.save(&mut storage);
    assert!(!SourceControl::load(Some(&storage))
        .drafts
        .contains_key(&root));
}

#[test]
fn workspace_switcher_preserves_drafts_and_files_across_themes_and_small_sidebars() {
    let (_directory, ctx, mut app, _git) = fixture();
    app.source_control.draft = "Keep my commit message".into();
    let id = app.active().view.id;
    for theme in ["Hedit Ink", "Hedit Paper"] {
        app.set_theme(&ctx, hedit::theme::builtin(theme).unwrap());
        click_label(&ctx, &mut app, "Files");
        assert!(!app.source_control.open);
        // Repaint after the mode switch so the Files panel has settled.
        frame(&ctx, &mut app, vec![]);
        click_label(&ctx, &mut app, "Source Control");
        finish_git(&ctx, &mut app);
        assert!(app.source_control.open);
        assert_eq!(app.source_control.draft, "Keep my commit message");
        assert_eq!(app.active().view.id, id);
        assert!(app.workspace.closed.is_empty());
    }
    // Both targets remain entirely inside a sidebar at its minimum width.
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(160.0, 80.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                controls::workspace_switcher(ui, &app.theme, false, "Ctrl+Shift+G");
            });
        },
    );
    assert!(visible(&output, "Files"));
    assert!(visible(&output, "Git"));
}

#[test]
fn staging_unstaging_and_committing_keep_the_selected_comparison_accurate() {
    let (_directory, ctx, mut app, _git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    std::fs::write(root.join("note.txt"), "new note\n").unwrap();
    app.source_control.select(
        &ctx,
        Selection {
            path: "note.txt".into(),
            kind: DiffKind::Untracked,
        },
    );
    finish_git(&ctx, &mut app);
    for (operation, kind) in [
        (Operation::Stage(vec!["note.txt".into()]), DiffKind::Staged),
        (
            Operation::Unstage(vec!["note.txt".into()]),
            DiffKind::Untracked,
        ),
        (Operation::Stage(vec!["note.txt".into()]), DiffKind::Staged),
    ] {
        app.git_action(&ctx, operation);
        finish_git(&ctx, &mut app);
        assert_eq!(app.source_control.selection.as_ref().unwrap().kind, kind);
        assert!(
            app.source_control.error.is_none(),
            "{:?}",
            app.source_control.error
        );
    }
    app.git_action(
        &ctx,
        Operation::Commit {
            message: "Add note".into(),
            amend: false,
        },
    );
    finish_git(&ctx, &mut app);
    assert!(app.source_control.selection.is_none());
    assert!(app.source_control.diff.is_none());
    assert!(app.source_control.error.is_none());
}

#[test]
fn index_only_hunk_unstage_keeps_unsaved_editor_text() {
    let (_directory, ctx, mut app, git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    git.execute(&root, &Operation::Stage(vec!["main.rs".into()]))
        .unwrap();
    app.open(&ctx, Some(root.join("main.rs")));
    finish_app_job(&mut app, &ctx);
    let index = app.workspace.active;
    let old = app.active().page.text.clone();
    app.workspace.open[index]
        .page
        .text
        .push_str("\n// keep this unsaved comment\n");
    app.workspace.edit(index, &old).unwrap();
    let kept = app.active().page.text.clone();
    let selection = Selection {
        path: "main.rs".into(),
        kind: DiffKind::Staged,
    };
    let diff = git.diff(&root, &selection).unwrap();
    app.git_action(
        &ctx,
        Operation::Hunk {
            selection,
            expected: diff.patch,
            hunk: 0,
        },
    );
    assert!(
        app.source_control.pending,
        "Unstaging the index must not require saving editor text"
    );
    finish_git(&ctx, &mut app);
    assert!(git
        .command(&root, &["diff", "--cached"])
        .unwrap()
        .is_empty());
    assert_eq!(app.active().page.text, kept);
    assert!(app.active().page.info.dirty);
}

#[test]
fn a_disappearing_nested_repository_cannot_stage_a_parent_file() {
    let (directory, ctx, mut app, git) = fixture();
    let nested = app.source_control.root.clone().unwrap();
    git.command(directory.path(), &["init", "-b", "parent"])
        .unwrap();
    std::fs::write(
        directory.path().join("main.rs"),
        "parent file must stay unstaged\n",
    )
    .unwrap();
    std::fs::rename(
        nested.join(".git"),
        directory.path().join("nested-git-backup"),
    )
    .unwrap();
    app.git_action(&ctx, Operation::Stage(vec!["main.rs".into()]));
    finish_git(&ctx, &mut app);
    assert!(git
        .command(directory.path(), &["diff", "--cached", "--name-only"])
        .unwrap()
        .is_empty());
    assert!(app
        .source_control
        .error
        .as_ref()
        .is_some_and(|e| e.contains("repository")));
}

#[test]
fn cancelled_repository_picker_preserves_selection_and_draft_without_an_error() {
    let (_directory, ctx, mut app, _git) = fixture();
    let selection = Selection {
        path: "main.rs".into(),
        kind: DiffKind::Working,
    };
    app.source_control.select(&ctx, selection.clone());
    finish_git(&ctx, &mut app);
    let root = app.source_control.root.clone();
    app.source_control.draft = "Retain this draft".into();
    app.source_control.pending = true;
    app.source_control.tx.send(None).unwrap();
    app.poll_source_control(&ctx);
    assert!(!app.source_control.pending);
    assert!(app.source_control.error.is_none());
    assert_eq!(app.source_control.root, root);
    assert_eq!(app.source_control.selection, Some(selection));
    assert!(app.source_control.diff.is_some());
    assert_eq!(app.source_control.draft, "Retain this draft");
}

#[test]
fn a_pending_diff_cannot_restore_a_selection_after_changing_views() {
    let (_directory, ctx, mut app, _git) = fixture();
    app.source_control.select(
        &ctx,
        Selection {
            path: "main.rs".into(),
            kind: DiffKind::Working,
        },
    );
    app.source_control.view = View::History;
    app.source_control.selection = None;
    app.source_control.diff = None;
    finish_git(&ctx, &mut app);
    assert!(app.source_control.selection.is_none());
    assert!(app.source_control.diff.is_none());
}

#[test]
#[ignore = "writes native source control preview images"]
fn source_control_visual_preview() {
    let (directory, ctx, mut app, git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/settings.rs"),
        "pub const AUTO_SAVE: bool = true;\n",
    )
    .unwrap();
    git.execute(&root, &Operation::Stage(vec!["src/settings.rs".into()]))
        .unwrap();
    std::fs::write(
        root.join("README.md"),
        "# Project notes\n\nA small native project.\n",
    )
    .unwrap();
    app.source_control.draft =
        "Handle missing notes gracefully\n\nUse an empty document when a file cannot be read."
            .into();
    app.source_control.request(&ctx, None);
    finish_git(&ctx, &mut app);
    app.source_control.select(
        &ctx,
        Selection {
            path: "main.rs".into(),
            kind: DiffKind::Working,
        },
    );
    finish_git(&ctx, &mut app);
    let destination = std::env::var_os("HEDIT_PREVIEW_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.path().into());
    std::fs::create_dir_all(&destination).unwrap();
    app.source_control.selected_lines = app
        .source_control
        .diff
        .as_ref()
        .unwrap()
        .lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| matches!(l.kind, '+' | '-').then_some(i))
        .collect();
    for name in ["Hedit Ink", "Hedit Paper"] {
        app.set_theme(&ctx, hedit::theme::builtin(name).unwrap());
        for split in [false, true] {
            app.source_control.split_diff = split;
            let output = frame(&ctx, &mut app, vec![]);
            let bytes =
                super::super::rtl_tests::rasterize(&ctx, output, egui::vec2(1280.0, 820.0), 1.0);
            std::fs::write(
                destination.join(format!(
                    "git-{}-{}.ppm",
                    if name == "Hedit Ink" { "dark" } else { "light" },
                    if split { "split" } else { "unified" }
                )),
                bytes,
            )
            .unwrap();
        }
    }
}

#[test]
fn branch_comparison_uses_read_only_diff_and_refreshes_moving_revisions() {
    let (_directory, ctx, mut app, git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    let base = git.resolve_commit(&root, "HEAD").unwrap();
    git.execute(&root, &Operation::Stage(vec!["main.rs".into()]))
        .unwrap();
    git.execute(
        &root,
        &Operation::Commit {
            message: "Handle missing notes".into(),
            amend: false,
        },
    )
    .unwrap();
    app.git_start_comparison(&ctx, base.clone(), "HEAD".into());
    finish_git(&ctx, &mut app);
    assert_eq!(app.source_control.view, View::Compare);
    assert!(app.source_control.error.is_none());
    click_label(&ctx, &mut app, "main.rs");
    finish_git(&ctx, &mut app);
    assert!(matches!(
        app.source_control.selection.as_ref().unwrap().kind,
        DiffKind::Compare { .. }
    ));
    assert!(!app.source_control.diff.as_ref().unwrap().supports_partial());
    for size in [egui::vec2(1280.0, 820.0), egui::vec2(800.0, 600.0)] {
        let output = frame_size(&ctx, &mut app, vec![], size);
        assert!(visible(&output, "Copy diff"));
        assert!(visible(&output, "Base"));
        assert!(!visible(&output, "Stage file"));
        assert!(!visible(&output, "Stage hunk"));
        let before =
            egui::containers::panel::PanelState::load(&ctx, egui::Id::new("source-control"))
                .unwrap()
                .rect
                .width();
        for _ in 0..8 {
            frame_size(&ctx, &mut app, vec![], size);
        }
        let after =
            egui::containers::panel::PanelState::load(&ctx, egui::Id::new("source-control"))
                .unwrap()
                .rect
                .width();
        assert!(
            (after - before).abs() < 1.0,
            "Revision inputs must not widen the sidebar on every frame: {before} to {after}"
        );
        assert!(after <= size.x * 0.4 + 1.0);
    }
    std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    git.execute(&root, &Operation::Stage(vec!["main.rs".into()]))
        .unwrap();
    git.execute(
        &root,
        &Operation::Commit {
            message: "Newer change".into(),
            amend: false,
        },
    )
    .unwrap();
    app.source_control.request(&ctx, None);
    finish_git(&ctx, &mut app);
    assert!(
        app.source_control.selection.is_none(),
        "A moved tip must not display the old diff under the new comparison"
    );
    assert!(app.source_control.diff.is_none());
}

#[test]
fn advanced_queries_keep_file_scope_and_ignore_outdated_worker_results() {
    let (_directory, ctx, mut app, git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    std::fs::write(root.join("other.txt"), "other\n").unwrap();
    git.execute(&root, &Operation::Stage(vec!["other.txt".into()]))
        .unwrap();
    git.execute(
        &root,
        &Operation::Commit {
            message: "Unrelated commit".into(),
            amend: false,
        },
    )
    .unwrap();
    app.git_file_history(&ctx, "main.rs".into());
    finish_git(&ctx, &mut app);
    let data = app.source_control.inspection_data().unwrap();
    let inspect::Data::History(commits) = &*data else {
        panic!("Expected scoped history");
    };
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].subject, "Read project notes");
    app.git_branch_history(&ctx, "HEAD".into());
    // Simulate closing/changing the view before its background request completes.
    app.source_control.view = View::Changes;
    finish_git(&ctx, &mut app);
    assert!(app.source_control.inspection_data().is_none());
    app.git_line_history(&ctx, "main.rs".into());
    finish_git(&ctx, &mut app);
    let data = app.source_control.inspection_data().unwrap();
    let inspect::Data::Blame(lines) = &*data else {
        panic!("Expected line history");
    };
    assert!(lines.iter().any(|l| l.text.contains("unwrap()")));
    assert!(lines.iter().all(|l| !l.text.contains("unwrap_or_default")));
    let output = frame_size(&ctx, &mut app, vec![], egui::vec2(800.0, 600.0));
    assert!(visible(&output, "Show line history"));
    assert!(visible(&output, "main.rs"));
    assert!(visible(&output, "use std::path::Path;"));
}

#[test]
fn recovery_branch_and_soft_undo_preserve_unsaved_editor_text_but_rebase_is_blocked() {
    let (_directory, ctx, mut app, git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    git.execute(&root, &Operation::Stage(vec!["main.rs".into()]))
        .unwrap();
    git.execute(
        &root,
        &Operation::Commit {
            message: "Second commit".into(),
            amend: false,
        },
    )
    .unwrap();
    let oid = git.resolve_commit(&root, "HEAD").unwrap();
    app.open(&ctx, Some(root.join("main.rs")));
    finish_app_job(&mut app, &ctx);
    let before = app.active().page.text.clone();
    let index = app.workspace.active;
    app.workspace.open[index]
        .page
        .text
        .push_str("// unsaved idea\n");
    app.workspace.edit(index, &before).unwrap();
    app.source_control.open = true;
    app.git_action(&ctx, Operation::Rebase("main".into()));
    assert!(!app.source_control.pending);
    assert!(app
        .source_control
        .error
        .as_ref()
        .unwrap()
        .contains("Save or close"));
    app.source_control.prompt = Some(Prompt::Named {
        action: inspect::NamedAction::Branch(oid.clone()),
        name: "recovered".into(),
        message: String::new(),
    });
    frame(&ctx, &mut app, vec![]);
    click_label(&ctx, &mut app, "Create branch");
    finish_git(&ctx, &mut app);
    assert_eq!(git.resolve_commit(&root, "recovered").unwrap(), oid);
    assert_eq!(git.snapshot(&root, 10).unwrap().branch, "main");
    app.git_action(&ctx, Operation::UndoCommit { expected: oid });
    finish_git(&ctx, &mut app);
    assert!(app.active().page.text.ends_with("// unsaved idea\n"));
    assert!(app.active().page.info.dirty);
    assert!(git.snapshot(&root, 10).unwrap().files[0].staged());
}

#[test]
fn more_menu_opens_tags_and_recovery_without_crowding_primary_views() {
    let (_directory, ctx, mut app, _git) = fixture();
    click_label(&ctx, &mut app, "More");
    click_label(&ctx, &mut app, "Tags");
    finish_git(&ctx, &mut app);
    assert_eq!(app.source_control.view, View::Tags);
    let output = frame_size(&ctx, &mut app, vec![], egui::vec2(800.0, 600.0));
    assert!(visible(&output, "Tag HEAD…"));
    // A view-specific menu title keeps the selected tool discoverable.
    click_label(&ctx, &mut app, "Tags");
    click_label(&ctx, &mut app, "Recovery");
    finish_git(&ctx, &mut app);
    assert_eq!(app.source_control.view, View::Recovery);
    assert!(
        matches!(&*app.source_control.inspection_data().unwrap(), inspect::Data::Recovery(entries) if !entries.is_empty())
    );
}

#[test]
#[ignore = "writes native advanced Git preview images"]
fn advanced_git_visual_preview() {
    let (directory, ctx, mut app, git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    let base = git.resolve_commit(&root, "HEAD").unwrap();
    git.execute(&root, &Operation::Stage(vec!["main.rs".into()]))
        .unwrap();
    git.execute(
        &root,
        &Operation::Commit {
            message: "Handle missing notes gracefully".into(),
            amend: false,
        },
    )
    .unwrap();
    let oid = git.resolve_commit(&root, "HEAD").unwrap();
    git.execute(
        &root,
        &Operation::CreateTag {
            name: "v1.0.0".into(),
            oid: oid.clone(),
            message: "First native release".into(),
        },
    )
    .unwrap();
    let destination = std::env::var_os("HEDIT_PREVIEW_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.path().into());
    std::fs::create_dir_all(&destination).unwrap();
    for (view, label) in [
        (View::Compare, "compare"),
        (View::Blame, "blame"),
        (View::Recovery, "recovery"),
        (View::History, "history"),
    ] {
        match view {
            View::Compare => {
                app.git_start_comparison(&ctx, base.clone(), "HEAD".into());
                finish_git(&ctx, &mut app);
                click_label(&ctx, &mut app, "main.rs");
                finish_git(&ctx, &mut app);
            }
            View::Blame => {
                app.git_line_history(&ctx, "main.rs".into());
                finish_git(&ctx, &mut app);
            }
            View::History => {
                app.git_branch_history(&ctx, "HEAD".into());
                finish_git(&ctx, &mut app);
                app.source_control.commit = Some(oid.clone());
                app.source_control.request(&ctx, None);
                finish_git(&ctx, &mut app);
                click_label(&ctx, &mut app, "main.rs");
                finish_git(&ctx, &mut app);
            }
            _ => {
                app.source_control.clear_review();
                app.source_control.view = view;
                app.source_control.request(&ctx, None);
                finish_git(&ctx, &mut app);
            }
        }
        for name in ["Hedit Ink", "Hedit Paper"] {
            app.set_theme(&ctx, hedit::theme::builtin(name).unwrap());
            for size in [egui::vec2(1280.0, 820.0), egui::vec2(800.0, 600.0)] {
                let output = frame_size(&ctx, &mut app, vec![], size);
                let bytes = super::super::rtl_tests::rasterize(&ctx, output, size, 1.0);
                std::fs::write(
                    destination.join(format!(
                        "git-{label}-{}-{}.ppm",
                        if name == "Hedit Ink" { "dark" } else { "light" },
                        size.x
                    )),
                    bytes,
                )
                .unwrap();
            }
        }
    }
}

#[test]
fn review_queue_and_keyboard_navigation_follow_the_filtered_files() {
    let (_directory, ctx, mut app, _git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    std::fs::write(root.join("a.txt"), "A\n").unwrap();
    std::fs::write(root.join("b.txt"), "B\n").unwrap();
    app.source_control.request(&ctx, None);
    finish_git(&ctx, &mut app);
    let first = app.source_control.review_queue();
    assert_eq!(first.len(), 3);
    assert!(
        Arc::ptr_eq(&first, &app.source_control.review_queue()),
        "Idle frames must reuse the review list"
    );
    click_label(&ctx, &mut app, "Review changes");
    finish_git(&ctx, &mut app);
    assert_eq!(
        app.source_control.selection.as_ref().unwrap().path,
        Path::new("a.txt")
    );
    frame(&ctx, &mut app, vec![key(Key::ArrowDown, Modifiers::ALT)]);
    finish_git(&ctx, &mut app);
    assert_eq!(
        app.source_control.selection.as_ref().unwrap().path,
        Path::new("b.txt")
    );
    click_label(&ctx, &mut app, "Previous");
    finish_git(&ctx, &mut app);
    assert_eq!(
        app.source_control.selection.as_ref().unwrap().path,
        Path::new("a.txt")
    );
    app.source_control.filter = "main".into();
    assert_eq!(app.source_control.review_queue().len(), 1);
    frame(&ctx, &mut app, vec![key(Key::ArrowDown, Modifiers::ALT)]);
    finish_git(&ctx, &mut app);
    assert_eq!(
        app.source_control.selection.as_ref().unwrap().path,
        Path::new("main.rs")
    );
    assert!(visible(&frame(&ctx, &mut app, vec![]), "File 1 of 1"));
    assert!(app.source_control.open);
}

#[test]
fn contextual_back_and_view_switches_restore_the_review_and_filters() {
    let (_directory, ctx, mut app, _git) = fixture();
    app.source_control.filter = "main".into();
    app.source_control.select(
        &ctx,
        Selection {
            path: "main.rs".into(),
            kind: DiffKind::Working,
        },
    );
    finish_git(&ctx, &mut app);
    let selection = app.source_control.selection.clone();
    app.git_file_history(&ctx, "main.rs".into());
    finish_git(&ctx, &mut app);
    assert_eq!(app.source_control.view, View::History);
    click_label(&ctx, &mut app, "Back to Changes");
    finish_git(&ctx, &mut app);
    assert_eq!(app.source_control.selection, selection);
    assert_eq!(app.source_control.filter, "main");
    app.source_control.visit(&ctx, View::History, false);
    finish_git(&ctx, &mut app);
    app.source_control.filter = "Read project".into();
    let oid = app.source_control.snapshot.as_ref().unwrap().commits[0]
        .oid
        .clone();
    app.source_control.commit = Some(oid.clone());
    app.source_control.request(&ctx, None);
    finish_git(&ctx, &mut app);
    app.source_control.select(
        &ctx,
        Selection {
            path: "main.rs".into(),
            kind: DiffKind::Commit(oid.clone()),
        },
    );
    finish_git(&ctx, &mut app);
    app.git_start_comparison(&ctx, oid.clone(), "HEAD".into());
    finish_git(&ctx, &mut app);
    click_label(&ctx, &mut app, "Back to History");
    finish_git(&ctx, &mut app);
    assert_eq!(app.source_control.filter, "Read project");
    assert_eq!(app.source_control.commit, Some(oid.clone()));
    assert_eq!(
        app.source_control.selection.as_ref().unwrap().kind,
        DiffKind::Commit(oid)
    );
    app.source_control.visit(&ctx, View::Changes, false);
    finish_git(&ctx, &mut app);
    assert_eq!(app.source_control.filter, "main");
    assert_eq!(app.source_control.selection, selection);
}

#[cfg(target_os = "macos")]
#[test]
fn commit_readiness_shortcut_feedback_and_amend_confirmation_are_consistent() {
    let (_directory, ctx, mut app, git) = fixture();
    ctx.enable_accesskit();
    let root = app.source_control.root.clone().unwrap();
    let output = frame(&ctx, &mut app, vec![]);
    assert!(accessible_node(&output, "Commit staged (0)").is_disabled());
    assert!(visible(
        &output,
        "Stage files or selected lines to include them."
    ));
    let editor_text = app.active().page.text.clone();
    app.git_action(&ctx, Operation::Stage(vec!["main.rs".into()]));
    finish_git(&ctx, &mut app);
    let output = frame(&ctx, &mut app, vec![]);
    assert!(visible(&output, "Files staged"));
    assert!(accessible_node(&output, "Commit staged (1)").is_disabled());
    app.source_control.draft = "Commit with a shortcut".into();
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::COMMAND)]);
    finish_git(&ctx, &mut app);
    assert!(app.source_control.open);
    assert_eq!(app.active().page.text, editor_text);
    assert_eq!(
        git.history(&root, 20).unwrap()[0].subject,
        "Commit with a shortcut"
    );
    assert!(visible(&frame(&ctx, &mut app, vec![]), "Commit created"));
    let oid = git.resolve_commit(&root, "HEAD").unwrap();
    app.source_control.amend = true;
    app.source_control.draft = "Amended message".into();
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::COMMAND)]);
    assert!(visible(&frame(&ctx, &mut app, vec![]), "Amend commit"));
    click_label(&ctx, &mut app, "Cancel");
    assert_eq!(git.resolve_commit(&root, "HEAD").unwrap(), oid);
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::COMMAND)]);
    click_label(&ctx, &mut app, "Amend commit");
    finish_git(&ctx, &mut app);
    assert_eq!(
        git.history(&root, 20).unwrap()[0].subject,
        "Amended message"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn background_refresh_keeps_navigation_live_and_repository_actions_disabled() {
    let (_directory, ctx, mut app, _git) = fixture();
    ctx.enable_accesskit();
    app.source_control.pending = true;
    let output = frame(&ctx, &mut app, vec![]);
    assert!(!visible(&output, "Git is working…"));
    assert!(!visible(&output, "Cancel"));
    assert!(!accessible_node(&output, "main.rs").is_disabled());
    assert!(accessible_node(&output, "Stage all").is_disabled());
    click_label(&ctx, &mut app, "History");
    assert_eq!(app.source_control.view, View::History);
    assert!(app.source_control.pending);
}

#[test]
fn compact_branch_picker_switches_branches_and_keeps_common_actions_discoverable() {
    let (_directory, ctx, mut app, git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    let oid = git.resolve_commit(&root, "HEAD").unwrap();
    git.execute(
        &root,
        &Operation::CreateBranchAt {
            name: "feature/notes".into(),
            oid,
        },
    )
    .unwrap();
    app.source_control.request(&ctx, None);
    finish_git(&ctx, &mut app);
    let output = frame_size(&ctx, &mut app, vec![], egui::vec2(800.0, 600.0));
    assert!(visible(&output, "Local repository"));
    assert!(visible(&output, "Add remote…"));
    assert!(!visible(&output, "Pull"));
    assert!(!visible(&output, "Push"));
    click_label(&ctx, &mut app, "main");
    assert!(visible(&frame(&ctx, &mut app, vec![]), "Find a branch…"));
    click_label(&ctx, &mut app, "feature/notes");
    finish_git(&ctx, &mut app);
    assert_eq!(git.snapshot(&root, 20).unwrap().branch, "feature/notes");
    assert!(visible(&frame(&ctx, &mut app, vec![]), "Branch switched"));
}

#[test]
fn comparison_starts_with_a_useful_base_and_swap_applies_immediately() {
    let (_directory, ctx, mut app, git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    let base = git.resolve_commit(&root, "HEAD").unwrap();
    git.execute(&root, &Operation::Stage(vec!["main.rs".into()]))
        .unwrap();
    git.execute(
        &root,
        &Operation::Commit {
            message: "A second commit".into(),
            amend: false,
        },
    )
    .unwrap();
    app.source_control.request(&ctx, None);
    finish_git(&ctx, &mut app);
    app.source_control.visit(&ctx, View::Compare, false);
    finish_git(&ctx, &mut app);
    let data = app.source_control.inspection_data().unwrap();
    let inspect::Data::Compare(before) = &*data else {
        panic!("Expected comparison");
    };
    assert_eq!(before.base, base);
    assert_eq!(before.files.len(), 1);
    click_label(&ctx, &mut app, "Swap");
    finish_git(&ctx, &mut app);
    let data = app.source_control.inspection_data().unwrap();
    let inspect::Data::Compare(after) = &*data else {
        panic!("Expected comparison");
    };
    assert_eq!(after.head, base);
    assert!(after.files.is_empty());
}

#[test]
fn git_commands_are_available_in_the_catalog_and_respect_custom_bindings() {
    let (_directory, ctx, mut app, git) = fixture();
    assert!(Command::ALL.contains(&Command::GitCommit));
    assert!(Command::ALL.contains(&Command::GitNextFile));
    assert!(!app.command_enabled(Command::GitCommit));
    assert!(app.command_enabled(Command::GitNextFile));
    assert!(!app.command_enabled(Command::GitPreviousFile));
    let root = app.source_control.root.clone().unwrap();
    app.git_action(&ctx, Operation::Stage(vec!["main.rs".into()]));
    finish_git(&ctx, &mut app);
    app.source_control.draft = "Use a custom Git binding".into();
    app.quality.config.keybindings.push(quality::Binding {
        command: Command::GitCommit,
        shortcut: "F9".into(),
        editor_only: false,
    });
    let before = git.resolve_commit(&root, "HEAD").unwrap();
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::COMMAND)]);
    assert!(!app.source_control.pending);
    assert!(app.source_control.open);
    assert_eq!(git.resolve_commit(&root, "HEAD").unwrap(), before);
    frame(&ctx, &mut app, vec![key(Key::F9, Modifiers::NONE)]);
    finish_git(&ctx, &mut app);
    assert_eq!(
        git.history(&root, 20).unwrap()[0].subject,
        "Use a custom Git binding"
    );
    assert!(app.source_control.open);
}

#[test]
fn missing_repository_and_filtered_changes_offer_a_useful_next_step() {
    let (_directory, ctx, mut app, _git) = fixture();
    app.source_control.filter = "does-not-match".into();
    let output = frame(&ctx, &mut app, vec![]);
    assert!(visible(&output, "No files match this filter"));
    assert!(!visible(&output, "Working tree clean"));
    click_label(&ctx, &mut app, "Clear filter");
    assert!(app.source_control.filter.is_empty());
    assert!(visible(&frame(&ctx, &mut app, vec![]), "Review changes"));
    app.source_control.snapshot = None;
    app.source_control.root = None;
    let output = frame(&ctx, &mut app, vec![]);
    assert!(visible(&output, "Open a Git repository to get started"));
    assert!(!visible(&output, "Working tree clean"));
}

#[cfg(target_os = "macos")]
#[test]
fn diverged_branches_and_operation_banners_offer_actions_that_can_succeed() {
    let (_directory, ctx, mut app, _git) = fixture();
    ctx.enable_accesskit();
    let mut snapshot = (**app.source_control.snapshot.as_ref().unwrap()).clone();
    snapshot.remotes = vec!["origin".into()];
    snapshot.upstream = "origin/main".into();
    snapshot.ahead = 2;
    snapshot.behind = 3;
    snapshot.operation = Some("Merge in progress".into());
    snapshot.files[0].conflict = true;
    app.source_control.snapshot = Some(Arc::new(snapshot));
    app.source_control.view = View::Branches;
    let output = frame_size(&ctx, &mut app, vec![], egui::vec2(800.0, 600.0));
    assert!(visible(&output, "Branches have diverged"));
    assert!(accessible_node(&output, "Pull 3").is_disabled());
    assert!(accessible_node(&output, "Push 2").is_disabled());
    assert!(!accessible_node(&output, "Compare").is_disabled());
    assert!(visible(&output, "Merge in progress"));
    assert!(visible(&output, "Resolve in Changes"));
    assert!(accessible_node(&output, "Continue").is_disabled());
    assert!(!accessible_node(&output, "Abort…").is_disabled());
}

#[test]
fn commit_shortcut_includes_text_delivered_in_the_same_input_batch() {
    let (_directory, ctx, mut app, git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    app.git_action(&ctx, Operation::Stage(vec!["main.rs".into()]));
    finish_git(&ctx, &mut app);
    app.source_control.draft = "Message".into();
    click_label(&ctx, &mut app, "Message");
    frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::Text("final".into()),
            key(Key::Enter, Modifiers::COMMAND),
        ],
    );
    finish_git(&ctx, &mut app);
    assert!(git.history(&root, 20).unwrap()[0].subject.contains("final"));
    assert!(app.source_control.draft.is_empty());
}

#[test]
fn missing_commit_details_report_an_error_instead_of_an_empty_review() {
    let (_directory, ctx, mut app, _git) = fixture();
    app.source_control.view = View::History;
    app.source_control.commit = Some("0123456789012345678901234567890123456789".into());
    app.source_control.request(&ctx, None);
    finish_git(&ctx, &mut app);
    assert!(
        app.source_control
            .error
            .as_ref()
            .is_some_and(|e| e.contains("commit")),
        "A pruned or otherwise missing commit must produce a visible error"
    );
    assert!(app.source_control.files.is_empty());
    assert!(app.source_control.diff.is_none());
}

#[test]
fn git_text_prompts_focus_the_first_field_and_accept_enter() {
    let (_directory, ctx, mut app, git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    app.source_control.prompt = Some(Prompt::Branch(String::new()));
    frame(&ctx, &mut app, vec![]);
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Text("typed-branch".into())],
    );
    assert!(
        matches!(&app.source_control.prompt, Some(Prompt::Branch(name)) if name == "typed-branch")
    );
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    finish_git(&ctx, &mut app);
    assert!(app.source_control.prompt.is_none());
    assert_eq!(git.snapshot(&root, 20).unwrap().branch, "typed-branch");
}

#[test]
fn remote_branch_picker_reuses_an_existing_local_tracking_branch() {
    let (_directory, ctx, mut app, git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    let remote = tempfile::tempdir().unwrap();
    git.command(remote.path(), &["init", "--bare", "-b", "main"])
        .unwrap();
    git.execute(
        &root,
        &Operation::AddRemote {
            name: "origin".into(),
            url: remote.path().to_string_lossy().into_owned(),
        },
    )
    .unwrap();
    git.execute(
        &root,
        &Operation::Publish {
            remote: "origin".into(),
            branch: "main".into(),
        },
    )
    .unwrap();
    app.source_control.request(&ctx, None);
    finish_git(&ctx, &mut app);
    click_label(&ctx, &mut app, "main");
    click_label(&ctx, &mut app, "origin/main");
    finish_git(&ctx, &mut app);
    assert!(
        app.source_control.error.is_none(),
        "Selecting an already-tracked remote branch must not fail: {:?}",
        app.source_control.error
    );
    assert_eq!(git.snapshot(&root, 20).unwrap().branch, "main");
}

#[test]
fn tracking_dialog_suggests_the_correct_name_for_remotes_containing_slashes() {
    let (_directory, ctx, mut app, git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    let oid = git.resolve_commit(&root, "HEAD").unwrap();
    let remote = tempfile::tempdir().unwrap();
    git.command(remote.path(), &["init", "--bare"]).unwrap();
    git.execute(
        &root,
        &Operation::AddRemote {
            name: "team/origin".into(),
            url: remote.path().to_string_lossy().into_owned(),
        },
    )
    .unwrap();
    git.command(
        &root,
        &["update-ref", "refs/remotes/team/origin/topic", &oid],
    )
    .unwrap();
    app.source_control.request(&ctx, None);
    finish_git(&ctx, &mut app);
    click_label(&ctx, &mut app, "main");
    click_label(&ctx, &mut app, "team/origin/topic");
    assert!(
        matches!(&app.source_control.prompt,Some(Prompt::Named { name, .. }) if name == "topic")
    );
    frame(&ctx, &mut app, vec![egui::Event::Text("my-topic".into())]);
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    finish_git(&ctx, &mut app);
    assert_eq!(git.snapshot(&root, 20).unwrap().branch, "my-topic");
    assert_eq!(
        git.command(&root, &["rev-parse", "--symbolic-full-name", "@{upstream}"])
            .unwrap()
            .trim(),
        "refs/remotes/team/origin/topic"
    );
}

#[test]
fn cloning_exposes_progress_and_a_failed_clone_preserves_the_current_review() {
    let (_directory, ctx, mut app, _git) = fixture();
    app.source_control.select(
        &ctx,
        Selection {
            path: "main.rs".into(),
            kind: DiffKind::Working,
        },
    );
    finish_git(&ctx, &mut app);
    app.source_control.draft = "Keep my commit draft".into();
    let root = app.source_control.root.clone().unwrap();
    let selected = app.source_control.selection.clone();
    let patch = app.source_control.diff.as_ref().unwrap().patch.clone();
    // An existing destination is rejected before any clone process can write.
    app.git_clone(&ctx, "/nonexistent/disposable-source".into(), root.clone());
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        app.source_control_editor(ctx)
    });
    assert!(visible(&output, "Cloning repository…"));
    assert!(visible(&output, "Cancel"));
    finish_git(&ctx, &mut app);
    assert!(app.source_control.error.is_some());
    assert_eq!(app.source_control.root, Some(root));
    assert!(app.source_control.snapshot.is_some());
    assert_eq!(app.source_control.selection, selected);
    assert_eq!(app.source_control.diff.as_ref().unwrap().patch, patch);
    assert_eq!(app.source_control.draft, "Keep my commit draft");
}

#[test]
fn tag_remote_and_clone_forms_work_by_keyboard_and_preserve_repository_drafts() {
    let (_directory, ctx, mut app, git) = fixture();
    let original = app.source_control.root.clone().unwrap();
    let oid = git.resolve_commit(&original, "HEAD").unwrap();
    app.source_control.draft = "Keep the original repository draft".into();
    app.source_control.prompt = Some(Prompt::Named {
        action: inspect::NamedAction::Tag(oid.clone()),
        name: String::new(),
        message: String::new(),
    });
    frame(&ctx, &mut app, vec![]);
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Text("keyboard-tag".into())],
    );
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Text("Keyboard annotation".into())],
    );
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::COMMAND)]);
    finish_git(&ctx, &mut app);
    assert!(app.source_control.prompt.is_none());
    let tag = git
        .tags(&original)
        .unwrap()
        .into_iter()
        .find(|tag| tag.name == "keyboard-tag")
        .unwrap();
    assert_eq!(tag.commit, Some(oid));
    assert!(git
        .command(&original, &["cat-file", "-p", &tag.object])
        .unwrap()
        .contains("Keyboard annotation"));
    let mirror = tempfile::tempdir().unwrap();
    git.command(mirror.path(), &["init", "--bare", "-b", "main"])
        .unwrap();
    app.source_control.prompt = Some(Prompt::Remote {
        name: "origin".into(),
        url: String::new(),
    });
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![egui::Event::Text("mirror".into())]);
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Text(
            mirror.path().to_string_lossy().into_owned(),
        )],
    );
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    finish_git(&ctx, &mut app);
    assert!(app.source_control.prompt.is_none());
    assert!(app.source_control.error.is_none());
    assert!(git
        .snapshot(&original, 20)
        .unwrap()
        .remotes
        .contains(&"mirror".into()));
    git.execute(
        &original,
        &Operation::Publish {
            remote: "mirror".into(),
            branch: "main".into(),
        },
    )
    .unwrap();
    let destination = tempfile::tempdir().unwrap();
    let cloned = destination.path().join("cloned");
    app.source_control.prompt = Some(Prompt::Clone {
        url: String::new(),
        destination: String::new(),
    });
    frame(&ctx, &mut app, vec![]);
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Text(
            mirror.path().to_string_lossy().into_owned(),
        )],
    );
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Text(cloned.to_string_lossy().into_owned())],
    );
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    finish_git(&ctx, &mut app);
    assert!(app.source_control.prompt.is_none());
    assert!(app.source_control.error.is_none());
    assert_eq!(
        app.source_control.root,
        Some(git.discover(&cloned).unwrap())
    );
    assert!(visible(&frame(&ctx, &mut app, vec![]), "Repository cloned"));
    app.source_control.change_root(&ctx, original);
    finish_git(&ctx, &mut app);
    assert_eq!(
        app.source_control.draft,
        "Keep the original repository draft"
    );
}

#[test]
fn history_rows_select_when_clicking_the_subject_author_or_reference_text() {
    let (_directory, ctx, mut app, _git) = fixture();
    ctx.style_mut(|style| style.interaction.selectable_labels = true);
    app.source_control.visit(&ctx, View::History, false);
    finish_git(&ctx, &mut app);
    for field in 0..3 {
        app.source_control.clear_review();
        app.source_control.last_refresh = Instant::now();
        let commit = app.source_control.snapshot.as_ref().unwrap().commits[0].clone();
        let label = match field {
            0 => commit.subject.clone(),
            1 => format!("{} · {}", commit.author, commit.date),
            _ => commit.refs.clone(),
        };
        assert!(!label.is_empty());
        click_label(&ctx, &mut app, &label);
        finish_git(&ctx, &mut app);
        assert_eq!(
            app.source_control.commit,
            Some(commit.oid),
            "Clicking commit text must select its row: {label}"
        );
        assert!(!app.source_control.files.is_empty());
    }
}

#[cfg(target_os = "macos")]
#[test]
fn history_rows_expose_accessible_selection_state() {
    let (_directory, ctx, mut app, _git) = fixture();
    ctx.enable_accesskit();
    app.source_control.visit(&ctx, View::History, false);
    finish_git(&ctx, &mut app);
    let subject = app.source_control.snapshot.as_ref().unwrap().commits[0]
        .subject
        .clone();
    for selected in [false, true] {
        if selected {
            click_label(&ctx, &mut app, &subject);
            finish_git(&ctx, &mut app);
        }
        let output = frame(&ctx, &mut app, vec![]);
        assert!(
            output
                .platform_output
                .accesskit_update
                .unwrap()
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some(subject.as_str())
                    && node.toggled()
                        == Some(if selected {
                            egui::accesskit::Toggled::True
                        } else {
                            egui::accesskit::Toggled::False
                        })),
            "Commit row must expose its selection state"
        );
    }
}

#[cfg(unix)]
#[test]
fn cancelling_a_running_commit_keeps_the_review_visible_and_allows_retry() {
    use std::os::unix::fs::PermissionsExt;
    let (_directory, ctx, mut app, git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    app.git_action(&ctx, Operation::Stage(vec!["main.rs".into()]));
    finish_git(&ctx, &mut app);
    let selection = Selection {
        path: "main.rs".into(),
        kind: DiffKind::Staged,
    };
    app.source_control.select(&ctx, selection.clone());
    finish_git(&ctx, &mut app);
    let before = git.command(&root, &["rev-parse", "HEAD"]).unwrap();
    let hooks = root.join("hooks");
    std::fs::create_dir(&hooks).unwrap();
    let hook = hooks.join("pre-commit");
    std::fs::write(
        &hook,
        "#!/bin/sh\nprintf ready > hook-started\nexec sleep 30\n",
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    git.command(
        &root,
        &["config", "core.hooksPath", hooks.to_str().unwrap()],
    )
    .unwrap();
    app.source_control.draft = "Retry this cancelled commit".into();
    let operation = Operation::Commit {
        message: app.source_control.draft.clone(),
        amend: false,
    };
    app.git_action(&ctx, operation.clone());
    let started = Instant::now();
    while !root.join("hook-started").exists() {
        assert!(started.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(5));
    }
    app.source_control.cancel.store(true, Ordering::Relaxed);
    while app.source_control.pending {
        assert!(started.elapsed() < Duration::from_secs(10));
        app.poll_source_control(&ctx);
        assert!(
            app.source_control.snapshot.is_some(),
            "Cancellation must keep the repository review visible"
        );
        assert_eq!(app.source_control.selection, Some(selection.clone()));
        assert!(app.source_control.diff.is_some());
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(git.command(&root, &["rev-parse", "HEAD"]).unwrap(), before);
    assert_eq!(app.source_control.draft, "Retry this cancelled commit");
    assert!(app
        .source_control
        .error
        .as_ref()
        .is_some_and(|e| e.contains("cancelled")));
    assert!(app
        .source_control
        .snapshot
        .as_ref()
        .unwrap()
        .files
        .iter()
        .any(FileStatus::staged));
    git.command(
        &root,
        &["config", "core.hooksPath", "/nonexistent/hedit-test-hooks"],
    )
    .unwrap();
    app.git_action(&ctx, operation);
    finish_git(&ctx, &mut app);
    assert_ne!(git.command(&root, &["rev-parse", "HEAD"]).unwrap(), before);
    assert!(app.source_control.draft.is_empty());
    assert!(app.source_control.error.is_none());
}
