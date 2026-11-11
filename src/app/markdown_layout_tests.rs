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

fn text<'a>(output: &'a egui::FullOutput, label: &str) -> &'a egui::epaint::TextShape {
    output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == label => Some(t),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing text {label:?}"))
}

#[test]
fn markdown_table_cells_reserve_their_full_wrapped_height() {
    let long = "Words that wrap within one cell and must never overlap the next row. ".repeat(3);
    let source = format!("| First | Second | Third |\n| --- | --- | --- |\n| Top | {long} | Side |\n| Next | Below | End |\n\nAfter table.\n");
    for (font, size) in [
        (14.0, egui::vec2(1280.0, 1600.0)),
        (22.0, egui::vec2(720.0, 2000.0)),
    ] {
        let output = render(&source, font, size);
        let cell = text(&output, long.trim_end());
        assert!(cell.galley.rows.len() > 1);
        let bottom = cell.pos.y + cell.galley.rect.bottom();
        for label in ["Next", "Below", "End", "After table."] {
            assert!(
                text(&output, label).pos.y > bottom,
                "{label} overlaps wrapped table cell ending at {bottom}"
            );
        }
        assert!(
            (text(&output, "Top").pos.y - cell.pos.y).abs() <= 1.0,
            "cells must align to the top of a row"
        );
    }
}

#[test]
fn markdown_tables_honor_column_alignment_and_keep_empty_cells() {
    let source = "| Left | Center | Right |\n| :--- | :---: | ---: |\n| A | B | C |\n| D | | F |\n| G | H | I |\n";
    let output = render(source, 14.0, egui::vec2(1280.0, 1100.0));
    let bounds = |label| {
        let t = text(&output, label);
        t.galley.rect.translate(t.pos.to_vec2())
    };
    assert!((bounds("Left").left() - bounds("A").left()).abs() <= 1.0);
    assert!(
        (bounds("Center").center().x - bounds("B").center().x).abs() <= 1.0,
        "center-aligned column"
    );
    assert!(
        (bounds("Right").right() - bounds("C").right()).abs() <= 1.0,
        "right-aligned column"
    );
    assert!(
        (bounds("C").right() - bounds("F").right()).abs() <= 1.0,
        "empty middle cell must keep the last column in place"
    );
    assert!((bounds("F").right() - bounds("I").right()).abs() <= 1.0);
}

#[test]
fn markdown_aligned_table_links_keep_their_click_targets() {
    for alignment in [":---:", "---:"] {
        let source = format!("| Header |\n| {alignment} |\n| [Read file](target.txt) |\n");
        let (directory, ctx, mut app) = fixture(&source);
        let target = directory.path().join("target.txt");
        std::fs::write(&target, "Opened from an aligned cell.").unwrap();
        app.execute(&ctx, Command::MarkdownPreview);
        let size = egui::vec2(1280.0, 1100.0);
        for _ in 0..4 {
            frame_size(&ctx, &mut app, vec![], size);
        }
        let output = frame_size(&ctx, &mut app, vec![], size);
        let link = text(&output, "Read file");
        let position = link.galley.rect.translate(link.pos.to_vec2()).center();
        super::tests::click_at(&ctx, &mut app, position, size);
        super::tests::finish_app_job(&mut app, &ctx);
        assert_eq!(
            app.active().page.info.path.as_deref(),
            std::fs::canonicalize(target).unwrap().to_str()
        );
    }
}

#[test]
fn markdown_wide_tables_scroll_without_widening_following_prose() {
    let header = (0..12)
        .map(|i| format!("Column {i}"))
        .collect::<Vec<_>>()
        .join(" | ");
    let source = format!(
        "| {header} |\n| {} |\n| {} |\n\nFollowing prose.\n",
        ["---"; 12].join(" | "),
        ["Value"; 12].join(" | ")
    );
    let (_directory, ctx, mut app) = fixture(&source);
    app.execute(&ctx, Command::MarkdownPreview);
    let size = egui::vec2(720.0, 1100.0);
    for _ in 0..4 {
        frame_size(&ctx, &mut app, vec![], size);
    }
    let output = frame_size(&ctx, &mut app, vec![], size);
    let after = text(&output, "Following prose.");
    assert!(after.pos.x < 300.0 && after.pos.x + after.galley.size().x < 720.0);
    let after_position = after.pos;
    let first = text(&output, "Column 0");
    let hover = first.pos + first.galley.rect.center().to_vec2();
    frame_size(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerMoved(hover),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(-2000.0, 0.0),
                modifiers: Modifiers::NONE,
            },
        ],
        size,
    );
    for _ in 0..4 {
        frame_size(&ctx, &mut app, vec![], size);
    }
    let output = frame_size(&ctx, &mut app, vec![], size);
    let last = text(&output, "Column 11");
    assert!(
        last.pos.x + last.galley.rect.right() < 720.0,
        "last column is reachable by scrolling"
    );
    assert_eq!(
        text(&output, "Following prose.").pos,
        after_position,
        "horizontal table scrolling must not move following prose"
    );
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
