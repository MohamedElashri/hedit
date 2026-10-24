//! Draw right-to-left text in a laid-out galley: joined Arabic letters and
//! bidirectional reordering, with carets, selections, and clicks to match.
//!
//! egui lays text out left to right, one glyph per character. The text stays
//! in logical order, so editing, copying, and searching are unchanged; only
//! the glyphs of lines with right-to-left text move to their visual places.
//! Rows of those lines then need these helpers for anything positional,
//! because egui assumes glyph x positions grow with the character index.
use eframe::egui::{
    self,
    epaint::text::{Fonts, Row},
    text::Galley,
};
use hedit::bidi;
use std::{ops::Range, sync::Arc};

/// The drawing direction of each glyph in a row whose line has
/// right-to-left text.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct RowDirs {
    pub rtl: Vec<bool>,
    /// The line reads right to left overall.
    pub base_rtl: bool,
}

/// Directions for the rows of one galley; empty when it has no
/// right-to-left text.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct BidiRows {
    rows: Vec<Option<RowDirs>>,
}

impl BidiRows {
    pub(super) fn any(&self) -> bool {
        !self.rows.is_empty()
    }
    pub(super) fn row(&self, row: usize) -> Option<&RowDirs> {
        self.rows.get(row).and_then(Option::as_ref)
    }
}

/// The rows of each line (paragraph) of `galley`.
fn lines(galley: &Galley) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (i, row) in galley.rows.iter().enumerate() {
        if row.ends_with_newline || i + 1 == galley.rows.len() {
            lines.push(start..i + 1);
            start = i + 1;
        }
    }
    lines
}

/// How each line's base direction is chosen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Base {
    /// From the line's first strong letter, for prose.
    Auto,
    /// Always left to right, for code and the interface.
    Ltr,
}

impl Base {
    /// Prose formats read each line in its own direction; code reads left to
    /// right, so a comment in Arabic keeps `//` at its start.
    pub(super) fn for_language(language: hedit::syntax::Language) -> Self {
        use hedit::syntax::Language;
        match language {
            Language::PlainText | Language::Markdown | Language::Latex => Self::Auto,
            _ => Self::Ltr,
        }
    }
}

/// Join and reorder the right-to-left lines of `galley`, returning it (a copy
/// when anything changed) and the directions of its rows.
pub(super) fn apply(fonts: &Fonts, galley: Arc<Galley>, base: Base) -> (Arc<Galley>, BidiRows) {
    if galley.elided || !bidi::has_rtl(galley.text()) {
        return (galley, BidiRows::default());
    }
    let mut galley = (*galley).clone();
    let mut dirs = vec![None; galley.rows.len()];
    let pixels_per_point = galley.pixels_per_point;
    for rows in lines(&galley) {
        let chars: Vec<char> = galley.rows[rows.clone()]
            .iter()
            .flat_map(|row| row.glyphs.iter().map(|g| g.chr))
            .collect();
        let text: String = chars.iter().collect();
        if !bidi::has_rtl(&text) {
            continue;
        }
        let line = bidi::Line::with_base(&text, (base == Base::Ltr).then_some(false));
        let shaped = bidi::shape(&chars);
        let mut start = 0;
        for r in rows {
            let row = &mut galley.rows[r];
            let count = row.glyphs.len();
            let rtl = line.row_rtl(start, start + count);
            let quads: Vec<bool> = row.glyphs.iter().map(|g| !g.uv_rect.is_nothing()).collect();
            let hidden: Vec<bool> = (0..count).map(|i| shaped[start + i].hidden).collect();
            substitute(fonts, &galley.job, row, &shaped[start..start + count], &rtl);
            for (glyph, hidden) in row.glyphs.iter_mut().zip(&hidden) {
                if *hidden {
                    glyph.advance_width = 0.0;
                }
            }
            let mut x = row.rect.min.x;
            for i in line.visual(start, start + count) {
                let glyph = &mut row.glyphs[i - start];
                glyph.pos.x = x;
                x += glyph.advance_width;
            }
            row.rect.max.x = x.max(row.rect.min.x);
            retessellate(row, &galley.job, &quads, &hidden, pixels_per_point);
            dirs[r] = Some(RowDirs {
                rtl,
                base_rtl: line.base_rtl(),
            });
            start += count;
        }
    }
    galley.mesh_bounds = galley.rows.iter().fold(egui::Rect::NOTHING, |bounds, row| {
        bounds.union(row.visuals.mesh_bounds)
    });
    let right = galley
        .rows
        .iter()
        .map(|row| row.rect.max.x)
        .fold(galley.rect.max.x, f32::max);
    galley.rect.max.x = right;
    (Arc::new(galley), BidiRows { rows: dirs })
}

/// Swap glyphs for their joined forms, and mirror brackets in right-to-left
/// runs, when the font has the replacement.
fn substitute(
    fonts: &Fonts,
    job: &egui::text::LayoutJob,
    row: &mut Row,
    shaped: &[bidi::Shaped],
    rtl: &[bool],
) {
    let mut wanted: Vec<(egui::FontId, String, Vec<usize>)> = Vec::new();
    for (i, glyph) in row.glyphs.iter().enumerate() {
        let shape = shaped[i];
        let target = if rtl[i] {
            bidi::mirror(shape.glyph).unwrap_or(shape.glyph)
        } else {
            shape.glyph
        };
        if shape.hidden || target == glyph.chr {
            continue;
        }
        let font = &job.sections[glyph.section_index as usize].format.font_id;
        if !fonts.has_glyph(font, target) {
            continue;
        }
        match wanted.iter_mut().find(|(f, _, _)| f == font) {
            Some((_, text, indices)) => {
                text.push(target);
                indices.push(i);
            }
            None => wanted.push((font.clone(), target.to_string(), vec![i])),
        }
    }
    for (font, text, indices) in wanted {
        let laid = fonts.layout_no_wrap(text, font, egui::Color32::WHITE);
        let Some(source) = laid.rows.first() else {
            continue;
        };
        for (glyph, &i) in source.glyphs.iter().zip(&indices) {
            row.glyphs[i].uv_rect = glyph.uv_rect;
            row.glyphs[i].advance_width = glyph.advance_width;
        }
    }
}

/// Move each glyph's quad in the row's mesh to its new place and texture.
/// Glyphs keep the vertices egui gave them, four per drawn glyph in order.
fn retessellate(
    row: &mut Row,
    job: &egui::text::LayoutJob,
    quads: &[bool],
    hidden: &[bool],
    pixels_per_point: f32,
) {
    let round = |v: f32| (v * pixels_per_point).round() / pixels_per_point;
    let mesh = &mut row.visuals.mesh;
    let mut vertex = row.visuals.glyph_vertex_range.start;
    for (i, glyph) in row.glyphs.iter().enumerate() {
        if !quads[i] {
            continue;
        }
        let Some(vertices) = mesh.vertices.get_mut(vertex..vertex + 4) else {
            break;
        };
        vertex += 4;
        if hidden[i] || glyph.uv_rect.is_nothing() {
            let at = vertices[0].pos;
            for v in vertices.iter_mut() {
                v.pos = at;
            }
            continue;
        }
        let uv = glyph.uv_rect;
        let mut left_top = glyph.pos + uv.offset;
        left_top.x = round(left_top.x);
        left_top.y = round(left_top.y);
        let rect = egui::Rect::from_min_size(left_top, uv.size);
        let texture = egui::Rect::from_min_max(
            egui::pos2(uv.min[0] as f32, uv.min[1] as f32),
            egui::pos2(uv.max[0] as f32, uv.max[1] as f32),
        );
        let slant = if job.sections[glyph.section_index as usize].format.italics {
            rect.height() * 0.25 * egui::Vec2::X
        } else {
            egui::Vec2::ZERO
        };
        let corners = [
            (rect.left_top() + slant, texture.left_top()),
            (rect.right_top() + slant, texture.right_top()),
            (rect.left_bottom(), texture.left_bottom()),
            (rect.right_bottom(), texture.right_bottom()),
        ];
        for (v, (pos, uv)) in vertices.iter_mut().zip(corners) {
            v.pos = pos;
            v.uv = uv;
        }
    }
    row.visuals.mesh_bounds = mesh.calc_bounds();
}

/// The x of a caret before character `column` of `row`, relative to the
/// galley. In right-to-left runs the caret sits on the glyph's right edge.
pub(super) fn caret_x(row: &Row, dirs: Option<&RowDirs>, column: usize) -> f32 {
    let Some(dirs) = dirs else {
        return row.x_offset(column);
    };
    match (column.min(row.glyphs.len()), row.glyphs.is_empty()) {
        (_, true) => row.rect.min.x,
        (0, false) => leading(row, dirs, 0),
        // Follow the character before the caret, the one just typed.
        (c, false) => trailing(row, dirs, c - 1),
    }
}

/// Where text read in the glyph's direction ends: right edge when left to right.
fn trailing(row: &Row, dirs: &RowDirs, i: usize) -> f32 {
    let glyph = &row.glyphs[i];
    glyph.pos.x
        + if dirs.rtl[i] {
            0.0
        } else {
            glyph.advance_width
        }
}

/// Where text read in the glyph's direction starts.
fn leading(row: &Row, dirs: &RowDirs, i: usize) -> f32 {
    let glyph = &row.glyphs[i];
    glyph.pos.x
        + if dirs.rtl[i] {
            glyph.advance_width
        } else {
            0.0
        }
}

/// Where a caret before `column` also belongs when the characters on either
/// side run in different directions: the start of the next character. GTK
/// shows this second place as a half-height caret.
pub(super) fn split_caret_x(row: &Row, dirs: Option<&RowDirs>, column: usize) -> Option<f32> {
    let dirs = dirs?;
    if column == 0 || column >= row.glyphs.len() {
        return None;
    }
    let other = leading(row, dirs, column);
    ((other - caret_x(row, Some(dirs), column)).abs() > 0.5).then_some(other)
}

/// Every screen position of each caret in `row`, as (column, x).
fn caret_places(row: &Row, dirs: &RowDirs) -> Vec<(usize, f32)> {
    (0..=row.glyphs.len())
        .flat_map(|c| {
            std::iter::once((c, caret_x(row, Some(dirs), c)))
                .chain(split_caret_x(row, Some(dirs), c).map(|x| (c, x)))
        })
        .collect()
}

/// The row of `galley` holding character `index`, and the column within it.
fn locate(galley: &Galley, index: usize) -> (usize, usize) {
    let cursor = galley.from_ccursor(egui::text::CCursor::new(index));
    (cursor.rcursor.row, cursor.rcursor.column)
}

/// The character index where `row` starts.
fn row_start(galley: &Galley, row: usize) -> usize {
    galley.rows[..row]
        .iter()
        .map(Row::char_count_including_newline)
        .sum()
}

/// The caret before character `index`, relative to the galley.
pub(super) fn caret_rect(galley: &Galley, bidi: &BidiRows, index: usize) -> egui::Rect {
    let (row, column) = locate(galley, index);
    let Some(dirs) = bidi.row(row) else {
        let cursor = galley.from_ccursor(egui::text::CCursor::new(index));
        return galley.pos_from_cursor(&cursor);
    };
    let r = &galley.rows[row];
    let x = caret_x(r, Some(dirs), column);
    egui::Rect::from_min_max(egui::pos2(x, r.min_y()), egui::pos2(x, r.max_y()))
}

/// Horizontal spans covering columns `start..end` of `row`: one for
/// left-to-right rows, and one per visual run otherwise.
pub(super) fn spans(
    row: &Row,
    dirs: Option<&RowDirs>,
    start: usize,
    end: usize,
) -> Vec<(f32, f32)> {
    let end = end.min(row.glyphs.len());
    if dirs.is_none() || start >= end {
        return vec![(row.x_offset(start), row.x_offset(end))];
    }
    let mut parts: Vec<(f32, f32)> = row.glyphs[start..end]
        .iter()
        .map(|g| (g.pos.x, g.pos.x + g.advance_width))
        .collect();
    parts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f32, f32)> = Vec::new();
    for (a, b) in parts {
        match merged.last_mut() {
            Some(last) if a <= last.1 + 0.5 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    merged
}

/// Rectangles, relative to the galley, covering characters `start..end`.
pub(super) fn range_rects(
    galley: &Galley,
    bidi: &BidiRows,
    start: usize,
    end: usize,
) -> Vec<egui::Rect> {
    if start >= end {
        return Vec::new();
    }
    let (first, first_column) = locate(galley, start);
    let (last, last_column) = locate(galley, end);
    let mut rects = Vec::new();
    for row in first..=last.min(galley.rows.len().saturating_sub(1)) {
        let r = &galley.rows[row];
        let a = if row == first { first_column } else { 0 };
        let b = if row == last {
            last_column
        } else {
            r.glyphs.len()
        };
        for (x0, x1) in spans(r, bidi.row(row), a, b) {
            // A selected line break shows as a sliver past the row's end.
            let x1 = if row != last && x1 <= x0 {
                x0 + 4.0
            } else {
                x1
            };
            rects.push(egui::Rect::from_min_max(
                egui::pos2(x0, r.min_y()),
                egui::pos2(x1, r.max_y()),
            ));
        }
    }
    rects
}

/// The glyph of character `index`, relative to the galley.
pub(super) fn glyph_rect(galley: &Galley, index: usize) -> Option<egui::Rect> {
    let (row, column) = locate(galley, index);
    let r = galley.rows.get(row)?;
    let glyph = r.glyphs.get(column)?;
    Some(egui::Rect::from_min_max(
        egui::pos2(glyph.pos.x, r.min_y()),
        egui::pos2(glyph.pos.x + glyph.advance_width, r.max_y()),
    ))
}

/// The row at height `y`, or the nearest one.
fn row_at(galley: &Galley, y: f32) -> usize {
    galley
        .rows
        .iter()
        .position(|r| y < r.max_y())
        .unwrap_or(galley.rows.len().saturating_sub(1))
}

/// The character index of the caret nearest `pos`, relative to the galley, or
/// `None` when that row needs no special handling.
pub(super) fn hit(galley: &Galley, bidi: &BidiRows, pos: egui::Vec2) -> Option<usize> {
    let row = row_at(galley, pos.y);
    let dirs = bidi.row(row)?;
    let r = &galley.rows[row];
    let column = caret_places(r, dirs)
        .into_iter()
        .min_by(|a, b| (a.1 - pos.x).abs().total_cmp(&(b.1 - pos.x).abs()))
        .map_or(0, |(c, _)| c);
    Some(row_start(galley, row) + column)
}

/// Where the caret at `index` goes with Left or Right in a row with
/// right-to-left text, moving by what is on screen. `None` leaves the key to
/// the usual logical movement.
pub(super) fn visual_step(
    galley: &Galley,
    bidi: &BidiRows,
    index: usize,
    right: bool,
) -> Option<usize> {
    let (row, column) = locate(galley, index);
    let dirs = bidi.row(row)?;
    let r = &galley.rows[row];
    let here = caret_x(r, Some(dirs), column);
    let candidates = caret_places(r, dirs).into_iter().filter_map(|(c, x)| {
        let ahead = if right {
            x > here + 0.5
        } else {
            x < here - 0.5
        };
        (c != column && ahead).then_some((c, (x - here).abs()))
    });
    let start = row_start(galley, row);
    match candidates.min_by(|a, b| a.1.total_cmp(&b.1)) {
        Some((c, _)) => Some(start + c),
        // At the edge of a right-to-left line, Right leaves through its start
        // and Left through its end; left-to-right lines move as usual.
        None if dirs.base_rtl && right => Some(start.saturating_sub(1)),
        None if dirs.base_rtl => {
            let end = start + r.char_count_including_newline();
            Some(end.min(galley.text().chars().count()))
        }
        None => None,
    }
}

/// How the editor draws its caret and selection.
pub(super) struct Look {
    pub caret: egui::Stroke,
    pub blink: bool,
    pub selection: egui::Color32,
}

/// After the editor ran, for a galley with right-to-left rows: correct where
/// clicks put the caret, then draw the selection (into `background`, below
/// the text) and the caret, which egui would place by character index.
pub(super) fn interact(
    ui: &egui::Ui,
    output: &mut egui::text_edit::TextEditOutput,
    bidi: &BidiRows,
    background: egui::layers::ShapeIdx,
    look: Look,
) {
    let galley = Arc::clone(&output.galley);
    let origin = output.galley_pos;
    let response = &output.response;
    let ctx = ui.ctx();
    // Word and line selection by double and triple click stay egui's.
    let pointer = (response.is_pointer_button_down_on() || response.clicked())
        && !response.double_clicked()
        && !response.triple_clicked();
    if let (true, Some(at), Some(range)) = (
        pointer,
        response.interact_pointer_pos(),
        output.cursor_range,
    ) {
        if let Some(index) = hit(&galley, bidi, at - origin) {
            let (pressed, shift) = ctx.input(|i| (i.pointer.primary_pressed(), i.modifiers.shift));
            let anchor = if pressed && !shift {
                index
            } else {
                range.secondary.ccursor.index
            };
            let cursor = |i| galley.from_ccursor(egui::text::CCursor::new(i));
            let range = egui::text_selection::CursorRange {
                primary: cursor(index),
                secondary: cursor(anchor),
            };
            output.cursor_range = Some(range);
            output.state.cursor.set_range(Some(range));
        }
    }
    if !output.response.has_focus() {
        return;
    }
    let Some(range) = output.cursor_range else {
        return;
    };
    let (head, anchor) = (range.primary.ccursor.index, range.secondary.ccursor.index);
    let rects = range_rects(&galley, bidi, head.min(anchor), head.max(anchor));
    ui.painter().set(
        background,
        egui::Shape::Vec(
            rects
                .into_iter()
                .map(|r| {
                    egui::Shape::rect_filled(r.translate(origin.to_vec2()), 0.0, look.selection)
                })
                .collect(),
        ),
    );
    if look.blink {
        // Half a second on, half off, like egui's own caret.
        let time = ctx.input(|i| i.time);
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(0.5 - time % 0.5));
        if time % 1.0 >= 0.5 {
            return;
        }
    }
    let caret = caret_rect(&galley, bidi, head).translate(origin.to_vec2());
    ui.painter()
        .vline(caret.left(), caret.y_range(), look.caret);
    let (row, column) = locate(&galley, head);
    if let Some(x) = split_caret_x(&galley.rows[row], bidi.row(row), column) {
        let x = origin.x + x;
        let half = egui::Rangef::new(caret.center().y, caret.bottom());
        ui.painter().vline(
            x,
            half,
            egui::Stroke::new(look.caret.width * 0.6, look.caret.color),
        );
    }
}

/// A single-line text field that draws right-to-left text joined and in
/// visual order, with the caret, selection, and clicks to match. `build`
/// configures the field (id, width, hint), as with `TextEdit::singleline`.
pub(super) fn field(
    ui: &mut egui::Ui,
    text: &mut String,
    build: impl FnOnce(egui::TextEdit<'_>) -> egui::TextEdit<'_>,
) -> egui::Response {
    let rtl = bidi::has_rtl(text);
    let latest = std::cell::RefCell::new(BidiRows::default());
    let mut layouter = |ui: &egui::Ui, text: &str, wrap_width: f32| {
        let font = egui::TextStyle::Body.resolve(ui.style());
        let color = ui
            .visuals()
            .override_text_color
            .unwrap_or_else(|| ui.visuals().widgets.inactive.text_color());
        let job = egui::text::LayoutJob::simple(text.to_owned(), font, color, wrap_width);
        let (galley, bidi) = ui.fonts(|f| apply(f, f.layout_job(job), Base::Ltr));
        *latest.borrow_mut() = bidi;
        galley
    };
    ui.scope(|ui| {
        let look = Look {
            caret: ui.visuals().text_cursor.stroke,
            blink: ui.visuals().text_cursor.blink,
            selection: ui.visuals().selection.bg_fill,
        };
        if rtl {
            ui.visuals_mut().text_cursor.stroke.color = egui::Color32::TRANSPARENT;
            ui.visuals_mut().selection.bg_fill = egui::Color32::TRANSPARENT;
        }
        let background = ui.painter().add(egui::Shape::Noop);
        let mut output = build(egui::TextEdit::singleline(text))
            .layouter(&mut layouter)
            .show(ui);
        let bidi = latest.take();
        if rtl && bidi.any() {
            interact(ui, &mut output, &bidi, background, look);
        }
        output.response
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;
    use hedit::syntax::{self, Language};

    /// Lay out `text` in the editor's monospace font with system fallbacks.
    fn laid_out(text: &str) -> (egui::Context, Arc<Galley>, BidiRows) {
        let ctx = egui::Context::default();
        super::super::settings_store::fallback_fonts();
        super::super::settings_store::set_editor_font(&ctx, None);
        let mut out = None;
        // The first frame installs the fonts; the second lays out with them.
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let theme = hedit::theme::builtins()[0].clone();
                    let job = syntax::highlight_language(
                        text,
                        18.0,
                        f32::INFINITY,
                        Language::PlainText,
                        &theme,
                    );
                    out = Some(ui.fonts(|f| apply(f, f.layout_job(job), Base::Auto)));
                });
            });
        }
        let (galley, bidi) = out.unwrap();
        (ctx, galley, bidi)
    }

    #[test]
    fn plain_text_is_left_alone() {
        let (_, galley, bidi) = laid_out("fn main() {}\nlet x = 1;");
        assert!(!bidi.any());
        let caret = caret_rect(&galley, &bidi, 3);
        assert_eq!(caret.left(), galley.rows[0].x_offset(3));
    }

    #[test]
    fn arabic_runs_read_right_to_left_with_carets_on_the_glyph_edges() {
        let (_, galley, bidi) = laid_out("ab سلام cd");
        assert!(bidi.any());
        let row = &galley.rows[0];
        let dirs = bidi.row(0).unwrap();
        assert!(!dirs.base_rtl);
        // س (index 3) is drawn to the right of م (index 6).
        assert!(row.glyphs[3].pos.x > row.glyphs[6].pos.x);
        // The Arabic word stays between "ab " and " cd".
        assert!(row.glyphs[6].pos.x >= row.glyphs[2].pos.x);
        assert!(row.glyphs[7].pos.x >= row.glyphs[3].pos.x);
        // The caret after س sits on its left edge; before it, on its right edge.
        assert_eq!(caret_x(row, Some(dirs), 4), row.glyphs[3].pos.x);
        let after_space = caret_x(row, Some(dirs), 3);
        assert_eq!(
            after_space,
            row.glyphs[2].pos.x + row.glyphs[2].advance_width
        );
        // Clicking just left of س's right edge puts the caret before it.
        let hit_x = row.glyphs[3].pos.x + row.glyphs[3].advance_width - 1.0;
        assert_eq!(
            hit(&galley, &bidi, egui::vec2(hit_x, row.rect.center().y)),
            Some(3)
        );
        // Right from after س (its left edge) goes to before it (its right edge).
        let next = visual_step(&galley, &bidi, 4, true).unwrap();
        assert_eq!(next, 3);
        // A selection of the whole word is one span.
        assert_eq!(range_rects(&galley, &bidi, 3, 7).len(), 1);
        // Selecting "b س" crosses a direction change: two spans.
        assert_eq!(range_rects(&galley, &bidi, 1, 4).len(), 2);
    }

    /// Paint the galley's glyph quads from the font atlas into a PNG, to see
    /// exactly what egui will draw.
    #[test]
    #[ignore = "writes an RTL rendering to HEDIT_PREVIEW_DIR"]
    fn export_rtl_rendering() {
        let output = std::path::PathBuf::from(
            std::env::var_os("HEDIT_PREVIEW_DIR").expect("set HEDIT_PREVIEW_DIR"),
        );
        let text = "مرحبا بالعالم\nسلام عليكم ورحمة الله\nEnglish then عربي (مع أقواس) and 123 numbers\nعدد 42 hello world.\nلا إله، فارسی: پگچژ\nשלום עולם\n你好世界 こんにちは\nplain line";
        let (ctx, galley, _) = laid_out(text);
        let atlas = ctx.fonts(|f| f.image());
        let (width, height) = (900usize, (galley.rect.height() as usize) + 20);
        let mut canvas = vec![255u8; width * height * 3];
        for row in &galley.rows {
            let mesh = &row.visuals.mesh;
            for quad in mesh.vertices[row.visuals.glyph_vertex_range.clone()].chunks(4) {
                let (lt, rb) = (quad[0], quad[3]);
                let (w, h) = ((rb.pos.x - lt.pos.x) as i64, (rb.pos.y - lt.pos.y) as i64);
                for dy in 0..h.max(0) {
                    for dx in 0..w.max(0) {
                        let u = lt.uv.x as usize + dx as usize;
                        let v = lt.uv.y as usize + dy as usize;
                        let coverage = atlas.pixels[v * atlas.size[0] + u];
                        let (x, y) = (lt.pos.x as i64 + dx + 10, lt.pos.y as i64 + dy + 10);
                        if x < 0 || y < 0 || x as usize >= width || y as usize >= height {
                            continue;
                        }
                        let p = (y as usize * width + x as usize) * 3;
                        for c in 0..3 {
                            let value = canvas[p + c] as f32 * (1.0 - coverage);
                            canvas[p + c] = value as u8;
                        }
                    }
                }
            }
        }
        let mut ppm = format!("P6 {width} {height} 255\n").into_bytes();
        ppm.extend(canvas);
        std::fs::write(output.join("rtl.ppm"), ppm).unwrap();
    }
}
