use super::tests::{click_label, finish_app_job, frame, frame_size, text_rect};
use super::*;
use hedit::session::MarkdownView;

const SAMPLE: &str = "# Project notebook\n\nA **native** preview of your _Markdown_.\n\n## Release checklist\n\n- [x] Save recovery data\n- [ ] Review the release\n\n> Keep notes close to the code.\n\n| Feature | Status |\n| --- | --- |\n| Live preview | Ready |\n\n```rust\nlet message = \"Hello, Markdown!\";\n```\n\n[Project website](https://example.com)\n";

fn fixture(text: &str) -> (tempfile::TempDir, egui::Context, HeditApp) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.md");
    std::fs::write(&path, text).unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    ctx.style_mut(|style| style.scroll_animation = egui::style::ScrollAnimation::none());
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.open(&ctx, Some(path));
    finish_app_job(&mut app, &ctx);
    app.settings.show_minimap = false;
    frame(&ctx, &mut app, vec![]);
    (directory, ctx, app)
}

fn visible(output: &egui::FullOutput, label: &str) -> bool {
    output.shapes.iter().any(|shape| {
        text_rect(&shape.shape, label).is_some_and(|rect| shape.clip_rect.contains_rect(rect))
    })
}

const MEDIA_SAMPLE: &str = "# Equations and diagrams\n\nInline math $E = mc^2$ stays beside the text.\n\n$$\n\\frac{1}{2} + x^2\n$$\n\n```math\n\\sum_{i=1}^{n} i = \\frac{n(n+1)}{2}\n```\n\n```mermaid\nflowchart LR\n    A[Edit Markdown] --> B[Live preview]\n    B --> C[Recover next launch]\n```\n\nOrdinary Markdown continues below.\n";

fn finish_media(ctx: &egui::Context, app: &mut HeditApp, expected: usize) -> egui::FullOutput {
    let started = Instant::now();
    loop {
        let output = frame(ctx, app, vec![]);
        if !app.markdown.media.pending() && app.markdown.media.textures().len() == expected {
            return output;
        }
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "media did not finish rendering"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn markdown_math_and_mermaid_render_refresh_and_release_without_editing_source() {
    let (_directory, ctx, mut app) = fixture(MEDIA_SAMPLE);
    let revision = app.active().page.info.revision;
    app.execute(&ctx, Command::MarkdownPreview);
    let output = finish_media(&ctx, &mut app, 4);
    assert!(visible(&output, "Ordinary Markdown continues below."));
    let old: Vec<_> = app.markdown.media.textures().into_keys().collect();
    frame(&ctx, &mut app, vec![]);
    assert!(
        app.markdown
            .media
            .textures()
            .keys()
            .all(|id| old.contains(id)),
        "redraw reuses cached textures"
    );
    app.execute(&ctx, Command::MarkdownRefresh);
    assert!(app.markdown.media.textures().is_empty());
    finish_media(&ctx, &mut app, 4);
    assert!(app
        .markdown
        .media
        .textures()
        .keys()
        .all(|id| !old.contains(id)));
    assert_eq!(app.active().page.text, MEDIA_SAMPLE);
    assert_eq!(app.active().page.info.revision, revision);
    app.set_theme(&ctx, hedit::theme::builtin("One Light").unwrap());
    finish_media(&ctx, &mut app, 8);

    // Revision changes detach old jobs and textures; their completion must not
    // repopulate the current document's cache.
    let index = app.workspace.active;
    let old = app.active().page.text.clone();
    app.workspace.open[index].page.text = "A new equation $y^3$.\n".into();
    app.workspace.edit(index, &old).unwrap();
    app.changed();
    finish_media(&ctx, &mut app, 1);
    app.workspace.close(index).unwrap();
    frame(&ctx, &mut app, vec![]);
    assert!(app.markdown.media.textures().is_empty());
}

#[test]
fn markdown_figures_preserve_quotes_lists_and_following_links() {
    let source = "> ```math\n> x^2 + y^2 = z^2\n> ```\n>\n> A quoted paragraph.\n\n1. ```mermaid\n   flowchart LR\n       A --> B\n   ```\n\n   A listed paragraph.\n\n[After figures](https://example.com)\n\n```rust\nlet x = 1;\n```\n";
    let (_directory, ctx, mut app) = fixture(source);
    app.execute(&ctx, Command::MarkdownPreview);
    let output = finish_media(&ctx, &mut app, 2);
    for label in [
        "A quoted paragraph.",
        "A listed paragraph.",
        "After figures",
    ] {
        assert!(visible(&output, label), "missing {label}");
    }
    click_label(&ctx, &mut app, "After figures");
    assert_eq!(app.active().page.text, source);
}

#[test]
fn markdown_invalid_math_and_mermaid_keep_the_source_readable() {
    let source = "```math\n\\frac{\n```\n\n```mermaid\nnot a diagram\n```\n";
    let (_directory, ctx, mut app) = fixture(source);
    app.execute(&ctx, Command::MarkdownPreview);
    let started = Instant::now();
    loop {
        let output = frame(&ctx, &mut app, vec![]);
        if visible(&output, "Math could not be rendered")
            && visible(&output, "Mermaid could not be rendered")
        {
            assert!(visible(&output, "\\frac{\n"));
            assert!(visible(&output, "not a diagram\n"));
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(10));
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(app.active().page.text, source);
    assert!(!app.active().page.info.dirty);
}

#[test]
fn markdown_expanded_cap_and_background_preparation_keep_the_latest_revision() {
    let reference = format!("[unused]: https://example.com/{}\n", "a".repeat(1000));
    let mut source = format!("# Expanded preview\n\n{}", reference.repeat(2000));
    source.push_str(&" ".repeat(2 * 1024 * 1024 - source.len()));
    let (_directory, ctx, mut app) = fixture(&source);
    assert!(
        app.command_enabled(Command::MarkdownPreview),
        "2 MiB is supported"
    );
    app.execute(&ctx, Command::MarkdownPreview);
    let started = Instant::now();
    loop {
        if visible(&frame(&ctx, &mut app, vec![]), "Expanded preview") {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(15));
        thread::sleep(Duration::from_millis(5));
    }
    let index = app.workspace.active;
    // Start another large preparation, then edit while that job is pending.
    let old = app.active().page.text.clone();
    app.workspace.open[index]
        .page
        .text
        .replace_range(2..10, "Outdated");
    app.workspace.edit(index, &old).unwrap();
    frame(&ctx, &mut app, vec![]);
    let old = app.active().page.text.clone();
    app.workspace.open[index].page.text = "# Latest revision\n\n$x$\n".into();
    app.workspace.edit(index, &old).unwrap();
    app.changed();
    let output = finish_media(&ctx, &mut app, 1);
    assert!(visible(&output, "Latest revision"));
    assert!(!visible(&output, "Expanded preview"));
    assert!(!visible(&output, "Outdated preview"));
    app.workspace.open[index].page.text = source + "x";
    assert!(
        !app.command_enabled(Command::MarkdownPreview),
        "over 2 MiB stays in Source"
    );
}

#[test]
fn markdown_preview_commands_only_apply_to_markdown() {
    let directory = tempfile::tempdir().unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    assert!(!app.command_enabled(Command::MarkdownPreview));
    app.workspace.open[0].set_language(Some(syntax::Language::Markdown));
    assert!(app.command_enabled(Command::MarkdownPreview));
    app.workspace.open[0].page.info.paged = true;
    assert!(!app.command_enabled(Command::MarkdownPreview));
    assert!(app.command_enabled(Command::MarkdownSource));
}

#[test]
fn markdown_preview_renders_rich_content_and_cannot_type_into_hidden_source() {
    let (_directory, ctx, mut app) = fixture(SAMPLE);
    assert_eq!(app.active().language(), syntax::Language::Markdown);
    click_label(&ctx, &mut app, "Preview");
    assert_eq!(app.active().view.markdown_view, MarkdownView::Preview);
    let output = frame(&ctx, &mut app, vec![]);
    for text in [
        "Project notebook",
        "native",
        "Release checklist",
        "Feature",
        "Status",
        "Ready",
    ] {
        assert!(visible(&output, text), "{text} should be rendered");
    }
    assert!(!visible(&output, SAMPLE), "source is hidden in Preview");
    let revision = app.active().page.info.revision;
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Text("do not insert".into())],
    );
    frame(
        &ctx,
        &mut app,
        vec![tests::key(Key::Backspace, Modifiers::NONE)],
    );
    assert_eq!(app.active().page.text, SAMPLE);
    assert_eq!(app.active().page.info.revision, revision);
    app.execute(&ctx, Command::Find);
    assert_eq!(app.active().view.markdown_view, MarkdownView::Split);
}

#[test]
fn markdown_split_preview_follows_unsaved_edits_and_modes_survive_restarts() {
    let (directory, ctx, mut app) = fixture("# Original heading\n");
    app.execute(&ctx, Command::MarkdownSplit);
    frame(&ctx, &mut app, vec![]);
    app.set_selections(vec![hedit::advanced::Selection {
        anchor: 0,
        head: app.active().page.text.chars().count(),
    }]);
    frame(&ctx, &mut app, vec![]);
    let output = frame(
        &ctx,
        &mut app,
        vec![egui::Event::Text("# Edited café heading\n".into())],
    );
    assert!(visible(&output, "Edited café heading"));
    assert_eq!(app.active().page.text, "# Edited café heading\n");
    assert_eq!(
        std::fs::read_to_string(directory.path().join("notes.md")).unwrap(),
        "# Original heading\n"
    );
    app.workspace.checkpoint().unwrap();
    drop(app);
    let (workspace, errors) = Workspace::restore(directory.path().join("session")).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    let active = &workspace.open[workspace.active];
    assert_eq!(active.view.markdown_view, MarkdownView::Split);
    assert_eq!(active.page.text, "# Edited café heading\n");
    assert!(active.page.info.dirty);
    let mut old = serde_json::to_value(&active.view).unwrap();
    old.as_object_mut().unwrap().remove("markdownView");
    let old: hedit::session::TabView = serde_json::from_value(old).unwrap();
    assert_eq!(
        old.markdown_view,
        MarkdownView::Source,
        "old sessions remain compatible"
    );
}

#[test]
fn markdown_modes_are_per_tab_and_compact_layout_keeps_both_views_usable() {
    let (_directory, ctx, mut app) = fixture("# Compact heading\n\nA short paragraph.\n");
    let first = app.workspace.active;
    app.execute(&ctx, Command::MarkdownSplit);
    for _ in 0..3 {
        frame_size(&ctx, &mut app, vec![], egui::vec2(720.0, 600.0));
    }
    let output = frame_size(&ctx, &mut app, vec![], egui::vec2(720.0, 600.0));
    assert!(visible(&output, "Compact heading"));
    assert!(visible(&output, "Source"));
    app.execute(&ctx, Command::New);
    assert_eq!(app.active().view.markdown_view, MarkdownView::Source);
    assert!(!app.command_enabled(Command::MarkdownPreview));
    app.workspace.active = first;
    assert_eq!(app.active().view.markdown_view, MarkdownView::Split);
}

#[test]
fn markdown_section_links_scroll_the_preview_and_local_file_links_open_in_editor() {
    let (directory, ctx, mut app) = fixture(&format!(
        "[Jump down](#details)\n\n{}\n## Details\n\n[Other note](other%20note.md)\n",
        "Filler paragraph.\n\n".repeat(100)
    ));
    std::fs::write(directory.path().join("other note.md"), "# Other document\n").unwrap();
    app.execute(&ctx, Command::MarkdownPreview);
    frame(&ctx, &mut app, vec![]);
    click_label(&ctx, &mut app, "Jump down");
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let output = frame(&ctx, &mut app, vec![]);
    assert!(
        visible(&output, "Details"),
        "fragment links should reveal their heading"
    );
    click_label(&ctx, &mut app, "Other note");
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().page.text, "# Other document\n");
}

#[test]
fn markdown_local_images_resolve_relative_and_absolute_paths_without_blocking() {
    let (directory, ctx, mut app) = fixture("# Images\n\n![Square](a%20square.png)\n");
    let path = directory.path().join("a square.png");
    image::RgbaImage::from_pixel(12, 12, image::Rgba([30, 160, 90, 255]))
        .save(&path)
        .unwrap();
    app.execute(&ctx, Command::MarkdownPreview);
    frame(&ctx, &mut app, vec![]);
    for target in [
        "a%20square.png".to_owned(),
        path.to_string_lossy().into_owned(),
    ] {
        let uri = format!("hedit-markdown://{}/{target}", app.active().view.id);
        let started = Instant::now();
        loop {
            match ctx.try_load_bytes(&uri).unwrap() {
                egui::load::BytesPoll::Ready { bytes, .. } => {
                    assert_eq!(image::load_from_memory(bytes.as_ref()).unwrap().width(), 12);
                    assert!(matches!(
                        ctx.try_load_image(&uri, egui::load::SizeHint::default())
                            .unwrap(),
                        egui::load::ImagePoll::Ready { .. }
                    ));
                    break;
                }
                egui::load::BytesPoll::Pending { .. } => {
                    assert!(started.elapsed() < Duration::from_secs(3));
                    thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }
    let moved = directory.path().join("moved");
    std::fs::create_dir(&moved).unwrap();
    image::RgbaImage::from_pixel(24, 24, image::Rgba([50, 80, 180, 255]))
        .save(moved.join("a square.png"))
        .unwrap();
    app.save_to_path(&ctx, app.active().view.id, Some(moved.join("notes.md")));
    finish_app_job(&mut app, &ctx);
    frame(&ctx, &mut app, vec![]);
    let uri = format!("hedit-markdown://{}/a%20square.png", app.active().view.id);
    let started = Instant::now();
    loop {
        match ctx
            .try_load_image(&uri, egui::load::SizeHint::default())
            .unwrap()
        {
            egui::load::ImagePoll::Ready { image } => {
                assert_eq!(
                    image.size,
                    [24, 24],
                    "Save As must reload relative images from the new folder"
                );
                break;
            }
            egui::load::ImagePoll::Pending { .. } => {
                assert!(started.elapsed() < Duration::from_secs(3));
                thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

#[test]
fn markdown_async_navigation_reveals_source_and_cancelled_goto_restores_preview() {
    let (_directory, ctx, mut app) = fixture("# A note\n\nFind this line.\n");
    app.execute(&ctx, Command::MarkdownPreview);
    frame(&ctx, &mut app, vec![]);
    // Folder results and location links navigate asynchronously, without the
    // Find/Goto commands that already expose the source.
    app.goto_location(&ctx, 3, 1);
    finish_app_job(&mut app, &ctx);
    frame(&ctx, &mut app, vec![]);
    assert_eq!(app.active().view.markdown_view, MarkdownView::Split);
    assert_eq!(app.active().view.head, "# A note\n\n".chars().count());
    app.execute(&ctx, Command::MarkdownPreview);
    app.preview_line(&ctx, 1, None);
    frame(&ctx, &mut app, vec![]);
    assert_eq!(app.active().view.markdown_view, MarkdownView::Split);
    app.cancel_goto_preview();
    frame(&ctx, &mut app, vec![]);
    assert_eq!(app.active().view.markdown_view, MarkdownView::Preview);
}

#[test]
fn markdown_external_reload_updates_preview_without_switching_to_source() {
    let (directory, ctx, mut app) = fixture("# Original\n");
    app.execute(&ctx, Command::MarkdownPreview);
    frame(&ctx, &mut app, vec![]);
    std::fs::write(directory.path().join("notes.md"), "# Updated externally\n").unwrap();
    app.load_page(&ctx, 0, true);
    finish_app_job(&mut app, &ctx);
    let output = frame(&ctx, &mut app, vec![]);
    assert_eq!(app.active().view.markdown_view, MarkdownView::Preview);
    assert!(visible(&output, "Updated externally"));
}

#[test]
fn markdown_links_inside_escaped_dollar_signs_and_reference_links_open_in_editor() {
    for source in [
        "\\$[Other note](other.md)\\$\n",
        "[Other note][doc]\n\n[doc]: other.md\n",
    ] {
        let (directory, ctx, mut app) = fixture(source);
        std::fs::write(directory.path().join("other.md"), "# Another note\n").unwrap();
        app.execute(&ctx, Command::MarkdownPreview);
        frame(&ctx, &mut app, vec![]);
        click_label(&ctx, &mut app, "Other note");
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, "# Another note\n", "{source}");
        assert_eq!(app.active().view.markdown_view, MarkdownView::Preview);
    }
}

#[test]
fn markdown_file_links_keep_their_section_fragment_for_new_and_open_tabs() {
    for already_open in [false, true] {
        let (directory, ctx, mut app) = fixture("[Open details](other.md#details)\n");
        let original = app.active().view.id;
        let path = directory.path().join("other.md");
        std::fs::write(
            &path,
            format!(
                "# Other document\n\n{}\n## Details\n",
                "Filler.\n\n".repeat(100)
            ),
        )
        .unwrap();
        if already_open {
            app.open(&ctx, Some(path));
            finish_app_job(&mut app, &ctx);
            app.workspace.active = app
                .workspace
                .open
                .iter()
                .position(|b| b.view.id == original)
                .unwrap();
        }
        app.execute(&ctx, Command::MarkdownPreview);
        frame(&ctx, &mut app, vec![]);
        click_label(&ctx, &mut app, "Open details");
        finish_app_job(&mut app, &ctx);
        for _ in 0..4 {
            frame(&ctx, &mut app, vec![]);
        }
        let output = frame(&ctx, &mut app, vec![]);
        assert_eq!(app.active().view.markdown_view, MarkdownView::Preview);
        assert!(
            visible(&output, "Details"),
            "file#section must reveal that section"
        );
    }
}

fn loaded_image(
    ctx: &egui::Context,
    uri: &str,
) -> Result<Arc<egui::ColorImage>, egui::load::LoadError> {
    let started = Instant::now();
    loop {
        match ctx.try_load_image(uri, egui::load::SizeHint::default())? {
            egui::load::ImagePoll::Ready { image } => return Ok(image),
            egui::load::ImagePoll::Pending { .. } => {
                assert!(started.elapsed() < Duration::from_secs(3));
                thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

#[test]
fn markdown_reentering_preview_retries_fixed_images_and_closing_releases_them() {
    let (directory, ctx, mut app) = fixture("![Picture](picture.png)\n");
    app.execute(&ctx, Command::MarkdownPreview);
    frame(&ctx, &mut app, vec![]);
    let uri = format!("hedit-markdown://{}/picture.png", app.active().view.id);
    assert!(loaded_image(&ctx, &uri).is_err());
    image::RgbaImage::from_pixel(8, 8, image::Rgba([20, 100, 80, 255]))
        .save(directory.path().join("picture.png"))
        .unwrap();
    app.execute(&ctx, Command::MarkdownSource);
    app.execute(&ctx, Command::MarkdownPreview);
    frame(&ctx, &mut app, vec![]);
    assert_eq!(loaded_image(&ctx, &uri).unwrap().size, [8, 8]);
    let revision = app.active().page.info.revision;
    image::RgbaImage::from_pixel(16, 16, image::Rgba([40, 120, 180, 255]))
        .save(directory.path().join("picture.png"))
        .unwrap();
    click_label(&ctx, &mut app, "Refresh");
    assert_eq!(loaded_image(&ctx, &uri).unwrap().size, [16, 16]);
    assert_eq!(
        app.active().page.info.revision,
        revision,
        "refresh never edits the source"
    );
    app.execute(&ctx, Command::Close);
    frame(&ctx, &mut app, vec![]);
    assert!(
        ctx.try_load_image(&uri, egui::load::SizeHint::default())
            .is_err(),
        "closing the tab should release its image data and textures"
    );
}

#[test]
fn markdown_many_images_finish_loading_and_oversized_images_fail_cleanly() {
    let source: String = (0..24)
        .map(|n| format!("![Image](image-{n}.png)\n\n"))
        .collect();
    let (directory, ctx, mut app) = fixture(&source);
    for n in 0..24 {
        image::RgbaImage::from_pixel(4, 4, image::Rgba([n, 100, 180, 255]))
            .save(directory.path().join(format!("image-{n}.png")))
            .unwrap();
    }
    app.execute(&ctx, Command::MarkdownPreview);
    frame(&ctx, &mut app, vec![]);
    for n in 0..24 {
        let uri = format!("hedit-markdown://{}/image-{n}.png", app.active().view.id);
        assert_eq!(loaded_image(&ctx, &uri).unwrap().size, [4, 4]);
    }
    let oversized = std::fs::File::create(directory.path().join("large.png")).unwrap();
    oversized.set_len(33 * 1024 * 1024).unwrap();
    let uri = format!("hedit-markdown://{}/large.png", app.active().view.id);
    assert!(loaded_image(&ctx, &uri)
        .unwrap_err()
        .to_string()
        .contains("32 MiB"));
    assert_eq!(app.active().page.text, source);
}

#[test]
fn markdown_duplicate_and_unicode_headings_and_top_links_navigate_correctly() {
    let filler = "A paragraph.\n\n".repeat(80);
    let source = format!("[Next duplicate](#details-1)\n\n[Unicode](#caf%C3%A9)\n\n# Details\n\n{filler}\n# Details\n\n[Back to top](#)\n\n# Café\n");
    let (_directory, ctx, mut app) = fixture(&source);
    app.execute(&ctx, Command::MarkdownPreview);
    frame(&ctx, &mut app, vec![]);
    click_label(&ctx, &mut app, "Next duplicate");
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let output = frame(&ctx, &mut app, vec![]);
    assert!(visible(&output, "Back to top"));
    click_label(&ctx, &mut app, "Back to top");
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let output = frame(&ctx, &mut app, vec![]);
    assert!(visible(&output, "Next duplicate"));
    click_label(&ctx, &mut app, "Unicode");
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let output = frame(&ctx, &mut app, vec![]);
    assert!(visible(&output, "Café"));
}

#[test]
fn markdown_section_links_include_headings_inside_quotes_and_lists() {
    let source = format!("[Quoted](#quoted-heading)\n\n[Listed](#listed-heading)\n\n{}\n> ## Quoted heading\n>\n> [Back](#)\n\n1. ### Listed heading\n", "Filler.\n\n".repeat(100));
    let (_directory, ctx, mut app) = fixture(&source);
    app.execute(&ctx, Command::MarkdownPreview);
    frame(&ctx, &mut app, vec![]);
    click_label(&ctx, &mut app, "Quoted");
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    assert!(visible(&frame(&ctx, &mut app, vec![]), "Quoted heading"));
    click_label(&ctx, &mut app, "Back");
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    click_label(&ctx, &mut app, "Listed");
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    assert!(visible(&frame(&ctx, &mut app, vec![]), "Listed heading"));
    assert_eq!(app.active().page.text, source);
}

#[test]
fn markdown_list_headings_do_not_run_into_the_following_paragraph() {
    let (_directory, ctx, mut app) =
        fixture("1. ### A listed heading\n\n   Its following paragraph.\n");
    app.execute(&ctx, Command::MarkdownPreview);
    frame(&ctx, &mut app, vec![]);
    let output = frame(&ctx, &mut app, vec![]);
    let rect = |label| {
        output
            .shapes
            .iter()
            .find_map(|shape| text_rect(&shape.shape, label))
            .unwrap()
    };
    assert!(rect("Its following paragraph.").top() >= rect("A listed heading").bottom());
}

#[test]
#[ignore = "writes Markdown UI screenshots to HEDIT_PREVIEW_DIR"]
fn export_markdown_media_previews() {
    let output = PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    for (theme, name) in [("One Dark", "dark"), ("One Light", "light")] {
        let (_directory, ctx, mut app) = fixture(MEDIA_SAMPLE);
        app.set_theme(&ctx, hedit::theme::builtin(theme).unwrap());
        app.execute(&ctx, Command::MarkdownPreview);
        let rendered = finish_media(&ctx, &mut app, 4);
        std::fs::write(
            output.join(format!("markdown-media-{name}.ppm")),
            super::rtl_tests::rasterize_with_textures(
                &ctx,
                rendered,
                egui::vec2(1280.0, 820.0),
                1.0,
                &app.markdown.media.textures(),
            ),
        )
        .unwrap();
    }
}

#[test]
#[ignore = "writes Markdown UI screenshots to HEDIT_PREVIEW_DIR"]
fn export_markdown_previews() {
    let output = PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    for (command, name, size) in [
        (Command::MarkdownSplit, "split", egui::vec2(1280.0, 820.0)),
        (
            Command::MarkdownPreview,
            "preview",
            egui::vec2(1280.0, 820.0),
        ),
        (Command::MarkdownSplit, "compact", egui::vec2(720.0, 600.0)),
    ] {
        let (_directory, ctx, mut app) = fixture(SAMPLE);
        app.execute(&ctx, command);
        for _ in 0..3 {
            frame_size(&ctx, &mut app, vec![], size);
        }
        let rendered = frame_size(&ctx, &mut app, vec![], size);
        std::fs::write(
            output.join(format!("markdown-{name}.ppm")),
            super::rtl_tests::rasterize(&ctx, rendered, size, 1.0),
        )
        .unwrap();
    }
}

#[test]
#[ignore = "writes nested Markdown screenshots to HEDIT_PREVIEW_DIR"]
fn export_markdown_nested_previews() {
    let output = PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let source = "# Container rendering\n\n> ## Quoted heading\n>\n> The heading and this paragraph stay inside the quote.\n\n1. ### Listed heading\n\n   The heading and this paragraph stay in the same list item.\n\n2. A second list item.\n\n```md\n# This is code, not a section.\n```\n";
    for (theme, name) in [("One Dark", "dark"), ("One Light", "light")] {
        let (_directory, ctx, mut app) = fixture(source);
        app.set_theme(&ctx, hedit::theme::builtin(theme).unwrap());
        app.execute(&ctx, Command::MarkdownPreview);
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        let rendered = frame(&ctx, &mut app, vec![]);
        std::fs::write(
            output.join(format!("markdown-nested-{name}.ppm")),
            super::rtl_tests::rasterize(&ctx, rendered, egui::vec2(1280.0, 820.0), 1.0),
        )
        .unwrap();
    }
}

#[test]
fn markdown_preview_button_uses_the_full_editor_area_after_split() {
    let paragraph = "The preview should use the available editor width. ".repeat(50);
    let (_directory, ctx, mut app) = fixture(&format!("# Wide preview\n\n{paragraph}\n"));
    let size = egui::vec2(1920.0, 1080.0);
    app.execute(&ctx, Command::MarkdownSplit);
    for _ in 0..3 {
        frame_size(&ctx, &mut app, vec![], size);
    }
    let output = frame_size(&ctx, &mut app, vec![], size);
    let preview = output
        .shapes
        .iter()
        .find_map(|s| text_rect(&s.shape, "Preview"))
        .unwrap();
    super::tests::click_at(&ctx, &mut app, preview.center(), size);
    for _ in 0..3 {
        frame_size(&ctx, &mut app, vec![], size);
    }
    let output = frame_size(&ctx, &mut app, vec![], size);
    assert_eq!(app.active().view.markdown_view, MarkdownView::Preview);
    let paragraph = output
        .shapes
        .iter()
        .find_map(|s| text_rect(&s.shape, paragraph.trim_end()))
        .unwrap();
    assert!(
        paragraph.right() >= size.x - 80.0,
        "preview text remained in a narrow column: {paragraph:?}"
    );
    let status = egui::containers::panel::PanelState::load(&ctx, egui::Id::new("status"))
        .unwrap()
        .rect;
    assert!(
        output
            .shapes
            .iter()
            .any(|s| matches!(&s.shape, egui::Shape::Rect(r)
        if r.fill == app.theme.surface.color() && r.rect.right() >= size.x - 8.0
            && r.rect.bottom() >= status.top() - 8.0)),
        "preview background did not fill the editor area"
    );
}

#[test]
fn markdown_separated_quoted_lists_do_not_inherit_the_intervening_document_height() {
    let question =
        "A new numbered question following the intervening text and an earlier quoted list. "
            .repeat(5)
            .trim_end()
            .to_owned();
    let mut source = "> An introductory paragraph in an earlier review.\n>\n".to_owned();
    for n in 1..=5 {
        source.push_str(&format!(
            "> {n}. {}\n>\n",
            "An earlier numbered question with some explanation. ".repeat(4)
        ));
    }
    source.push_str("> Closing remarks in the earlier quote.\n\n");
    source
        .push_str(&"The first answer uses $x$ and a detailed explanation of the work. ".repeat(10));
    source.push_str("\n\n");
    source.push_str(&"The next answer discusses how to proceed with the review. ".repeat(10));
    source.push_str(&format!(
        "\n\n> A later section:\n\n> 1. {question}\n\nThe answer follows the new question.\n"
    ));
    for mode in [Command::MarkdownPreview, Command::MarkdownSplit] {
        let (_directory, ctx, mut app) = fixture(&source);
        app.set_theme(&ctx, Theme::paper());
        app.execute(&ctx, mode);
        let size = egui::vec2(1280.0, 10000.0);
        for _ in 0..3 {
            frame_size(&ctx, &mut app, vec![], size);
        }
        let output = frame_size(&ctx, &mut app, vec![], size);
        let rect = |label| {
            output
                .shapes
                .iter()
                .find_map(|s| text_rect(&s.shape, label))
                .unwrap()
        };
        let title = rect("A later section:");
        let question = rect(&question);
        let answer = rect("The answer follows the new question.");
        assert!(
            question.top() - title.bottom() < 70.0,
            "gap before the later quoted list: {title:?} -> {question:?}"
        );
        assert!(
            answer.top() - question.bottom() < 70.0,
            "gap after the later quoted list: {question:?} -> {answer:?}"
        );
        assert_eq!(app.active().page.text, source);
        assert!(!app.active().page.info.dirty);
    }
}

#[test]
#[ignore = "renders HEDIT_MARKDOWN_REPRO into HEDIT_PREVIEW_DIR for local diagnosis"]
fn export_markdown_repro() {
    let source = std::fs::read_to_string(
        std::env::var_os("HEDIT_MARKDOWN_REPRO").expect("set HEDIT_MARKDOWN_REPRO"),
    )
    .unwrap();
    let directory =
        PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").expect("set HEDIT_PREVIEW_DIR"));
    std::fs::create_dir_all(&directory).unwrap();
    for (mode, name) in [
        (Command::MarkdownPreview, "preview"),
        (Command::MarkdownSplit, "split"),
    ] {
        let (_temporary, ctx, mut app) = fixture(&source);
        app.set_theme(&ctx, Theme::paper());
        app.execute(&ctx, mode);
        let size = egui::vec2(1280.0, 30000.0);
        let started = Instant::now();
        loop {
            let output = frame_size(&ctx, &mut app, vec![], size);
            if !app.markdown.media.pending() && started.elapsed() > Duration::from_secs(2) {
                let svg = crate::preview::export(
                    &output.shapes,
                    [size.x, size.y],
                    app.theme.background.color(),
                );
                std::fs::write(directory.join(format!("review-{name}.svg")), svg).unwrap();
                let report: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::Text(t) => Some(format!(
                            "{:?}\t{:?}",
                            t.galley.rect.translate(t.pos.to_vec2()),
                            t.galley.job.text
                        )),
                        _ => None,
                    })
                    .collect();
                std::fs::write(
                    directory.join(format!("review-{name}.txt")),
                    report.join("\n"),
                )
                .unwrap();
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "preview did not settle"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
}

#[test]
fn markdown_nested_quoted_blocks_keep_content_and_reference_links() {
    let source = "> Outer quote before the nested blocks.\n>\n> > Inner quote with a [linked note][note].\n> >\n> > 1. First inner item.\n> > 2. Second inner item.\n>\n> Outer quote continues here.\n>\n> | Column | Value |\n> | --- | --- |\n> | Entry | Present |\n>\n> ```rust\n> let value = 1;\n> ```\n\nThe text after all nested blocks.\n\n[note]: other.md\n";
    let (directory, ctx, mut app) = fixture(source);
    std::fs::write(directory.path().join("other.md"), "# Linked document\n").unwrap();
    app.execute(&ctx, Command::MarkdownPreview);
    let size = egui::vec2(1280.0, 3000.0);
    for _ in 0..3 {
        frame_size(&ctx, &mut app, vec![], size);
    }
    let output = frame_size(&ctx, &mut app, vec![], size);
    for label in [
        "First inner item.",
        "Second inner item.",
        "Outer quote continues here.",
        "Entry",
        "Present",
        "let value = 1;",
        "The text after all nested blocks.",
    ] {
        assert!(
            output
                .shapes
                .iter()
                .any(|s| text_rect(&s.shape, label).is_some()),
            "missing {label:?}"
        );
    }
    assert_eq!(app.active().page.text, source);
    // Wrapped inline links include preceding indentation in their galley bounds;
    // click a visible glyph rather than the center of that padded rectangle.
    let link = output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == "linked note" => {
                let row = &t.galley.rows[0];
                let glyph = &row.glyphs[0];
                Some(
                    t.pos
                        + egui::vec2(glyph.pos.x + glyph.advance_width / 2.0, row.rect.center().y),
                )
            }
            _ => None,
        })
        .unwrap();
    super::tests::click_at(&ctx, &mut app, link, size);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().page.text, "# Linked document\n");
    assert_eq!(app.active().view.markdown_view, MarkdownView::Preview);
}

fn scroll_markdown(
    ctx: &egui::Context,
    app: &mut HeditApp,
    size: egui::Vec2,
    at: egui::Pos2,
    delta: f32,
) {
    frame_size(
        ctx,
        app,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, delta),
                modifiers: Modifiers::NONE,
            },
        ],
        size,
    );
    for _ in 0..40 {
        frame_size(ctx, app, vec![], size);
    }
}

#[test]
fn markdown_split_scrolls_both_directions_without_moving_the_caret_or_editing_text() {
    let source: String = (0..60).map(|n| format!("## Section {n}\n\nA paragraph with **formatting** and enough words to wrap in the rendered preview.\n\n> 1. A quoted question.\n>\n> A quoted answer.\n\n```rust\nlet value = {n};\n```\n\n")).collect();
    for size in [egui::vec2(1280.0, 820.0), egui::vec2(720.0, 600.0)] {
        let (_directory, ctx, mut app) = fixture(&source);
        app.execute(&ctx, Command::MarkdownSplit);
        for _ in 0..4 {
            frame_size(&ctx, &mut app, vec![], size);
        }
        let id = app.active().view.id;
        let revision = app.active().page.info.revision;
        let caret = (app.active().view.anchor, app.active().view.head);
        let (editor, preview) = app.markdown.scroll.snapshot(id, 0).unwrap();
        assert!(editor.max > 0.0 && preview.max > 0.0);
        scroll_markdown(&ctx, &mut app, size, editor.rect.center(), -400.0);
        let (editor, preview) = app.markdown.scroll.snapshot(id, 0).unwrap();
        assert!(editor.offset > 100.0, "editor did not scroll: {editor:?}");
        assert!(
            preview.offset > 100.0,
            "preview did not follow: {preview:?}"
        );
        let before = (editor.offset, preview.offset);
        scroll_markdown(&ctx, &mut app, size, preview.rect.center(), -500.0);
        let (editor, preview) = app.markdown.scroll.snapshot(id, 0).unwrap();
        assert!(
            editor.offset > before.0 + 10.0,
            "editor did not follow preview: {before:?} -> {editor:?}"
        );
        assert!(preview.offset > before.1 + 100.0);
        let settled = (editor.offset, preview.offset);
        for _ in 0..8 {
            frame_size(&ctx, &mut app, vec![], size);
        }
        let (editor, preview) = app.markdown.scroll.snapshot(id, 0).unwrap();
        assert!(
            (editor.offset - settled.0).abs() < 1.0 && (preview.offset - settled.1).abs() < 1.0,
            "synchronized panes drifted: {settled:?} -> ({}, {})",
            editor.offset,
            preview.offset
        );
        scroll_markdown(&ctx, &mut app, size, preview.rect.center(), -100000.0);
        let (editor, preview) = app.markdown.scroll.snapshot(id, 0).unwrap();
        assert!(
            (editor.max - editor.offset).abs() < 1.0 && (preview.max - preview.offset).abs() < 1.0,
            "panes did not reach the bottom together: {editor:?}, {preview:?}"
        );
        scroll_markdown(&ctx, &mut app, size, preview.rect.center(), 100000.0);
        let (editor, preview) = app.markdown.scroll.snapshot(id, 0).unwrap();
        assert!(
            editor.offset < 1.0 && preview.offset < 1.0,
            "panes did not return to the top"
        );
        assert_eq!(app.active().page.text, source);
        assert_eq!(app.active().page.info.revision, revision);
        assert_eq!((app.active().view.anchor, app.active().view.head), caret);
    }
}

#[test]
fn markdown_split_scrolling_is_scoped_to_the_document_and_handles_short_content() {
    let source = "# Section\n\nA paragraph that stays in its document.\n\n".repeat(100);
    let (directory, ctx, mut app) = fixture(&source);
    app.execute(&ctx, Command::MarkdownSplit);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let first = app.active().view.id;
    let (editor, _) = app.markdown.scroll.snapshot(first, 0).unwrap();
    scroll_markdown(
        &ctx,
        &mut app,
        egui::vec2(1280.0, 820.0),
        editor.rect.center(),
        -1000.0,
    );
    let other = directory.path().join("short.md");
    std::fs::write(&other, "# Short note\n\nOne paragraph.\n").unwrap();
    app.open(&ctx, Some(other));
    finish_app_job(&mut app, &ctx);
    app.execute(&ctx, Command::MarkdownSplit);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let id = app.active().view.id;
    assert!(app.markdown.scroll.snapshot(first, 0).is_none());
    let (editor, preview) = app.markdown.scroll.snapshot(id, 0).unwrap();
    assert!(
        preview.offset < 1.0 && editor.offset < 1.0,
        "inherited another document's scroll"
    );
    scroll_markdown(
        &ctx,
        &mut app,
        egui::vec2(1280.0, 820.0),
        preview.rect.center(),
        -500.0,
    );
    let (editor, preview) = app.markdown.scroll.snapshot(id, 0).unwrap();
    assert!(editor.offset.is_finite() && preview.offset == 0.0);
    app.execute(&ctx, Command::MarkdownSource);
    frame(&ctx, &mut app, vec![]);
    assert!(app.markdown.scroll.snapshot(id, 0).is_none());
}

#[test]
fn markdown_split_sync_keeps_its_place_when_wrapped_math_content_resizes() {
    let source = format!(
        "{MEDIA_SAMPLE}\n\n{}",
        (0..40)
            .map(|n| format!(
                "## Section {n}\n\n{}\n\n",
                "A longer paragraph with café and Ελληνικά text for wrapping. ".repeat(n % 5 + 2)
            ))
            .collect::<String>()
    );
    let (_directory, ctx, mut app) = fixture(&source);
    app.settings.word_wrap = true;
    app.execute(&ctx, Command::MarkdownSplit);
    finish_media(&ctx, &mut app, 4);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let id = app.active().view.id;
    let (_, preview) = app.markdown.scroll.snapshot(id, 0).unwrap();
    scroll_markdown(
        &ctx,
        &mut app,
        egui::vec2(1280.0, 820.0),
        preview.rect.center(),
        -1800.0,
    );
    let source_line = |app: &HeditApp| {
        let (viewport, _) = app.markdown.scroll.snapshot(id, 0).unwrap();
        let (galley, _) = app.editor_layout.last().unwrap();
        let at = galley
            .cursor_from_pos(egui::vec2(0.0, (viewport.offset - 2.0).max(0.0)))
            .ccursor
            .index;
        source.chars().take(at).filter(|&ch| ch == '\n').count()
    };
    let before = source_line(&app);
    assert!(before > 5);
    for _ in 0..12 {
        frame_size(&ctx, &mut app, vec![], egui::vec2(720.0, 600.0));
    }
    let after = source_line(&app);
    assert!(
        before.abs_diff(after) <= 1,
        "resizing lost the corresponding source paragraph: {before} -> {after}"
    );
    assert_eq!(app.active().page.text, source);
    assert!(!app.active().page.info.dirty);
}

#[test]
fn markdown_split_sync_keeps_editor_groups_and_horizontal_scroll_independent() {
    let source = (0..60)
        .map(|n| {
            format!(
                "## Section {n}\n\n{}\n\n",
                "A very long source line. ".repeat(30)
            )
        })
        .collect::<String>();
    let (_directory, ctx, mut app) = fixture(&source);
    app.execute(&ctx, Command::MarkdownSplit);
    app.split(1);
    let size = egui::vec2(1920.0, 1080.0);
    for _ in 0..4 {
        frame_size(&ctx, &mut app, vec![], size);
    }
    let id = app.active().view.id;
    let (editor, _) = app.markdown.scroll.snapshot(id, 0).unwrap();
    let (other_editor, other_preview) = app.markdown.scroll.snapshot(id, 1).unwrap();
    scroll_markdown(&ctx, &mut app, size, editor.rect.center(), -500.0);
    let (editor, preview) = app.markdown.scroll.snapshot(id, 0).unwrap();
    assert!(editor.offset > 0.0 && preview.offset > 0.0);
    let (other_source, other_rendered) = app.markdown.scroll.snapshot(id, 1).unwrap();
    assert_eq!(other_source.offset, other_editor.offset);
    assert_eq!(other_rendered.offset, other_preview.offset);
    // Horizontal scrolling remains local to the source and is not reset by sync.
    let index = app.workspace.active;
    app.workspace.open[index].view.scroll_left = 120.0;
    app.workspace.open[index].restore_cursor = true;
    frame_size(&ctx, &mut app, vec![], size);
    scroll_markdown(&ctx, &mut app, size, preview.rect.center(), -300.0);
    assert!((app.active().view.scroll_left - 120.0).abs() < 1.0);
    assert_eq!(app.active().page.text, source);
}
