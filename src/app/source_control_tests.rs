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
