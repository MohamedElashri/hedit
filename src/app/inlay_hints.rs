//! Virtual labels reserve layout space without entering the editable text.
use super::*;
use egui::text::{ByteIndex, LayoutJob, LayoutSection};
use hedit::{advanced::Projection, language_server::text::InlayHint};

struct Placed {
    scalar: usize,
    byte: usize,
    trailing: bool,
    label: String,
}

pub(super) struct Display {
    projection: Arc<Projection>,
    hints: Arc<Vec<InlayHint>>,
    placed: Vec<Placed>,
}

impl Display {
    pub(super) fn current(
        &self,
        projection: &Arc<Projection>,
        hints: &Arc<Vec<InlayHint>>,
    ) -> bool {
        Arc::ptr_eq(&self.projection, projection) && Arc::ptr_eq(&self.hints, hints)
    }

    pub(super) fn matches(&self, text: &str) -> bool {
        self.projection.text == text
    }

    pub(super) fn new(
        projection: Arc<Projection>,
        hints: Arc<Vec<InlayHint>>,
        folds: &[hedit::advanced::Fold],
    ) -> Self {
        let mut placed: Vec<Placed> = Vec::new();
        let mut chars = projection.text.char_indices().enumerate().peekable();
        for hint in hints.iter() {
            if folds
                .iter()
                .any(|fold| (fold.start..fold.end).contains(&hint.scalar))
            {
                continue;
            }
            let scalar = projection.display(hint.scalar);
            if let Some(previous) = placed
                .last_mut()
                .filter(|previous| previous.scalar == scalar)
            {
                if previous.label.len() + hint.label.len() < 240 {
                    previous.label.push(' ');
                    previous.label.push_str(&hint.label);
                }
                continue;
            }
            while chars.peek().is_some_and(|(index, _)| *index < scalar) {
                chars.next();
            }
            let (byte, next) = chars
                .peek()
                .map_or((projection.text.len(), '\n'), |(_, pair)| *pair);
            placed.push(Placed {
                scalar,
                byte,
                trailing: matches!(next, '\r' | '\n'),
                label: hint.label.clone(),
            });
        }
        Self {
            projection,
            hints,
            placed,
        }
    }

    fn font(size: f32) -> FontId {
        FontId::monospace((size - 2.0).max(8.0))
    }

    pub(super) fn reserve(&self, ui: &egui::Ui, job: &mut LayoutJob, size: f32) {
        let font = Self::font(size);
        let mut gaps = self.placed.iter().filter(|hint| !hint.trailing).peekable();
        let mut sections = Vec::with_capacity(job.sections.len() + self.placed.len());
        for mut section in std::mem::take(&mut job.sections) {
            while let Some(hint) = gaps.next_if(|hint| hint.byte < section.byte_range.end.0) {
                let at = ByteIndex(hint.byte);
                if at < section.byte_range.start {
                    continue;
                }
                if at > section.byte_range.start {
                    sections.push(LayoutSection {
                        leading_space: section.leading_space,
                        byte_range: section.byte_range.start..at,
                        format: section.format.clone(),
                    });
                    section.byte_range.start = at;
                    section.leading_space = 0.0;
                }
                section.leading_space += ui
                    .painter()
                    .layout_no_wrap(hint.label.clone(), font.clone(), egui::Color32::WHITE)
                    .size()
                    .x
                    + 8.0;
            }
            sections.push(section);
        }
        job.sections = sections;
    }

    pub(super) fn paint(
        &self,
        ui: &egui::Ui,
        output: &egui::text_edit::TextEditOutput,
        bidi: &bidi_view::BidiRows,
        visible: std::ops::Range<usize>,
        size: f32,
        theme: &Theme,
    ) {
        let galley = &output.galley;
        if !self.matches(galley.text()) || visible.is_empty() {
            return;
        }
        let start = galley.row_start(visible.start);
        let end = galley.row_start(visible.end);
        let first = self.placed.partition_point(|hint| hint.scalar < start);
        let painter = ui.painter();
        let font = Self::font(size);
        for hint in self.placed[first..]
            .iter()
            .take_while(|hint| hint.scalar <= end)
        {
            let caret = egui::text::CCursor::new(hint.scalar);
            if bidi.row(galley.layout_from_cursor(caret).row).is_some() {
                continue;
            }
            let label =
                painter.layout_no_wrap(hint.label.clone(), font.clone(), theme.muted.color());
            let label_size = label.size();
            let pos = if hint.trailing {
                let caret = galley.pos_from_cursor(caret);
                egui::pos2(caret.left() + 4.0, caret.center().y - label_size.y * 0.5)
            } else if let Some(glyph) = bidi_view::glyph_rect(galley, hint.scalar) {
                let x = glyph.left() - label_size.x - 4.0;
                // A wrap may move the following glyph to the start of a new row.
                if x < 0.0 {
                    continue;
                }
                egui::pos2(x, glyph.center().y - label_size.y * 0.5)
            } else {
                continue;
            } + output.galley_pos.to_vec2();
            painter.rect_filled(
                egui::Rect::from_min_size(pos, label_size).expand2(egui::vec2(2.0, 0.0)),
                2.0,
                theme.panel.color(),
            );
            painter.galley(pos, label, theme.muted.color());
        }
    }
}

#[cfg(test)]
#[test]
fn inlay_hint_gaps_preserve_the_original_editable_text() {
    let text = "draw(value)";
    let projection = Arc::new(Projection::new(text, 4, &[]));
    let display = Display::new(
        projection,
        Arc::new(vec![InlayHint {
            scalar: 5,
            label: "color:".into(),
        }]),
        &[],
    );
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let font = FontId::monospace(14.0);
        let mut job = LayoutJob::simple(text.into(), font, egui::Color32::WHITE, f32::INFINITY);
        let before = ui.fonts_mut(|fonts| fonts.layout_job(job.clone()));
        display.reserve(ui, &mut job, 14.0);
        let after = ui.fonts_mut(|fonts| fonts.layout_job(job));
        assert_eq!(after.text(), text);
        let previous = bidi_view::glyph_rect(&before, 5).unwrap();
        let next = bidi_view::glyph_rect(&after, 5).unwrap();
        assert!(next.left() > previous.left() + 20.0);
    });
    output.textures_delta.clear();
}
