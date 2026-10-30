//! Bidirectional Markdown scrolling, using source positions and rendered sections.
use super::*;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
pub(super) struct Viewport {
    pub offset: f32,
    pub max: f32,
    pub rect: egui::Rect,
}
impl Viewport {
    pub fn new<R>(output: &egui::scroll_area::ScrollAreaOutput<R>, rect: egui::Rect) -> Self {
        Self {
            offset: output.state.offset.y,
            max: (output.content_size.y - output.inner_rect.height()).max(0.0),
            rect,
        }
    }
}

pub(super) struct Source {
    pub viewport: Viewport,
    pub galley: Arc<egui::Galley>,
    pub projection: Arc<hedit::advanced::Projection>,
    pub inset: f32,
    pub revision: u64,
    pub navigation: bool,
}
impl Source {
    fn row_positions(&self, row: usize) -> (f32, f32) {
        let at = |row| {
            self.projection.source(
                self.galley
                    .from_rcursor(egui::epaint::text::cursor::RCursor { row, column: 0 })
                    .ccursor
                    .index,
            ) as f32
        };
        (at(row), at(row + 1))
    }
    fn position(&self) -> f32 {
        let y = (self.viewport.offset - self.inset).max(0.0);
        let row = self
            .galley
            .rows
            .partition_point(|row| row.rect.bottom() <= y)
            .min(self.galley.rows.len() - 1);
        let rect = self.galley.rows[row].rect;
        let (start, end) = self.row_positions(row);
        start + (end - start) * ((y - rect.top()) / rect.height().max(1.0)).clamp(0.0, 1.0)
    }
    fn offset(&self, position: f32) -> f32 {
        let display = self.projection.display(position.max(0.0) as usize);
        let row = self
            .galley
            .from_ccursor(egui::text::CCursor::new(display))
            .rcursor
            .row;
        let rect = self.galley.rows[row].rect;
        let (start, end) = self.row_positions(row);
        let fraction = if end > start {
            ((position - start) / (end - start)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (self.inset + rect.top() + fraction * rect.height()).clamp(0.0, self.viewport.max)
    }
}

pub(super) struct Preview {
    pub viewport: Viewport,
    /// Source character positions paired with preview content coordinates.
    pub sections: Vec<(f32, f32)>,
    pub revision: u64,
    pub navigation: bool,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Leader {
    #[default]
    Source,
    Preview,
}
#[derive(Default)]
struct Pair {
    used: bool,
    source: Option<Source>,
    preview: Option<Preview>,
    leader: Leader,
    pending_source: Option<f32>,
    pending_preview: Option<f32>,
    position: Option<f32>,
    at_end: bool,
    revision: Option<u64>,
    source_layout: Option<Arc<egui::Galley>>,
    preview_sections: Vec<(f32, f32)>,
    sizes: Option<(egui::Vec2, egui::Vec2)>,
}
#[derive(Default)]
pub(super) struct Sync {
    pairs: HashMap<(u64, usize), Pair>,
}

/// Interpolate within a section; inverse mapping uses rendered coordinates.
fn mapped(points: &[(f32, f32)], value: f32, inverse: bool) -> f32 {
    let axes = |point: (f32, f32)| if inverse { (point.1, point.0) } else { point };
    let index = points.partition_point(|&p| axes(p).0 <= value);
    let Some(&before) = index.checked_sub(1).and_then(|i| points.get(i)) else {
        return axes(points[0]).1;
    };
    let Some(&after) = points.get(index) else {
        return axes(before).1;
    };
    let (x, y) = axes(before);
    let (xx, yy) = axes(after);
    if xx <= x {
        y
    } else {
        y + (yy - y) * ((value - x) / (xx - x)).clamp(0.0, 1.0)
    }
}

impl Sync {
    pub fn retain(&mut self, open: &[Buffer]) {
        // Keep layouts only for recently rendered panes, rather than pinning the
        // glyph data of every inactive Markdown tab in memory.
        self.pairs.retain(|(id, _), pair| {
            let keep = pair.used
                && open.iter().any(|b| {
                    b.view.id == *id
                        && b.view.markdown_view == hedit::session::MarkdownView::Split
                        && b.language() == syntax::Language::Markdown
                });
            pair.used = false;
            keep
        });
    }
    pub fn reset(&mut self, id: u64) {
        self.pairs.retain(|(view, _), _| *view != id);
    }
    pub fn source_offset(&mut self, id: u64, pane: usize) -> Option<f32> {
        self.pairs
            .entry((id, pane))
            .or_default()
            .pending_source
            .take()
    }
    pub fn preview_offset(&mut self, id: u64, pane: usize) -> Option<f32> {
        self.pairs
            .entry((id, pane))
            .or_default()
            .pending_preview
            .take()
    }
    pub fn source(&mut self, id: u64, pane: usize, source: Source) {
        let pair = self.pairs.entry((id, pane)).or_default();
        pair.used = true;
        pair.source = Some(source);
    }
    pub fn preview(&mut self, id: u64, pane: usize, preview: Preview) {
        let pair = self.pairs.entry((id, pane)).or_default();
        pair.used = true;
        pair.preview = Some(preview);
    }

    /// Returns a new source offset when the preview is driving the editor.
    pub fn synchronize(&mut self, ctx: &egui::Context, id: u64, pane: usize) -> Option<f32> {
        let pair = self.pairs.get_mut(&(id, pane))?;
        let (Some(source), Some(preview)) = (&pair.source, &pair.preview) else {
            return None;
        };
        if source.revision != preview.revision || preview.sections.len() < 2 {
            return None;
        }
        let input_at = |rect: egui::Rect| {
            ctx.input(|input| {
                input
                    .pointer
                    .hover_pos()
                    .is_some_and(|pos| rect.contains(pos))
                    && (input.raw_scroll_delta.y != 0.0 || input.pointer.primary_down())
            })
        };
        let preview_input = input_at(preview.viewport.rect) && preview.viewport.max > 0.0;
        let source_input =
            input_at(source.viewport.rect) && source.viewport.max > 0.0 || source.navigation;
        let revision_changed = pair
            .revision
            .is_some_and(|revision| revision != source.revision);
        if preview.navigation || preview_input {
            pair.leader = Leader::Preview;
        } else if source_input || revision_changed {
            pair.leader = Leader::Source;
        }
        let sizes = (source.viewport.rect.size(), preview.viewport.rect.size());
        let layout_changed = pair
            .source_layout
            .as_ref()
            .is_some_and(|galley| !Arc::ptr_eq(galley, &source.galley))
            || pair.preview_sections != preview.sections
            || pair.sizes != Some(sizes);
        let keep_position = layout_changed
            && !source_input
            && !preview_input
            && !preview.navigation
            && !revision_changed
            && pair.position.is_some();
        let viewport = if pair.leader == Leader::Source {
            source.viewport
        } else {
            preview.viewport
        };
        if !keep_position {
            pair.at_end = viewport.max > 0.0 && viewport.offset >= viewport.max - 0.5;
            pair.position = Some(if viewport.offset <= 0.5 {
                0.0
            } else if pair.leader == Leader::Source {
                source.position()
            } else {
                mapped(&preview.sections, preview.viewport.offset, true)
            });
        }
        let position = pair.position.unwrap_or(0.0);
        pair.source_layout = Some(Arc::clone(&source.galley));
        pair.preview_sections.clone_from(&preview.sections);
        pair.sizes = Some(sizes);
        pair.revision = Some(source.revision);
        let mut source_target = None;
        if pair.leader == Leader::Preview || keep_position {
            let target = if pair.at_end {
                source.viewport.max
            } else if position <= 0.0 {
                0.0
            } else {
                source.offset(position)
            };
            if (target - source.viewport.offset).abs() > 0.75 {
                pair.pending_source = Some(target);
                source_target = Some(target);
            }
        }
        if pair.leader == Leader::Source || keep_position {
            let target = if pair.at_end {
                preview.viewport.max
            } else if position <= 0.0 {
                0.0
            } else {
                mapped(&preview.sections, position, false).clamp(0.0, preview.viewport.max)
            };
            if (target - preview.viewport.offset).abs() > 0.75 {
                pair.pending_preview = Some(target);
            }
        }
        if pair.pending_source.is_some() || pair.pending_preview.is_some() {
            ctx.request_repaint();
        }
        source_target
    }

    #[cfg(test)]
    pub fn snapshot(&self, id: u64, pane: usize) -> Option<(Viewport, Viewport)> {
        let pair = self.pairs.get(&(id, pane))?;
        Some((
            pair.source.as_ref()?.viewport,
            pair.preview.as_ref()?.viewport,
        ))
    }
}
