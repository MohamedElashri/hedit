//! Fold arrows in the gutter: ▾ beside lines that start an indented block
//! (shown while the pointer is over the gutter) and ▸ beside folded ones.
use super::*;
use hedit::advanced::Fold;

/// Foldable blocks by header line, for one view and revision.
#[derive(Default)]
pub(super) struct FoldCache {
    key: (u64, u64, usize),
    heads: Arc<Vec<(usize, Fold)>>,
}

/// The 1-based line a fold hangs from: the line before a fold that starts a
/// line, or the line it starts in.
pub(super) fn header_line(text: &str, fold: Fold) -> usize {
    let start = hedit::advanced::byte(text, fold.start);
    let newlines = memchr::memchr_iter(b'\n', &text.as_bytes()[..start]).count();
    if start == 0 || text.as_bytes()[start - 1] == b'\n' {
        newlines
    } else {
        newlines + 1
    }
}

impl FoldCache {
    /// Foldable blocks of `text` by header line, computed once per revision.
    pub(super) fn heads(
        &mut self,
        view: u64,
        revision: u64,
        width: usize,
        text: &str,
    ) -> Arc<Vec<(usize, Fold)>> {
        let key = (view, revision, width);
        if self.key != key {
            let mut heads: Vec<(usize, Fold)> = hedit::advanced::indentation_folds(text, width)
                .into_iter()
                .map(|f| (header_line(text, f), f))
                .collect();
            // The outermost block wins when several start below one line.
            heads.sort_by_key(|(line, f)| (*line, std::cmp::Reverse(f.end)));
            heads.dedup_by_key(|(line, _)| *line);
            self.key = key;
            self.heads = Arc::new(heads);
        }
        Arc::clone(&self.heads)
    }
}

/// Draw a small triangle pointing down (open) or right (folded).
pub(super) fn arrow(
    painter: &egui::Painter,
    center: egui::Pos2,
    folded: bool,
    color: egui::Color32,
) {
    let s = 3.5;
    let points = if folded {
        vec![
            center + egui::vec2(-s * 0.6, -s),
            center + egui::vec2(s * 0.9, 0.0),
            center + egui::vec2(-s * 0.6, s),
        ]
    } else {
        vec![
            center + egui::vec2(-s, -s * 0.6),
            center + egui::vec2(s, -s * 0.6),
            center + egui::vec2(0.0, s * 0.9),
        ]
    };
    painter.add(egui::Shape::convex_polygon(
        points,
        color,
        egui::Stroke::NONE,
    ));
}
