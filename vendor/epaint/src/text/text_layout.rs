use std::ops::RangeInclusive;
use std::sync::Arc;

use emath::{pos2, vec2, Align, GuiRounding as _, NumExt, Pos2, Rect, Vec2};

use crate::{stroke::PathStroke, text::font::Font, Color32, Mesh, Stroke, Vertex};

use super::{FontsImpl, Galley, Glyph, LayoutJob, LayoutSection, Row, RowVisuals};

// ----------------------------------------------------------------------------

/// Represents GUI scale and convenience methods for rounding to pixels.
#[derive(Clone, Copy)]
struct PointScale {
    pub pixels_per_point: f32,
}

impl PointScale {
    #[inline(always)]
    pub fn new(pixels_per_point: f32) -> Self {
        Self { pixels_per_point }
    }

    #[inline(always)]
    pub fn pixels_per_point(&self) -> f32 {
        self.pixels_per_point
    }

    #[inline(always)]
    pub fn round_to_pixel(&self, point: f32) -> f32 {
        (point * self.pixels_per_point).round() / self.pixels_per_point
    }

    #[inline(always)]
    pub fn floor_to_pixel(&self, point: f32) -> f32 {
        (point * self.pixels_per_point).floor() / self.pixels_per_point
    }
}

// ----------------------------------------------------------------------------

/// Temporary storage before line-wrapping.
#[derive(Clone)]
struct Paragraph {
    /// Start of the next glyph to be added.
    pub cursor_x: f32,

    /// This is included in case there are no glyphs
    pub section_index_at_start: u32,

    pub glyphs: Vec<Glyph>,

    /// In case of an empty paragraph ("\n"), use this as height.
    pub empty_paragraph_height: f32,
}

impl Paragraph {
    pub fn from_section_index(section_index_at_start: u32, capacity: usize) -> Self {
        Self {
            cursor_x: 0.0,
            section_index_at_start,
            glyphs: Vec::with_capacity(capacity),
            empty_paragraph_height: 0.0,
        }
    }
}

/// Layout text into a [`Galley`].
///
/// In most cases you should use [`crate::Fonts::layout_job`] instead
/// since that memoizes the input, making subsequent layouting of the same text much faster.
pub fn layout(fonts: &mut FontsImpl, job: Arc<LayoutJob>) -> Galley {
    let capacities = paragraph_capacities(&job);
    layout_with_capacities(fonts, job, &capacities)
}

pub(super) const LONG_PARAGRAPH_BYTES: usize = 64 * 1024;

/// Continue the final section of a long, unwrapped paragraph. The caller must
/// obtain `previous` from the same font system's raw galley cache: edited,
/// translated, bidi-shaped or foreign-atlas galleys are not candidates.
pub(super) fn append_layout(
    fonts: &mut FontsImpl,
    job: Arc<LayoutJob>,
    previous: &Galley,
) -> Option<Galley> {
    let old = &previous.job;
    if old.text.len() < LONG_PARAGRAPH_BYTES
        || job.text.len() <= old.text.len()
        || previous.elided
        || previous.rows.len() != 1
        || previous.rows[0].ends_with_newline
        || !previous.rows[0].layout_rect.is_finite()
        || job.wrap.max_width != f32::INFINITY
        || old.wrap != job.wrap
        || old.break_on_newline != job.break_on_newline
        || old.first_row_min_height != job.first_row_min_height
        || old.halign != job.halign
        || old.justify != job.justify
        || old.round_output_to_gui != job.round_output_to_gui
        || job.halign != Align::LEFT
        || job.justify
        || previous.pixels_per_point != fonts.pixels_per_point()
        || job.sections.is_empty()
        || old.sections.len() != job.sections.len()
        || !job.text.starts_with(&old.text)
    {
        return None;
    }
    let suffix = &job.text[old.text.len()..];
    if job.break_on_newline && suffix.contains('\n') {
        return None;
    }
    let last_section = job.sections.len() - 1;
    let mut end = 0;
    for (i, (a, b)) in old.sections.iter().zip(&job.sections).enumerate() {
        if a.byte_range.start != end
            || b.byte_range.start != end
            || a.leading_space != b.leading_space
            || a.format != b.format
            || (i != last_section && a.byte_range != b.byte_range)
            || a.format.background != Color32::TRANSPARENT
            || a.format.underline != Stroke::NONE
            || a.format.strikethrough != Stroke::NONE
        {
            return None;
        }
        end = a.byte_range.end;
    }
    if end != old.text.len() || job.sections[last_section].byte_range.end != job.text.len() {
        return None;
    }
    let row = &previous.rows[0];
    let last = row.glyphs.last()?;
    if last.section_index as usize != last_section {
        return None;
    }
    let mut continuation = job.sections[last_section].clone();
    continuation.byte_range.start = old.text.len();
    continuation.leading_space = 0.0; // already applied before the cached prefix
    let font = fonts.font(&continuation.format.font_id);
    let cursor_x = font.round_to_pixel(last.max_x());
    let last_id = font.font_impl_and_glyph_info(last.chr).1.id;
    let mut paragraph = Paragraph::from_section_index(
        row.section_index_at_start,
        row.glyphs.len() + suffix.chars().count(),
    );
    paragraph.cursor_x = cursor_x;
    paragraph.glyphs.extend_from_slice(&row.glyphs);
    let mut paragraphs = vec![paragraph];
    layout_section(
        fonts,
        &job,
        last_section as u32,
        &continuation,
        &mut paragraphs,
        &[],
        Some(last_id),
    );
    let mut elided = false;
    let rows = rows_from_paragraphs(paragraphs, &job, &mut elided);
    Some(galley_from_rows(
        PointScale::new(fonts.pixels_per_point()),
        job,
        rows,
        elided,
        Some(row),
    ))
}

// Reserve large jobs once per paragraph, including paragraphs split across
// format sections. Count the actual shaped sections, not unused job text.
// Small labels keep the original path without an allocation-planning pass.
fn paragraph_capacities(job: &LayoutJob) -> Vec<usize> {
    if job.wrap.max_rows == 0 || job.text.len() < 4096 {
        return Vec::new();
    }
    let mut counts = vec![0];
    for section in &job.sections {
        let text = &job.text[section.byte_range.clone()];
        if job.break_on_newline {
            let mut lines = text.split('\n');
            *counts.last_mut().unwrap() += lines.next().unwrap().chars().count();
            counts.extend(lines.map(|line| line.chars().count()));
        } else {
            counts[0] += text.chars().count();
        }
    }
    counts
}

fn layout_with_capacities(
    fonts: &mut FontsImpl,
    job: Arc<LayoutJob>,
    capacities: &[usize],
) -> Galley {
    if job.wrap.max_rows == 0 {
        // Early-out: no text
        return Galley {
            job,
            rows: Default::default(),
            cursor_cache: Default::default(),
            rect: Rect::from_min_max(Pos2::ZERO, Pos2::ZERO),
            mesh_bounds: Rect::NOTHING,
            num_vertices: 0,
            num_indices: 0,
            pixels_per_point: fonts.pixels_per_point(),
            elided: true,
        };
    }

    // For most of this we ignore the y coordinate:

    let mut paragraphs = Vec::with_capacity(capacities.len().max(1));
    paragraphs.push(Paragraph::from_section_index(
        0,
        capacities.first().copied().unwrap_or(0),
    ));
    for (section_index, section) in job.sections.iter().enumerate() {
        layout_section(
            fonts,
            &job,
            section_index as u32,
            section,
            &mut paragraphs,
            capacities,
            None,
        );
    }

    let point_scale = PointScale::new(fonts.pixels_per_point());

    let mut elided = false;
    let mut rows = rows_from_paragraphs(paragraphs, &job, &mut elided);
    if elided {
        if let Some(last_row) = rows.last_mut() {
            replace_last_glyph_with_overflow_character(fonts, &job, last_row);
            if let Some(last) = last_row.glyphs.last() {
                last_row.rect.max.x = last.max_x();
            }
        }
    }

    let justify = job.justify && job.wrap.max_width.is_finite();

    if justify || job.halign != Align::LEFT {
        let num_rows = rows.len();
        for (i, row) in rows.iter_mut().enumerate() {
            let is_last_row = i + 1 == num_rows;
            let justify_row = justify && !row.ends_with_newline && !is_last_row;
            halign_and_justify_row(
                point_scale,
                row,
                job.halign,
                job.wrap.max_width,
                justify_row,
            );
        }
    }

    // Calculate the Y positions and tessellate the text:
    galley_from_rows(point_scale, job, rows, elided, None)
}

// Ignores the Y coordinate.
fn layout_section(
    fonts: &mut FontsImpl,
    job: &LayoutJob,
    section_index: u32,
    section: &LayoutSection,
    out_paragraphs: &mut Vec<Paragraph>,
    capacities: &[usize],
    last_glyph_id: Option<ab_glyph::GlyphId>,
) {
    let text = &job.text[section.byte_range.clone()];
    if text.len() >= LONG_PARAGRAPH_BYTES && text.is_ascii() {
        layout_section_glyphs::<true>(
            fonts,
            job,
            section_index,
            section,
            out_paragraphs,
            capacities,
            last_glyph_id,
        );
    } else {
        layout_section_glyphs::<false>(
            fonts,
            job,
            section_index,
            section,
            out_paragraphs,
            capacities,
            last_glyph_id,
        );
    }
}

fn layout_section_glyphs<const CACHE_KERNING: bool>(
    fonts: &mut FontsImpl,
    job: &LayoutJob,
    section_index: u32,
    section: &LayoutSection,
    out_paragraphs: &mut Vec<Paragraph>,
    capacities: &[usize],
    mut last_glyph_id: Option<ab_glyph::GlyphId>,
) {
    let LayoutSection {
        leading_space,
        byte_range,
        format,
    } = section;
    let font = fonts.font(&format.font_id);
    let line_height = section
        .format
        .line_height
        .unwrap_or_else(|| font.row_height());
    let extra_letter_spacing = section.format.extra_letter_spacing;

    let mut paragraph = out_paragraphs.last_mut().unwrap();
    if paragraph.glyphs.is_empty() {
        paragraph.empty_paragraph_height = line_height; // TODO(emilk): replace this hack with actually including `\n` in the glyphs?
    }

    paragraph.cursor_x += leading_space;

    let text = &job.text[byte_range.clone()];
    // Long ASCII lines tend to repeat glyph pairs thousands of times. Keep
    // kerning local to this font and layout section.
    let mut kerning = if CACHE_KERNING {
        vec![f32::NAN; 128 * 128]
    } else {
        Vec::new()
    };
    let mut previous_ascii: Option<usize> = None;
    for chr in text.chars() {
        if job.break_on_newline && chr == '\n' {
            let capacity = capacities.get(out_paragraphs.len()).copied().unwrap_or(0);
            out_paragraphs.push(Paragraph::from_section_index(section_index, capacity));
            paragraph = out_paragraphs.last_mut().unwrap();
            paragraph.empty_paragraph_height = line_height; // TODO(emilk): replace this hack with actually including `\n` in the glyphs?
        } else {
            let (font_impl, glyph_info) = font.font_impl_and_glyph_info(chr);
            if let Some(font_impl) = font_impl {
                if let Some(last_glyph_id) = last_glyph_id {
                    let kern = if CACHE_KERNING {
                        if let Some(previous) = previous_ascii {
                            let cached = &mut kerning[previous * 128 + chr as usize];
                            if cached.is_nan() {
                                *cached = font_impl.pair_kerning(last_glyph_id, glyph_info.id);
                            }
                            *cached
                        } else {
                            font_impl.pair_kerning(last_glyph_id, glyph_info.id)
                        }
                    } else {
                        font_impl.pair_kerning(last_glyph_id, glyph_info.id)
                    };
                    paragraph.cursor_x += kern;
                    paragraph.cursor_x += extra_letter_spacing;
                }
            }

            paragraph.glyphs.push(Glyph {
                chr,
                pos: pos2(paragraph.cursor_x, f32::NAN),
                advance_width: glyph_info.advance_width,
                line_height,
                font_impl_height: font_impl.map_or(0.0, |f| f.row_height()),
                font_impl_ascent: font_impl.map_or(0.0, |f| f.ascent()),
                font_height: font.row_height(),
                font_ascent: font.ascent(),
                uv_rect: glyph_info.uv_rect,
                section_index,
            });

            paragraph.cursor_x += glyph_info.advance_width;
            paragraph.cursor_x = font.round_to_pixel(paragraph.cursor_x);
            last_glyph_id = Some(glyph_info.id);
            if CACHE_KERNING {
                previous_ascii = Some(chr as usize);
            }
        }
    }
}

/// We ignore y at this stage
fn rect_from_x_range(x_range: RangeInclusive<f32>) -> Rect {
    Rect::from_x_y_ranges(x_range, 0.0..=0.0)
}

// Ignores the Y coordinate.
fn rows_from_paragraphs(
    paragraphs: Vec<Paragraph>,
    job: &LayoutJob,
    elided: &mut bool,
) -> Vec<Row> {
    let num_paragraphs = paragraphs.len();

    let mut rows = vec![];

    for (i, paragraph) in paragraphs.into_iter().enumerate() {
        if job.wrap.max_rows <= rows.len() {
            *elided = true;
            break;
        }

        let is_last_paragraph = (i + 1) == num_paragraphs;

        if paragraph.glyphs.is_empty() {
            rows.push(Row {
                layout_rect: Rect::NOTHING,
                section_index_at_start: paragraph.section_index_at_start,
                glyphs: vec![],
                visuals: Default::default(),
                rect: Rect::from_min_size(
                    pos2(paragraph.cursor_x, 0.0),
                    vec2(0.0, paragraph.empty_paragraph_height),
                ),
                ends_with_newline: !is_last_paragraph,
            });
        } else {
            let paragraph_max_x = paragraph.glyphs.last().unwrap().max_x();
            if paragraph_max_x <= job.effective_wrap_width() {
                // Early-out optimization: the whole paragraph fits on one row.
                let paragraph_min_x = paragraph.glyphs[0].pos.x;
                rows.push(Row {
                    layout_rect: Rect::NOTHING,
                    section_index_at_start: paragraph.section_index_at_start,
                    glyphs: paragraph.glyphs,
                    visuals: Default::default(),
                    rect: rect_from_x_range(paragraph_min_x..=paragraph_max_x),
                    ends_with_newline: !is_last_paragraph,
                });
            } else {
                line_break(&paragraph, job, &mut rows, elided);
                rows.last_mut().unwrap().ends_with_newline = !is_last_paragraph;
            }
        }
    }

    rows
}

fn line_break(paragraph: &Paragraph, job: &LayoutJob, out_rows: &mut Vec<Row>, elided: &mut bool) {
    let wrap_width = job.effective_wrap_width();

    // Keeps track of good places to insert row break if we exceed `wrap_width`.
    let mut row_break_candidates = RowBreakCandidates::default();

    let mut first_row_indentation = paragraph.glyphs[0].pos.x;
    let mut row_start_x = 0.0;
    let mut row_start_idx = 0;

    for i in 0..paragraph.glyphs.len() {
        if job.wrap.max_rows <= out_rows.len() {
            *elided = true;
            break;
        }

        let potential_row_width = paragraph.glyphs[i].max_x() - row_start_x;

        if wrap_width < potential_row_width {
            // Row break:

            if first_row_indentation > 0.0
                && !row_break_candidates.has_good_candidate(job.wrap.break_anywhere)
            {
                // Allow the first row to be completely empty, because we know there will be more space on the next row:
                // TODO(emilk): this records the height of this first row as zero, though that is probably fine since first_row_indentation usually comes with a first_row_min_height.
                out_rows.push(Row {
                    layout_rect: Rect::NOTHING,
                    section_index_at_start: paragraph.section_index_at_start,
                    glyphs: vec![],
                    visuals: Default::default(),
                    rect: rect_from_x_range(first_row_indentation..=first_row_indentation),
                    ends_with_newline: false,
                });
                row_start_x += first_row_indentation;
                first_row_indentation = 0.0;
            } else if let Some(last_kept_index) = row_break_candidates.get(job.wrap.break_anywhere)
            {
                let glyphs: Vec<Glyph> = paragraph.glyphs[row_start_idx..=last_kept_index]
                    .iter()
                    .copied()
                    .map(|mut glyph| {
                        glyph.pos.x -= row_start_x;
                        glyph
                    })
                    .collect();

                let section_index_at_start = glyphs[0].section_index;
                let paragraph_min_x = glyphs[0].pos.x;
                let paragraph_max_x = glyphs.last().unwrap().max_x();

                out_rows.push(Row {
                    layout_rect: Rect::NOTHING,
                    section_index_at_start,
                    glyphs,
                    visuals: Default::default(),
                    rect: rect_from_x_range(paragraph_min_x..=paragraph_max_x),
                    ends_with_newline: false,
                });

                // Start a new row:
                row_start_idx = last_kept_index + 1;
                row_start_x = paragraph.glyphs[row_start_idx].pos.x;
                row_break_candidates.forget_before_idx(row_start_idx);
            } else {
                // Found no place to break, so we have to overrun wrap_width.
            }
        }

        row_break_candidates.add(i, &paragraph.glyphs[i..]);
    }

    if row_start_idx < paragraph.glyphs.len() {
        // Final row of text:

        if job.wrap.max_rows <= out_rows.len() {
            *elided = true; // can't fit another row
        } else {
            let glyphs: Vec<Glyph> = paragraph.glyphs[row_start_idx..]
                .iter()
                .copied()
                .map(|mut glyph| {
                    glyph.pos.x -= row_start_x;
                    glyph
                })
                .collect();

            let section_index_at_start = glyphs[0].section_index;
            let paragraph_min_x = glyphs[0].pos.x;
            let paragraph_max_x = glyphs.last().unwrap().max_x();

            out_rows.push(Row {
                layout_rect: Rect::NOTHING,
                section_index_at_start,
                glyphs,
                visuals: Default::default(),
                rect: rect_from_x_range(paragraph_min_x..=paragraph_max_x),
                ends_with_newline: false,
            });
        }
    }
}

/// Trims the last glyphs in the row and replaces it with an overflow character (e.g. `…`).
///
/// Called before we have any Y coordinates.
fn replace_last_glyph_with_overflow_character(
    fonts: &mut FontsImpl,
    job: &LayoutJob,
    row: &mut Row,
) {
    fn row_width(row: &Row) -> f32 {
        if let (Some(first), Some(last)) = (row.glyphs.first(), row.glyphs.last()) {
            last.max_x() - first.pos.x
        } else {
            0.0
        }
    }

    fn row_height(section: &LayoutSection, font: &Font) -> f32 {
        section
            .format
            .line_height
            .unwrap_or_else(|| font.row_height())
    }

    let Some(overflow_character) = job.wrap.overflow_character else {
        return;
    };

    // We always try to just append the character first:
    if let Some(last_glyph) = row.glyphs.last() {
        let section_index = last_glyph.section_index;
        let section = &job.sections[section_index as usize];
        let font = fonts.font(&section.format.font_id);
        let line_height = row_height(section, font);

        let (_, last_glyph_info) = font.font_impl_and_glyph_info(last_glyph.chr);

        let mut x = last_glyph.pos.x + last_glyph.advance_width;

        let (font_impl, replacement_glyph_info) = font.font_impl_and_glyph_info(overflow_character);

        {
            // Kerning:
            x += section.format.extra_letter_spacing;
            if let Some(font_impl) = font_impl {
                x += font_impl.pair_kerning(last_glyph_info.id, replacement_glyph_info.id);
            }
        }

        row.glyphs.push(Glyph {
            chr: overflow_character,
            pos: pos2(x, f32::NAN),
            advance_width: replacement_glyph_info.advance_width,
            line_height,
            font_impl_height: font_impl.map_or(0.0, |f| f.row_height()),
            font_impl_ascent: font_impl.map_or(0.0, |f| f.ascent()),
            font_height: font.row_height(),
            font_ascent: font.ascent(),
            uv_rect: replacement_glyph_info.uv_rect,
            section_index,
        });
    } else {
        let section_index = row.section_index_at_start;
        let section = &job.sections[section_index as usize];
        let font = fonts.font(&section.format.font_id);
        let line_height = row_height(section, font);

        let x = 0.0; // TODO(emilk): heed paragraph leading_space 😬

        let (font_impl, replacement_glyph_info) = font.font_impl_and_glyph_info(overflow_character);

        row.glyphs.push(Glyph {
            chr: overflow_character,
            pos: pos2(x, f32::NAN),
            advance_width: replacement_glyph_info.advance_width,
            line_height,
            font_impl_height: font_impl.map_or(0.0, |f| f.row_height()),
            font_impl_ascent: font_impl.map_or(0.0, |f| f.ascent()),
            font_height: font.row_height(),
            font_ascent: font.ascent(),
            uv_rect: replacement_glyph_info.uv_rect,
            section_index,
        });
    }

    if row_width(row) <= job.effective_wrap_width() || row.glyphs.len() == 1 {
        return; // we are done
    }

    // We didn't fit it. Remove it again…
    row.glyphs.pop();

    // …then go into a loop where we replace the last character with the overflow character
    // until we fit within the max_width:

    loop {
        let (prev_glyph, last_glyph) = match row.glyphs.as_mut_slice() {
            [.., prev, last] => (Some(prev), last),
            [.., last] => (None, last),
            _ => {
                unreachable!("We've already explicitly handled the empty row");
            }
        };

        let section = &job.sections[last_glyph.section_index as usize];
        let extra_letter_spacing = section.format.extra_letter_spacing;
        let font = fonts.font(&section.format.font_id);

        if let Some(prev_glyph) = prev_glyph {
            let prev_glyph_id = font.font_impl_and_glyph_info(prev_glyph.chr).1.id;

            // Undo kerning with previous glyph:
            let (font_impl, glyph_info) = font.font_impl_and_glyph_info(last_glyph.chr);
            last_glyph.pos.x -= extra_letter_spacing;
            if let Some(font_impl) = font_impl {
                last_glyph.pos.x -= font_impl.pair_kerning(prev_glyph_id, glyph_info.id);
            }

            // Replace the glyph:
            last_glyph.chr = overflow_character;
            let (font_impl, glyph_info) = font.font_impl_and_glyph_info(last_glyph.chr);
            last_glyph.advance_width = glyph_info.advance_width;
            last_glyph.font_impl_ascent = font_impl.map_or(0.0, |f| f.ascent());
            last_glyph.font_impl_height = font_impl.map_or(0.0, |f| f.row_height());
            last_glyph.uv_rect = glyph_info.uv_rect;

            // Reapply kerning:
            last_glyph.pos.x += extra_letter_spacing;
            if let Some(font_impl) = font_impl {
                last_glyph.pos.x += font_impl.pair_kerning(prev_glyph_id, glyph_info.id);
            }

            // Check if we're within width budget:
            if row_width(row) <= job.effective_wrap_width() || row.glyphs.len() == 1 {
                return; // We are done
            }

            // We didn't fit - pop the last glyph and try again.
            row.glyphs.pop();
        } else {
            // Just replace and be done with it.
            last_glyph.chr = overflow_character;
            let (font_impl, glyph_info) = font.font_impl_and_glyph_info(last_glyph.chr);
            last_glyph.advance_width = glyph_info.advance_width;
            last_glyph.font_impl_ascent = font_impl.map_or(0.0, |f| f.ascent());
            last_glyph.font_impl_height = font_impl.map_or(0.0, |f| f.row_height());
            last_glyph.uv_rect = glyph_info.uv_rect;
            return;
        }
    }
}

/// Horizontally aligned the text on a row.
///
/// Ignores the Y coordinate.
fn halign_and_justify_row(
    point_scale: PointScale,
    row: &mut Row,
    halign: Align,
    wrap_width: f32,
    justify: bool,
) {
    if row.glyphs.is_empty() {
        return;
    }

    let num_leading_spaces = row
        .glyphs
        .iter()
        .take_while(|glyph| glyph.chr.is_whitespace())
        .count();

    let glyph_range = if num_leading_spaces == row.glyphs.len() {
        // There is only whitespace
        (0, row.glyphs.len())
    } else {
        let num_trailing_spaces = row
            .glyphs
            .iter()
            .rev()
            .take_while(|glyph| glyph.chr.is_whitespace())
            .count();

        (num_leading_spaces, row.glyphs.len() - num_trailing_spaces)
    };
    let num_glyphs_in_range = glyph_range.1 - glyph_range.0;
    assert!(num_glyphs_in_range > 0);

    let original_min_x = row.glyphs[glyph_range.0].logical_rect().min.x;
    let original_max_x = row.glyphs[glyph_range.1 - 1].logical_rect().max.x;
    let original_width = original_max_x - original_min_x;

    let target_width = if justify && num_glyphs_in_range > 1 {
        wrap_width
    } else {
        original_width
    };

    let (target_min_x, target_max_x) = match halign {
        Align::LEFT => (0.0, target_width),
        Align::Center => (-target_width / 2.0, target_width / 2.0),
        Align::RIGHT => (-target_width, 0.0),
    };

    let num_spaces_in_range = row.glyphs[glyph_range.0..glyph_range.1]
        .iter()
        .filter(|glyph| glyph.chr.is_whitespace())
        .count();

    let mut extra_x_per_glyph = if num_glyphs_in_range == 1 {
        0.0
    } else {
        (target_width - original_width) / (num_glyphs_in_range as f32 - 1.0)
    };
    extra_x_per_glyph = extra_x_per_glyph.at_least(0.0); // Don't contract

    let mut extra_x_per_space = 0.0;
    if 0 < num_spaces_in_range && num_spaces_in_range < num_glyphs_in_range {
        // Add an integral number of pixels between each glyph,
        // and add the balance to the spaces:

        extra_x_per_glyph = point_scale.floor_to_pixel(extra_x_per_glyph);

        extra_x_per_space = (target_width
            - original_width
            - extra_x_per_glyph * (num_glyphs_in_range as f32 - 1.0))
            / (num_spaces_in_range as f32);
    }

    let mut translate_x = target_min_x - original_min_x - extra_x_per_glyph * glyph_range.0 as f32;

    for glyph in &mut row.glyphs {
        glyph.pos.x += translate_x;
        glyph.pos.x = point_scale.round_to_pixel(glyph.pos.x);
        translate_x += extra_x_per_glyph;
        if glyph.chr.is_whitespace() {
            translate_x += extra_x_per_space;
        }
    }

    // Note we ignore the leading/trailing whitespace here!
    row.rect.min.x = target_min_x;
    row.rect.max.x = target_max_x;
}

/// Calculate the Y positions and tessellate the text.
fn galley_from_rows(
    point_scale: PointScale,
    job: Arc<LayoutJob>,
    mut rows: Vec<Row>,
    elided: bool,
    prefix: Option<&Row>,
) -> Galley {
    let mut first_row_min_height = job.first_row_min_height;
    let mut cursor_y = 0.0;
    let mut min_x: f32 = 0.0;
    let mut max_x: f32 = 0.0;
    for (i, row) in rows.iter_mut().enumerate() {
        let prefix = prefix.filter(|_| i == 0);
        let prefix_len = prefix.map_or(0, |row| row.glyphs.len());
        let mut max_row_height = first_row_min_height.max(row.rect.height());
        if let Some(prefix) = prefix {
            max_row_height = max_row_height.max(prefix.layout_rect.height());
        }
        first_row_min_height = 0.0;
        for glyph in &row.glyphs[prefix_len..] {
            max_row_height = max_row_height.max(glyph.line_height);
        }
        max_row_height = point_scale.round_to_pixel(max_row_height);

        // Now position each glyph vertically:
        for glyph in &mut row.glyphs[prefix_len..] {
            let format = &job.sections[glyph.section_index as usize].format;

            glyph.pos.y = cursor_y
                + glyph.font_impl_ascent

                // Apply valign to the different in height of the entire row, and the height of this `Font`:
                + format.valign.to_factor() * (max_row_height - glyph.line_height)

                // When mixing different `FontImpl` (e.g. latin and emojis),
                // we always center the difference:
                + 0.5 * (glyph.font_height - glyph.font_impl_height);

            glyph.pos.y = point_scale.round_to_pixel(glyph.pos.y);
        }

        row.rect.min.y = cursor_y;
        row.rect.max.y = cursor_y + max_row_height;
        row.layout_rect = row.rect;

        min_x = min_x.min(row.rect.min.x);
        max_x = max_x.max(row.rect.max.x);
        cursor_y += max_row_height;
        cursor_y = point_scale.round_to_pixel(cursor_y); // TODO(emilk): it would be better to do the calculations in pixels instead.
    }

    let format_summary = format_summary(&job);

    let mut mesh_bounds = Rect::NOTHING;
    let mut num_vertices = 0;
    let mut num_indices = 0;

    for (i, row) in rows.iter_mut().enumerate() {
        row.visuals = tessellate_row(
            point_scale,
            &job,
            &format_summary,
            row,
            prefix.filter(|_| i == 0),
        );
        mesh_bounds = mesh_bounds.union(row.visuals.mesh_bounds);
        num_vertices += row.visuals.mesh.vertices.len();
        num_indices += row.visuals.mesh.indices.len();
    }

    let mut rect = Rect::from_min_max(pos2(min_x, 0.0), pos2(max_x, cursor_y));

    if job.round_output_to_gui {
        for row in &mut rows {
            row.rect = row.rect.round_ui();
        }

        let did_exceed_wrap_width_by_a_lot = rect.width() > job.wrap.max_width + 1.0;

        rect = rect.round_ui();

        if did_exceed_wrap_width_by_a_lot {
            // If the user picked a too aggressive wrap width (e.g. more narrow than any individual glyph),
            // we should let the user know by reporting that our width is wider than the wrap width.
        } else {
            // Make sure we don't report being wider than the wrap width the user picked:
            rect.max.x = rect
                .max
                .x
                .at_most(rect.min.x + job.wrap.max_width)
                .floor_ui();
        }
    }

    Galley {
        job,
        rows: rows.into_iter().map(Arc::new).collect(),
        cursor_cache: Default::default(),
        elided,
        rect,
        mesh_bounds,
        num_vertices,
        num_indices,
        pixels_per_point: point_scale.pixels_per_point,
    }
}

#[derive(Default)]
struct FormatSummary {
    any_background: bool,
    any_underline: bool,
    any_strikethrough: bool,
}

fn format_summary(job: &LayoutJob) -> FormatSummary {
    let mut format_summary = FormatSummary::default();
    for section in &job.sections {
        format_summary.any_background |= section.format.background != Color32::TRANSPARENT;
        format_summary.any_underline |= section.format.underline != Stroke::NONE;
        format_summary.any_strikethrough |= section.format.strikethrough != Stroke::NONE;
    }
    format_summary
}

fn tessellate_row(
    point_scale: PointScale,
    job: &LayoutJob,
    format_summary: &FormatSummary,
    row: &Row,
    prefix: Option<&Row>,
) -> RowVisuals {
    if row.glyphs.is_empty() {
        return Default::default();
    }

    let mut mesh = Mesh::default();

    mesh.reserve_triangles(row.glyphs.len() * 2);
    mesh.reserve_vertices(row.glyphs.len() * 4);

    if format_summary.any_background {
        add_row_backgrounds(job, row, &mut mesh);
    }

    let glyph_index_start = mesh.indices.len();
    let glyph_vertex_start = mesh.vertices.len();
    let prefix_len = prefix.map_or(0, |row| row.glyphs.len());
    if let Some(prefix) = prefix {
        debug_assert!(
            !format_summary.any_background
                && !format_summary.any_underline
                && !format_summary.any_strikethrough
        );
        mesh.vertices
            .extend_from_slice(&prefix.visuals.mesh.vertices);
        mesh.indices.extend_from_slice(&prefix.visuals.mesh.indices);
    }
    let new_vertex_start = mesh.vertices.len();
    tessellate_glyphs(point_scale, job, &row.glyphs[prefix_len..], &mut mesh);
    let glyph_vertex_end = mesh.vertices.len();

    if format_summary.any_underline {
        add_row_hline(point_scale, row, &mut mesh, |glyph| {
            let format = &job.sections[glyph.section_index as usize].format;
            let stroke = format.underline;
            let y = glyph.logical_rect().bottom();
            (stroke, y)
        });
    }

    if format_summary.any_strikethrough {
        add_row_hline(point_scale, row, &mut mesh, |glyph| {
            let format = &job.sections[glyph.section_index as usize].format;
            let stroke = format.strikethrough;
            let y = glyph.logical_rect().center().y;
            (stroke, y)
        });
    }

    let mesh_bounds = if let Some(prefix) = prefix {
        let mut bounds = prefix.visuals.mesh_bounds;
        for vertex in &mesh.vertices[new_vertex_start..] {
            bounds.extend_with(vertex.pos);
        }
        bounds
    } else {
        mesh.calc_bounds()
    };

    RowVisuals {
        mesh_cache: Default::default(),
        mesh: Arc::new(mesh),
        mesh_offset: Vec2::ZERO,
        mesh_bounds,
        glyph_index_start,
        glyph_vertex_range: glyph_vertex_start..glyph_vertex_end,
    }
}

/// Create background for glyphs that have them.
/// Creates as few rectangular regions as possible.
fn add_row_backgrounds(job: &LayoutJob, row: &Row, mesh: &mut Mesh) {
    if row.glyphs.is_empty() {
        return;
    }

    let mut end_run = |start: Option<(Color32, Rect)>, stop_x: f32| {
        if let Some((color, start_rect)) = start {
            let rect = Rect::from_min_max(start_rect.left_top(), pos2(stop_x, start_rect.bottom()));
            let rect = rect.expand(1.0); // looks better
            mesh.add_colored_rect(rect, color);
        }
    };

    let mut run_start = None;
    let mut last_rect = Rect::NAN;

    for glyph in &row.glyphs {
        let format = &job.sections[glyph.section_index as usize].format;
        let color = format.background;
        let rect = glyph.logical_rect();

        if color == Color32::TRANSPARENT {
            end_run(run_start.take(), last_rect.right());
        } else if let Some((existing_color, start)) = run_start {
            if existing_color == color
                && start.top() == rect.top()
                && start.bottom() == rect.bottom()
            {
                // continue the same background rectangle
            } else {
                end_run(run_start.take(), last_rect.right());
                run_start = Some((color, rect));
            }
        } else {
            run_start = Some((color, rect));
        }

        last_rect = rect;
    }

    end_run(run_start.take(), last_rect.right());
}

fn tessellate_glyphs(point_scale: PointScale, job: &LayoutJob, glyphs: &[Glyph], mesh: &mut Mesh) {
    for glyph in glyphs {
        let uv_rect = glyph.uv_rect;
        if !uv_rect.is_nothing() {
            let mut left_top = glyph.pos + uv_rect.offset;
            left_top.x = point_scale.round_to_pixel(left_top.x);
            left_top.y = point_scale.round_to_pixel(left_top.y);

            let rect = Rect::from_min_max(left_top, left_top + uv_rect.size);
            let uv = Rect::from_min_max(
                pos2(uv_rect.min[0] as f32, uv_rect.min[1] as f32),
                pos2(uv_rect.max[0] as f32, uv_rect.max[1] as f32),
            );

            let format = &job.sections[glyph.section_index as usize].format;

            let color = format.color;

            if format.italics {
                let idx = mesh.vertices.len() as u32;
                mesh.add_triangle(idx, idx + 1, idx + 2);
                mesh.add_triangle(idx + 2, idx + 1, idx + 3);

                let top_offset = rect.height() * 0.25 * Vec2::X;

                mesh.vertices.push(Vertex {
                    pos: rect.left_top() + top_offset,
                    uv: uv.left_top(),
                    color,
                });
                mesh.vertices.push(Vertex {
                    pos: rect.right_top() + top_offset,
                    uv: uv.right_top(),
                    color,
                });
                mesh.vertices.push(Vertex {
                    pos: rect.left_bottom(),
                    uv: uv.left_bottom(),
                    color,
                });
                mesh.vertices.push(Vertex {
                    pos: rect.right_bottom(),
                    uv: uv.right_bottom(),
                    color,
                });
            } else {
                mesh.add_rect_with_uv(rect, uv, color);
            }
        }
    }
}

/// Add a horizontal line over a row of glyphs with a stroke and y decided by a callback.
fn add_row_hline(
    point_scale: PointScale,
    row: &Row,
    mesh: &mut Mesh,
    stroke_and_y: impl Fn(&Glyph) -> (Stroke, f32),
) {
    let mut end_line = |start: Option<(Stroke, Pos2)>, stop_x: f32| {
        if let Some((stroke, start)) = start {
            add_hline(point_scale, [start, pos2(stop_x, start.y)], stroke, mesh);
        }
    };

    let mut line_start = None;
    let mut last_right_x = f32::NAN;

    for glyph in &row.glyphs {
        let (stroke, y) = stroke_and_y(glyph);

        if stroke == Stroke::NONE {
            end_line(line_start.take(), last_right_x);
        } else if let Some((existing_stroke, start)) = line_start {
            if existing_stroke == stroke && start.y == y {
                // continue the same line
            } else {
                end_line(line_start.take(), last_right_x);
                line_start = Some((stroke, pos2(glyph.pos.x, y)));
            }
        } else {
            line_start = Some((stroke, pos2(glyph.pos.x, y)));
        }

        last_right_x = glyph.max_x();
    }

    end_line(line_start.take(), last_right_x);
}

fn add_hline(point_scale: PointScale, [start, stop]: [Pos2; 2], stroke: Stroke, mesh: &mut Mesh) {
    let antialiased = true;

    if antialiased {
        let mut path = crate::tessellator::Path::default(); // TODO(emilk): reuse this to avoid re-allocations.
        path.add_line_segment([start, stop]);
        let feathering = 1.0 / point_scale.pixels_per_point();
        path.stroke_open(feathering, &PathStroke::from(stroke), mesh);
    } else {
        // Thin lines often lost, so this is a bad idea

        assert_eq!(start.y, stop.y);

        let min_y = point_scale.round_to_pixel(start.y - 0.5 * stroke.width);
        let max_y = point_scale.round_to_pixel(min_y + stroke.width);

        let rect = Rect::from_min_max(
            pos2(point_scale.round_to_pixel(start.x), min_y),
            pos2(point_scale.round_to_pixel(stop.x), max_y),
        );

        mesh.add_colored_rect(rect, stroke.color);
    }
}

// ----------------------------------------------------------------------------

/// Keeps track of good places to break a long row of text.
/// Will focus primarily on spaces, secondarily on things like `-`
#[derive(Clone, Copy, Default)]
struct RowBreakCandidates {
    /// Breaking at ` ` or other whitespace
    /// is always the primary candidate.
    space: Option<usize>,

    /// Logograms (single character representing a whole word) or kana (Japanese hiragana and katakana) are good candidates for line break.
    cjk: Option<usize>,

    /// Breaking anywhere before a CJK character is acceptable too.
    pre_cjk: Option<usize>,

    /// Breaking at a dash is a super-
    /// good idea.
    dash: Option<usize>,

    /// This is nicer for things like URLs, e.g. www.
    /// example.com.
    punctuation: Option<usize>,

    /// Breaking after just random character is some
    /// times necessary.
    any: Option<usize>,
}

impl RowBreakCandidates {
    fn add(&mut self, index: usize, glyphs: &[Glyph]) {
        let chr = glyphs[0].chr;
        const NON_BREAKING_SPACE: char = '\u{A0}';
        if chr.is_whitespace() && chr != NON_BREAKING_SPACE {
            self.space = Some(index);
        } else if is_cjk(chr) && (glyphs.len() == 1 || is_cjk_break_allowed(glyphs[1].chr)) {
            self.cjk = Some(index);
        } else if chr == '-' {
            self.dash = Some(index);
        } else if chr.is_ascii_punctuation() {
            self.punctuation = Some(index);
        } else if glyphs.len() > 1 && is_cjk(glyphs[1].chr) {
            self.pre_cjk = Some(index);
        }
        self.any = Some(index);
    }

    fn word_boundary(&self) -> Option<usize> {
        [self.space, self.cjk, self.pre_cjk]
            .into_iter()
            .max()
            .flatten()
    }

    fn has_good_candidate(&self, break_anywhere: bool) -> bool {
        if break_anywhere {
            self.any.is_some()
        } else {
            self.word_boundary().is_some()
        }
    }

    fn get(&self, break_anywhere: bool) -> Option<usize> {
        if break_anywhere {
            self.any
        } else {
            self.word_boundary()
                .or(self.dash)
                .or(self.punctuation)
                .or(self.any)
        }
    }

    fn forget_before_idx(&mut self, index: usize) {
        let Self {
            space,
            cjk,
            pre_cjk,
            dash,
            punctuation,
            any,
        } = self;
        if space.is_some_and(|s| s < index) {
            *space = None;
        }
        if cjk.is_some_and(|s| s < index) {
            *cjk = None;
        }
        if pre_cjk.is_some_and(|s| s < index) {
            *pre_cjk = None;
        }
        if dash.is_some_and(|s| s < index) {
            *dash = None;
        }
        if punctuation.is_some_and(|s| s < index) {
            *punctuation = None;
        }
        if any.is_some_and(|s| s < index) {
            *any = None;
        }
    }
}

#[inline]
fn is_cjk_ideograph(c: char) -> bool {
    ('\u{4E00}' <= c && c <= '\u{9FFF}')
        || ('\u{3400}' <= c && c <= '\u{4DBF}')
        || ('\u{2B740}' <= c && c <= '\u{2B81F}')
}

#[inline]
fn is_kana(c: char) -> bool {
    ('\u{3040}' <= c && c <= '\u{309F}') // Hiragana block
        || ('\u{30A0}' <= c && c <= '\u{30FF}') // Katakana block
}

#[inline]
fn is_cjk(c: char) -> bool {
    // TODO(bigfarts): Add support for Korean Hangul.
    is_cjk_ideograph(c) || is_kana(c)
}

#[inline]
fn is_cjk_break_allowed(c: char) -> bool {
    // See: https://en.wikipedia.org/wiki/Line_breaking_rules_in_East_Asian_languages#Characters_not_permitted_on_the_start_of_a_line.
    !")]｝〕〉》」』】〙〗〟'\"｠»ヽヾーァィゥェォッャュョヮヵヶぁぃぅぇぉっゃゅょゎゕゖㇰㇱㇲㇳㇴㇵㇶㇷㇸㇹㇺㇻㇼㇽㇾㇿ々〻‐゠–〜?!‼⁇⁈⁉・、:;,。.".contains(c)
}

// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{super::*, *};

    #[test]
    fn appended_long_paragraphs_match_full_layout_and_preserve_previous_frames() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for family in [FontFamily::Monospace, FontFamily::Proportional] {
                let mut fonts = FontsImpl::new(scale, 4096, FontDefinitions::default());
                let mut reference = FontsImpl::new(scale, 4096, FontDefinitions::default());
                let mut job = LayoutJob::default();
                job.first_row_min_height = 37.25;
                for (text, size, leading, height) in [
                    ("AVATAR To fi ".repeat(6000), 14.0, 2.25, 20.25),
                    ("αe\u{301}🦀שלום مرحبا\t\r".into(), 19.25, 3.5, 29.5),
                ] {
                    job.append(
                        &text,
                        leading,
                        TextFormat {
                            font_id: FontId::new(size, family.clone()),
                            line_height: Some(height),
                            color: Color32::LIGHT_BLUE,
                            italics: true,
                            extra_letter_spacing: 0.25,
                            ..Default::default()
                        },
                    );
                }
                let mut previous = layout(&mut fonts, Arc::new(job.clone()));
                let _ = layout(&mut reference, Arc::new(job.clone()));
                for suffix in ["x", "AV", "e\u{301}α🦀", "\t\r", "\u{200d}", "שלום مرحبا"]
                {
                    let saved = previous.clone();
                    let _ = previous.cursor_index();
                    job.text.push_str(suffix);
                    job.sections.last_mut().unwrap().byte_range.end = job.text.len();
                    let job = Arc::new(job.clone());
                    let actual = append_layout(&mut fonts, job.clone(), &previous).unwrap();
                    let expected = layout(&mut reference, job);
                    assert_eq!(actual, expected, "scale={scale} suffix={suffix:?}");
                    assert_eq!(
                        previous, saved,
                        "a live previous frame must remain immutable"
                    );
                    assert_eq!(
                        &actual.rows[0].visuals.mesh.vertices[..previous.num_vertices],
                        previous.rows[0].visuals.mesh.vertices
                    );
                    previous = actual;
                }
            }
        }
    }

    #[test]
    fn append_layout_falls_back_for_other_edits_styles_wrapping_and_elision() {
        let mut fonts = FontsImpl::new(1.25, 4096, FontDefinitions::default());
        let original = LayoutJob::single_section("AVATAR ".repeat(10_000), TextFormat::default());
        let previous = layout(&mut fonts, Arc::new(original.clone()));
        for variant in 0..12 {
            let mut job = original.clone();
            job.text.push('x');
            job.sections[0].byte_range.end = job.text.len();
            match variant {
                0 => job.text.insert(0, 'x'),
                1 => job.text.push('\n'),
                2 => job.wrap.max_width = 900.0,
                3 => job.halign = Align::RIGHT,
                4 => job.justify = true,
                5 => job.sections[0].format.color = Color32::RED,
                6 => job.sections[0].format.background = Color32::RED,
                7 => job.sections[0].format.underline = Stroke::new(1.0_f32, Color32::RED),
                8 => job.wrap.max_rows = 0,
                9 => job.sections[0].byte_range.start = 1,
                10 => job.first_row_min_height = 99.0,
                11 => job.sections[0].leading_space = 5.0,
                _ => unreachable!(),
            }
            assert!(
                append_layout(&mut fonts, Arc::new(job), &previous).is_none(),
                "variant={variant}"
            );
        }
    }

    #[test]
    fn cached_append_candidates_do_not_outlive_font_resets_or_extend_retention() {
        let fonts = Fonts::new(1.0, 4096, FontDefinitions::default());
        let mut job = LayoutJob::single_section("AVATAR ".repeat(10_000), TextFormat::default());
        let previous = fonts.layout_job(job.clone());
        let weak = Arc::downgrade(&previous);
        // The application retains the active raw galley, even after the
        // renderer's ordinary per-frame cache has forgotten its job.
        fonts.begin_pass(1.0, 4096);
        fonts.begin_pass(1.0, 4096);
        job.text.push_str("α🦀");
        job.sections[0].byte_range.end = job.text.len();
        let actual = fonts.layout_job(job.clone());
        let mut reference = FontsImpl::new(1.0, 4096, FontDefinitions::default());
        let _ = layout(&mut reference, previous.job.clone());
        assert_eq!(*actual, layout(&mut reference, Arc::new(job.clone())));
        fonts.begin_pass(1.5, 2048);
        let mut reference = FontsImpl::new(1.5, 2048, FontDefinitions::default());
        assert_eq!(
            *fonts.layout_job(job),
            layout(&mut reference, actual.job.clone())
        );
        drop(previous);
        drop(actual);
        assert!(
            weak.upgrade().is_none(),
            "the candidate must be weak, not a text-retaining cache"
        );
    }

    #[test]
    #[ignore = "manual optimized paired profile of full versus appended long-paragraph layout"]
    fn profile_long_paragraph_appends() {
        for (label, text) in [
            ("165k ASCII", "abcdefghij ".repeat(15_000)),
            ("1M ASCII", "abcdefghij".repeat(100_000)),
            ("Unicode", "αe\u{301}🦀 שלום مرحبا ".repeat(6000)),
        ] {
            let mut full_fonts = FontsImpl::new(1.0, 4096, FontDefinitions::default());
            let mut incremental_fonts = FontsImpl::new(1.0, 4096, FontDefinitions::default());
            let mut job = LayoutJob::single_section(
                text,
                TextFormat {
                    font_id: FontId::monospace(14.0),
                    ..Default::default()
                },
            );
            let mut previous = layout(&mut incremental_fonts, Arc::new(job.clone()));
            let _ = layout(&mut full_fonts, Arc::new(job.clone()));
            let mut times = [Vec::new(), Vec::new()];
            for n in 0..20 {
                job.text.push('x');
                job.sections[0].byte_range.end = job.text.len();
                let job = Arc::new(job.clone());
                let mut next = None;
                for incremental in if n % 2 == 0 {
                    [false, true]
                } else {
                    [true, false]
                } {
                    let start = std::time::Instant::now();
                    let galley = if incremental {
                        append_layout(&mut incremental_fonts, job.clone(), &previous).unwrap()
                    } else {
                        layout(&mut full_fonts, job.clone())
                    };
                    times[usize::from(incremental)].push(start.elapsed().as_secs_f64() * 1000.0);
                    if incremental {
                        next = Some(galley);
                    } else {
                        std::hint::black_box(galley);
                    }
                }
                previous = next.unwrap();
            }
            for samples in &mut times {
                samples.sort_by(f64::total_cmp);
            }
            eprintln!(
                "Append {label}: full p50={:.3} p95={:.3}ms; incremental p50={:.3} p95={:.3}ms",
                times[0][10], times[0][18], times[1][10], times[1][18]
            );
        }
    }

    #[test]
    fn direct_glyph_lookup_preserves_full_layout_across_font_and_atlas_changes() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for reverse_fallbacks in [false, true] {
                let mut definitions = FontDefinitions::default();
                if reverse_fallbacks {
                    for fonts in definitions.families.values_mut() {
                        fonts.reverse();
                    }
                }
                // Independent font systems also check cold atlas allocation
                // order and ensure no UVs leak from a different font/atlas.
                let mut direct = FontsImpl::new(scale, 4096, definitions.clone());
                let mut reference = FontsImpl::new(scale, 4096, definitions);
                for width in [0.0, 137.5, f32::INFINITY] {
                    for break_on_newline in [false, true] {
                        for max_rows in [0, 1, 3, usize::MAX] {
                            let mut job = LayoutJob::default();
                            job.wrap.max_width = width;
                            job.wrap.max_rows = max_rows;
                            job.break_on_newline = break_on_newline;
                            job.halign = Align::RIGHT;
                            job.justify = width.is_finite();
                            job.first_row_min_height = 25.5;
                            for (text, font_id) in [
                                (
                                    "AVATAR To Wa fi\t\r\n".repeat(150),
                                    FontId::proportional(17.25),
                                ),
                                (
                                    "αe\u{301}🦀שלום مرحبا\u{10ffff}\n".repeat(80),
                                    FontId::monospace(14.0),
                                ),
                                ("tail AV\n\n".into(), FontId::proportional(23.0)),
                            ] {
                                direct.font(&font_id);
                                reference.font(&font_id).use_hash_glyph_cache();
                                job.append(
                                    &text,
                                    2.25,
                                    TextFormat {
                                        font_id,
                                        extra_letter_spacing: 0.25,
                                        line_height: Some(29.25),
                                        italics: true,
                                        underline: Stroke::new(1.0_f32, Color32::GREEN),
                                        ..Default::default()
                                    },
                                );
                            }
                            let job = Arc::new(job);
                            assert_eq!(
                                layout(&mut direct, job.clone()),
                                layout(&mut reference, job)
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    #[ignore = "manual optimized paired profile of hash versus direct ASCII glyph lookup"]
    fn profile_glyph_lookup() {
        for (label, source, width, font_id) in [
            (
                "short label",
                "AVATAR To file.rs".into(),
                f32::INFINITY,
                FontId::proportional(14.0),
            ),
            (
                "165k unwrapped",
                "abcdefghij ".repeat(15_000),
                f32::INFINITY,
                FontId::monospace(14.0),
            ),
            (
                "1M unwrapped",
                "abcdefghij".repeat(100_000),
                f32::INFINITY,
                FontId::monospace(14.0),
            ),
            (
                "165k wrapped",
                "abcdefghij ".repeat(15_000),
                900.0,
                FontId::monospace(14.0),
            ),
            (
                "mixed Unicode",
                "AV α🦀 e\u{301} שלום مرحبا\t\r\n".repeat(5000),
                900.0,
                FontId::proportional(14.0),
            ),
            (
                "Unicode only",
                "αβγδεζηθ🦀שלוםمرحبا".repeat(10_000),
                900.0,
                FontId::proportional(14.0),
            ),
        ] {
            let mut direct = FontsImpl::new(1.0, 4096, FontDefinitions::default());
            let mut reference = FontsImpl::new(1.0, 4096, FontDefinitions::default());
            reference.font(&font_id).use_hash_glyph_cache();
            let mut job = LayoutJob::single_section(
                source,
                TextFormat {
                    font_id,
                    ..Default::default()
                },
            );
            job.wrap.max_width = width;
            let warm = Arc::new(job.clone());
            std::hint::black_box(layout(&mut direct, warm.clone()));
            std::hint::black_box(layout(&mut reference, warm));
            let mut samples = [Vec::new(), Vec::new()];
            for n in 0..20 {
                job.text.push('x');
                job.sections[0].byte_range.end = job.text.len();
                let job = Arc::new(job.clone());
                for fast in if n % 2 == 0 {
                    [false, true]
                } else {
                    [true, false]
                } {
                    let fonts = if fast { &mut direct } else { &mut reference };
                    let start = std::time::Instant::now();
                    let g = layout(fonts, job.clone());
                    samples[usize::from(fast)].push(start.elapsed().as_secs_f64() * 1000.0);
                    std::hint::black_box(g);
                }
            }
            for sample in &mut samples {
                sample.sort_by(f64::total_cmp);
            }
            eprintln!(
                "Glyph lookup {label}: hash p50={:.4} p95={:.4}ms; direct p50={:.4} p95={:.4}ms",
                samples[0][10], samples[0][18], samples[1][10], samples[1][18]
            );
        }
    }

    #[test]
    #[ignore = "manual optimized paired profile of original growth versus paragraph reservation"]
    fn profile_paragraph_allocation() {
        for (label, source, width) in [
            (
                "165k unwrapped",
                "abcdefghij ".repeat(15_000),
                f32::INFINITY,
            ),
            ("1M unwrapped", "abcdefghij".repeat(100_000), f32::INFINITY),
            ("165k wrapped", "abcdefghij ".repeat(15_000), 900.0),
        ] {
            let mut fonts = FontsImpl::new(1.0, 4096, FontDefinitions::default());
            let mut job = LayoutJob::single_section(
                source,
                TextFormat {
                    font_id: FontId::monospace(14.0),
                    ..Default::default()
                },
            );
            job.wrap.max_width = width;
            std::hint::black_box(layout_with_capacities(
                &mut fonts,
                Arc::new(job.clone()),
                &[],
            ));
            let mut original_times = Vec::new();
            let mut planned_times = Vec::new();
            let mut bytes = [0; 2];
            for n in 0..8 {
                job.text.push('x');
                job.sections[0].byte_range.end = job.text.len();
                let job = Arc::new(job.clone());
                for planned in if n % 2 == 0 {
                    [false, true]
                } else {
                    [true, false]
                } {
                    let start = std::time::Instant::now();
                    let g = if planned {
                        layout(&mut fonts, job.clone())
                    } else {
                        layout_with_capacities(&mut fonts, job.clone(), &[])
                    };
                    let ms = start.elapsed().as_secs_f64() * 1000.0;
                    if planned {
                        planned_times.push(ms);
                    } else {
                        original_times.push(ms);
                    }
                    bytes[usize::from(planned)] = g
                        .rows
                        .iter()
                        .map(|r| r.glyphs.capacity() * std::mem::size_of::<Glyph>())
                        .sum::<usize>();
                    std::hint::black_box(g);
                }
            }
            original_times.sort_by(f64::total_cmp);
            planned_times.sort_by(f64::total_cmp);
            eprintln!("Paragraph {label}: original p50={:.3} p95={:.3}ms capacity={}B; planned p50={:.3} p95={:.3}ms capacity={}B", original_times[4], original_times[7], bytes[0], planned_times[4], planned_times[7], bytes[1]);
        }
    }

    #[test]
    fn planned_paragraph_storage_matches_original_geometry_and_wrapping() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let mut original_fonts = FontsImpl::new(scale, 4096, FontDefinitions::default());
            let mut planned_fonts = FontsImpl::new(scale, 4096, FontDefinitions::default());
            for width in [0.0, 87.5, f32::INFINITY] {
                for break_on_newline in [false, true] {
                    for max_rows in [0, 1, 3, usize::MAX] {
                        for (align, justify) in [
                            (Align::LEFT, false),
                            (Align::Center, false),
                            (Align::RIGHT, true),
                        ] {
                            let mut job = LayoutJob::default();
                            job.wrap.max_width = width;
                            job.wrap.max_rows = max_rows;
                            job.wrap.break_anywhere = max_rows % 2 == 1;
                            job.break_on_newline = break_on_newline;
                            job.halign = align;
                            job.justify = justify;
                            job.first_row_min_height = 27.25;
                            for (text, size, leading, height) in [
                                ("AVATAR To e\u{301}🦀\r\n".repeat(150), 14.0, 3.25, 18.0),
                                ("مرحبا שלום\t\n\n".repeat(120), 19.0, 0.0, 25.5),
                                (String::new(), 12.0, 5.0, 16.0),
                                ("tail text ".repeat(300), 12.0, 0.0, 16.0),
                            ] {
                                job.append(
                                    &text,
                                    leading,
                                    TextFormat {
                                        font_id: FontId::proportional(size),
                                        line_height: Some(height),
                                        italics: true,
                                        background: Color32::DARK_BLUE,
                                        underline: Stroke::new(1.0_f32, Color32::GREEN),
                                        ..Default::default()
                                    },
                                );
                            }
                            let job = Arc::new(job);
                            let original =
                                layout_with_capacities(&mut original_fonts, job.clone(), &[]);
                            let planned = layout(&mut planned_fonts, job);
                            assert_eq!(planned, original, "reservation must not change glyphs, rows, UVs, colors, kerning or wrapping");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn capacity_planning_counts_formatted_sections_including_gaps_and_overlaps() {
        let mut fonts = FontsImpl::new(1.25, 4096, FontDefinitions::default());
        let mut job = LayoutJob::default();
        job.text = format!("{}A\nB", "unused".repeat(1000));
        let end = job.text.len();
        for range in [end - 3..end, end - 3..end - 2] {
            job.sections.push(LayoutSection {
                leading_space: 0.0,
                byte_range: range,
                format: TextFormat::default(),
            });
        }
        let g = layout(&mut fonts, Arc::new(job));
        assert_eq!(
            g.rows.iter().map(|row| row.text()).collect::<Vec<_>>(),
            vec!["A", "BA"]
        );
        assert!(
            g.rows.iter().map(|r| r.glyphs.capacity()).sum::<usize>() <= 3,
            "unused job text must not reserve glyph storage"
        );
    }

    #[test]
    fn large_unwrapped_paragraphs_do_not_retain_geometric_growth_slack() {
        let mut fonts = FontsImpl::new(1.0, 4096, FontDefinitions::default());
        let mut job = LayoutJob::default();
        for _ in 0..1000 {
            job.append("word ", 0.0, TextFormat::default());
        }
        job.wrap.max_width = f32::INFINITY;
        let g = layout(&mut fonts, Arc::new(job));
        assert_eq!(g.rows.len(), 1);
        assert_eq!(g.rows[0].glyphs.len(), 5000);
        assert_eq!(
            g.rows[0].glyphs.capacity(),
            5000,
            "format changes must not trigger repeated exact reservations"
        );
    }

    #[test]
    fn test_zero_max_width() {
        let mut fonts = FontsImpl::new(1.0, 1024, FontDefinitions::default());
        let mut layout_job = LayoutJob::single_section("W".into(), TextFormat::default());
        layout_job.wrap.max_width = 0.0;
        let galley = layout(&mut fonts, layout_job.into());
        assert_eq!(galley.rows.len(), 1);
    }

    #[test]
    fn test_truncate_with_newline() {
        // No matter where we wrap, we should be appending the newline character.

        let mut fonts = FontsImpl::new(1.0, 1024, FontDefinitions::default());
        let text_format = TextFormat {
            font_id: FontId::monospace(12.0),
            ..Default::default()
        };

        for text in ["Hello\nworld", "\nfoo"] {
            for break_anywhere in [false, true] {
                for max_width in [0.0, 5.0, 10.0, 20.0, f32::INFINITY] {
                    let mut layout_job =
                        LayoutJob::single_section(text.into(), text_format.clone());
                    layout_job.wrap.max_width = max_width;
                    layout_job.wrap.max_rows = 1;
                    layout_job.wrap.break_anywhere = break_anywhere;

                    let galley = layout(&mut fonts, layout_job.into());

                    assert!(galley.elided);
                    assert_eq!(galley.rows.len(), 1);
                    let row_text = galley.rows[0].text();
                    assert!(
                        row_text.ends_with('…'),
                        "Expected row to end with `…`, got {row_text:?} when line-breaking the text {text:?} with max_width {max_width} and break_anywhere {break_anywhere}.",
                    );
                }
            }
        }

        {
            let mut layout_job = LayoutJob::single_section("Hello\nworld".into(), text_format);
            layout_job.wrap.max_width = 50.0;
            layout_job.wrap.max_rows = 1;
            layout_job.wrap.break_anywhere = false;

            let galley = layout(&mut fonts, layout_job.into());

            assert!(galley.elided);
            assert_eq!(galley.rows.len(), 1);
            let row_text = galley.rows[0].text();
            assert_eq!(row_text, "Hello…");
        }
    }

    #[test]
    fn test_cjk() {
        let mut fonts = FontsImpl::new(1.0, 1024, FontDefinitions::default());
        let mut layout_job = LayoutJob::single_section(
            "日本語とEnglishの混在した文章".into(),
            TextFormat::default(),
        );
        layout_job.wrap.max_width = 90.0;
        let galley = layout(&mut fonts, layout_job.into());
        assert_eq!(
            galley.rows.iter().map(|row| row.text()).collect::<Vec<_>>(),
            vec!["日本語と", "Englishの混在", "した文章"]
        );
    }

    #[test]
    fn test_pre_cjk() {
        let mut fonts = FontsImpl::new(1.0, 1024, FontDefinitions::default());
        let mut layout_job = LayoutJob::single_section(
            "日本語とEnglishの混在した文章".into(),
            TextFormat::default(),
        );
        layout_job.wrap.max_width = 110.0;
        let galley = layout(&mut fonts, layout_job.into());
        assert_eq!(
            galley.rows.iter().map(|row| row.text()).collect::<Vec<_>>(),
            vec!["日本語とEnglish", "の混在した文章"]
        );
    }

    #[test]
    fn test_truncate_width() {
        let mut fonts = FontsImpl::new(1.0, 1024, FontDefinitions::default());
        let mut layout_job =
            LayoutJob::single_section("# DNA\nMore text".into(), TextFormat::default());
        layout_job.wrap.max_width = f32::INFINITY;
        layout_job.wrap.max_rows = 1;
        layout_job.round_output_to_gui = false;
        let galley = layout(&mut fonts, layout_job.into());
        assert!(galley.elided);
        assert_eq!(
            galley.rows.iter().map(|row| row.text()).collect::<Vec<_>>(),
            vec!["# DNA…"]
        );
        let row = &galley.rows[0];
        assert_eq!(row.rect.max.x, row.glyphs.last().unwrap().max_x());
    }
}
