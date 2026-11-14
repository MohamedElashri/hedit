//! Reuse shaped rows across edits. Blocks always end at paragraph boundaries,
//! so wrapping, Arabic joining and bidi resolution see complete paragraphs.
use super::*;
use egui::emath::GuiRounding as _;
use egui::{
    epaint::text::{Fonts, LayoutSection, Row},
    text::LayoutJob,
    Rect,
};
use std::ops::Range;
const BLOCK_BYTES: usize = 4096;
#[derive(Clone)]
pub(super) struct Block {
    bytes: Range<usize>,
    rows: Range<usize>,
    styles: Vec<LayoutSection>,
    sections: Vec<usize>,
    width: f32,
}
impl Block {
    pub(super) fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.styles.len() * std::mem::size_of::<LayoutSection>()
            + self.sections.len() * std::mem::size_of::<usize>()
    }
}
type Previous<'a> = (
    &'a Galley,
    &'a super::super::bidi_view::BidiRows,
    &'a [Block],
);

fn pieces(text: &str, old: Option<Previous<'_>>) -> Vec<(Range<usize>, Option<usize>)> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut tail = Vec::new();
    if let Some((old, _, blocks)) = old {
        let (prefix, end, inserted) = hedit::workspace::splice(&old.job.text, text);
        for (i, block) in blocks.iter().enumerate() {
            if block.bytes.end > prefix
                || (block.bytes.end == old.job.text.len() && text.len() != old.job.text.len())
            {
                break;
            }
            pieces.push((block.bytes.clone(), Some(i)));
            start = block.bytes.end;
        }
        let delta = inserted.len() as isize - (end - prefix) as isize;
        for (i, block) in blocks.iter().enumerate().rev() {
            if block.bytes.start < end || block.bytes.start < start {
                break;
            }
            let at = (block.bytes.start as isize + delta) as usize;
            if at < start || (at > 0 && text.as_bytes().get(at - 1) != Some(&b'\n')) {
                break;
            }
            tail.push((at..(block.bytes.end as isize + delta) as usize, Some(i)));
        }
        tail.reverse();
    }
    let end = tail.first().map_or(text.len(), |(range, _)| range.start);
    while start < end {
        let target = (start + BLOCK_BYTES).min(end);
        let next = if target == end {
            end
        } else {
            memchr::memchr(b'\n', &text.as_bytes()[target..end]).map_or(end, |n| target + n + 1)
        };
        pieces.push((start..next, None));
        start = next;
    }
    pieces.extend(tail);
    if pieces.is_empty() {
        pieces.push((0..0, None));
    }
    pieces
}
fn styles(job: &LayoutJob, range: &Range<usize>) -> (Vec<LayoutSection>, Vec<usize>) {
    let first = job
        .sections
        .partition_point(|s| s.byte_range.end <= range.start);
    let mut styles = Vec::new();
    let mut sections = Vec::new();
    for (i, section) in job.sections.iter().enumerate().skip(first) {
        if section.byte_range.start >= range.end {
            break;
        }
        let mut style = section.clone();
        style.byte_range = section.byte_range.start.max(range.start) - range.start
            ..section.byte_range.end.min(range.end) - range.start;
        styles.push(style);
        sections.push(i);
    }
    if styles.is_empty() {
        if let Some((i, section)) = job.sections.iter().enumerate().next_back() {
            let mut style = section.clone();
            style.byte_range = 0..0;
            styles.push(style);
            sections.push(i);
        }
    }
    (styles, sections)
}
fn place(row: &mut Arc<Row>, dy: f32, old: &[usize], new: &[usize], job: &LayoutJob, ppp: f32) {
    if dy == 0.0 && old == new {
        return;
    }
    let row = Arc::make_mut(row);
    if old != new {
        let remap = |id: u32| {
            new[old
                .binary_search(&(id as usize))
                .expect("cached section belongs to its block")] as u32
        };
        row.section_index_at_start = remap(row.section_index_at_start);
        for glyph in &mut row.glyphs {
            glyph.section_index = remap(glyph.section_index);
        }
    }
    if dy != 0.0 {
        let pixel = |v: f32| (v * ppp).round() / ppp;
        let height = pixel(row.layout_rect.height());
        let top = pixel(row.layout_rect.min.y + dy);
        let dy = top - row.layout_rect.min.y;
        row.layout_rect.min.y = top;
        row.layout_rect.max.y = top + height;
        row.rect.min.y = top.round_ui();
        row.rect.max.y = (top + height).round_ui();
        for vertex in &mut row.visuals.mesh.vertices {
            vertex.pos.y += dy;
        }
        let mut vertex = row.visuals.glyph_vertex_range.start;
        for glyph in &mut row.glyphs {
            let format = &job.sections[glyph.section_index as usize].format;
            let y = pixel(
                top + glyph.font_impl_ascent
                    + format.valign.to_factor() * (height - glyph.line_height)
                    + 0.5 * (glyph.font_height - glyph.font_impl_height),
            );
            let correction = y - (glyph.pos.y + dy);
            glyph.pos.y = y;
            if !glyph.uv_rect.is_nothing() {
                if let Some(quad) = row.visuals.mesh.vertices.get_mut(vertex..vertex + 4) {
                    for v in quad {
                        v.pos.y += correction;
                    }
                }
                vertex += 4;
            }
        }
        row.visuals.mesh_bounds = row.visuals.mesh.calc_bounds();
        row.visuals.mesh_cache.invalidate();
    }
}
pub(super) fn layout(
    fonts: &Fonts,
    job: LayoutJob,
    base: super::super::bidi_view::Base,
    old: Option<Previous<'_>>,
) -> (Arc<Galley>, super::super::bidi_view::BidiRows, Vec<Block>) {
    let mut rows = Vec::new();
    let mut dirs = Vec::new();
    let mut blocks = Vec::new();
    let mut y = 0.0;
    let mut width: f32 = 0.0;
    let ranges = pieces(&job.text, old);
    for (index, (bytes, previous)) in ranges.iter().enumerate() {
        let (styles, sections) = styles(&job, bytes);
        let before = rows.len();
        let block_width;
        if let Some((galley, bidi, block)) = previous
            .and_then(|i| old.map(|(g, b, blocks)| (g, b, &blocks[i])))
            .filter(|(_, _, block)| block.styles == styles)
        {
            block_width = block.width;
            let dy = y - galley.rows[block.rows.start].layout_rect.top();
            rows.extend(galley.rows[block.rows.clone()].iter().cloned());
            for row in &mut rows[before..] {
                place(
                    row,
                    dy,
                    &block.sections,
                    &sections,
                    &job,
                    fonts.pixels_per_point(),
                );
            }
            if !dirs.is_empty() || bidi.any() {
                dirs.resize(before, None);
                dirs.extend(block.rows.clone().map(|i| bidi.row(i).cloned()));
            }
        } else {
            let mini = LayoutJob {
                text: job.text[bytes.clone()].into(),
                sections: styles.clone(),
                wrap: job.wrap.clone(),
                first_row_min_height: 0.0,
                break_on_newline: job.break_on_newline,
                halign: job.halign,
                justify: job.justify,
                round_output_to_gui: job.round_output_to_gui,
            };
            let (shaped, bidi) =
                super::super::bidi_view::apply(fonts, fonts.layout_job(mini), base);
            block_width = shaped.rect.width();
            let count = shaped.rows.len()
                - usize::from(index + 1 < ranges.len() && job.text[bytes.clone()].ends_with('\n'));
            rows.extend(shaped.rows[..count].iter().cloned());
            let local = (0..sections.len()).collect::<Vec<_>>();
            for row in &mut rows[before..] {
                place(row, y, &local, &sections, &job, fonts.pixels_per_point());
            }
            if !dirs.is_empty() || bidi.any() {
                dirs.resize(before, None);
                dirs.extend((0..count).map(|i| bidi.row(i).cloned()));
            }
        }
        width = width.max(block_width);
        let ppp = fonts.pixels_per_point();
        y = rows
            .last()
            .map_or(y, |row| (row.layout_rect.bottom() * ppp).round() / ppp);
        blocks.push(Block {
            bytes: bytes.clone(),
            rows: before..rows.len(),
            styles,
            sections,
            width: block_width,
        });
    }
    if dirs.iter().all(Option::is_none) {
        dirs.clear();
    } else {
        dirs.resize(rows.len(), None);
    }
    let mesh_bounds = rows.iter().fold(Rect::NOTHING, |bounds, row| {
        bounds.union(row.visuals.mesh_bounds)
    });
    let num_vertices = rows.iter().map(|row| row.visuals.mesh.vertices.len()).sum();
    let num_indices = rows.iter().map(|row| row.visuals.mesh.indices.len()).sum();
    let galley = Galley {
        job: Arc::new(job),
        rows,
        cursor_cache: Default::default(),
        elided: false,
        rect: Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(width, y.round_ui())),
        mesh_bounds,
        num_vertices,
        num_indices,
        pixels_per_point: fonts.pixels_per_point(),
    };
    (
        Arc::new(galley),
        super::super::bidi_view::BidiRows { rows: dirs },
        blocks,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[track_caller]
    fn close(a: f32, b: f32) {
        assert!((a - b).abs() < 0.04, "{a} != {b}");
    }
    fn compare(actual: &Galley, expected: &Galley) {
        assert_eq!(actual.job.text, expected.job.text);
        assert_eq!(actual.rows.len(), expected.rows.len());
        close(actual.rect.width(), expected.rect.width());
        close(actual.rect.height(), expected.rect.height());
        for (a, b) in actual.rows.iter().zip(&expected.rows) {
            close(a.rect.top(), b.rect.top());
            close(a.rect.bottom(), b.rect.bottom());
            assert_eq!(a.ends_with_newline, b.ends_with_newline);
            assert_eq!(a.glyphs.len(), b.glyphs.len());
            for (a, b) in a.glyphs.iter().zip(&b.glyphs) {
                assert_eq!(a.chr, b.chr);
                close(a.pos.x, b.pos.x);
                close(a.pos.y, b.pos.y);
                close(a.advance_width, b.advance_width);
                assert_eq!(
                    actual.job.sections[a.section_index as usize].format,
                    expected.job.sections[b.section_index as usize].format
                );
            }
            assert_eq!(a.visuals.mesh.indices, b.visuals.mesh.indices);
            assert_eq!(a.visuals.mesh.vertices.len(), b.visuals.mesh.vertices.len());
            for (a, b) in a.visuals.mesh.vertices.iter().zip(&b.visuals.mesh.vertices) {
                close(a.pos.x, b.pos.x);
                close(a.pos.y, b.pos.y);
                assert_eq!(a.color, b.color);
                assert_eq!(a.uv, b.uv);
            }
        }
    }
    #[test]
    fn edits_match_full_layout_with_wrapping_unicode_and_changed_syntax() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(scale);
            let _ = ctx.run(Default::default(), |ctx| {
                ctx.fonts(|fonts| {
                    for (language, width) in [
                        (Language::PlainText, f32::INFINITY),
                        (Language::Rust, 195.0),
                        (Language::Markdown, 310.0),
                    ] {
                        let mut text = (0..500)
                            .map(|i| {
                                format!(
                                    "let value_{i} = \"hello שלום world لا العربية\"; // line\r\n\n"
                                )
                            })
                            .collect::<String>();
                        let theme = Theme::default();
                        let font = Font {
                            size: 14.0,
                            line_height: Some(19.0),
                        };
                        let mut previous: Option<(
                            Arc<Galley>,
                            super::super::super::bidi_view::BidiRows,
                            Vec<Block>,
                        )> = None;
                        for edit in 0..8 {
                            match edit {
                                1 => text.push_str("tail🦀"),
                                2 => text.insert_str(0, "// header\n"),
                                3 => {
                                    let a = text.find("value_250").unwrap();
                                    text.replace_range(a..a + 9, "renamed");
                                }
                                4 => {
                                    let a = text.find("value_30").unwrap();
                                    let end = text[a..].find('\n').unwrap() + a + 1;
                                    text.replace_range(a..end, "");
                                }
                                5 => {
                                    text = text.lines().take(20).collect::<Vec<_>>().join("\n");
                                }
                                6 => text.clear(),
                                7 => text.push_str("new\n"),
                                _ => (),
                            }
                            let job = code_job(&text, language, font, width, &theme, None);
                            let (expected, _) = super::super::super::bidi_view::apply(
                                fonts,
                                fonts.layout_job(job.clone()),
                                super::super::super::bidi_view::Base::for_language(language),
                            );
                            let old = previous
                                .as_ref()
                                .map(|(g, b, c)| (g.as_ref(), b, c.as_slice()));
                            let current = layout(
                                fonts,
                                job,
                                super::super::super::bidi_view::Base::for_language(language),
                                old,
                            );
                            compare(&current.0, &expected);
                            previous = Some(current);
                        }
                    }
                });
            });
        }
    }
    #[test]
    fn inline_edit_shares_unmodified_rows_while_previous_frame_remains_live() {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            ctx.fonts(|fonts| {
                let source = "a short line\n".repeat(2000);
                let font = Font {
                    size: 14.0,
                    line_height: None,
                };
                let theme = Theme::default();
                let base = super::super::super::bidi_view::Base::Auto;
                let first = layout(
                    fonts,
                    code_job(
                        &source,
                        Language::PlainText,
                        font,
                        f32::INFINITY,
                        &theme,
                        None,
                    ),
                    base,
                    None,
                );
                let next = format!("{source}x");
                let second = layout(
                    fonts,
                    code_job(
                        &next,
                        Language::PlainText,
                        font,
                        f32::INFINITY,
                        &theme,
                        None,
                    ),
                    base,
                    Some((&first.0, &first.1, &first.2)),
                );
                assert!(Arc::ptr_eq(&first.0.rows[0], &second.0.rows[0]));
                assert_eq!(first.0.job.text, source);
                let (expected, _) = super::super::super::bidi_view::apply(
                    fonts,
                    fonts.layout_job(code_job(
                        &next,
                        Language::PlainText,
                        font,
                        f32::INFINITY,
                        &theme,
                        None,
                    )),
                    base,
                );
                compare(&second.0, &expected);
            })
        });
    }
}
