use super::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct CursorView {
    anchor: usize,
    head: usize,
    scroll_top: f64,
    scroll_left: f64,
    selections: Vec<hedit::advanced::Selection>,
    folds: Vec<hedit::advanced::Fold>,
}
impl CursorView {
    fn get(b: &Buffer) -> Self {
        Self {
            anchor: b.view.anchor,
            head: b.view.head,
            scroll_top: b.view.scroll_top,
            scroll_left: b.view.scroll_left,
            selections: b.view.selections.clone(),
            folds: b.view.folds.clone(),
        }
    }
    fn put(&self, b: &mut Buffer) {
        b.view.anchor = self.anchor.min(b.page.text.chars().count());
        b.view.head = self.head.min(b.page.text.chars().count());
        b.view.scroll_top = self.scroll_top;
        b.view.scroll_left = self.scroll_left;
        b.view.selections = self.selections.clone();
        b.view.folds = self.folds.clone();
        b.restore_cursor = true;
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Pane {
    id: u64,
    cursor: CursorView,
    revision: u64,
}
/// The most groups a layout has.
pub(super) const MAX_GROUPS: usize = 4;

/// Layouts, as in Sublime: 0 single, 1 two columns, 2 two rows, 3 three
/// columns, 4 four columns, 5 three rows, 6 a 2×2 grid.
pub(super) const LAYOUTS: u8 = 7;

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Panes {
    pub mode: u8,
    pub focused: usize,
    views: Vec<Pane>,
}
impl Panes {
    /// How many editor groups a layout shows.
    pub(super) fn count(mode: u8) -> usize {
        match mode {
            1 | 2 => 2,
            3 | 5 => 3,
            4 | 6 => 4,
            _ => 1,
        }
    }
    pub(super) fn groups(&self) -> usize {
        Self::count(self.mode)
    }
    /// Keep the layout, focus, and per-group views consistent after loading.
    pub(super) fn normalize(&mut self) {
        if self.mode >= LAYOUTS {
            self.mode = 0;
        }
        let count = self.groups();
        let filler = self.views.first().cloned().unwrap_or_default();
        self.views.resize(count.max(1), filler);
        self.focused = self.focused.min(count - 1);
    }
    pub(super) fn remap(&mut self, ids: &std::collections::HashMap<u64, (u64, u64)>) {
        self.normalize();
        for pane in &mut self.views {
            if let Some(&(id, revision)) = ids.get(&pane.id) {
                pane.id = id;
                pane.revision = revision;
            } else {
                pane.id = 0;
            }
        }
    }
    pub(super) fn secondary_id(&self) -> Option<u64> {
        (self.mode != 0)
            .then(|| self.views.get(1).map(|v| v.id))
            .flatten()
    }
    pub(super) fn set_secondary(&mut self, id: u64) {
        if let Some(view) = self.views.get_mut(1) {
            view.id = id;
        }
    }
}

/// Each group's area, separated by a small gap.
fn group_rects(mode: u8, area: egui::Rect) -> Vec<egui::Rect> {
    const GAP: f32 = 8.0;
    let columns = |n: usize| -> Vec<egui::Rect> {
        let width = (area.width() - GAP * (n - 1) as f32) / n as f32;
        (0..n)
            .map(|i| {
                let x = area.left() + i as f32 * (width + GAP);
                egui::Rect::from_min_max(
                    egui::pos2(x, area.top()),
                    egui::pos2(x + width, area.bottom()),
                )
            })
            .collect()
    };
    let rows = |n: usize| -> Vec<egui::Rect> {
        let height = (area.height() - GAP * (n - 1) as f32) / n as f32;
        (0..n)
            .map(|i| {
                let y = area.top() + i as f32 * (height + GAP);
                egui::Rect::from_min_max(
                    egui::pos2(area.left(), y),
                    egui::pos2(area.right(), y + height),
                )
            })
            .collect()
    };
    match mode {
        1 => columns(2),
        2 => rows(2),
        3 => columns(3),
        4 => columns(4),
        5 => rows(3),
        6 => {
            let (w, h) = ((area.width() - GAP) / 2.0, (area.height() - GAP) / 2.0);
            (0..4)
                .map(|i| {
                    let min = egui::pos2(
                        area.left() + (i % 2) as f32 * (w + GAP),
                        area.top() + (i / 2) as f32 * (h + GAP),
                    );
                    egui::Rect::from_min_size(min, egui::vec2(w, h))
                })
                .collect()
        }
        _ => vec![area],
    }
}
impl Pane {
    fn refresh(&mut self, b: &Buffer) {
        if self.revision == b.page.info.revision {
            return;
        }
        for change in b
            .changes
            .iter()
            .filter(|c| c.revision >= self.revision && c.revision < b.page.info.revision)
        {
            let map = |n: usize| {
                if n >= change.end {
                    n.saturating_sub(change.end - change.start) + change.added
                } else if n > change.start {
                    change.start + change.added
                } else {
                    n
                }
            };
            self.cursor.anchor = map(self.cursor.anchor);
            self.cursor.head = map(self.cursor.head);
        }
        self.cursor.folds.clear();
        self.cursor.selections.clear();
        self.revision = b.page.info.revision;
    }
}
impl HeditApp {
    /// Switch layout. New groups show the current file; fewer groups keep the
    /// first ones, with the current file in the first group.
    pub(super) fn split(&mut self, mode: u8) {
        let pane = Pane {
            id: self.active().page.info.id,
            cursor: CursorView::get(self.active()),
            revision: self.active().page.info.revision,
        };
        let count = Panes::count(mode);
        if self.panes.views.is_empty() || mode == 0 {
            self.panes.views = vec![pane.clone()];
            self.panes.focused = 0;
        } else {
            let focused = self.panes.focused.min(self.panes.views.len() - 1);
            self.panes.views[focused] = pane.clone();
        }
        self.panes.views.resize(count, pane);
        self.panes.views.truncate(count);
        self.panes.focused = self.panes.focused.min(count - 1);
        self.panes.mode = mode;
        self.focus_editor = true;
    }
    /// Focus the next group (`other`) or move the file there (`!other`).
    pub(super) fn focus_pane(&mut self, other: bool) {
        let next = (self.panes.focused + 1) % self.panes.groups().max(1);
        self.focus_group(next, !other);
    }
    /// Focus group `next`, or move the current file into it when `carry`.
    pub(super) fn focus_group(&mut self, next: usize, carry: bool) {
        let other = !carry;
        if self.panes.mode == 0 || next >= self.panes.groups() || next == self.panes.focused {
            return;
        }
        let current = self.panes.focused;
        self.panes.views[current] = Pane {
            id: self.active().page.info.id,
            cursor: CursorView::get(self.active()),
            revision: self.active().page.info.revision,
        };
        if !other {
            let displaced = self.panes.views[next].clone();
            self.panes.views[next] = self.panes.views[current].clone();
            if displaced.id != self.panes.views[current].id {
                self.panes.views[current] = displaced;
            }
        }
        let id = self.panes.views[next].id;
        if let Some(index) = self
            .workspace
            .open
            .iter()
            .position(|b| b.page.info.id == id)
        {
            self.workspace.active = index;
            self.panes.views[next].refresh(&self.workspace.open[index]);
            self.panes.views[next]
                .cursor
                .put(&mut self.workspace.open[index]);
            self.panes.focused = next;
            self.focus_editor = true;
        }
    }
    pub(super) fn pane_layout(&mut self, ui: &mut egui::Ui) {
        if self.panes.mode == 0 {
            self.editor_pane(ui, 0);
            return;
        }
        let mut panes = std::mem::take(&mut self.panes);
        panes.normalize();
        let current = panes.focused;
        panes.views[current] = Pane {
            id: self.active().page.info.id,
            cursor: CursorView::get(self.active()),
            revision: self.active().page.info.revision,
        };
        let area = ui.available_rect_before_wrap();
        let rects = group_rects(panes.mode, area);
        let mut focused = current;
        let request = self.focus_editor;
        for (number, &rect) in rects.iter().enumerate() {
            let id = panes.views[number].id;
            let index = self
                .workspace
                .open
                .iter()
                .position(|b| b.page.info.id == id)
                .unwrap_or(self.workspace.active);
            self.workspace.active = index;
            let canonical = CursorView::get(self.active());
            panes.views[number].refresh(&self.workspace.open[index]);
            panes.views[number]
                .cursor
                .put(&mut self.workspace.open[index]);
            self.focus_editor = request && number == current;
            ui.scope_builder(
                egui::UiBuilder::new()
                    .id_salt(("pane", number))
                    .max_rect(rect),
                |ui| {
                    ui.set_clip_rect(rect);
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(number == current, format!("Group {}", number + 1))
                            .clicked()
                        {
                            focused = number;
                        }
                        let selected = self.active().page.info.id;
                        egui::ComboBox::from_id_salt(("pane-file", number))
                            .width((rect.width() - 100.0).max(80.0))
                            .selected_text(&self.active().view.name)
                            .show_ui(ui, |ui| {
                                for (i, b) in self.workspace.open.iter().enumerate() {
                                    if ui
                                        .selectable_label(b.page.info.id == selected, &b.view.name)
                                        .clicked()
                                    {
                                        panes.views[number] = Pane {
                                            id: b.page.info.id,
                                            cursor: CursorView::get(b),
                                            revision: b.page.info.revision,
                                        };
                                        self.workspace.active = i;
                                        focused = number;
                                    }
                                }
                            });
                    });
                    let before = ui.available_rect_before_wrap();
                    self.editor_pane(ui, number);
                    // Ignore clicks on a window that happens to float over this pane.
                    if ui.input(|i| i.pointer.any_pressed())
                        && ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| {
                            before.contains(p) && ui.ctx().layer_id_at(p) == Some(ui.layer_id())
                        })
                    {
                        focused = number;
                    }
                },
            );
            let actual = self.workspace.active;
            panes.views[number] = Pane {
                id: self.active().page.info.id,
                cursor: CursorView::get(self.active()),
                revision: self.active().page.info.revision,
            };
            if actual == index {
                canonical.put(&mut self.workspace.open[index]);
            }
        }
        panes.focused = focused;
        if let Some(index) = self
            .workspace
            .open
            .iter()
            .position(|b| b.page.info.id == panes.views[focused].id)
        {
            self.workspace.active = index;
            panes.views[focused]
                .cursor
                .put(&mut self.workspace.open[index]);
        }
        self.focus_editor = focused != current;
        self.panes = panes;
        ui.allocate_rect(area, egui::Sense::hover());
    }
}
