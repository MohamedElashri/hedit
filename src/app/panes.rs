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
    pub(super) fn get(b: &Buffer) -> Self {
        Self {
            anchor: b.view.anchor,
            head: b.view.head,
            scroll_top: b.view.scroll_top,
            scroll_left: b.view.scroll_left,
            selections: b.view.selections.clone(),
            folds: b.view.folds.clone(),
        }
    }
    pub(super) fn put(&self, b: &mut Buffer) {
        // Split views restore every frame; scan only up to the furthest offset.
        let characters = b.page.text.chars().take(self.anchor.max(self.head)).count();
        b.view.anchor = self.anchor.min(characters);
        b.view.head = self.head.min(characters);
        b.view.scroll_top = self.scroll_top;
        b.view.scroll_left = self.scroll_left;
        b.view.selections.clone_from(&self.selections);
        b.view.folds.clone_from(&self.folds);
        b.restore_cursor = true;
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Pane {
    group_id: u64,
    id: u64,
    cursor: CursorView,
    revision: u64,
    tabs: Vec<TabView>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct TabView {
    id: u64,
    cursor: CursorView,
    revision: u64,
}
impl TabView {
    fn get(b: &Buffer) -> Self {
        Self {
            id: b.view.id,
            cursor: CursorView::get(b),
            revision: b.page.info.revision,
        }
    }
}
impl Pane {
    fn from_tab(tab: TabView) -> Self {
        Self {
            group_id: 0,
            id: tab.id,
            cursor: tab.cursor.clone(),
            revision: tab.revision,
            tabs: vec![tab],
        }
    }
    fn save(&mut self, b: &Buffer) {
        if self.id == b.view.id
            && self.revision == b.page.info.revision
            && self.cursor.anchor == b.view.anchor
            && self.cursor.head == b.view.head
            && self.cursor.scroll_top == b.view.scroll_top
            && self.cursor.scroll_left == b.view.scroll_left
            && self.cursor.selections == b.view.selections
            && self.cursor.folds == b.view.folds
            && self.tabs.iter().any(|t| t.id == b.view.id)
        {
            return;
        }
        let tab = TabView::get(b);
        self.id = tab.id;
        self.cursor = tab.cursor.clone();
        self.revision = tab.revision;
        if let Some(saved) = self.tabs.iter_mut().find(|t| t.id == tab.id) {
            *saved = tab;
        } else {
            self.tabs.push(tab);
        }
    }
    fn restore(&mut self, b: &mut Buffer) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == b.view.id) {
            tab.refresh(b);
            tab.cursor.put(b);
        }
        self.save(b);
    }
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
    initialized: bool,
    next_group: u64,
    #[serde(skip)]
    known_ids: Vec<u64>,
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
        while self.views.len() < count {
            let tab = self
                .views
                .first()
                .map(|p| TabView {
                    id: p.id,
                    cursor: p.cursor.clone(),
                    revision: p.revision,
                })
                .unwrap_or_default();
            self.views.push(Pane::from_tab(tab));
        }
        if self.views.len() > count {
            let removed = self.views.split_off(count);
            for pane in removed {
                for tab in pane.tabs {
                    if !self.views[0].tabs.iter().any(|t| t.id == tab.id) {
                        self.views[0].tabs.push(tab);
                    }
                }
            }
        }
        self.focused = self.focused.min(count - 1);
        let mut seen = std::collections::HashSet::new();
        for pane in &mut self.views {
            if pane.group_id == 0 || !seen.insert(pane.group_id) {
                loop {
                    self.next_group = self.next_group.wrapping_add(1).max(1);
                    if seen.insert(self.next_group) {
                        pane.group_id = self.next_group;
                        break;
                    }
                }
            }
        }
    }
    pub(super) fn focused_id(&self) -> u64 {
        self.views.get(self.focused).map_or(0, |p| p.group_id)
    }
    pub(super) fn remap(&mut self, ids: &std::collections::HashMap<u64, (u64, u64)>) {
        self.normalize();
        self.known_ids.clear();
        for pane in &mut self.views {
            if let Some(&(id, revision)) = ids.get(&pane.id) {
                pane.id = id;
                pane.revision = revision;
            } else {
                pane.id = 0;
            }
            pane.tabs.retain_mut(|tab| {
                if let Some(&(id, revision)) = ids.get(&tab.id) {
                    tab.id = id;
                    tab.revision = revision;
                    true
                } else {
                    false
                }
            });
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
impl TabView {
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
    fn save_group_cursor(&mut self) {
        let group = self.panes.focused;
        if let Some(pane) = self.panes.views.get_mut(group) {
            if pane.id == self.workspace.open[self.workspace.active].view.id {
                pane.save(&self.workspace.open[self.workspace.active]);
            }
        }
    }

    /// Register document IDs, never documents, in groups. Old saved layouts
    /// migrate all existing tabs into the first group without losing a file.
    pub(super) fn sync_groups(&mut self) {
        self.panes.normalize();
        if self.panes.initialized
            && self.panes.known_ids.len() == self.workspace.open.len()
            && self
                .panes
                .known_ids
                .iter()
                .zip(&self.workspace.open)
                .all(|(id, b)| *id == b.view.id)
            && self.panes.views[self.panes.focused].id == self.active().view.id
        {
            return;
        }
        if !self.panes.initialized {
            self.panes.views[0].tabs = self.workspace.open.iter().map(TabView::get).collect();
            for pane in self.panes.views.iter_mut().skip(1) {
                if let Some(b) = self.workspace.open.iter().find(|b| b.view.id == pane.id) {
                    pane.tabs.push(TabView::get(b));
                }
            }
            self.panes.initialized = true;
        }
        let live: std::collections::HashSet<_> =
            self.workspace.open.iter().map(|b| b.view.id).collect();
        self.editor_layout.retain_documents(&live);
        for layout in &mut self.extra_layouts {
            layout.retain_documents(&live);
        }
        self.quality.git_base.retain(|id, _| live.contains(id));
        self.quality.git_cache.retain(|id, _| live.contains(id));
        self.quality.git_lines.retain(|id, _| live.contains(id));
        for pane in &mut self.panes.views {
            pane.tabs.retain(|tab| live.contains(&tab.id));
        }
        // A document installed by a background read still has a home. Foreground
        // navigation additionally attaches it to the focused group below.
        let owned: std::collections::HashSet<_> = self
            .panes
            .views
            .iter()
            .flat_map(|p| p.tabs.iter().map(|t| t.id))
            .collect();
        let group = self.panes.focused;
        for b in &self.workspace.open {
            if !owned.contains(&b.view.id) {
                self.panes.views[group].tabs.push(TabView::get(b));
            }
        }
        let mut removed = false;
        for number in (0..self.panes.views.len()).rev() {
            if self.panes.views.len() > 1 && self.panes.views[number].tabs.is_empty() {
                self.remove_editor_group(number);
                removed = true;
            }
        }
        if removed {
            self.restore_focused_group();
        }
        let group = self.panes.focused;
        let previous = self.panes.views[group].id;
        if previous != 0 && !live.contains(&previous) {
            if let Some(tab) = self.panes.views[group].tabs.last().cloned() {
                if let Some(index) = self.workspace.open.iter().position(|b| b.view.id == tab.id) {
                    self.workspace.active = index;
                    self.panes.views[group].restore(&mut self.workspace.open[index]);
                }
            }
        }
        let b = &self.workspace.open[self.workspace.active];
        self.panes.views[group].save(b);
        self.panes.known_ids = self.workspace.open.iter().map(|b| b.view.id).collect();
        if self.panes.mode == 0
            && (self.panes.views[0].tabs.len() != self.workspace.open.len()
                || self.panes.views[0]
                    .tabs
                    .iter()
                    .zip(&self.workspace.open)
                    .any(|(t, b)| t.id != b.view.id))
        {
            // Retain the established single-group ordering and open-file sidebar.
            let mut saved: std::collections::HashMap<_, _> = self.panes.views[0]
                .tabs
                .drain(..)
                .map(|t| (t.id, t))
                .collect();
            self.panes.views[0].tabs = self
                .workspace
                .open
                .iter()
                .map(|b| saved.remove(&b.view.id).unwrap_or_else(|| TabView::get(b)))
                .collect();
        }
    }

    pub(super) fn review_in_group(&self, group: usize) -> bool {
        self.source_control.review_tab
            && self.source_control.review_selected
            && self
                .panes
                .views
                .get(group)
                .is_some_and(|p| Some(p.group_id) == self.source_control.review_group)
    }
    pub(super) fn sync_review_group(&mut self) {
        if !self.source_control.review_tab {
            self.source_control.review_selected = false;
            self.source_control.review_group = None;
            return;
        }
        if self.source_control.open && !self.source_control.review_selected {
            self.source_control.review_selected = true;
        }
        if self
            .source_control
            .review_group
            .is_none_or(|id| !self.panes.views.iter().any(|p| p.group_id == id))
        {
            self.source_control.review_group = Some(self.panes.focused_id());
        }
        self.source_control.open = self.review_in_group(self.panes.focused);
    }
    pub(super) fn leave_review(&mut self) {
        if self.source_control.review_group.is_none()
            || self.source_control.review_group == Some(self.panes.focused_id())
        {
            self.source_control.review_selected = false;
        }
        self.source_control.open = false;
    }
    pub(super) fn activate_review(&mut self) {
        self.source_control.review_selected = true;
        self.source_control.review_group = Some(self.panes.focused_id());
        self.source_control.open = true;
        self.focus_editor = false;
    }
    pub(super) fn save_view_in_group(&mut self, group: u64, index: usize) {
        let tab = TabView::get(&self.workspace.open[index]);
        if let Some(pane) = self.panes.views.iter_mut().find(|p| p.group_id == group) {
            if let Some(saved) = pane.tabs.iter_mut().find(|t| t.id == tab.id) {
                *saved = tab.clone();
            }
            if pane.id == tab.id {
                pane.cursor = tab.cursor;
                pane.revision = tab.revision;
            }
        }
    }
    pub(super) fn group_contains(&self, group: u64, id: u64) -> bool {
        self.panes
            .views
            .iter()
            .any(|p| p.group_id == group && p.tabs.iter().any(|t| t.id == id))
    }
    pub(super) fn detach_shared_preview(&mut self, group: u64, id: u64) -> bool {
        let shared = self
            .panes
            .views
            .iter()
            .any(|p| p.group_id != group && p.tabs.iter().any(|t| t.id == id));
        if shared {
            if let Some(pane) = self.panes.views.iter_mut().find(|p| p.group_id == group) {
                pane.tabs.retain(|t| t.id != id);
                if pane.id == id {
                    pane.id = pane.tabs.last().map_or(0, |t| t.id);
                }
            }
        }
        shared
    }
    pub(super) fn can_close_group_tab(&self, index: usize) -> bool {
        self.workspace.can_close(index)
            || (self.panes.groups() > 1
                && self.workspace.open.get(index).is_some_and(|b| {
                    self.panes
                        .views
                        .iter()
                        .filter(|p| p.tabs.iter().any(|t| t.id == b.view.id))
                        .count()
                        > 1
                }))
    }
    pub(super) fn group_tab_count(&self) -> usize {
        self.panes
            .views
            .get(self.panes.focused)
            .map_or(self.workspace.open.len(), |p| p.tabs.len())
    }
    pub(super) fn group_tab_ids(&self, group: usize) -> Vec<u64> {
        self.panes
            .views
            .get(group)
            .map_or_else(Vec::new, |p| p.tabs.iter().map(|t| t.id).collect())
    }

    pub(super) fn attach_open_tab(&mut self, group: Option<u64>, id: u64) {
        let index = group
            .and_then(|id| self.panes.views.iter().position(|p| p.group_id == id))
            .unwrap_or(self.panes.focused);
        self.attach_group_tab(index, id);
    }
    pub(super) fn attach_group_tab(&mut self, group: usize, id: u64) {
        let Some(b) = self.workspace.open.iter().find(|b| b.view.id == id) else {
            return;
        };
        let group = group.min(self.panes.views.len().saturating_sub(1));
        if let Some(pane) = self.panes.views.get_mut(group) {
            if !pane.tabs.iter().any(|t| t.id == id) {
                pane.tabs.push(TabView::get(b));
            }
        }
    }

    pub(super) fn select_group_tab(&mut self, index: usize) {
        self.sync_groups();
        self.save_group_cursor();
        let group = self.panes.focused;
        let b = &mut self.workspace.open[index];
        self.panes.views[group].restore(b);
    }

    /// Removing the last reference in this group leaves a shared document and
    /// its undo journal alive elsewhere. Only its last tab may ask to discard.
    pub(super) fn detach_shared_tabs(&mut self, ids: Vec<u64>) -> Vec<u64> {
        self.sync_groups();
        let group = self.panes.focused;
        let mut last = Vec::new();
        for id in ids {
            let shared = self
                .panes
                .views
                .iter()
                .enumerate()
                .any(|(n, p)| n != group && p.tabs.iter().any(|t| t.id == id));
            if shared {
                self.panes.views[group].tabs.retain(|t| t.id != id);
            } else {
                last.push(id);
            }
        }
        if self.panes.views[group].tabs.is_empty() && self.panes.views.len() > 1 {
            self.remove_editor_group(group);
        }
        self.restore_focused_group();
        last
    }

    fn remove_editor_group(&mut self, group: usize) {
        if self.panes.views.len() <= 1 || group >= self.panes.views.len() {
            return;
        }
        self.panes.views.remove(group);
        self.release_unused_pane_caches();
        self.panes.focused = self
            .panes
            .focused
            .saturating_sub(usize::from(group <= self.panes.focused))
            .min(self.panes.views.len() - 1);
        self.panes.mode = match self.panes.views.len() {
            1 => 0,
            2 if matches!(self.panes.mode, 2 | 5) => 2,
            2 => 1,
            3 if self.panes.mode == 5 => 5,
            3 => 3,
            _ => 4,
        };
    }

    fn release_unused_pane_caches(&mut self) {
        let count = self.panes.views.len();
        self.extra_layouts[count - 1..].fill_with(Default::default);
        self.projections[count..].fill_with(Default::default);
        self.row_indexes[count..].fill_with(Default::default);
        self.minimaps[count..].fill_with(Default::default);
    }

    fn restore_focused_group(&mut self) {
        let group = self.panes.focused;
        let pane = &mut self.panes.views[group];
        if !pane.tabs.iter().any(|t| t.id == pane.id) {
            pane.id = pane.tabs.last().map_or(0, |t| t.id);
        }
        if let Some(index) = self
            .workspace
            .open
            .iter()
            .position(|b| b.view.id == pane.id)
        {
            self.workspace.active = index;
            pane.restore(&mut self.workspace.open[index]);
        }
        self.sync_review_group();
        self.focus_editor = !self.source_control.open;
    }

    /// Fewer groups merge their remaining tabs into the first group.
    pub(super) fn split(&mut self, mode: u8) {
        self.sync_groups();
        self.save_group_cursor();
        let mode = mode.min(LAYOUTS - 1);
        let count = Panes::count(mode);
        let current = TabView::get(self.active());
        if count < self.panes.views.len() {
            let removed = self.panes.views.split_off(count);
            self.release_unused_pane_caches();
            for pane in removed {
                for tab in pane.tabs {
                    if !self.panes.views[0].tabs.iter().any(|t| t.id == tab.id) {
                        self.panes.views[0].tabs.push(tab);
                    }
                }
            }
        }
        while self.panes.views.len() < count {
            self.panes.views.push(Pane::from_tab(current.clone()));
        }
        self.panes.focused = self.panes.focused.min(count - 1);
        self.panes.mode = mode;
        self.panes.normalize();
        self.attach_group_tab(self.panes.focused, current.id);
        self.panes.views[self.panes.focused].save(&self.workspace.open[self.workspace.active]);
        if mode == 0 {
            let order: std::collections::HashMap<_, _> = self.panes.views[0]
                .tabs
                .iter()
                .enumerate()
                .map(|(i, t)| (t.id, i))
                .collect();
            self.workspace
                .open
                .sort_by_key(|b| order.get(&b.view.id).copied().unwrap_or(usize::MAX));
            self.workspace.active = self
                .workspace
                .open
                .iter()
                .position(|b| b.view.id == current.id)
                .unwrap_or(0);
        }
        self.focus_editor = true;
    }

    pub(super) fn focus_pane(&mut self, other: bool) {
        let next = (self.panes.focused + 1) % self.panes.groups().max(1);
        self.focus_group(next, !other);
    }
    pub(super) fn focus_group(&mut self, next: usize, carry: bool) {
        self.sync_groups();
        if next < self.panes.groups() {
            self.terminal.release_input();
            self.focus_editor = true;
        }
        if next >= self.panes.groups() || next == self.panes.focused {
            return;
        }
        if carry {
            let id = self.active().view.id;
            self.move_group_tab(self.panes.focused, next, id, usize::MAX);
            return;
        }
        self.save_group_cursor();
        self.panes.focused = next;
        self.restore_focused_group();
    }

    pub(super) fn move_group_tab(
        &mut self,
        source: usize,
        target: usize,
        id: u64,
        position: usize,
    ) {
        self.sync_groups();
        if source >= self.panes.views.len() || target >= self.panes.views.len() {
            return;
        }
        self.save_group_cursor();
        let Some(index) = self.panes.views[source]
            .tabs
            .iter()
            .position(|t| t.id == id)
        else {
            return;
        };
        let tab = self.panes.views[source].tabs.remove(index);
        self.panes.views[target].tabs.retain(|t| t.id != id);
        let position = position.min(self.panes.views[target].tabs.len());
        self.panes.views[target].tabs.insert(position, tab);
        self.panes.views[target].id = id;
        self.panes.focused = target;
        if source != target && self.panes.views[source].tabs.is_empty() {
            self.remove_editor_group(source);
        } else if source != target && self.panes.views[source].id == id {
            self.panes.views[source].id = self.panes.views[source].tabs.last().map_or(0, |t| t.id);
        }
        self.restore_focused_group();
        self.changed();
    }

    pub(super) fn pane_layout(&mut self, ui: &mut egui::Ui) {
        self.sync_groups();
        if self.panes.mode == 0 {
            self.document_breadcrumbs(ui);
            self.editor_pane(ui, 0);
            self.save_group_cursor();
            return;
        }
        self.save_group_cursor();
        let current = self.panes.focused;
        let request = self.focus_editor;
        let area = ui.available_rect_before_wrap();
        let rects = group_rects(self.panes.mode, area);
        let mut focused = current;
        let mut structure = false;
        for (number, &rect) in rects.iter().enumerate() {
            if number >= self.panes.views.len() {
                break;
            }
            let id = self.panes.views[number].id;
            let index = self
                .workspace
                .open
                .iter()
                .position(|b| b.view.id == id)
                .unwrap_or(self.workspace.active);
            self.workspace.active = index;
            let canonical = CursorView::get(self.active());
            self.panes.views[number].restore(&mut self.workspace.open[index]);
            self.panes.focused = number;
            self.source_control.open = self.review_in_group(number);
            self.focus_editor = request && number == current;
            let before_groups = self.panes.views.len();
            ui.scope_builder(
                egui::UiBuilder::new()
                    .id_salt(("pane", number))
                    .max_rect(rect),
                |ui| {
                    ui.set_clip_rect(rect);
                    if self.group_tabs(ui, number, current) {
                        focused = number;
                    }
                    if self.panes.views.len() != before_groups {
                        structure = true;
                        return;
                    }
                    let before = ui.available_rect_before_wrap();
                    if self.review_in_group(number) {
                        self.source_control_contents(ui);
                    } else {
                        self.document_breadcrumbs(ui);
                        self.editor_pane(ui, number);
                    }
                    if ui.input(|i| i.pointer.any_released()) && ui.rect_contains_pointer(before) {
                        if let Some(id) = self.quality.tab_drag.take() {
                            let source = self.quality.tab_drag_group.take().unwrap_or(number);
                            self.move_group_tab(source, number, id, usize::MAX);
                            focused = self.panes.focused;
                            structure = self.panes.views.len() != before_groups;
                        }
                    }
                    if ui.input(|i| i.pointer.any_pressed())
                        && ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| {
                            before.contains(p) && ui.ctx().layer_id_at(p) == Some(ui.layer_id())
                        })
                    {
                        focused = number;
                    }
                },
            );
            if structure {
                break;
            }
            self.save_group_cursor();
            if self.workspace.active == index {
                canonical.put(&mut self.workspace.open[index]);
            }
        }
        if !structure {
            self.panes.focused = focused.min(self.panes.views.len() - 1);
        }
        self.restore_focused_group();
        self.sync_review_group();
        self.focus_editor =
            !self.source_control.open && (structure || self.panes.focused != current);
        ui.allocate_rect(area, egui::Sense::hover());
    }
}

#[cfg(test)]
mod group_tests {
    use super::*;
    use crate::app::tests::{finish_app_job, frame};
    fn app() -> (tempfile::TempDir, egui::Context, HeditApp) {
        let dir = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
        workspace.open[0].page.text = "abcdef".into();
        workspace.edit(0, "").unwrap();
        workspace.new_buffer().unwrap();
        workspace.name_scratchpad(1, "second").unwrap();
        workspace.active = 0;
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.settings.auto_complete = false;
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        (dir, ctx, app)
    }
    #[test]
    fn groups_own_tab_order_and_navigation_without_cloning_documents() {
        let (_dir, ctx, mut app) = app();
        let first = app.workspace.open[0].view.id;
        let second = app.workspace.open[1].view.id;
        app.split(1);
        assert_eq!(app.group_tab_ids(0), vec![first, second]);
        assert_eq!(app.group_tab_ids(1), vec![first]);
        app.focus_group(1, false);
        app.execute(&ctx, Command::New);
        let third = app.active().view.id;
        frame(&ctx, &mut app, vec![]);
        assert_eq!(app.group_tab_ids(1), vec![first, third]);
        assert_eq!(app.group_tab_ids(0), vec![first, second]);
        app.execute(&ctx, Command::Tab1);
        assert_eq!(app.active().view.id, first);
        app.execute(&ctx, Command::NextTab);
        assert_eq!(app.active().view.id, third);
        app.execute(&ctx, Command::NextTab);
        assert_eq!(app.active().view.id, first);
        assert_eq!(app.workspace.open.len(), 3);
    }
    #[test]
    fn closing_a_shared_dirty_tab_only_detaches_its_group() {
        let (_dir, ctx, mut app) = app();
        let first = app.active().view.id;
        app.split(1);
        app.focus_group(1, false);
        app.execute(&ctx, Command::Close);
        assert_eq!(app.panes.groups(), 1);
        assert!(!app.closing && app.close_flow.is_none());
        assert_eq!(app.workspace.open.len(), 2);
        assert_eq!(app.active().view.id, first);
        assert_eq!(app.active().page.text, "abcdef");
        assert!(app.workspace.closed.is_empty());
        app.execute(&ctx, Command::Close);
        assert!(
            app.close_flow.is_some(),
            "The final dirty reference asks before discarding"
        );
        app.choose_close(&ctx, close::Choice::Cancel);
        assert_eq!(app.active().page.text, "abcdef");
    }
    #[test]
    fn empty_shared_groups_can_close_without_creating_recovery_notes() {
        let dir = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.split(1);
        app.focus_group(1, false);
        assert!(app.command_enabled(Command::Close));
        app.execute(&ctx, Command::Close);
        assert_eq!(app.panes.groups(), 1);
        assert_eq!(app.workspace.open.len(), 1);
        assert!(app.workspace.closed.is_empty());
    }
    #[test]
    fn moves_and_layout_reductions_keep_every_tab_and_dirty_buffer() {
        let (_dir, _ctx, mut app) = app();
        let first = app.workspace.open[0].view.id;
        let second = app.workspace.open[1].view.id;
        app.split(1);
        app.move_group_tab(0, 1, second, 0);
        assert_eq!(app.group_tab_ids(0), vec![first]);
        assert_eq!(app.group_tab_ids(1), vec![second, first]);
        assert_eq!(app.active().view.id, second);
        app.split(0);
        let ids = app.group_tab_ids(0);
        assert!(ids.contains(&first) && ids.contains(&second));
        assert_eq!(ids.len(), 2);
        assert_eq!(app.workspace.open.len(), 2);
        assert_eq!(
            app.workspace
                .open
                .iter()
                .find(|b| b.view.id == first)
                .unwrap()
                .page
                .text,
            "abcdef"
        );
        assert!(app.workspace.closed.is_empty());
    }
    #[test]
    fn shared_edits_and_undo_transform_independent_group_cursors() {
        let (_dir, ctx, mut app) = app();
        app.workspace.open[0].page.text = "café / 東京 / 😀".into();
        app.workspace.edit(0, "abcdef").unwrap();
        app.split(1);
        app.focus_group(1, false);
        app.set_selections(vec![hedit::advanced::Selection::caret(13)]);
        frame(&ctx, &mut app, vec![]);
        app.focus_group(0, false);
        app.set_selections(vec![hedit::advanced::Selection::caret(0)]);
        frame(&ctx, &mut app, vec![egui::Event::Text("x".into())]);
        assert_eq!(app.active().page.text, "xcafé / 東京 / 😀");
        app.focus_group(1, false);
        assert_eq!(app.active().view.head, 14);
        app.execute(&ctx, Command::Undo);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, "café / 東京 / 😀");
        assert_eq!(app.workspace.open.len(), 2);
    }
    #[test]
    fn cancelling_a_preview_restores_its_group_without_moving_another_cursor() {
        let (_dir, ctx, mut app) = app();
        app.split(1);
        app.preview_line(&ctx, 1, Some(4));
        frame(&ctx, &mut app, vec![]);
        app.focus_group(1, false);
        app.set_selections(vec![hedit::advanced::Selection::caret(5)]);
        frame(&ctx, &mut app, vec![]);
        app.cancel_goto_preview();
        frame(&ctx, &mut app, vec![]);
        assert_eq!(app.active().view.head, 5);
        app.focus_group(0, false);
        assert_eq!(app.active().view.head, 0);
    }
    #[test]
    fn replacing_preview_tabs_keeps_documents_open_in_other_groups() {
        let (dir, ctx, mut app) = app();
        let mut ids = Vec::new();
        for name in ["one.txt", "two.txt", "three.txt"] {
            let path = dir.path().join(name);
            std::fs::write(&path, name).unwrap();
            app.open(&ctx, Some(path));
            finish_app_job(&mut app, &ctx);
            frame(&ctx, &mut app, vec![]);
            ids.push(app.active().view.id);
        }
        let index = app
            .workspace
            .open
            .iter()
            .position(|b| b.view.id == ids[0])
            .unwrap();
        app.select_tab(index);
        app.split(1);
        app.focus_group(1, false);
        for name in ["two.txt", "three.txt"] {
            app.quality.preview_request = true;
            app.open(&ctx, Some(dir.path().join(name)));
            finish_app_job(&mut app, &ctx);
            frame(&ctx, &mut app, vec![]);
        }
        assert!(app.group_tab_ids(0).contains(&ids[1]));
        assert!(!app.group_tab_ids(1).contains(&ids[1]));
        assert!(app
            .workspace
            .open
            .iter()
            .any(|b| b.view.id == ids[1] && b.page.text == "two.txt"));
    }
    #[test]
    fn group_membership_and_cursor_metadata_survive_checkpoint_restore() {
        let (dir, ctx, mut app) = app();
        app.split(1);
        app.focus_group(1, false);
        app.set_selections(vec![hedit::advanced::Selection::caret(4)]);
        frame(&ctx, &mut app, vec![]);
        app.workspace.checkpoint().unwrap();
        let saved = serde_json::to_vec(&app.panes).unwrap();
        let expected = app.group_tab_ids(1);
        drop(app);
        let (workspace, errors) = Workspace::restore(dir.path().join("session")).unwrap();
        assert!(errors.is_empty());
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.panes = serde_json::from_slice(&saved).unwrap();
        app.sync_groups();
        app.restore_focused_group();
        assert_eq!(app.group_tab_ids(1), expected);
        assert_eq!(app.active().page.text, "abcdef");
        assert_eq!(app.active().view.head, 4);
    }
}

#[cfg(test)]
mod drag_tests {
    use super::*;
    use crate::app::tests::{frame, text_rect};
    #[test]
    fn pointer_moves_between_groups_and_cancels_cleanly_outside_the_editor() {
        let dir = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
        workspace.new_buffer().unwrap();
        workspace.name_scratchpad(1, "moving.txt").unwrap();
        workspace.active = 0;
        let id = workspace.open[1].view.id;
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.split(1);
        for _ in 0..4 {
            frame(&ctx, &mut app, vec![]);
        }
        let out = frame(&ctx, &mut app, vec![]);
        let source = out
            .shapes
            .iter()
            .find_map(|s| text_rect(&s.shape, "moving.txt"))
            .unwrap()
            .center();
        let group = out
            .shapes
            .iter()
            .find_map(|s| text_rect(&s.shape, "Group 2"))
            .unwrap();
        let target = group.center() + egui::vec2(60.0, 100.0);
        frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(source),
                egui::Event::PointerButton {
                    pos: source,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        frame(
            &ctx,
            &mut app,
            vec![egui::Event::PointerMoved(source + egui::vec2(35.0, 0.0))],
        );
        frame(&ctx, &mut app, vec![egui::Event::PointerMoved(target)]);
        frame(
            &ctx,
            &mut app,
            vec![egui::Event::PointerButton {
                pos: target,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
        );
        assert!(!app.group_tab_ids(0).contains(&id));
        assert!(app.group_tab_ids(1).contains(&id));
        assert!(app.quality.tab_drag.is_none() && app.quality.tab_drag_group.is_none());
        app.quality.tab_drag = Some(id);
        app.quality.tab_drag_group = Some(1);
        let outside = egui::pos2(-20.0, -20.0);
        frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(outside),
                egui::Event::PointerButton {
                    pos: outside,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        assert!(app.quality.tab_drag.is_none() && app.quality.tab_drag_group.is_none());
        assert!(app.group_tab_ids(1).contains(&id));
        assert_eq!(app.workspace.open.len(), 2);
    }
}
