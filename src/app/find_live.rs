//! Find as you type, highlighting every match, and outlining other
//! occurrences of the selected word, as in Sublime Text.
use super::*;
use eframe::egui::text::Galley;
use hedit::search::Options;

type Ranges = Arc<Vec<(usize, usize)>>;

#[derive(Default)]
pub(super) struct LiveFind {
    /// The cursor (view, character) when Find opened; typing searches from here.
    pub origin: Option<(u64, usize)>,
    cache: Option<(Key, Ranges)>,
    selection: Option<(Key, Ranges)>,
}

/// What a set of matches was computed for.
#[derive(Clone, PartialEq)]
struct Key {
    view: u64,
    revision: u64,
    start: u64,
    query: String,
    options: Options,
    scope: std::ops::Range<usize>,
}

/// Match ranges in characters, at most `limit` of them.
fn find_ranges(
    text: &str,
    query: &str,
    options: &Options,
    scope: std::ops::Range<usize>,
    limit: usize,
) -> Option<Vec<(usize, usize)>> {
    let regex = options.compile(query).ok()?;
    let scoped = text.get(scope.clone())?;
    let mut ranges = Vec::new();
    let (mut byte, mut char) = (0, 0);
    let mut to_char = |target: usize| {
        char += text[byte..target].chars().count();
        byte = target;
        char
    };
    for m in regex.find_iter(scoped).take(limit) {
        let a = to_char(scope.start + m.start());
        let b = to_char(scope.start + m.end());
        ranges.push((a, b));
    }
    Some(ranges)
}

impl LiveFind {
    fn cached(&mut self, key: Key, text: &str, selection: bool) -> Option<Ranges> {
        let slot = if selection {
            &mut self.selection
        } else {
            &mut self.cache
        };
        if let Some((cached, ranges)) = slot {
            if *cached == key {
                return Some(Arc::clone(ranges));
            }
        }
        let ranges = Arc::new(find_ranges(
            text,
            &key.query,
            &key.options,
            key.scope.clone(),
            20_000,
        )?);
        *slot = Some((key, Arc::clone(&ranges)));
        Some(ranges)
    }
}

impl HeditApp {
    fn live_key(&self, query: &str, options: Options) -> Key {
        let b = self.active();
        Key {
            view: b.view.id,
            revision: b.page.info.revision,
            start: b.page.start,
            query: query.to_owned(),
            options,
            scope: 0..b.page.text.len(),
        }
    }
    /// Every match of the Find bar's query on the page, when Find is open.
    pub(super) fn find_highlights(&mut self) -> Option<Ranges> {
        if !self.find_open || self.query.is_empty() {
            return None;
        }
        let mut key = self.live_key(&self.query, self.quality.options.clone());
        if let Some((_, start, end)) = self
            .quality
            .scope
            .filter(|(view, _, _)| self.quality.selection_only && *view == key.view)
        {
            let page = &self.active().page;
            key.scope = start
                .saturating_sub(page.start as usize)
                .min(page.text.len())
                ..end.saturating_sub(page.start as usize).min(page.text.len());
        }
        let text = &self.workspace.open[self.workspace.active].page.text;
        self.live_find.cached(key, text, false)
    }
    /// Other occurrences of a selected word, like Sublime's `match_selection`.
    pub(super) fn selection_highlights(&mut self) -> Option<Ranges> {
        if !self.settings.match_selection || self.find_open {
            return None;
        }
        let b = self.active();
        let (a, z) = (
            b.view.anchor.min(b.view.head),
            b.view.anchor.max(b.view.head),
        );
        if a == z || z - a > 100 || !b.view.selections.is_empty() {
            return None;
        }
        let word: String = b.page.text.chars().skip(a).take(z - a).collect();
        if !word.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return None;
        }
        let options = Options {
            regex: false,
            case_sensitive: true,
            whole_word: true,
        };
        let key = self.live_key(&word, options);
        let text = &self.workspace.open[self.workspace.active].page.text;
        let ranges = self.live_find.cached(key, text, true)?;
        (ranges.len() > 1).then_some(ranges)
    }
    /// Select the first match at or after where Find opened, as the query changes.
    pub(super) fn find_as_you_type(&mut self) {
        if self.active().page.info.paged {
            return; // Large paged files search on Enter.
        }
        let b = self.active();
        let origin = match self.live_find.origin {
            Some((view, at)) if view == b.view.id => at,
            _ => b.view.anchor.min(b.view.head),
        };
        self.live_find.origin = Some((b.view.id, origin));
        if self.query.is_empty() {
            self.quality.match_status.clear();
            return;
        }
        let Some(ranges) = self.find_highlights() else {
            self.quality.match_status = "Invalid pattern".into();
            return;
        };
        let index = self.workspace.active;
        let b = &mut self.workspace.open[index];
        let found = ranges
            .iter()
            .position(|&(start, _)| start >= origin)
            .or((!ranges.is_empty()).then_some(0));
        match found {
            Some(i) => {
                let (start, end) = ranges[i];
                b.view.anchor = start;
                b.view.head = end;
                self.quality.last_find =
                    Some(quality::LastFind::at(b, &self.query, &self.quality.options));
                self.quality.match_status = format!("{} of {}", i + 1, ranges.len());
            }
            None => {
                b.view.anchor = origin;
                b.view.head = origin;
                self.quality.last_find = None;
                self.quality.match_status = "No matches".into();
            }
        }
        b.view.selections.clear();
        b.reveal_cursor = true;
        b.restore_cursor = true;
        self.changed();
    }
}

/// Fill or outline character ranges (in display coordinates) on the rows of
/// `galley` that are inside `clip`.
pub(super) fn paint_ranges(
    painter: &egui::Painter,
    galley: &Galley,
    bidi: &super::bidi_view::BidiRows,
    origin: egui::Pos2,
    clip: egui::Rect,
    ranges: &[(usize, usize)],
    (fill, stroke): (egui::Color32, egui::Stroke),
) {
    let mut first = 0;
    let mut row_start = 0;
    for (index, row) in galley.rows.iter().enumerate() {
        let row_end = row_start + row.char_count_including_newline();
        let top = origin.y + row.rect.top();
        let bottom = origin.y + row.rect.bottom();
        if top > clip.bottom() {
            break;
        }
        if bottom >= clip.top() {
            for &(a, b) in ranges[first..].iter().take_while(|(a, _)| *a < row_end) {
                if b <= row_start {
                    continue;
                }
                for (x0, x1) in super::bidi_view::spans(
                    row,
                    bidi.row(index),
                    a.max(row_start) - row_start,
                    b.min(row_end) - row_start,
                ) {
                    let rect = egui::Rect::from_min_max(
                        egui::pos2(origin.x + x0, top),
                        egui::pos2(origin.x + x1.max(x0 + 2.0), bottom),
                    );
                    painter.rect(rect, 2.0, fill, stroke, egui::StrokeKind::Inside);
                }
            }
        }
        while first < ranges.len() && ranges[first].1 <= row_end {
            first += 1;
        }
        row_start = row_end;
    }
}
