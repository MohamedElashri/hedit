//! Derived tab geometry only; no document text or persistent UI cache.
use super::*;
#[derive(Default)]
pub(super) struct Cache {
    entries: Vec<Entry>,
    offsets: Vec<f32>,
    style: Option<(egui::FontId, f32, egui::Vec2)>,
    last_active: Option<(u64, usize)>,
    last_width: f32,
    dirty: bool,
}
struct Entry {
    id: u64,
    name: String,
    pinned: bool,
    close: bool,
    pub title: String,
    width: f32,
}
impl Cache {
    pub fn sync(&mut self, ui: &egui::Ui, workspace: &Workspace) {
        let font = ui.style().text_styles[&egui::TextStyle::Button].clone();
        let style = (
            font,
            ui.ctx().pixels_per_point(),
            ui.spacing().button_padding,
        );
        let restyle = self.style.as_ref() != Some(&style);
        let mut changed = restyle || self.entries.len() != workspace.open.len() || self.dirty;
        for (i, b) in workspace.open.iter().enumerate() {
            let close = workspace.can_close(i);
            if !restyle
                && self.entries.get(i).is_some_and(|e| {
                    e.id == b.view.id
                        && e.name == b.view.name
                        && e.pinned == b.view.pinned
                        && e.close == close
                })
            {
                continue;
            }
            let visual = hedit::bidi::display(&b.view.name);
            let mut chars = visual.chars();
            let mut title: String = chars.by_ref().take(30).collect();
            if chars.next().is_some() {
                title.push('…');
            }
            if b.view.pinned {
                title.insert_str(0, "◆ ");
            }
            let galley = egui::WidgetText::from(RichText::new(&title).size(12.0)).into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                egui::TextStyle::Button,
            );
            let width = 20.0
                + (galley.size().x + ui.spacing().button_padding.x * 2.0).max(96.0)
                + if close { 24.0 } else { 0.0 };
            let entry = Entry {
                id: b.view.id,
                name: b.view.name.clone(),
                pinned: b.view.pinned,
                close,
                title,
                width,
            };
            if i < self.entries.len() {
                self.entries[i] = entry;
            } else {
                self.entries.push(entry);
            }
            changed = true;
        }
        self.entries.truncate(workspace.open.len());
        if changed {
            self.offsets.clear();
            self.offsets.push(0.0);
            for entry in &self.entries {
                self.offsets
                    .push(self.offsets.last().copied().unwrap() + entry.width + 2.0);
            }
        }
        if restyle {
            self.last_active = None;
        }
        self.style = Some(style);
        self.dirty = false;
    }
    pub fn title(&self, i: usize) -> &str {
        &self.entries[i].title
    }
    pub fn offset(&self, i: usize) -> f32 {
        self.offsets[i]
    }
    pub fn width(&self) -> f32 {
        self.offsets.last().copied().unwrap_or(0.0)
    }
    pub fn height(&self) -> f32 {
        if self.entries.iter().any(|e| e.close) {
            28.0
        } else {
            26.0
        }
    }
    pub fn visible(&self, viewport: egui::Rect) -> std::ops::Range<usize> {
        let first = self
            .offsets
            .partition_point(|x| *x < viewport.left())
            .saturating_sub(2)
            .min(self.entries.len());
        let end =
            (self.offsets.partition_point(|x| *x <= viewport.right()) + 1).min(self.entries.len());
        first..end.max(first)
    }
    pub fn reveal(&mut self, id: u64, index: usize, width: f32) -> bool {
        let reveal = self.last_active != Some((id, index)) || self.last_width != width;
        self.last_active = Some((id, index));
        self.last_width = width;
        reveal
    }
    pub fn measured(&mut self, i: usize, width: f32) -> bool {
        if (self.entries[i].width - width).abs() > 0.01 {
            self.entries[i].width = width;
            self.dirty = true;
            return true;
        }
        false
    }
}
