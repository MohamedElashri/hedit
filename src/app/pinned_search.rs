//! Read-only snapshots of folder searches, selectable in each editor group.
use super::*;
use hedit::search;
use std::collections::HashMap;

const MAX_TABS: usize = 8;
const SNAPSHOT_BYTES: usize = 4 * 1024 * 1024;

struct Tab {
    id: u64,
    query: String,
    roots: Vec<PathBuf>,
    hits: Vec<search::Hit>,
    truncated: bool,
}

#[derive(Default)]
pub(super) struct Tabs {
    next: u64,
    tabs: Vec<Tab>,
    active: HashMap<u64, u64>,
    reveal: Option<(u64, u64)>,
}

impl Tabs {
    fn pin(&mut self, group: u64, query: &str, roots: &[PathBuf], results: &search::Results) {
        let mut budget = SNAPSHOT_BYTES;
        let mut hits = Vec::new();
        for hit in &results.hits {
            let text: String = hit.text.chars().take(320).collect();
            let bytes =
                hit.path.as_os_str().len() + text.len() + std::mem::size_of::<search::Hit>();
            if bytes > budget {
                break;
            }
            budget -= bytes;
            hits.push(search::Hit {
                path: hit.path.clone(),
                text,
                ..*hit
            });
        }
        self.next = self.next.wrapping_add(1);
        self.tabs.push(Tab {
            id: self.next,
            query: query.into(),
            roots: roots.to_vec(),
            truncated: results.truncated || hits.len() < results.hits.len(),
            hits,
        });
        self.active.insert(group, self.next);
        self.reveal = Some((group, self.next));
    }

    fn close(&mut self, id: u64) {
        self.tabs.retain(|tab| tab.id != id);
        self.active.retain(|_, active| *active != id);
    }

    pub(super) fn retain_groups(&mut self, valid: impl Fn(u64) -> bool) {
        self.active.retain(|id, _| valid(*id));
    }
}

impl HeditApp {
    pub(super) fn can_pin_search(&self) -> bool {
        self.pinned_search.tabs.len() < MAX_TABS
            && matches!(self.quality.folder_state, folder_search::State::Ready)
            && self.quality.folder_snapshot.is_some()
            && !self.quality.results.hits.is_empty()
    }

    pub(super) fn pin_search(&mut self, ctx: &egui::Context) {
        if !self.can_pin_search() {
            return;
        }
        self.sync_groups();
        self.leave_review();
        let Some((query, _, _)) = &self.quality.folder_snapshot else {
            return;
        };
        self.pinned_search.pin(
            self.panes.focused_id(),
            query,
            &self.quality.folder_roots,
            &self.quality.results,
        );
        self.quality.folders_open = false;
        self.focus_editor = false;
        ctx.request_repaint();
    }

    pub(super) fn showing_pinned_search(&self) -> bool {
        self.pinned_search
            .active
            .contains_key(&self.panes.focused_id())
    }

    pub(super) fn leave_pinned_search(&mut self) {
        self.pinned_search.active.remove(&self.panes.focused_id());
    }

    pub(super) fn close_pinned_search(&mut self) -> bool {
        let group = self.panes.focused_id();
        let Some(id) = self.pinned_search.active.get(&group).copied() else {
            return false;
        };
        self.pinned_search.close(id);
        self.focus_editor = true;
        true
    }

    pub(super) fn pinned_search_tab_strip(&mut self, ui: &mut egui::Ui) -> bool {
        let group = self.panes.focused_id();
        let selected = self.pinned_search.active.get(&group).copied();
        let mut activate = None;
        let mut close = None;
        for tab in &self.pinned_search.tabs {
            ui.push_id(("pinned-search", tab.id), |ui| {
                let mut title: String = tab.query.chars().take(42).collect();
                if title.len() < tab.query.len() {
                    title.push('…');
                }
                let response = ui
                    .selectable_label(selected == Some(tab.id), format!("Search: {title}"))
                    .on_hover_text(&tab.query);
                if self.pinned_search.reveal == Some((group, tab.id)) {
                    response.scroll_to_me(Some(egui::Align::Max));
                    self.pinned_search.reveal = None;
                }
                if response.clicked() {
                    activate = Some(tab.id);
                }
                if controls::close_button(ui, "Close pinned search").clicked()
                    || response.clicked_by(egui::PointerButton::Middle)
                {
                    close = Some(tab.id);
                }
            });
        }
        if let Some(id) = close {
            self.pinned_search.close(id);
        }
        if let Some(id) = activate.filter(|id| Some(*id) != close) {
            self.leave_review();
            self.pinned_search.active.insert(group, id);
            self.focus_editor = false;
            true
        } else {
            false
        }
    }

    pub(super) fn pinned_search_pane(&mut self, ui: &mut egui::Ui) -> bool {
        let group = self.panes.focused_id();
        let Some(id) = self.pinned_search.active.get(&group) else {
            return false;
        };
        let Some(tab) = self.pinned_search.tabs.iter().find(|tab| tab.id == *id) else {
            return false;
        };
        let mut close = false;
        ui.horizontal_wrapped(|ui| {
            ui.strong(&tab.query);
            close = ui.small_button("Close Results").clicked();
            ui.weak(format!("{} matches", tab.hits.len()));
            if tab.truncated {
                ui.weak("Result limit reached");
            }
        });
        if close {
            self.close_pinned_search();
            return true;
        }
        let mut destination = None;
        let height = ui.text_style_height(&egui::TextStyle::Body).max(18.0) + 6.0;
        egui::ScrollArea::vertical()
            .id_salt(("pinned-search-rows", group, tab.id))
            .auto_shrink([false, false])
            .show_rows(ui, height, tab.hits.len(), |ui, visible| {
                for hit in &tab.hits[visible] {
                    let location = format!("{}:{}:{}", hit.path.display(), hit.line, hit.column);
                    let relative = hedit::folder::display_path(&tab.roots, &hit.path);
                    let label = format!(
                        "{relative}:{}:{}  {}",
                        hit.line,
                        hit.column,
                        hit.text.replace(['\r', '\n'], " ")
                    );
                    if ui
                        .add_enabled(
                            !self.busy && !self.closing,
                            egui::Button::new(label)
                                .truncate()
                                .min_size(egui::vec2(ui.available_width(), height)),
                        )
                        .on_hover_text(format!("{location}\n{}", hit.text))
                        .clicked()
                    {
                        destination = Some((hit.path.clone(), hit.line, hit.column));
                    }
                }
            });
        if let Some((path, line, column)) = destination {
            self.leave_pinned_search();
            self.open_existing_location(ui.ctx(), path, line as u64, column as u64);
        }
        true
    }
}

#[cfg(test)]
#[test]
fn pinned_searches_hold_independent_snapshots_in_different_groups() {
    let mut tabs = Tabs::default();
    let mut results = search::Results {
        hits: vec![search::Hit {
            path: "one.rs".into(),
            line: 1,
            column: 2,
            text: "first".into(),
        }],
        ..Default::default()
    };
    tabs.pin(1, "first", &[], &results);
    results.hits[0].text = "second".into();
    tabs.pin(2, "second", &[], &results);
    assert_ne!(tabs.active[&1], tabs.active[&2]);
    assert_eq!(tabs.tabs[0].hits[0].text, "first");
    tabs.close(tabs.active[&1]);
    assert!(!tabs.active.contains_key(&1));
    assert_eq!(tabs.tabs[0].hits[0].text, "second");
}

#[cfg(test)]
#[test]
fn pinned_searches_stay_selected_across_editor_group_switches() {
    let directory = tempfile::tempdir().unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.split(1);
    app.quality.folder_state = folder_search::State::Ready;
    app.quality.folder_snapshot = Some(("first".into(), String::new(), search::Options::default()));
    app.quality.results.hits.push(search::Hit {
        path: directory.path().join("one.rs"),
        line: 1,
        column: 1,
        text: "first".into(),
    });
    app.pin_search(&ctx);
    let first_group = app.panes.focused_id();
    app.focus_group(1, false);
    app.quality.folder_snapshot.as_mut().unwrap().0 = "second".into();
    app.pin_search(&ctx);
    let second_group = app.panes.focused_id();
    super::tests::frame(&ctx, &mut app, vec![]);
    assert_ne!(first_group, second_group);
    assert_ne!(
        app.pinned_search.active[&first_group],
        app.pinned_search.active[&second_group]
    );
    assert_eq!(app.workspace.open.len(), 1);
}

#[cfg(test)]
#[test]
fn pinned_result_view_blocks_edits_to_the_hidden_document() {
    let directory = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    workspace.open[0].page.text = "keep this line\nsecond line\n".into();
    workspace.edit(0, "").unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.quality.folder_state = folder_search::State::Ready;
    app.quality.folder_snapshot = Some(("keep".into(), String::new(), search::Options::default()));
    app.quality.results.hits.push(search::Hit {
        path: directory.path().join("one.txt"),
        line: 1,
        column: 1,
        text: "keep this line".into(),
    });
    app.pin_search(&ctx);
    assert!(app.showing_pinned_search());
    app.execute(&ctx, Command::DeleteLine);
    assert_eq!(app.active().page.text, "keep this line\nsecond line\n");
    assert!(app.showing_pinned_search());
    assert!(!app.command_enabled(Command::Undo));
    assert!(!app.command_enabled(Command::ReplaceAll));
}
