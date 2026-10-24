//! Fold arrows in the gutter: ▾ beside lines that start an indented block
//! (shown while the pointer is over the gutter) and ▸ beside folded ones.
use super::*;
use hedit::advanced::Fold;
use std::collections::HashMap;

/// Folds by their 1-based header line.
type Heads = Arc<Vec<(usize, Fold)>>;

/// Fold header lines, cached per view so split panes do not evict each other.
#[derive(Default)]
pub(super) struct FoldCache {
    /// Foldable blocks by header line, keyed by revision and tab width.
    heads: HashMap<u64, ((u64, usize), Heads)>,
    /// The view's folds by header line, keyed by revision and the folds.
    folded: HashMap<u64, ((u64, Vec<Fold>), Heads)>,
}

/// The 1-based line each fold hangs from: the line before a fold that starts
/// a line, or the line it starts in. One pass over the text.
fn header_lines(text: &str, folds: &[Fold]) -> Vec<(usize, Fold)> {
    let mut sorted: Vec<Fold> = folds.to_vec();
    sorted.sort_by_key(|f| f.start);
    let mut out = Vec::with_capacity(sorted.len());
    let mut next = sorted.iter().peekable();
    let mut line = 1;
    let mut previous = '\n';
    for (index, c) in text.chars().enumerate() {
        while let Some(f) = next.next_if(|f| f.start == index) {
            out.push((if previous == '\n' { line - 1 } else { line }, *f));
        }
        if next.peek().is_none() {
            break;
        }
        if c == '\n' {
            line += 1;
        }
        previous = c;
    }
    // Folds that start at the very end.
    for f in next {
        out.push((if previous == '\n' { line - 1 } else { line }, *f));
    }
    out
}

impl FoldCache {
    /// Foldable blocks of `text` by header line, computed once per revision.
    pub(super) fn heads(&mut self, view: u64, revision: u64, width: usize, text: &str) -> Heads {
        let key = (revision, width);
        if let Some((k, heads)) = self.heads.get(&view) {
            if *k == key {
                return Arc::clone(heads);
            }
        }
        let folds = hedit::advanced::indentation_folds(text, width);
        let mut heads = header_lines(text, &folds);
        // The outermost block wins when several start below one line.
        heads.sort_by_key(|(line, f)| (*line, std::cmp::Reverse(f.end)));
        heads.dedup_by_key(|(line, _)| *line);
        let heads = Arc::new(heads);
        self.heads.insert(view, (key, Arc::clone(&heads)));
        heads
    }
    /// The view's folds by header line, computed when the folds or text change.
    pub(super) fn folded(&mut self, view: u64, revision: u64, folds: &[Fold], text: &str) -> Heads {
        if let Some(((r, f), heads)) = self.folded.get(&view) {
            if *r == revision && f == folds {
                return Arc::clone(heads);
            }
        }
        let heads = Arc::new(header_lines(text, folds));
        self.folded
            .insert(view, ((revision, folds.to_vec()), Arc::clone(&heads)));
        heads
    }
    /// Forget views that are no longer open.
    pub(super) fn retain(&mut self, open: impl Fn(u64) -> bool) {
        self.heads.retain(|view, _| open(*view));
        self.folded.retain(|view, _| open(*view));
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
