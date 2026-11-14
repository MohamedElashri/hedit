//! Logical offsets for an immutable galley, with logarithmic viewport lookup.
//! Weak identity avoids retaining layouts outside the editor's layout budget.
use super::*;
use egui::text::Galley;
use std::{ops::Range, sync::Weak};

#[derive(Clone, Copy)]
pub(super) struct Row {
    pub start: usize,
    pub line: usize,
    pub first: bool,
}
pub(super) struct Rows {
    pub rows: Vec<Row>,
    pub chars: usize,
}
impl Rows {
    fn new(galley: &Galley) -> Self {
        let mut rows = Vec::with_capacity(galley.rows.len());
        let (mut start, mut line, mut first) = (0, 0, true);
        for row in &galley.rows {
            rows.push(Row { start, line, first });
            start += row.char_count_including_newline();
            if row.ends_with_newline {
                line += 1;
            }
            first = row.ends_with_newline;
        }
        Self { rows, chars: start }
    }
    pub fn visible(&self, galley: &Galley, origin: egui::Pos2, clip: egui::Rect) -> Range<usize> {
        if !clip.is_positive() {
            return 0..0;
        }
        let start = galley
            .rows
            .partition_point(|r| origin.y + r.rect.bottom() < clip.top());
        let end = galley
            .rows
            .partition_point(|r| origin.y + r.rect.top() <= clip.bottom());
        start..end.max(start)
    }
    pub fn end(&self, row: usize) -> usize {
        self.rows.get(row + 1).map_or(self.chars, |r| r.start)
    }
}
/// Seek across hidden lines without materializing every source character. Within
/// a long logical line the iterator advances once, even across many fold markers.
pub(super) struct SourceChars<'a> {
    text: &'a str,
    index: &'a editor_layout::text_index::TextIndex,
    chars: std::str::Chars<'a>,
    at: usize,
    last: Option<(usize, Option<char>)>,
}
impl<'a> SourceChars<'a> {
    pub fn new(text: &'a str, index: &'a editor_layout::text_index::TextIndex) -> Self {
        Self {
            text,
            index,
            chars: text.chars(),
            at: 0,
            last: None,
        }
    }
    pub fn get(&mut self, position: usize) -> Option<char> {
        if let Some((_, value)) = self.last.filter(|(at, _)| *at == position) {
            return value;
        }
        let (byte, chars) = self.index.checkpoint(position);
        if chars >= self.at || position < self.at {
            self.chars = self.text[byte..].chars();
            self.at = chars;
        }
        let value = self.chars.nth(position.saturating_sub(self.at));
        self.at = position.saturating_add(1);
        self.last = Some((position, value));
        value
    }
}
#[derive(Default)]
pub(super) struct Cache {
    galley: Weak<Galley>,
    index: Option<Arc<Rows>>,
}
impl Cache {
    pub fn get(&mut self, galley: &Arc<Galley>) -> Arc<Rows> {
        if self
            .galley
            .upgrade()
            .is_none_or(|cached| !Arc::ptr_eq(&cached, galley))
        {
            self.galley = Arc::downgrade(galley);
            self.index = Some(Arc::new(Rows::new(galley)));
        }
        self.index.as_ref().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(in crate::app) fn layout(text: &str, width: f32, scale: f32) -> Arc<Galley> {
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(scale);
        let mut galley = None;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                galley = Some(ui.fonts(|f| {
                    f.layout(
                        text.into(),
                        FontId::monospace(14.0),
                        egui::Color32::WHITE,
                        width,
                    )
                }));
            });
        });
        galley.unwrap()
    }
    #[test]
    fn source_character_stream_skips_hidden_lines_and_handles_tabs_unicode_and_reverse_lookups() {
        let source = format!("\tα🦀\r\n{}\nlast שלום", "hidden data\n".repeat(10_000));
        let index = editor_layout::TextIndex::new(&source);
        let expected: Vec<_> = source.chars().collect();
        let mut stream = SourceChars::new(&source, &index);
        for at in [
            0,
            0,
            0,
            1,
            2,
            5,
            100_001,
            100_002,
            expected.len() - 2,
            expected.len() - 1,
            expected.len(),
            expected.len(),
            0,
            1,
        ] {
            assert_eq!(stream.get(at), expected.get(at).copied());
        }
        let source = "α".repeat(100_000);
        let index = editor_layout::TextIndex::new(&source);
        let mut stream = SourceChars::new(&source, &index);
        for at in [0, 10_000, 20_000, 30_000, 99_999] {
            assert_eq!(stream.get(at), Some('α'));
        }
    }
    #[test]
    fn cached_offsets_and_visible_rows_match_a_full_unicode_wrapped_reference() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for text in [
                "",
                "α🦀\r\n\nשלום\n",
                "a long wrapped line with words ".repeat(400).as_str(),
            ] {
                let galley = layout(text, 140.0, scale);
                let mut cache = Cache::default();
                let rows = cache.get(&galley);
                assert!(Arc::ptr_eq(&rows, &cache.get(&galley)));
                let (mut chars, mut line, mut first) = (0, 0, true);
                for (i, row) in galley.rows.iter().enumerate() {
                    assert_eq!(rows.rows[i].start, chars);
                    assert_eq!(rows.rows[i].line, line);
                    assert_eq!(rows.rows[i].first, first);
                    chars += row.char_count_including_newline();
                    assert_eq!(rows.end(i), chars);
                    if row.ends_with_newline {
                        line += 1;
                    }
                    first = row.ends_with_newline;
                }
                let origin = egui::pos2(17.0, -20.0);
                for y in [-100.0, 0.0, 13.5, 100.0, 7000.0, 1_000_000.0] {
                    let clip =
                        egui::Rect::from_min_size(egui::pos2(0.0, y), egui::vec2(500.0, 60.0));
                    let expected: Vec<_> = galley
                        .rows
                        .iter()
                        .enumerate()
                        .filter(|(_, r)| {
                            origin.y + r.rect.bottom() >= clip.top()
                                && origin.y + r.rect.top() <= clip.bottom()
                        })
                        .map(|(i, _)| i)
                        .collect();
                    assert_eq!(
                        rows.visible(&galley, origin, clip).collect::<Vec<_>>(),
                        expected
                    );
                }
                drop(rows);
                let weak = Arc::downgrade(&galley);
                drop(galley);
                assert!(
                    weak.upgrade().is_none(),
                    "row offsets must not retain a layout"
                );
            }
        }
    }
    #[test]
    fn replaced_layouts_invalidate_offsets_and_large_views_only_visit_visible_rows() {
        let mut cache = Cache::default();
        let galley = layout(&"line\n".repeat(40_000), f32::INFINITY, 1.0);
        let rows = cache.get(&galley);
        let clip = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 200.0));
        let visible = rows.visible(&galley, egui::pos2(0.0, -100_000.0), clip);
        assert!(visible.start > 1000);
        assert!(visible.len() < 30);
        let next = layout("new\nlayout", f32::INFINITY, 1.0);
        let fresh = cache.get(&next);
        assert!(!Arc::ptr_eq(&rows, &fresh));
        assert_eq!(fresh.rows.len(), 2);
    }
}
