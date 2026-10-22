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
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Panes {
    pub mode: u8,
    pub focused: usize,
    views: [Pane; 2],
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
    pub(super) fn split(&mut self, mode: u8) {
        let pane = Pane {
            id: self.active().page.info.id,
            cursor: CursorView::get(self.active()),
            revision: self.active().page.info.revision,
        };
        if mode == 0 {
            self.panes.views[0] = pane;
            self.panes.focused = 0;
        } else if self.panes.mode == 0 {
            self.panes.views = [pane.clone(), pane];
            self.panes.focused = 0;
        }
        self.panes.mode = mode;
        self.focus_editor = true;
    }
    pub(super) fn focus_pane(&mut self, other: bool) {
        if self.panes.mode == 0 {
            return;
        }
        let current = self.panes.focused;
        self.panes.views[current] = Pane {
            id: self.active().page.info.id,
            cursor: CursorView::get(self.active()),
            revision: self.active().page.info.revision,
        };
        let next = 1 - current;
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
        let current = panes.focused.min(1);
        panes.focused = current;
        panes.views[current] = Pane {
            id: self.active().page.info.id,
            cursor: CursorView::get(self.active()),
            revision: self.active().page.info.revision,
        };
        let area = ui.available_rect_before_wrap();
        let mut focused = current;
        let request = self.focus_editor;
        for number in 0..2 {
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
            let mut rect = area;
            if panes.mode == 1 {
                let mid = area.center().x;
                if number == 0 {
                    rect.max.x = mid - 4.0;
                } else {
                    rect.min.x = mid + 4.0;
                }
            } else {
                let mid = area.center().y;
                if number == 0 {
                    rect.max.y = mid - 4.0;
                } else {
                    rect.min.y = mid + 4.0;
                }
            }
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
                    if ui.input(|i| i.pointer.any_pressed())
                        && ui
                            .input(|i| i.pointer.interact_pos())
                            .is_some_and(|p| before.contains(p))
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
