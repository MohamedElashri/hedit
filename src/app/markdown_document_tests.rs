//! Document-level UX contracts based on VS Code's built-in Markdown preview.
use super::markdown_tests::fixture;
use super::tests::frame_size;
use super::*;

fn output(source: &str) -> egui::FullOutput {
    let (_directory, ctx, mut app) = fixture(source);
    app.execute(&ctx, Command::MarkdownPreview);
    for _ in 0..4 {
        frame_size(&ctx, &mut app, vec![], egui::vec2(1280.0, 2000.0));
    }
    frame_size(&ctx, &mut app, vec![], egui::vec2(1280.0, 2000.0))
}

fn text<'a>(output: &'a egui::FullOutput, label: &str) -> &'a egui::epaint::TextShape {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == label => Some(text),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing text {label:?}"))
}

#[test]
fn markdown_document_has_a_compact_type_scale_and_explicit_paragraph_margins() {
    let output = output("# One\n\n## Two\n\n### Three\n\n#### Four\n\n##### Five\n\n###### Six\n\nFirst paragraph.\n\nSecond paragraph.\n");
    for (label, size) in [
        ("One", 28.0),
        ("Two", 21.0),
        ("Three", 17.5),
        ("Four", 14.0),
        ("Five", 12.25),
        ("Six", 11.9),
        ("First paragraph.", 14.0),
    ] {
        assert!(
            (text(&output, label).galley.job.sections[0]
                .format
                .font_id
                .size
                - size)
                .abs()
                < 0.1,
            "{label} type size"
        );
    }
    let first = text(&output, "First paragraph.");
    let second = text(&output, "Second paragraph.");
    assert!((second.pos.y - first.pos.y - first.galley.size().y - 16.0).abs() <= 1.0);
    for (label, size) in [("One", 28.0), ("Two", 21.0)] {
        let heading = text(&output, label);
        let expected_y = heading.pos.y + heading.galley.size().y + size * 0.3;
        assert!(
            output.shapes.iter().any(
                |s| matches!(&s.shape, egui::Shape::LineSegment { points, .. }
            if (points[0].y - expected_y).abs() < 2.0 && points[1].x - points[0].x > 800.0)
            ),
            "{label} has its own section rule"
        );
    }
}

#[test]
fn markdown_short_tables_keep_natural_width_and_long_columns_get_the_space() {
    let short = output("| Key | Value |\n| --- | --- |\n| a | b |\n");
    let value = text(&short, "Value");
    assert!(
        value.pos.x + value.galley.size().x < 500.0,
        "a two-word table must not stretch to page width"
    );
    let long = "This column contains a detailed explanation that needs to wrap on narrow panes. "
        .repeat(4);
    let output = output(&format!(
        "| Key | Description |\n| --- | --- |\n| a | {long} |\n| z | Below |\n"
    ));
    let key = text(&output, "a");
    let content = text(&output, long.trim_end());
    assert!(
        content.pos.x - key.pos.x < 140.0,
        "short keys do not consume half of the table"
    );
    assert!(content.galley.rows.len() > 1);
    assert!(text(&output, "Below").pos.y > content.pos.y + content.galley.size().y);
}

#[test]
fn markdown_comments_have_no_layout_footprint_and_alerts_keep_their_semantics() {
    let plain = output("First.\n\nSecond.\n");
    let comments = output("First.\n\n<!-- a comment\nthat spans lines -->\n\nSecond.\n");
    assert_eq!(text(&plain, "Second.").pos, text(&comments, "Second.").pos);
    let alert =
        output("> [!NOTE]\n> Keep this content.\n\n- [x] Finished\n- [ ] Pending\n- Ordinary\n");
    text(&alert, "Note");
    text(&alert, "Keep this content.");
    assert!((text(&alert, "Finished").pos.x - text(&alert, "Ordinary").pos.x).abs() <= 1.0);
    assert!((text(&alert, "Pending").pos.x - text(&alert, "Ordinary").pos.x).abs() <= 1.0);
    assert!(!alert
        .shapes
        .iter()
        .any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().contains("[!NOTE]"))));
}

#[test]
fn markdown_deep_nesting_remains_readable_and_does_not_overflow_the_page() {
    let source = format!("{}Deep content\n\nAfter the quote.\n", "> ".repeat(512));
    let output = output(&source);
    let deep = text(&output, "Deep content");
    assert!(deep.pos.x + deep.galley.size().x < 1280.0);
    assert!(text(&output, "After the quote.").pos.x < 300.0);
}

#[test]
fn markdown_links_underline_on_hover_and_allow_copying_the_address() {
    let (_directory, ctx, mut app) = fixture("A [reference](https://example.com) here.\n");
    app.execute(&ctx, Command::MarkdownPreview);
    let size = egui::vec2(1280.0, 1100.0);
    for _ in 0..3 {
        frame_size(&ctx, &mut app, vec![], size);
    }
    let output = frame_size(&ctx, &mut app, vec![], size);
    let paragraph = text(&output, "A reference here.");
    assert!(paragraph
        .galley
        .job
        .sections
        .iter()
        .all(|s| s.format.underline.width == 0.0));
    let rect = paragraph.galley.rows[0].glyphs[2..11]
        .iter()
        .fold(egui::Rect::NOTHING, |rect, glyph| {
            rect.union(glyph.logical_rect().translate(paragraph.pos.to_vec2()))
        });
    let color = paragraph
        .galley
        .job
        .sections
        .iter()
        .find(|s| s.byte_range.contains(&2))
        .unwrap()
        .format
        .color;
    frame_size(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(rect.center())],
        size,
    );
    let output = frame_size(&ctx, &mut app, vec![], size);
    assert!(output.shapes.iter().any(
        |s| matches!(&s.shape, egui::Shape::LineSegment { stroke, .. } if stroke.color == color)
    ));
    let button = |pressed| egui::Event::PointerButton {
        pos: rect.center(),
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    frame_size(&ctx, &mut app, vec![button(true)], size);
    frame_size(&ctx, &mut app, vec![button(false)], size);
    let menu = frame_size(&ctx, &mut app, vec![], size);
    let copy = text(&menu, "Copy link address");
    let pos = copy.pos + copy.galley.rect.center().to_vec2();
    let primary = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    frame_size(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(pos), primary(true)],
        size,
    );
    let output = frame_size(&ctx, &mut app, vec![primary(false)], size);
    assert!(output
        .platform_output
        .commands
        .iter()
        .any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == "https://example.com")));
}

#[test]
fn markdown_code_blocks_grow_with_the_document_and_copy_without_a_header_row() {
    let code = "let value = 1;\n".repeat(40);
    let source = format!("```rust\n{code}```\n\nAfter the sample.\n");
    let (_directory, ctx, mut app) = fixture(&source);
    app.execute(&ctx, Command::MarkdownPreview);
    let size = egui::vec2(1280.0, 2000.0);
    for _ in 0..4 {
        frame_size(&ctx, &mut app, vec![], size);
    }
    let output = frame_size(&ctx, &mut app, vec![], size);
    let code_shape = text(&output, code.trim_end_matches('\n'));
    assert!(
        code_shape.galley.size().y > 700.0,
        "tall samples belong to the document scroll"
    );
    assert!(
        text(&output, "After the sample.").pos.y > code_shape.pos.y + code_shape.galley.size().y
    );
    assert!(!output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "rust" || t.galley.text() == "Copy")));
    let output = frame_size(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(
            code_shape.pos + egui::vec2(4.0, 4.0),
        )],
        size,
    );
    text(&output, "Copy");
    assert_eq!(app.active().page.text, source);
}

#[test]
fn markdown_selection_copies_across_paragraphs_and_code_without_changing_source() {
    let source = "First paragraph.\n\n```text\ncode sample\n```\n\nLast paragraph.\n";
    let (_directory, ctx, mut app) = fixture(source);
    app.execute(&ctx, Command::MarkdownPreview);
    let size = egui::vec2(1280.0, 1100.0);
    for _ in 0..4 {
        frame_size(&ctx, &mut app, vec![], size);
    }
    let output = frame_size(&ctx, &mut app, vec![], size);
    let first = text(&output, "First paragraph.");
    let last = text(&output, "Last paragraph.");
    let start = first.pos + egui::vec2(0.0, first.galley.size().y * 0.5);
    let end = last.pos + egui::vec2(last.galley.size().x + 1.0, last.galley.size().y * 0.5);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    frame_size(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(start), button(start, true)],
        size,
    );
    frame_size(&ctx, &mut app, vec![egui::Event::PointerMoved(end)], size);
    frame_size(&ctx, &mut app, vec![button(end, false)], size);
    let output = frame_size(&ctx, &mut app, vec![egui::Event::Copy], size);
    let copied = output
        .platform_output
        .commands
        .iter()
        .find_map(|c| match c {
            egui::OutputCommand::CopyText(t) => Some(t),
            _ => None,
        })
        .expect("native preview text selection");
    assert!(
        copied.contains("First paragraph.")
            && copied.contains("code sample")
            && copied.contains("Last paragraph."),
        "{copied:?}"
    );
    assert_eq!(app.active().page.text, source);
}

#[test]
fn markdown_math_cells_share_the_table_alignment_without_moving_other_columns() {
    let source = "| Left | Center has a wider heading | Right has a wider heading |\n| :--- | :---: | ---: |\n| a | $x$ | $y$ |\n| b | Centered | Right aligned |\n";
    let (_directory, ctx, mut app) = fixture(source);
    app.execute(&ctx, Command::MarkdownPreview);
    super::markdown_tests::finish_media(&ctx, &mut app, 2);
    let output = frame_size(&ctx, &mut app, vec![], egui::vec2(1280.0, 1100.0));
    let textures = app.markdown.media.textures();
    let mut images: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Mesh(mesh) if textures.contains_key(&mesh.texture_id) => {
                Some(mesh.vertices.iter().fold(egui::Rect::NOTHING, |rect, v| {
                    rect.union(egui::Rect::from_min_max(v.pos, v.pos))
                }))
            }
            _ => None,
        })
        .collect();
    images.sort_by(|a, b| a.left().total_cmp(&b.left()));
    assert_eq!(images.len(), 2);
    let center = text(&output, "Centered");
    let right = text(&output, "Right aligned");
    // Aligned text galleys anchor at the alignment point, not their left edge.
    assert!(
        (images[0].center().x - center.pos.x).abs() <= 2.0,
        "math {:?}, centered text {:?}",
        images[0],
        center.pos
    );
    assert!(
        (images[1].right() - right.pos.x).abs() <= 2.0,
        "math {:?}, right text {:?}",
        images[1],
        right.pos
    );
    assert!((text(&output, "a").pos.x - text(&output, "b").pos.x).abs() < 1.0);
    assert!(center.pos.y > images[0].bottom());
    assert_eq!(app.active().page.text, source);
}

const SAMPLE: &str = r#"# Markdown preview

A familiar document surface for **important ideas**, _real italic emphasis_, `inline code`, and [references](https://example.com). The preview uses the available pane width and a consistent rhythm between blocks.

## Structure that stays out of the way

Paragraphs, headings, lists and tables each own their space. Switch between source and preview while keeping the document in view.

- A compact list item.
- Another item with **emphasis** and `a_value`.
  - A nested item stays aligned with its parent text.
- [x] Completed work
- [ ] Work still to do

> A quote has a simple rule and normal readable text.
>
> It can contain multiple paragraphs without affecting the surrounding document.

> [!NOTE]
> An alert retains its meaning and its visual hierarchy.

## Data belongs in a table

| Setting | Description | Default |
| :--- | :--- | ---: |
| `font_size` | A detailed explanation gets the room it needs while short keys stay compact. | `14` |
| `word_wrap` | Long cells wrap without colliding with following rows or columns. | `false` |
| Empty value | | `0` |

## Code stays part of the document

```rust
fn main() {
    let message = "Hello, Markdown";
    println!("{message}");
}
```

Inline mathematics $E = mc^2$ shares a baseline with the text.

$$
\int_0^1 x^2\,dx = \frac{1}{3}
$$

The document continues naturally after the code and equations.
"#;

#[test]
#[ignore = "writes the redesigned Markdown reading surfaces to HEDIT_PREVIEW_DIR"]
fn export_markdown_document_previews() {
    let directory = PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").unwrap());
    std::fs::create_dir_all(&directory).unwrap();
    for (theme, name) in [(Theme::default(), "dark"), (Theme::paper(), "light")] {
        for (mode, layout, size) in [
            (
                Command::MarkdownPreview,
                "preview",
                egui::vec2(1280.0, 1700.0),
            ),
            (Command::MarkdownSplit, "split", egui::vec2(1500.0, 1700.0)),
            (Command::MarkdownSplit, "compact", egui::vec2(720.0, 1400.0)),
        ] {
            let (_temporary, ctx, mut app) = fixture(SAMPLE);
            app.set_theme(&ctx, theme.clone());
            app.execute(&ctx, mode);
            super::markdown_tests::finish_media(&ctx, &mut app, 2);
            for _ in 0..4 {
                frame_size(&ctx, &mut app, vec![], size);
            }
            let output = frame_size(&ctx, &mut app, vec![], size);
            std::fs::write(
                directory.join(format!("markdown-document-{name}-{layout}.ppm")),
                super::rtl_tests::rasterize_with_textures(
                    &ctx,
                    output,
                    size,
                    1.0,
                    &app.markdown.media.textures(),
                ),
            )
            .unwrap();
        }
    }
}
