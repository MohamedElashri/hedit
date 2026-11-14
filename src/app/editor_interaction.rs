//! Preserve a drag anchor when native events are coalesced into one frame.
use super::*;
/// Ordinary multi-frame drags and word/line/multi-cursor gestures stay egui's.
pub(super) fn complete_drag(
    ui: &egui::Ui,
    output: &mut egui::text_edit::TextEditOutput,
    bidi: &bidi_view::BidiRows,
    previous_anchor: usize,
) -> bool {
    let response = &output.response;
    if !response.enabled()
        || !response.sense.senses_drag()
        || response.double_clicked()
        || response.triple_clicked()
        || response.changed()
    {
        return false;
    }
    let gesture = ui.input(|i| {
        let buttons: Vec<_> = i
            .events
            .iter()
            .filter_map(|e| {
                if let egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers,
                } = e
                {
                    Some((*pos, *pressed, *modifiers))
                } else {
                    None
                }
            })
            .collect();
        match buttons.as_slice() {
            [(start, true, modifiers), (end, false, _)]
                if !modifiers.alt && !modifiers.ctrl && !modifiers.command =>
            {
                Some((*start, *end, modifiers.shift))
            }
            _ => None,
        }
    });
    let Some((start, end, shift)) = gesture else {
        return false;
    };
    let threshold = ui.ctx().options(|o| o.input_options.max_click_dist);
    if !start.is_finite()
        || !end.is_finite()
        || start.distance(end) <= threshold
        || !response.interact_rect.contains(start)
        || ui.ctx().layer_id_at(start) != Some(response.layer_id)
    {
        return false;
    }
    let hit = |pos| {
        bidi_view::hit(&output.galley, bidi, pos - output.galley_pos).unwrap_or_else(|| {
            output
                .galley
                .cursor_from_pos(pos - output.galley_pos)
                .ccursor
                .index
        })
    };
    let anchor = if shift { previous_anchor } else { hit(start) };
    let head = hit(end);
    let cursor = |at| output.galley.from_ccursor(egui::text::CCursor::new(at));
    let range = egui::text_selection::CursorRange {
        primary: cursor(head),
        secondary: cursor(anchor),
    };
    output.cursor_range = Some(range);
    output.state.cursor.set_range(Some(range));
    response.request_focus();
    true
}

/// Equivalent to egui's selection rectangles, restricted to visible rows.
/// The cached galley and its glyph meshes remain shared even for Select All.
pub(super) fn selection_rects(
    galley: &egui::Galley,
    range: &egui::text_selection::CursorRange,
    rows: std::ops::Range<usize>,
) -> Vec<egui::Rect> {
    if range.is_empty() {
        return vec![];
    }
    let [min, max] = range.sorted_cursors();
    let (min, max) = (min.rcursor, max.rcursor);
    let end = max
        .row
        .saturating_add(1)
        .min(rows.end)
        .min(galley.rows.len());
    (min.row.max(rows.start)..end)
        .map(|ri| {
            let row = &galley.rows[ri];
            let left = if ri == min.row {
                row.x_offset(min.column)
            } else {
                row.rect.left()
            };
            let right = if ri == max.row {
                row.x_offset(max.column)
            } else {
                row.rect.right()
                    + if row.ends_with_newline {
                        row.height() / 2.0
                    } else {
                        0.0
                    }
            };
            egui::Rect::from_min_max(
                egui::pos2(left, row.min_y()),
                egui::pos2(right, row.max_y()),
            )
        })
        .collect()
}
