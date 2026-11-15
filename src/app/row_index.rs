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
    index: Arc<egui::epaint::text::CursorIndex>,
    pub chars: usize,
}
impl Rows {
    fn new(galley: &Galley) -> Self {
        let index = galley.shared_cursor_index();
        Self {
            chars: index.end.start,
            index,
        }
    }
    pub fn row(&self, row: usize) -> Row {
        let at = self.index.rows[row];
        Row {
            start: at.start,
            line: at.paragraph,
            first: row == 0 || self.index.rows[row - 1].paragraph != at.paragraph,
        }
    }
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.index.rows.len()
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
        self.index.rows.get(row + 1).map_or(self.chars, |r| r.start)
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
    job: Weak<egui::text::LayoutJob>,
    index: Option<Arc<Rows>>,
    checked_text: Option<CheckedText>,
}
struct CheckedText {
    job: Weak<egui::text::LayoutJob>,
    projection: Weak<hedit::advanced::Projection>,
    matches: bool,
}
impl Cache {
    pub fn text_matches(&mut self, projection: &Arc<hedit::advanced::Projection>) -> bool {
        let identity = Arc::downgrade(projection);
        if let Some(checked) = &self.checked_text {
            if checked.job.ptr_eq(&self.job) && checked.projection.ptr_eq(&identity) {
                return checked.matches;
            }
        }
        // Compare once per layout/projection pair, never rescan unchanged text
        // merely because the selection recolored a copy-on-write galley.
        let matches = self
            .job
            .upgrade()
            .is_some_and(|job| job.text == projection.text);
        self.checked_text = Some(CheckedText {
            job: self.job.clone(),
            projection: identity,
            matches,
        });
        matches
    }
    pub fn get(&mut self, galley: &Arc<Galley>) -> Arc<Rows> {
        if self
            .galley
            .upgrade()
            .is_none_or(|cached| !Arc::ptr_eq(&cached, galley))
        {
            self.galley = Arc::downgrade(galley);
            self.job = Arc::downgrade(&galley.job);
            self.index = Some(Arc::new(Rows::new(galley)));
        }
        self.index.as_ref().unwrap().clone()
    }
}

#[cfg(test)]
pub(super) mod tests {
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
                assert!(std::ptr::eq(&*rows.index, galley.cursor_index()));
                let (mut chars, mut line, mut first) = (0, 0, true);
                for (i, row) in galley.rows.iter().enumerate() {
                    assert_eq!(rows.row(i).start, chars);
                    assert_eq!(rows.row(i).line, line);
                    assert_eq!(rows.row(i).first, first);
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
    fn cursor_text_guard_reuses_weak_identities_and_rechecks_changed_layouts() {
        let mut cache = Cache::default();
        let text = "abc\ndef\n";
        let projection = Arc::new(hedit::advanced::Projection::new(text, 4, &[]));
        let galley = layout(text, f32::INFINITY, 1.0);
        cache.get(&galley);
        assert!(cache.text_matches(&projection));
        assert!(cache.text_matches(&projection));
        let changed = Arc::new(hedit::advanced::Projection::new("abc def\n", 4, &[]));
        assert!(!cache.text_matches(&changed));
        assert!(!cache.text_matches(&changed));
        let next = layout(&changed.text, f32::INFINITY, 1.0);
        cache.get(&next);
        assert!(cache.text_matches(&changed));
        assert!(!cache.text_matches(&projection));
        let weak_job = Arc::downgrade(&next.job);
        let weak_projection = Arc::downgrade(&projection);
        drop(next);
        drop(projection);
        assert!(weak_job.upgrade().is_none());
        assert!(weak_projection.upgrade().is_none());
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
        assert_eq!(fresh.len(), 2);
    }
}

#[cfg(test)]
mod performance {
    use super::*;
    #[test]
    #[ignore = "manual release CPU profile of large-galley cursor lookups"]
    fn profile_cursor_navigation() {
        let galley = super::tests::layout(
            &"    α line with words\n".repeat(40_000),
            f32::INFINITY,
            1.0,
        );
        let row = galley.rows.len() - 2;
        let cursor = galley.from_rcursor(egui::epaint::text::cursor::RCursor { row, column: 12 });
        let point = galley.pos_from_cursor(&cursor).center().to_vec2();
        for (name, run) in [
            ("character", 0),
            ("paragraph", 1),
            ("row", 2),
            ("position", 3),
            ("pointer", 4),
            ("end", 5),
        ] {
            let start = Instant::now();
            for _ in 0..500 {
                match run {
                    0 => {
                        std::hint::black_box(
                            galley.from_ccursor(std::hint::black_box(cursor.ccursor)),
                        );
                    }
                    1 => {
                        std::hint::black_box(
                            galley.from_pcursor(std::hint::black_box(cursor.pcursor)),
                        );
                    }
                    2 => {
                        std::hint::black_box(
                            galley.from_rcursor(std::hint::black_box(cursor.rcursor)),
                        );
                    }
                    3 => {
                        std::hint::black_box(galley.pos_from_cursor(std::hint::black_box(&cursor)));
                    }
                    4 => {
                        std::hint::black_box(galley.cursor_from_pos(std::hint::black_box(point)));
                    }
                    _ => {
                        std::hint::black_box(galley.end());
                    }
                }
            }
            eprintln!(
                "Cursor {name}, 40000 rows: {:.3} us/lookup (500 warm CPU calls)",
                start.elapsed().as_secs_f64() * 1_000_000.0 / 500.0
            );
        }
    }
}
