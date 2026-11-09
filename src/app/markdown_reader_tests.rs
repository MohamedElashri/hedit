//! Reading layout, equation baselines, navigation, and cache lifecycle.
use super::markdown_tests::{finish_media, fixture};
use super::tests::{click_label, frame, frame_size, text_rect};
use super::*;

fn rect(output: &egui::FullOutput, text: &str) -> egui::Rect {
    output
        .shapes
        .iter()
        .find_map(|s| text_rect(&s.shape, text))
        .unwrap()
}

#[test]
fn markdown_inline_fractions_share_the_text_baseline_and_leave_room_below() {
    for equation in [
        r"E = mc^2",
        r"\frac{a_i}{b_j}",
        r"\sum_{i=1}^{n} x_i",
        r"\begin{bmatrix} a & b \\ c & d \end{bmatrix}",
    ] {
        for theme in [Theme::default(), Theme::paper()] {
            let source = format!(
                "A paragraph above.\n\nBefore ${equation}$ after.\n\nThe next paragraph.\n"
            );
            let (_directory, ctx, mut app) = fixture(&source);
            app.set_theme(&ctx, theme);
            app.execute(&ctx, Command::MarkdownPreview);
            let output = finish_media(&ctx, &mut app, 1);
            let baselines = app.markdown.media.baselines();
            let (image, baseline) = output
                .shapes
                .iter()
                .find_map(|s| {
                    let egui::Shape::Mesh(mesh) = &s.shape else {
                        return None;
                    };
                    let baseline = *baselines.get(&mesh.texture_id)?;
                    let bounds = mesh.vertices.iter().fold(egui::Rect::NOTHING, |r, v| {
                        r.union(egui::Rect::from_min_max(v.pos, v.pos))
                    });
                    Some((bounds, baseline))
                })
                .expect("equation image");
            for text in ["Before ", " after."] {
                let baseline_y = output
                    .shapes
                    .iter()
                    .find_map(|s| match &s.shape {
                        egui::Shape::Text(t) if t.galley.text() == text => {
                            Some(t.pos.y + t.galley.rows[0].glyphs[0].pos.y)
                        }
                        _ => None,
                    })
                    .unwrap();
                assert!(
                    (baseline_y - image.top() - baseline).abs() <= 1.5,
                    "{equation}: text baseline {baseline_y}, math baseline {}, image {image:?}, labels {:?}",
                    image.top() + baseline,
                    output.shapes.iter().filter_map(|s| match &s.shape {
                        egui::Shape::Text(t) if t.galley.text().contains("Before") => Some((&t.galley.job, t.pos, &t.galley.rect)),
                        _ => None,
                    }).collect::<Vec<_>>()
                );
            }
            assert!(rect(&output, "The next paragraph.").top() > image.bottom());
            assert!(image.top() > rect(&output, "A paragraph above.").bottom());
            assert_eq!(app.active().page.text, source);
        }
    }
}

#[test]
fn markdown_wrapped_prose_keeps_the_last_line_aligned_with_inline_math() {
    for font_size in [14.0, 22.0] {
        let prefix = "A paragraph with enough words to wrap before the equation. ".repeat(4);
        let source = format!("{prefix}$\\frac{{a_i}}{{b_j}}$ after.\n");
        let (_directory, ctx, mut app) = fixture(&source);
        app.settings.font_size = font_size;
        app.execute(&ctx, Command::MarkdownPreview);
        let output = finish_media(&ctx, &mut app, 1);
        let baseline = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == prefix => {
                    assert!(t.galley.rows.len() > 1);
                    Some(t.pos.y + t.galley.rows.last().unwrap().glyphs.last().unwrap().pos.y)
                }
                _ => None,
            })
            .unwrap();
        let baselines = app.markdown.media.baselines();
        let image_baseline = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Mesh(m) => baselines
                    .get(&m.texture_id)
                    .map(|b| m.vertices[0].pos.y + b),
                _ => None,
            })
            .unwrap();
        assert!(
            (baseline - image_baseline).abs() <= 1.5,
            "font {font_size}: prose {baseline}, equation {image_baseline}"
        );
        assert_eq!(app.active().page.text, source);
    }
}

#[test]
fn markdown_styled_text_and_link_punctuation_wrap_as_continuous_prose() {
    let source = "A calm reading surface for **important ideas**, clear [references](https://example.com), and equations that belong with the text.\n";
    let (_directory, ctx, mut app) = fixture(source);
    app.execute(&ctx, Command::MarkdownPreview);
    let size = egui::vec2(720.0, 820.0);
    for _ in 0..3 {
        frame_size(&ctx, &mut app, vec![], size);
    }
    let output = frame_size(&ctx, &mut app, vec![], size);
    let paragraph = output.shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.text() == "A calm reading surface for important ideas, clear references, and equations that belong with the text." => Some(&t.galley),
        _ => None,
    }).expect("one continuous styled paragraph");
    assert!(paragraph.rows.len() > 1);
    assert!(paragraph
        .rows
        .iter()
        .all(|row| !row.text().trim_start().starts_with(',')));
    assert!(paragraph
        .job
        .sections
        .iter()
        .any(|s| s.format.underline.width > 0.0));
    assert_eq!(app.active().page.text, source);
}

#[test]
fn markdown_display_equations_are_centered_and_keep_following_paragraphs_separate() {
    let source = "# Equations\n\nBefore the equation.\n\n$$\n\\frac{1}{2} + x^2\n$$\n\nAfter the equation.\n";
    let (_directory, ctx, mut app) = fixture(source);
    app.execute(&ctx, Command::MarkdownPreview);
    let output = finish_media(&ctx, &mut app, 1);
    let textures = app.markdown.media.textures();
    let equation = ctx
        .tessellate(output.shapes.clone(), 1.0)
        .iter()
        .find_map(|s| {
            let egui::epaint::Primitive::Mesh(mesh) = &s.primitive else {
                return None;
            };
            textures.contains_key(&mesh.texture_id).then(|| {
                mesh.vertices.iter().fold(egui::Rect::NOTHING, |r, v| {
                    r.union(egui::Rect::from_min_max(v.pos, v.pos))
                })
            })
        })
        .unwrap();
    assert!((equation.center().x - 748.0).abs() < 24.0, "{equation:?}");
    assert!(equation.top() > rect(&output, "Before the equation.").bottom());
    assert!(rect(&output, "After the equation.").top() > equation.bottom());
}

#[test]
fn markdown_math_keeps_its_baseline_inside_lists_quotes_and_tables() {
    let source = "- List before $\\frac{a_i}{b_j}$ after.\n\n> Quote before $\\begin{bmatrix} a & b \\\\ c & d \\end{bmatrix}$ after.\n\n| Container | Content |\n| --- | --- |\n| Cell | Cell before $\\frac{a_i}{b_j}$ after. |\n\nA paragraph after the containers.\n";
    let (_directory, ctx, mut app) = fixture(source);
    app.execute(&ctx, Command::MarkdownPreview);
    let output = finish_media(&ctx, &mut app, 2);
    let baselines = app.markdown.media.baselines();
    let images: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|s| {
            let egui::Shape::Mesh(mesh) = &s.shape else {
                return None;
            };
            let baseline = *baselines.get(&mesh.texture_id)?;
            Some((mesh.vertices[0].pos.y + baseline, mesh.vertices[0].pos.x))
        })
        .collect();
    for label in ["List before ", "Quote before ", "Cell before "] {
        let (baseline, left) = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == label => {
                    Some((t.pos.y + t.galley.rows[0].glyphs[0].pos.y, t.pos.x))
                }
                _ => None,
            })
            .unwrap();
        assert!(
            images
                .iter()
                .any(|&(y, x)| (baseline - y).abs() <= 1.5 && x > left),
            "{label}: {baseline} versus {images:?}"
        );
    }
    rect(&output, "A paragraph after the containers.");
    assert_eq!(app.active().page.text, source);
}

const TABLE_SAMPLE: &str = "Above the table.\n\n| Name | Description |\n| --- | --- |\n| Wrapped row | A longer description that wraps across several lines inside its cell, with **emphasis**, `code`, and a [reference](https://example.com). |\n| Next row | Short content. |\n\nBelow the table.\n";

#[test]
fn markdown_table_rows_and_surrounding_paragraphs_do_not_overlap() {
    for command in [Command::MarkdownPreview, Command::MarkdownSplit] {
        let (_directory, ctx, mut app) = fixture(TABLE_SAMPLE);
        app.execute(&ctx, command);
        for width in [1280.0, 720.0, 1280.0] {
            let size = egui::vec2(width, 1200.0);
            for pass in 0..4 {
                let output = frame_size(&ctx, &mut app, vec![], size);
                let above = rect(&output, "Above the table.");
                let header = rect(&output, "Description");
                let wrapped = rect(&output, "A longer description that wraps across several lines inside its cell, with emphasis, code, and a reference.");
                let next = rect(&output, "Short content.");
                let below = rect(&output, "Below the table.");
                assert!(
                    above.bottom() < header.top()
                        && header.bottom() < wrapped.top()
                        && wrapped.bottom() < next.top()
                        && next.bottom() < below.top(),
                    "width {width}, pass {pass}: {above:?}, {header:?}, {wrapped:?}, {next:?}, {below:?}"
                );
                let border = output
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::Rect(r)
                            if r.stroke.width > 0.0
                                && r.rect.contains_rect(header)
                                && r.rect.contains_rect(next) =>
                        {
                            Some(r.rect)
                        }
                        _ => None,
                    })
                    .min_by(|a, b| a.area().total_cmp(&b.area()))
                    .expect("table border");
                assert!(
                    border.contains_rect(wrapped),
                    "{wrapped:?} outside {border:?}"
                );
            }
        }
        assert_eq!(app.active().page.text, TABLE_SAMPLE);
    }
}

#[test]
fn markdown_table_wrapped_headers_and_math_stay_inside_their_rows() {
    let source = "Above the table.\n\n| Name | A description heading that wraps onto multiple lines |\n| --- | --- |\n| Fraction | Fraction before $\\frac{a_i}{b_j}$ fraction after. |\n| Matrix | Matrix before $\\begin{bmatrix} a & b \\\\ c & d \\end{bmatrix}$ matrix after. |\n| Break | First line<br>Second line |\n\nBelow the table.\n";
    for command in [Command::MarkdownPreview, Command::MarkdownSplit] {
        let (_directory, ctx, mut app) = fixture(source);
        app.execute(&ctx, command);
        finish_media(&ctx, &mut app, 2);
        let textures = app.markdown.media.textures();
        for width in [1280.0, 720.0, 1280.0] {
            for pass in 0..3 {
                let output = frame_size(&ctx, &mut app, vec![], egui::vec2(width, 1400.0));
                let mut equations: Vec<_> = output
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
                equations.sort_by(|a, b| a.top().total_cmp(&b.top()));
                assert_eq!(equations.len(), 2);
                let rows = [
                    rect(&output, "Above the table."),
                    rect(
                        &output,
                        "A description heading that wraps onto multiple lines",
                    ),
                    rect(&output, "Fraction before ")
                        .union(equations[0])
                        .union(rect(&output, " fraction after.")),
                    rect(&output, "Matrix before ")
                        .union(equations[1])
                        .union(rect(&output, " matrix after.")),
                    rect(&output, "First line\nSecond line"),
                    rect(&output, "Below the table."),
                ];
                assert!(
                    rows.windows(2).all(|r| r[0].bottom() < r[1].top()),
                    "width {width}, pass {pass}: {rows:?}"
                );
            }
        }
        assert_eq!(app.active().page.text, source);
    }
}

#[test]
fn markdown_wide_equations_scroll_locally_without_shrinking_or_widening_prose() {
    let equation = "x + ".repeat(80) + "y";
    let source =
        format!("# Wide equation\n\n$$\n{equation}\n$$\n\nA paragraph after the wide equation.\n");
    let (_directory, ctx, mut app) = fixture(&source);
    app.execute(&ctx, Command::MarkdownPreview);
    let output = finish_media(&ctx, &mut app, 1);
    let textures = app.markdown.media.textures();
    let primitives = ctx.tessellate(output.shapes.clone(), 1.0);
    let figures: Vec<_> = primitives.iter().filter(|p| matches!(&p.primitive, egui::epaint::Primitive::Mesh(m) if textures.contains_key(&m.texture_id))).collect();
    assert!(
        textures.len() > 1,
        "wide raster should use multiple texture tiles"
    );
    let side = ctx.input(|i| i.max_texture_side);
    assert!(textures
        .values()
        .all(|image| image.size[0] <= side && image.size[1] <= side));
    let bounds = figures.iter().fold(egui::Rect::NOTHING, |bounds, p| {
        let egui::epaint::Primitive::Mesh(mesh) = &p.primitive else {
            unreachable!()
        };
        mesh.vertices.iter().fold(bounds, |r, v| {
            r.union(egui::Rect::from_min_max(v.pos, v.pos))
        })
    });
    assert!(bounds.width() > 900.0 && figures.iter().all(|p| p.clip_rect.width() < 800.0));
    assert!(rect(&output, "A paragraph after the wide equation.").left() < 400.0);
    assert_eq!(app.active().page.text, source);
}

#[test]
fn markdown_heading_math_matches_heading_type_and_coexists_with_body_math() {
    let source = "# Before $x^2$ after.\n\nA paragraph $y$.\n";
    let (_directory, ctx, mut app) = fixture(source);
    app.execute(&ctx, Command::MarkdownPreview);
    let output = finish_media(&ctx, &mut app, 2);
    let baselines = app.markdown.media.baselines();
    let images: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|s| {
            let egui::Shape::Mesh(mesh) = &s.shape else {
                return None;
            };
            Some(mesh.vertices[0].pos.y + baselines.get(&mesh.texture_id)?)
        })
        .collect();
    for label in ["Before ", " after."] {
        let y = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == label => {
                    Some(t.pos.y + t.galley.rows[0].glyphs[0].pos.y)
                }
                _ => None,
            })
            .unwrap();
        assert!(
            images.iter().any(|baseline| (baseline - y).abs() <= 1.5),
            "{label}: {y} versus {images:?}"
        );
    }
    let textures: std::collections::HashSet<_> =
        app.markdown.media.textures().into_keys().collect();
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    assert_eq!(
        textures,
        app.markdown.media.textures().into_keys().collect()
    );
    assert_eq!(app.active().page.text, source);
}

#[test]
fn markdown_oversized_inline_math_wraps_into_its_own_scrollable_row() {
    let source = format!(
        "Before ${}y$ after.\n\nAnother paragraph.\n",
        "x + ".repeat(80)
    );
    let (_directory, ctx, mut app) = fixture(&source);
    app.execute(&ctx, Command::MarkdownPreview);
    let output = finish_media(&ctx, &mut app, 1);
    let before = rect(&output, "Before ");
    let after = rect(&output, " after.");
    assert!(after.top() > before.bottom());
    assert!(after.left() < 400.0 && rect(&output, "Another paragraph.").left() < 400.0);
    assert!(app.markdown.media.textures().len() > 1);
    assert_eq!(app.active().page.text, source);
}

#[test]
fn markdown_prose_edits_reuse_equations_and_removed_equations_release_textures() {
    let source = "A paragraph.\n\nInline $x^2$.\n";
    let (_directory, ctx, mut app) = fixture(source);
    app.execute(&ctx, Command::MarkdownPreview);
    finish_media(&ctx, &mut app, 1);
    let before: std::collections::HashSet<_> = app.markdown.media.textures().into_keys().collect();
    let index = app.workspace.active;
    app.workspace.open[index].page.text = source.replace("A paragraph.", "An edited paragraph.");
    app.workspace.edit(index, source).unwrap();
    finish_media(&ctx, &mut app, 1);
    assert_eq!(before, app.markdown.media.textures().into_keys().collect());
    let old = app.active().page.text.clone();
    app.workspace.open[index].page.text = "Only prose remains.".into();
    app.workspace.edit(index, &old).unwrap();
    frame(&ctx, &mut app, vec![]);
    assert!(app.markdown.media.textures().is_empty());
}

#[test]
fn markdown_contents_navigates_without_moving_the_source_caret() {
    let source = format!(
        "# Start\n\n{}\n\n## Destination\n\nTarget paragraph.\n",
        "A paragraph with enough content to scroll. ".repeat(200)
    );
    let (_directory, ctx, mut app) = fixture(&source);
    let caret = (app.active().view.anchor, app.active().view.head);
    app.execute(&ctx, Command::MarkdownPreview);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    click_label(&ctx, &mut app, "Contents");
    click_label(&ctx, &mut app, "Destination");
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let output = frame(&ctx, &mut app, vec![]);
    let destination = rect(&output, "Destination");
    assert!(
        destination.top() > 100.0 && destination.bottom() < 760.0,
        "{destination:?}"
    );
    assert_eq!((app.active().view.anchor, app.active().view.head), caret);
    assert_eq!(app.active().page.text, source);
}

#[test]
fn markdown_comments_are_hidden_and_code_samples_remain_literal() {
    let source = "# Heading\n\n<!--hedit-markdown-figure:0-->\n\n<!--\nHidden secret\n-->\n\nA visible paragraph.\n\n```html\n<!--hedit-markdown-anchor:0-->\n```\n";
    let (_directory, ctx, mut app) = fixture(source);
    app.execute(&ctx, Command::MarkdownPreview);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let output = frame(&ctx, &mut app, vec![]);
    assert!(output
        .shapes
        .iter()
        .all(|s| text_rect(&s.shape, "<!--hedit-markdown-figure:0-->\n").is_none()));
    rect(&output, "A visible paragraph.");
    assert!(output.shapes.iter().all(
        |s| !matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().contains("Hidden secret"))
    ));
    rect(&output, "<!--hedit-markdown-anchor:0-->");
    assert!(!app.markdown.media.pending());
}

#[test]
fn markdown_equation_context_menu_copies_notation_without_editing_the_document() {
    let source = "A formula $x^2$.\n";
    let (_directory, ctx, mut app) = fixture(source);
    app.execute(&ctx, Command::MarkdownPreview);
    let output = finish_media(&ctx, &mut app, 1);
    let textures = app.markdown.media.textures();
    let point = output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Mesh(mesh) if textures.contains_key(&mesh.texture_id) => {
                Some((mesh.vertices[0].pos + mesh.vertices[2].pos.to_vec2()) / 2.0)
            }
            _ => None,
        })
        .unwrap();
    let event = |pos, button, pressed| egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    };
    frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerMoved(point),
            event(point, egui::PointerButton::Secondary, true),
        ],
    );
    frame(
        &ctx,
        &mut app,
        vec![event(point, egui::PointerButton::Secondary, false)],
    );
    let menu = frame(&ctx, &mut app, vec![]);
    let copy = rect(&menu, "Copy source").center();
    frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerMoved(copy),
            event(copy, egui::PointerButton::Primary, true),
        ],
    );
    let copied = frame(
        &ctx,
        &mut app,
        vec![event(copy, egui::PointerButton::Primary, false)],
    );
    assert!(copied
        .platform_output
        .commands
        .iter()
        .any(|cmd| matches!(cmd, egui::OutputCommand::CopyText(text) if text == "x^2")));
    assert_eq!(app.active().page.text, source);
    assert!(!app.active().page.info.dirty);
}

const READER_SAMPLE: &str = r#"# Notes worth reading

A calm reading surface for **important ideas**, clear [references](https://example.com), and equations that belong with the text. Inline $E = mc^2$ and $\frac{a_i}{b_j}$ share the surrounding baseline.

## From an idea to an equation

Text keeps a comfortable line length, even in a wide window. Display equations have their own space, while the next paragraph continues naturally.

$$
\int_0^1 x^2\,dx = \frac{1}{3}
$$

> A useful note can contain **emphasis**, a list, and mathematics without changing the layout of the rest of the document.
>
> - Keep the context close.
> - Keep the hierarchy clear.

| Surface | Behavior |
| :--- | :--- |
| Prose | Comfortable type, spacing, and a bounded reading column |
| Equations | Native rendering with a real mathematical baseline |

```rust
fn main() {
    println!("A native Markdown reader");
}
```
"#;

#[test]
#[ignore = "writes reader audit screenshots to HEDIT_PREVIEW_DIR"]
fn export_markdown_reader_previews() {
    let output = PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    for (sample_name, sample, media) in [("reader", READER_SAMPLE, 3), ("tables", TABLE_SAMPLE, 0)]
    {
        for (theme, name) in [(Theme::default(), "dark"), (Theme::paper(), "light")] {
            for (command, layout, size) in [
                (
                    Command::MarkdownPreview,
                    "preview",
                    egui::vec2(1280.0, 1100.0),
                ),
                (Command::MarkdownSplit, "split", egui::vec2(1280.0, 1100.0)),
                (Command::MarkdownSplit, "compact", egui::vec2(720.0, 1000.0)),
            ] {
                let (_directory, ctx, mut app) = fixture(sample);
                app.set_theme(&ctx, theme.clone());
                app.execute(&ctx, command);
                finish_media(&ctx, &mut app, media);
                for _ in 0..4 {
                    frame_size(&ctx, &mut app, vec![], size);
                }
                let rendered = frame_size(&ctx, &mut app, vec![], size);
                std::fs::write(
                    output.join(format!("markdown-{sample_name}-{name}-{layout}.ppm")),
                    super::rtl_tests::rasterize_with_textures(
                        &ctx,
                        rendered,
                        size,
                        1.0,
                        &app.markdown.media.textures(),
                    ),
                )
                .unwrap();
            }
        }
    }
}
