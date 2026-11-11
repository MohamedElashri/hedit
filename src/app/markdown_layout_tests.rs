//! Geometry regressions and visual fixtures for the shared Markdown renderer.
use super::markdown_tests::fixture;
use super::tests::frame_size;
use super::*;

fn render(source: &str, font_size: f32, size: egui::Vec2) -> egui::FullOutput {
    let (_directory, ctx, mut app) = fixture(source);
    app.settings.font_size = font_size;
    app.execute(&ctx, Command::MarkdownPreview);
    for _ in 0..4 {
        frame_size(&ctx, &mut app, vec![], size);
    }
    frame_size(&ctx, &mut app, vec![], size)
}

#[test]
fn markdown_inline_code_uses_the_surrounding_type_size() {
    for font_size in [14.0, 22.0, 29.0] {
        let output = render(
            "Before `code` after.\n\n# Before `code` after.\n",
            font_size,
            egui::vec2(1280.0, 1100.0),
        );
        let paragraphs: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == "Before code after." => Some(&t.galley),
                _ => None,
            })
            .collect();
        assert_eq!(paragraphs.len(), 2);
        for galley in paragraphs {
            let prose = &galley.job.sections[0].format;
            let code = &galley.job.sections[1].format;
            assert_eq!(
                code.font_id.size, prose.font_id.size,
                "inline code must scale with its paragraph or heading"
            );
            let row = &galley.rows[0];
            let before = &row.glyphs[0];
            let code = &row.glyphs[7];
            assert!(
                (before.pos.y - code.pos.y).abs() <= 1.0,
                "code and prose must share a baseline: {row:?}"
            );
            assert!(
                (code.logical_rect().center().y - row.rect.center().y).abs() <= 1.0,
                "code background must be centered in the line: {row:?}"
            );
        }
    }
}

const LAYOUT_SAMPLE: &str = r#"# Markdown layout audit

Before `inline code` after, with **bold**, _italic_ and [a link](https://example.com).
Same letters: Hgjy `Hgjy` Hgjy. Adjacent `one` `two` and punctuation `three`, done.

## Heading with `code`

| Option | Description | Default |
| :--- | :---: | ---: |
| `font_size` | A long description that should wrap over several lines without touching its neighbours. | `14` |
| Next row | Short | 123 |
| Empty neighbour | | 456 |
| **Bold and _nested_ still bold** | A<br>hard break | 789 |

After the table, ordinary prose stays in the reading column.

- Before `list code` after.
- **Bold with **nested bold** still bold**, and _italic with *nested italic* still italic_.

> Before `quote code` after, and **important text**.

Last paragraph stays separate.
"#;

#[test]
#[ignore = "writes Markdown layout audit screenshots to HEDIT_PREVIEW_DIR"]
fn export_markdown_layout_previews() {
    let output = PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    for (theme, name) in [(Theme::default(), "dark"), (Theme::paper(), "light")] {
        for (command, layout, size) in [
            (
                Command::MarkdownPreview,
                "preview",
                egui::vec2(1280.0, 1100.0),
            ),
            (Command::MarkdownSplit, "compact", egui::vec2(720.0, 1100.0)),
        ] {
            let (_directory, ctx, mut app) = fixture(LAYOUT_SAMPLE);
            app.set_theme(&ctx, theme.clone());
            app.execute(&ctx, command);
            for _ in 0..5 {
                frame_size(&ctx, &mut app, vec![], size);
            }
            let rendered = frame_size(&ctx, &mut app, vec![], size);
            std::fs::write(
                output.join(format!("markdown-layout-{name}-{layout}.ppm")),
                super::rtl_tests::rasterize(&ctx, rendered, size, 1.0),
            )
            .unwrap();
        }
    }
}
