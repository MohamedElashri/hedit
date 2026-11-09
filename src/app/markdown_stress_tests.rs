//! Real-file stress fixtures and regressions discovered during reader audits.
use super::markdown_tests::fixture;
use super::tests::{finish_app_job, frame_size, text_rect};
use super::*;
use std::path::Path;

fn text<'a>(output: &'a egui::FullOutput, label: &str) -> &'a egui::epaint::TextShape {
    output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == label => Some(t),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing rendered text: {label}"))
}

#[test]
fn markdown_nested_styles_and_styled_table_headers_keep_the_outer_style() {
    let source = "**Outer __inner__ strong tail.**\n\n*Outer _inner_ emphasis tail.*\n\n| Header **middle** tail |\n| --- |\n| Value |\n";
    let (_directory, ctx, mut app) = fixture(source);
    app.execute(&ctx, Command::MarkdownPreview);
    let output = super::tests::frame(&ctx, &mut app, vec![]);
    for label in ["Outer inner strong tail.", "Header middle tail"] {
        let shape = text(&output, label);
        assert!(
            shape
                .galley
                .job
                .sections
                .iter()
                .all(|s| s.format.font_id.family
                    == egui::FontFamily::Name("hedit-reader-strong".into())),
            "lost strong style in {label}: {:?}",
            shape.galley.job.sections
        );
    }
    assert!(text(&output, "Outer inner emphasis tail.")
        .galley
        .job
        .sections
        .iter()
        .all(|s| s.format.italics));
}

#[test]
fn markdown_table_column_alignment_applies_to_headers_and_values() {
    let source = "| Left header | Center header | Right header |\n| :--- | :---: | ---: |\n| Left | Center | 123 |\n| Another | X | 9.25 |\n";
    let (_directory, ctx, mut app) = fixture(source);
    app.execute(&ctx, Command::MarkdownPreview);
    for width in [1280.0, 720.0, 1280.0] {
        let size = egui::vec2(width, 1000.0);
        for _ in 0..3 {
            frame_size(&ctx, &mut app, vec![], size);
        }
        let output = frame_size(&ctx, &mut app, vec![], size);
        let bounds = |label| {
            output
                .shapes
                .iter()
                .find_map(|s| text_rect(&s.shape, label))
                .unwrap()
        };
        assert!((bounds("Left header").left() - bounds("Left").left()).abs() < 1.0);
        assert!(
            (bounds("Center header").center().x - bounds("Center").center().x).abs() < 1.0,
            "center column ignores alignment"
        );
        assert!(
            (bounds("Right header").right() - bounds("123").right()).abs() < 1.0,
            "numeric column ignores right alignment"
        );
    }
}

#[test]
fn markdown_wrapped_table_cells_do_not_indent_the_first_line() {
    let source = "| Name | Content |\n| --- | --- |\n| Row | Several words that wrap across many lines without an unexplained indent at the start of the cell. |\n";
    let (_directory, ctx, mut app) = fixture(source);
    app.execute(&ctx, Command::MarkdownPreview);
    let size = egui::vec2(720.0, 900.0);
    for _ in 0..3 {
        frame_size(&ctx, &mut app, vec![], size);
    }
    let output = frame_size(&ctx, &mut app, vec![], size);
    let shape = text(&output, "Several words that wrap across many lines without an unexplained indent at the start of the cell.");
    assert!(shape.galley.rows.len() > 1);
    let x = shape.galley.rows[0].rect.left();
    assert!(shape
        .galley
        .rows
        .iter()
        .all(|r| (r.rect.left() - x).abs() < 0.5));
}

#[test]
fn markdown_aligned_math_cells_and_link_hit_targets_follow_the_column() {
    let source = "| Left | Center heading | Right heading |\n| :--- | :---: | ---: |\n| Row | Before $x^2$ after | Before $x^2$ after |\n| Links | [Centered link](https://example.com/center) | [Right link](https://example.com/right) |\n";
    let (_directory, ctx, mut app) = fixture(source);
    app.execute(&ctx, Command::MarkdownPreview);
    super::markdown_tests::finish_media(&ctx, &mut app, 1);
    let size = egui::vec2(1280.0, 1000.0);
    for _ in 0..3 {
        frame_size(&ctx, &mut app, vec![], size);
    }
    let output = frame_size(&ctx, &mut app, vec![], size);
    let textures = app.markdown.media.textures();
    let mut images: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Mesh(m) if textures.contains_key(&m.texture_id) => {
                Some(m.vertices.iter().fold(egui::Rect::NOTHING, |r, v| {
                    r.union(egui::Rect::from_min_max(v.pos, v.pos))
                }))
            }
            _ => None,
        })
        .collect();
    images.sort_by(|a, b| a.left().total_cmp(&b.left()));
    assert_eq!(images.len(), 2);
    let prefixes: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "Before " => {
                Some(t.galley.rect.translate(t.pos.to_vec2()))
            }
            _ => None,
        })
        .collect();
    let suffixes: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == " after" => {
                Some(t.galley.rect.translate(t.pos.to_vec2()))
            }
            _ => None,
        })
        .collect();
    let center = prefixes[0].union(images[0]).union(suffixes[0]);
    let right = prefixes[1].union(images[1]).union(suffixes[1]);
    let header = |label| {
        let t = text(&output, label);
        t.galley.rect.translate(t.pos.to_vec2())
    };
    assert!(
        (center.center().x - header("Center heading").center().x).abs() < 1.5,
        "{center:?}"
    );
    assert!(
        (right.right() - header("Right heading").right()).abs() < 1.5,
        "{right:?}"
    );
    for (label, target) in [
        ("Centered link", "https://example.com/center"),
        ("Right link", "https://example.com/right"),
    ] {
        let output = frame_size(&ctx, &mut app, vec![], size);
        let at = output
            .shapes
            .iter()
            .find_map(|s| text_rect(&s.shape, label))
            .unwrap()
            .center();
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        frame_size(
            &ctx,
            &mut app,
            vec![egui::Event::PointerMoved(at), button(true)],
            size,
        );
        let clicked = frame_size(&ctx, &mut app, vec![button(false)], size);
        assert!(
            clicked
                .platform_output
                .commands
                .iter()
                .any(|c| matches!(c, egui::OutputCommand::OpenUrl(url) if url.url == target)),
            "link hit target missed {label}"
        );
    }
    assert_eq!(app.active().page.text, source);
}

#[test]
fn markdown_large_local_images_fit_the_gpu_texture_limit() {
    let source = "Above the image.\n\n![Wide image](wide.png)\n\nBelow the image.\n";
    let (directory, ctx, mut app) = fixture(source);
    image::RgbaImage::from_pixel(4096, 64, image::Rgba([80, 140, 200, 255]))
        .save(directory.path().join("wide.png"))
        .unwrap();
    app.execute(&ctx, Command::MarkdownPreview);
    let uri = format!("hedit-markdown://{}/wide.png", app.active().view.id);
    let started = Instant::now();
    loop {
        super::tests::frame(&ctx, &mut app, vec![]);
        if let egui::load::ImagePoll::Ready { image } = ctx
            .try_load_image(&uri, egui::load::SizeHint::default())
            .unwrap()
        {
            assert_eq!(image.size, [2048, 32]);
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(10));
        thread::sleep(Duration::from_millis(5));
    }
    let output = super::tests::frame(&ctx, &mut app, vec![]);
    assert!(output
        .shapes
        .iter()
        .any(|s| text_rect(&s.shape, "Below the image.").is_some()));
    assert_eq!(app.active().page.text, source);
}

#[test]
fn markdown_definition_lists_keep_continuous_formatting_and_block_spacing() {
    let source = "Term **one**\n: A definition with **strong text**, a [link](https://example.com), and punctuation.\n\nTerm two\n: Second definition.\n\nAfter definitions.\n";
    let (_directory, ctx, mut app) = fixture(source);
    app.execute(&ctx, Command::MarkdownPreview);
    let output = super::tests::frame(&ctx, &mut app, vec![]);
    let definition = text(
        &output,
        "A definition with strong text, a link, and punctuation.",
    );
    let term = text(&output, "Term one");
    assert!(definition.pos.x > term.pos.x);
    let after = text(&output, "After definitions.");
    let second = text(&output, "Second definition.");
    assert!(after.pos.y > second.pos.y + second.galley.rect.height());
}

#[test]
fn markdown_footnote_references_are_distinct_and_navigate_to_the_definition() {
    let source = format!(
        "Read the note[^detail].\n\n{}\n[^detail]: Footnote target text.\n",
        "A long intervening paragraph.\n\n".repeat(80)
    );
    let (_directory, ctx, mut app) = fixture(&source);
    app.execute(&ctx, Command::MarkdownPreview);
    let output = super::tests::frame(&ctx, &mut app, vec![]);
    text(&output, "Read the note[detail].");
    let at = output
        .shapes
        .iter()
        .find_map(|s| super::markdown_tests::text_rect(&s.shape, "[detail]"))
        .unwrap()
        .center();
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    super::tests::frame(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(at), button(true)],
    );
    super::tests::frame(&ctx, &mut app, vec![button(false)]);
    for _ in 0..3 {
        super::tests::frame(&ctx, &mut app, vec![]);
    }
    let output = super::tests::frame(&ctx, &mut app, vec![]);
    let definition = text(&output, "Footnote target text.");
    assert!(output.shapes.iter().any(|s| s
        .clip_rect
        .contains_rect(definition.galley.rect.translate(definition.pos.to_vec2()))));
    assert_eq!(app.active().page.text, source);
    assert_eq!(
        app.active().view.markdown_view,
        hedit::session::MarkdownView::Preview
    );
}

#[test]
fn markdown_diagrams_and_large_text_errors_keep_their_full_height() {
    let source = "```mermaid\nflowchart TD\n    A[Start] --> B[Review]\n    B --> C[Finish]\n```\n\nAfter the diagram.\n";
    let (_directory, ctx, mut app) = fixture(source);
    app.execute(&ctx, Command::MarkdownPreview);
    let output = super::markdown_tests::finish_media(&ctx, &mut app, 1);
    let textures = app.markdown.media.textures();
    let mut seen = false;
    for shape in &output.shapes {
        if let egui::Shape::Mesh(m) = &shape.shape {
            if !textures.contains_key(&m.texture_id) {
                continue;
            }
            let bounds = m.vertices.iter().fold(egui::Rect::NOTHING, |r, v| {
                r.union(egui::Rect::from_min_max(v.pos, v.pos))
            });
            assert!(bounds.height() > 64.0 && bounds.height() < 480.0);
            assert!(
                shape.clip_rect.contains_rect(bounds),
                "diagram clipped: {bounds:?} in {:?}",
                shape.clip_rect
            );
            assert!(text(&output, "After the diagram.").pos.y > bounds.bottom());
            seen = true;
        }
    }
    assert!(seen);
    for (language, invalid, warning) in [
        ("mermaid", "not a diagram", "Mermaid could not be rendered"),
        (
            "mermaid",
            "not a diagram\nwith several\nsource lines",
            "Mermaid could not be rendered",
        ),
        ("math", r"\frac{", "Math could not be rendered"),
    ] {
        let source = format!("```{language}\n{invalid}\n```\n\nAfter the error.\n");
        let (_directory, ctx, mut app) = fixture(&source);
        app.settings.font_size = 22.0;
        app.execute(&ctx, Command::MarkdownPreview);
        let started = Instant::now();
        let output = loop {
            let output = super::tests::frame(&ctx, &mut app, vec![]);
            if !app.markdown.media.pending()
                && output
                    .shapes
                    .iter()
                    .any(|s| text_rect(&s.shape, warning).is_some())
            {
                break output;
            }
            assert!(started.elapsed() < Duration::from_secs(10));
            thread::sleep(Duration::from_millis(5));
        };
        let shape = text(&output, invalid);
        let bounds = shape.galley.rect.translate(shape.pos.to_vec2());
        assert!(
            output.shapes.iter().any(
                |s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == invalid)
                    && s.clip_rect.contains_rect(bounds)
            ),
            "error source clipped: {bounds:?}"
        );
        assert!(text(&output, "After the error.").pos.y > bounds.bottom());
    }
}

#[test]
fn markdown_image_links_activate_and_missing_images_explain_their_content() {
    let source = "[![Linked image](image.png)](https://example.com/image-target)\n\n![Missing chart](missing.png)\n";
    let (directory, ctx, mut app) = fixture(source);
    image::RgbaImage::from_pixel(32, 32, image::Rgba([120, 160, 200, 255]))
        .save(directory.path().join("image.png"))
        .unwrap();
    app.execute(&ctx, Command::MarkdownPreview);
    let uri = format!("hedit-markdown://{}/image.png", app.active().view.id);
    let started = Instant::now();
    let texture = loop {
        super::tests::frame(&ctx, &mut app, vec![]);
        if let egui::load::TexturePoll::Ready { texture } = ctx
            .try_load_texture(
                &uri,
                egui::TextureOptions::default(),
                egui::load::SizeHint::default(),
            )
            .unwrap()
        {
            break texture.id;
        }
        assert!(started.elapsed() < Duration::from_secs(10));
        thread::sleep(Duration::from_millis(5));
    };
    let output = super::tests::frame(&ctx, &mut app, vec![]);
    text(&output, "Image unavailable: Missing chart");
    let primitives = ctx.tessellate(output.shapes, 1.0);
    let at = primitives
        .iter()
        .find_map(|p| match &p.primitive {
            egui::epaint::Primitive::Mesh(m) if m.texture_id == texture => Some(
                m.vertices
                    .iter()
                    .fold(egui::Rect::NOTHING, |r, v| {
                        r.union(egui::Rect::from_min_max(v.pos, v.pos))
                    })
                    .center(),
            ),
            _ => None,
        })
        .expect("linked image");
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    super::tests::frame(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(at), button(true)],
    );
    let clicked = super::tests::frame(&ctx, &mut app, vec![button(false)]);
    assert!(clicked.platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::OpenUrl(url) if url.url == "https://example.com/image-target")));
    assert_eq!(app.active().page.text, source);
}

#[test]
fn markdown_wide_tables_scroll_without_moving_surrounding_prose() {
    let source = "| One | Two | Three | Four | Five | Six | Seven | Eight |\n| --- | --- | --- | --- | --- | --- | --- | --- |\n| Alpha | Bravo | Charlie | Delta | Echo | Foxtrot | Golf | Hotel |\n\nParagraph after the table.\n";
    let (_directory, ctx, mut app) = fixture(source);
    app.execute(&ctx, Command::MarkdownPreview);
    let size = egui::vec2(720.0, 900.0);
    for _ in 0..3 {
        frame_size(&ctx, &mut app, vec![], size);
    }
    let output = frame_size(&ctx, &mut app, vec![], size);
    let at = output
        .shapes
        .iter()
        .find_map(|s| text_rect(&s.shape, "Alpha"))
        .unwrap()
        .center();
    let left = text(&output, "Paragraph after the table.").pos.x;
    frame_size(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(-2000.0, 0.0),
                modifiers: Modifiers::NONE,
            },
        ],
        size,
    );
    for _ in 0..40 {
        frame_size(&ctx, &mut app, vec![], size);
    }
    let output = frame_size(&ctx, &mut app, vec![], size);
    assert!(output
        .shapes
        .iter()
        .any(|s| text_rect(&s.shape, "Hotel").is_some_and(|rect| s.clip_rect.contains_rect(rect))));
    assert!((text(&output, "Paragraph after the table.").pos.x - left).abs() < 0.5);
    assert_eq!(app.active().page.text, source);
}

fn real_file(path: &Path) -> (tempfile::TempDir, egui::Context, HeditApp) {
    let directory = tempfile::tempdir().unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    ctx.style_mut(|style| style.scroll_animation = egui::style::ScrollAnimation::none());
    settings_store::fallback_fonts();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.open(&ctx, Some(path.to_owned()));
    finish_app_job(&mut app, &ctx);
    app.settings.show_minimap = false;
    (directory, ctx, app)
}

const FILES: &[&str] = &[
    "tests/fixtures/markdown/typography.md",
    "tests/fixtures/markdown/tables.md",
    "tests/fixtures/markdown/math.md",
    "tests/fixtures/markdown/containers.md",
    "tests/fixtures/markdown/code.md",
    "tests/fixtures/markdown/diagrams.md",
    "tests/fixtures/markdown/media.md",
    "README.md",
    "USER_GUIDE.md",
    "DEVELOPER_GUIDE.md",
    "CHANGELOG.md",
];

#[test]
#[ignore = "manual large-document frame timings in HEDIT_PREVIEW_DIR"]
fn profile_markdown_document_sizes() {
    let directory = PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").unwrap());
    std::fs::create_dir_all(&directory).unwrap();
    let paragraph = "A real reading paragraph with **strong words**, *emphasis*, `inline code`, and a [reference](https://example.com). It contains enough words to wrap several times at compact widths, while keeping the source and the rendered text unchanged.\n\n";
    let mut report = Vec::new();
    for target in [64 * 1024, 256 * 1024, 2 * 1024 * 1024] {
        let mut source = format!("# Large document {target}\n\n");
        source.push_str(&paragraph.repeat((target - source.len()) / paragraph.len()));
        let (_temp, ctx, mut app) = fixture(&source);
        app.execute(&ctx, Command::MarkdownPreview);
        let started = Instant::now();
        loop {
            let output = super::tests::frame(&ctx, &mut app, vec![]);
            if output
                .shapes
                .iter()
                .any(|s| text_rect(&s.shape, &format!("Large document {target}")).is_some())
            {
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(60));
            thread::sleep(Duration::from_millis(5));
        }
        let initial = started.elapsed().as_secs_f32() * 1000.0;
        let mut times = Vec::new();
        for _ in 0..5 {
            let started = Instant::now();
            super::tests::frame(&ctx, &mut app, vec![]);
            times.push(started.elapsed().as_secs_f32() * 1000.0);
        }
        times.sort_by(f32::total_cmp);
        assert_eq!(app.active().page.text, source);
        assert!(!app.active().page.info.dirty);
        report.push(format!(
            "{} bytes\tinitial {initial:.1}ms\tmedian {:.1}ms\tmax {:.1}ms",
            source.len(),
            times[2],
            times[4]
        ));
        std::fs::write(directory.join("frame-times.tsv"), report.join("\n")).unwrap();
    }
}

fn audit_frame(
    ctx: &egui::Context,
    app: &mut HeditApp,
    size: egui::Vec2,
    events: Vec<egui::Event>,
    textures: &mut std::collections::HashMap<egui::TextureId, egui::ColorImage>,
) -> egui::FullOutput {
    let output = frame_size(ctx, app, events, size);
    for (id, delta) in &output.textures_delta.set {
        if let egui::ImageData::Color(image) = &delta.image {
            if let Some([x, y]) = delta.pos {
                let target = textures.get_mut(id).expect("partial texture before upload");
                for row in 0..image.size[1] {
                    let at = (row + y) * target.size[0] + x;
                    target.pixels[at..at + image.size[0]].copy_from_slice(
                        &image.pixels[row * image.size[0]..(row + 1) * image.size[0]],
                    );
                }
            } else {
                textures.insert(*id, (**image).clone());
            }
        }
    }
    output
}

#[test]
#[ignore = "broad real-file audit with software screenshots in HEDIT_PREVIEW_DIR"]
fn audit_markdown_real_files() {
    let output_dir = PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").unwrap());
    std::fs::create_dir_all(&output_dir).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut report = Vec::new();
    let filter = std::env::var("HEDIT_AUDIT_FILTER").unwrap_or_default();
    for file in FILES {
        if !file.contains(&filter) {
            continue;
        }
        for (name, theme, command, size, font_size) in [
            (
                "dark-preview",
                Theme::default(),
                Command::MarkdownPreview,
                egui::vec2(1280.0, 900.0),
                14.0,
            ),
            (
                "light-large",
                Theme::paper(),
                Command::MarkdownPreview,
                egui::vec2(1280.0, 900.0),
                22.0,
            ),
            (
                "dark-split",
                Theme::default(),
                Command::MarkdownSplit,
                egui::vec2(1280.0, 900.0),
                14.0,
            ),
            (
                "light-compact",
                Theme::paper(),
                Command::MarkdownSplit,
                egui::vec2(720.0, 900.0),
                14.0,
            ),
        ] {
            let path = root.join(file);
            let original = std::fs::read_to_string(&path).unwrap();
            let (_directory, ctx, mut app) = real_file(&path);
            app.set_theme(&ctx, theme);
            app.settings.font_size = font_size;
            app.execute(&ctx, command);
            let prepared = markdown::prepare(&original);
            let image_uris: Vec<_> = prepared
                .parsed
                .events()
                .filter_map(|(event, _)| match event {
                    egui_commonmark::pulldown_cmark::Event::Start(
                        egui_commonmark::pulldown_cmark::Tag::Image { dest_url, .. },
                    ) if !dest_url.contains(':') => Some(format!(
                        "hedit-markdown://{}/{dest_url}",
                        app.active().view.id
                    )),
                    _ => None,
                })
                .collect();
            let mut textures = std::collections::HashMap::new();
            let started = Instant::now();
            let mut settled = 0;
            while settled < 4 {
                audit_frame(&ctx, &mut app, size, vec![], &mut textures);
                let loading_images = image_uris.iter().any(|uri| {
                    matches!(
                        ctx.try_load_image(uri, egui::load::SizeHint::default()),
                        Ok(egui::load::ImagePoll::Pending { .. })
                    )
                });
                settled = if app.markdown.media.pending() || loading_images {
                    0
                } else {
                    settled + 1
                };
                assert!(
                    started.elapsed() < Duration::from_secs(30),
                    "media stalled: {file}"
                );
                thread::sleep(Duration::from_millis(10));
            }
            let stem = path.file_stem().unwrap().to_str().unwrap();
            for position in ["top", "bottom"] {
                let output = audit_frame(&ctx, &mut app, size, vec![], &mut textures);
                assert!(output.shapes.iter().all(|s| s.clip_rect.is_finite()));
                assert_eq!(app.active().page.text, original, "rendering edited {file}");
                assert!(!app.active().page.info.dirty);
                assert!(app.error.is_none(), "{file}: {:?}", app.error);
                std::fs::write(
                    output_dir.join(format!("{stem}-{name}-{position}.ppm")),
                    super::rtl_tests::rasterize_with_textures(&ctx, output, size, 1.0, &textures),
                )
                .unwrap();
                let point = egui::pos2(size.x - 30.0, size.y - 80.0);
                audit_frame(
                    &ctx,
                    &mut app,
                    size,
                    vec![
                        egui::Event::PointerMoved(point),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, -1_000_000.0),
                            modifiers: Modifiers::NONE,
                        },
                    ],
                    &mut textures,
                );
                for _ in 0..40 {
                    audit_frame(&ctx, &mut app, size, vec![], &mut textures);
                }
            }
            if command == Command::MarkdownSplit {
                let (source, preview) = app
                    .markdown
                    .scroll
                    .snapshot(app.active().view.id, 0)
                    .unwrap();
                assert!(!source.rect.intersects(preview.rect));
                assert!(
                    (preview.max - preview.offset).abs() < 1.0,
                    "preview did not reach bottom: {file}: {preview:?}"
                );
                assert!(
                    (source.max - source.offset).abs() < 1.0,
                    "source did not follow to bottom: {file}: {source:?}"
                );
            }
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
            report.push(format!(
                "{file}\t{name}\t{} bytes\t{:.2}s",
                original.len(),
                started.elapsed().as_secs_f32()
            ));
            std::fs::write(output_dir.join("audit.tsv"), report.join("\n")).unwrap();
        }
    }
}
