use super::tests::{click_label, finish_app_job, frame_size, text_rect};
use super::*;

fn fixture(lines: usize) -> (tempfile::TempDir, egui::Context, HeditApp) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    std::fs::create_dir_all(root.join("src")).unwrap();
    let mut text = "// Module context\n".to_owned();
    for i in 0..lines {
        text.push_str(&format!("let item_{i} = needle();\n"));
    }
    text.push_str("// End of module\n");
    std::fs::write(root.join("src/processor.rs"), text).unwrap();
    std::fs::write(root.join("z-last.txt"), "needle in configuration\n").unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.set_folder(hedit::folder::read_directory(root).unwrap());
    app.quality.folder_query = "needle".into();
    app.quality.folder_replacement = "replacement".into();
    app.quality.folders_open = true;
    app.folder_search(&ctx, false);
    finish_app_job(&mut app, &ctx);
    (directory, ctx, app)
}

#[test]
fn persistent_search_preserves_editor_find_state_and_reuses_grouped_rows() {
    let (_directory, ctx, mut app) = fixture(1500);
    app.query = "editor-only".into();
    app.replacement = "editor replacement".into();
    app.execute(&ctx, Command::FindFiles);
    assert!(!app.quality.folders_open, "docked search is not an overlay");
    assert_eq!(app.sidebar, workbench_navigation::Sidebar::Search);
    let size = egui::vec2(720.0, 480.0);
    for _ in 0..4 {
        frame_size(&ctx, &mut app, vec![], size);
    }
    let builds = app.quality.folder_rows.builds;
    for _ in 0..20 {
        frame_size(&ctx, &mut app, vec![], size);
    }
    assert_eq!(app.quality.folder_rows.builds, builds);
    frame_size(&ctx, &mut app, vec![egui::Event::Text("x".into())], size);
    assert_eq!(app.query, "editor-only");
    assert_eq!(app.replacement, "editor replacement");
    assert_eq!(app.quality.folder_query, "needlex");
    frame_size(
        &ctx,
        &mut app,
        vec![super::tests::key(Key::Escape, Modifiers::NONE)],
        size,
    );
    assert_eq!(app.sidebar, workbench_navigation::Sidebar::Search);
    assert!(app.settings.show_sidebar);
    frame_size(
        &ctx,
        &mut app,
        vec![egui::Event::Text("draft".into())],
        size,
    );
    assert_eq!(app.active().page.text, "draft");
    assert_eq!(app.quality.folder_rows.builds, builds);
    let hits = app.quality.results.hits.len();
    app.sidebar = workbench_navigation::Sidebar::Files;
    frame_size(&ctx, &mut app, vec![], size);
    app.execute(&ctx, Command::FindFiles);
    frame_size(&ctx, &mut app, vec![], size);
    assert_eq!(app.quality.results.hits.len(), hits);
    assert_eq!(app.quality.folder_rows.builds, builds);
}

#[test]
fn search_sidebar_keeps_the_document_visible_at_large_ui_sizes() {
    let (_directory, ctx, mut app) = fixture(40);
    app.workspace.open[0].page.text = "Keep editing this unsaved note".into();
    app.workspace.edit(0, "").unwrap();
    app.execute(&ctx, Command::FindFiles);
    app.settings.ui_font_size = 20.0;
    app.settings.ui_density = hedit::ui_style::Density::Comfortable;
    let size = egui::vec2(720.0, 480.0);
    for _ in 0..5 {
        frame_size(&ctx, &mut app, vec![], size);
    }
    let out = frame_size(&ctx, &mut app, vec![], size);
    let side = egui::containers::panel::PanelState::load(&ctx, egui::Id::new("search-sidebar"))
        .unwrap()
        .rect;
    assert!(side.width() < size.x * 0.6);
    assert!(out
        .shapes
        .iter()
        .any(|s| text_rect(&s.shape, "Keep editing this unsaved note")
            .is_some_and(|r| r.intersects(s.clip_rect))));
    assert!(!app.source_control.open);
    assert_eq!(app.workspace.open.len(), 1);
}

#[test]
fn compact_search_and_replacement_keep_their_controls_visible() {
    for replace in [false, true] {
        let (_directory, ctx, mut app) = fixture(30);
        app.quality.folder_replace = replace;
        if replace {
            app.folder_search(&ctx, true);
            finish_app_job(&mut app, &ctx);
        }
        let size = egui::vec2(720.0, 480.0);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        for _ in 0..4 {
            frame_size(&ctx, &mut app, vec![], size);
        }
        let output = frame_size(&ctx, &mut app, vec![], size);
        for label in [
            if replace { "Preview" } else { "Find all" },
            "Next",
            "Previous",
            if replace {
                "Apply to 2 files"
            } else {
                "project · 0 skipped"
            },
        ] {
            assert!(
                output.shapes.iter().any(|s| text_rect(&s.shape, label)
                    .is_some_and(|r| screen.contains_rect(r) && s.clip_rect.contains_rect(r))),
                "{label} must remain visible"
            );
        }
    }
}

#[test]
fn f4_reveals_offscreen_matches_and_expands_their_file_group() {
    let (_directory, ctx, mut app) = fixture(1500);
    let size = egui::vec2(720.0, 480.0);
    for _ in 0..4 {
        frame_size(&ctx, &mut app, vec![], size);
    }
    let last = app.quality.results.hits.len() - 1;
    let path = app.quality.results.hits[last].path.clone();
    app.quality.folder_collapsed.insert(path.clone());
    app.quality.result_current = Some(last - 1);
    app.next_result(true);
    finish_app_job(&mut app, &ctx);
    for _ in 0..3 {
        frame_size(&ctx, &mut app, vec![], size);
    }
    let output = frame_size(&ctx, &mut app, vec![], size);
    assert!(!app.quality.folder_collapsed.contains(&path));
    assert!(output
        .shapes
        .iter()
        .any(|s| text_rect(&s.shape, "needle in configuration")
            .is_some_and(|r| s.clip_rect.contains_rect(r))));
}

#[test]
fn clicking_a_match_sets_the_start_for_next_result_navigation() {
    let (_directory, ctx, mut app) = fixture(8);
    app.quality.results_newer = false;
    tests::frame(&ctx, &mut app, vec![]);
    tests::frame(&ctx, &mut app, vec![]);
    click_label(&ctx, &mut app, "let item_1 = needle();");
    assert_eq!(app.quality.result_current, Some(1));
    assert!(app.quality.results_newer);
    finish_app_job(&mut app, &ctx);
    app.next_result(true);
    assert_eq!(app.quality.result_current, Some(2));
}

#[test]
fn matches_are_highlighted_and_context_is_optional() {
    let (_directory, ctx, mut app) = fixture(3);
    tests::frame(&ctx, &mut app, vec![]);
    let output = tests::frame(&ctx, &mut app, vec![]);
    assert!(!output
        .shapes
        .iter()
        .any(|s| text_rect(&s.shape, "// Module context").is_some()));
    assert!(output.shapes.iter().any(|s| match &s.shape {
        egui::Shape::Text(text) if text.galley.job.text == "let item_0 = needle();" => text
            .galley
            .job
            .sections
            .iter()
            .any(|s| s.format.background != egui::Color32::TRANSPARENT),
        _ => false,
    }));
    app.quality.folder_context = true;
    let output = tests::frame(&ctx, &mut app, vec![]);
    assert!(output
        .shapes
        .iter()
        .any(|s| text_rect(&s.shape, "// Module context").is_some()));
}

#[test]
fn invalid_search_is_reported_inside_the_results_window() {
    let (_directory, ctx, mut app) = fixture(4);
    app.quality.folder_options.regex = true;
    app.quality.folder_query = "[".into();
    app.folder_search(&ctx, false);
    finish_app_job(&mut app, &ctx);
    assert!(matches!(
        app.quality.folder_state,
        folder_search::State::Failed(_)
    ));
    assert!(app.error.is_none());
    assert!(app.quality.results.hits.is_empty());
}

#[test]
fn stale_or_incomplete_replacement_previews_cannot_be_applied() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let path = root.join("file.txt");
    std::fs::write(&path, "old text\n").unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.set_folder(hedit::folder::read_directory(root).unwrap());
    app.quality.folder_query = "old".into();
    app.quality.folder_replacement = "new".into();
    app.folder_search(&ctx, true);
    tests::finish_app_job(&mut app, &ctx);
    app.quality.folder_query = "different".into();
    app.apply_folder_replacements(&ctx);
    assert!(!app.busy);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "old text\n");
    assert_eq!(app.quality.results.plans.len(), 1);
    app.quality.folder_query = "old".into();
    app.quality.results.truncated = true;
    app.apply_folder_replacements(&ctx);
    assert!(!app.busy);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "old text\n");
    assert_eq!(app.quality.results.plans.len(), 1);
}

#[test]
#[ignore = "writes folder-search SVG previews to HEDIT_PREVIEW_DIR"]
fn export_folder_search_previews() {
    let output = PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    for (theme, name) in [(Theme::default(), "ink"), (Theme::paper(), "paper")] {
        for replace in [false, true] {
            for size in [egui::vec2(720.0, 480.0), egui::vec2(1280.0, 820.0)] {
                let (_directory, ctx, mut app) = fixture(16);
                app.set_theme(&ctx, theme.clone());
                app.quality.folder_replace = replace;
                if replace {
                    app.folder_search(&ctx, true);
                    finish_app_job(&mut app, &ctx);
                }
                for _ in 0..4 {
                    frame_size(&ctx, &mut app, vec![], size);
                }
                let rendered = frame_size(&ctx, &mut app, vec![], size);
                let svg = crate::preview::export(
                    &rendered.shapes,
                    [size.x, size.y],
                    app.theme.background.color(),
                );
                std::fs::write(
                    output.join(format!(
                        "folders-{name}-{}-{}.svg",
                        if replace { "replace" } else { "find" },
                        size.x
                    )),
                    svg,
                )
                .unwrap();
            }
        }
    }
}

#[test]
fn pending_folder_search_keeps_typing_and_other_job_errors_independent() {
    let (_directory, ctx, mut app) = fixture(2);
    app.quality.folders_open = false;
    app.focus_editor = true;
    frame_size(&ctx, &mut app, vec![], egui::vec2(900.0, 700.0));
    app.folder_search(&ctx, true);
    assert!(!app.busy);
    // Hold a result channel open to model a slow disk or SSH host deterministically.
    let (_tx, rx) = mpsc::channel();
    app.quality.folder_task = Some(folder_search::Task {
        cancel: Arc::new(AtomicBool::new(false)),
        rx,
    });
    frame_size(
        &ctx,
        &mut app,
        vec![egui::Event::Text("typed while searching".into())],
        egui::vec2(900.0, 700.0),
    );
    assert_eq!(app.active().page.text, "typed while searching");
    assert!(!app.folder_preview_valid());
    app.busy = true;
    app.tx.send(Err("Unrelated save failed".into())).unwrap();
    app.receive(&ctx);
    assert_eq!(app.error.as_deref(), Some("Unrelated save failed"));
    assert!(matches!(
        app.quality.folder_state,
        folder_search::State::Searching
    ));
}

#[test]
fn search_completion_cannot_unlock_a_mutation_and_cancel_is_scoped() {
    let (_directory, ctx, mut app) = fixture(2);
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    app.quality.folder_task = Some(folder_search::Task {
        cancel: cancel.clone(),
        rx,
    });
    app.quality.folder_state = folder_search::State::Searching;
    app.busy = true;
    cancel.store(true, Ordering::Relaxed);
    tx.send(Ok(Default::default())).unwrap();
    app.receive(&ctx);
    assert!(
        app.busy,
        "a search receipt cannot release another operation's lock"
    );
    assert!(
        !app.cancel.load(Ordering::Relaxed),
        "search cancellation is independent"
    );
    assert!(matches!(
        app.quality.folder_state,
        folder_search::State::Cancelled
    ));
}

#[test]
fn changed_open_buffers_and_roots_invalidate_replacement_snapshots() {
    let (_directory, ctx, mut app) = fixture(2);
    app.folder_search(&ctx, true);
    finish_app_job(&mut app, &ctx);
    assert!(app.folder_preview_valid());
    let old = app.active().page.text.clone();
    let index = app.workspace.active;
    app.workspace.open[index]
        .page
        .text
        .push_str("new unsaved text");
    app.workspace.edit(index, &old).unwrap();
    assert!(!app.folder_preview_valid());
    app.folder_search(&ctx, true);
    finish_app_job(&mut app, &ctx);
    assert!(app.folder_preview_valid());
    app.workspace.folders.clear();
    assert!(!app.folder_preview_valid());
}
