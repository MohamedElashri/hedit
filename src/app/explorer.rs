//! File navigation owns selection and drafts, never document contents.
use super::*;
use project::tree::Kind;
use std::path::Path;
mod badges;
#[derive(Default)]
pub(super) struct State {
    pub selected: Option<String>,
    pub scroll: bool,
    typed: String,
    typed_at: Option<Instant>,
    reveal: Option<(String, Vec<String>)>,
    next_entry: u64,
    pub badges: badges::State,
}
pub(super) enum EntryOutcome {
    Created(String, bool),
    Renamed(PathBuf, PathBuf),
}
fn selectable(kind: &Kind) -> bool {
    !matches!(kind, Kind::Message { .. } | Kind::Draft)
}
fn directory(kind: &Kind) -> bool {
    matches!(
        kind,
        Kind::Root
            | Kind::Entry {
                directory: true,
                ..
            }
    )
}
fn valid_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0', '\n', '\r'])
}
impl HeditApp {
    pub(super) fn focus_explorer(&mut self, ctx: &egui::Context) {
        self.sidebar = workbench_navigation::Sidebar::Files;
        self.settings.show_sidebar = true;
        self.settings.show_folders = true;
        self.distraction_free = false;
        if self.explorer.selected.is_none() {
            self.explorer.selected = self.workspace.folders.first().map(|f| f.path.clone());
        }
        self.explorer.scroll = true;
        self.focus_editor = false;
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("explorer-tree")));
    }
    pub(super) fn prepare_explorer(&mut self, ctx: &egui::Context) {
        if self.settings.explorer_auto_reveal {
            if let Some(path) = self.active().page.info.path.as_ref() {
                let changed = self.explorer.reveal.as_ref().is_none_or(|(prior, roots)| {
                    prior != path
                        || !roots.iter().map(String::as_str).eq(self
                            .workspace
                            .folders
                            .iter()
                            .map(|f| f.path.as_str()))
                });
                if changed {
                    let path = path.clone();
                    self.explorer.reveal = Some((
                        path.clone(),
                        self.workspace
                            .folders
                            .iter()
                            .map(|f| f.path.clone())
                            .collect(),
                    ));
                    self.reveal_path_in_sidebar(path.clone());
                    self.explorer.selected = Some(path);
                }
            }
        } else {
            self.explorer.reveal = None;
        }
        self.poll_explorer_badges(ctx);
    }
    fn selected_explorer_node(&self) -> Option<&project::tree::Node> {
        self.project
            .tree
            .as_ref()?
            .rows
            .iter()
            .find(|n| Some(&n.path) == self.explorer.selected.as_ref() && selectable(&n.kind))
    }
    pub(super) fn explorer_command(&mut self, ctx: &egui::Context, command: Command) {
        self.focus_explorer(ctx);
        let node = self.selected_explorer_node();
        let selected = node
            .map(|n| (n.path.clone(), directory(&n.kind)))
            .or_else(|| {
                self.workspace
                    .folders
                    .first()
                    .map(|f| (f.path.clone(), true))
            });
        let Some((path, is_directory)) = selected else {
            return;
        };
        if command == Command::ExplorerRename {
            self.start_explorer_entry(ctx, path, is_directory, true);
            return;
        }
        let parent = if is_directory {
            path
        } else {
            Path::new(&path)
                .parent()
                .unwrap_or(Path::new(&path))
                .to_string_lossy()
                .into_owned()
        };
        self.start_explorer_entry(ctx, parent, command == Command::ExplorerNewFolder, false);
    }
    pub(super) fn start_explorer_entry(
        &mut self,
        ctx: &egui::Context,
        path: String,
        directory: bool,
        rename: bool,
    ) {
        if self.busy || self.closing {
            return;
        }
        self.focus_explorer(ctx);
        self.explorer.next_entry = self.explorer.next_entry.wrapping_add(1);
        let (parent, name, source) = if rename {
            let p = PathBuf::from(path);
            (
                p.parent().unwrap_or(&p).to_string_lossy().into_owned(),
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                Some(p),
            )
        } else {
            (path, String::new(), None)
        };
        self.reveal_path_in_sidebar(parent.clone());
        self.project.expanded.insert(parent.clone());
        if let Some(root) = self.workspace.folders.iter_mut().find(|f| f.path == parent) {
            root.collapsed = false;
        }
        self.project.create = Some(project::CreateEntry {
            parent: parent.clone(),
            directory,
            name,
            focus: true,
            inline: true,
            rename: source,
            error: None,
            pending: false,
            token: self.explorer.next_entry,
        });
        self.project.tree = None;
        self.project.reveal = Some((parent, Instant::now() + Duration::from_secs(3)));
        self.sync_folder_view();
    }
    pub(super) fn explorer_entry_row(&mut self, ui: &mut egui::Ui, depth: usize) {
        let Some(draft) = self.project.create.as_mut() else {
            return;
        };
        let mut submit = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            ui.add_space((depth.min(2) * 12) as f32);
            let invalid = draft.error.is_some();
            let mut input = egui::TextEdit::singleline(&mut draft.name)
                .id(egui::Id::new("explorer-entry-name"))
                .hint_text("Name")
                .desired_width((ui.available_width() - 98.0).max(48.0));
            if invalid {
                input = input.text_color(self.theme.error.color());
            }
            let name = ui.add_enabled(!draft.pending, input);
            if name.changed() {
                draft.error = None;
            }
            if std::mem::take(&mut draft.focus) {
                name.request_focus();
            }
            if let Some(error) = &draft.error {
                name.clone().on_hover_text(error);
            }
            let enter = name.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            let label = if draft.rename.is_some() {
                "Rename"
            } else {
                "Create"
            };
            submit = ui
                .add_enabled(!draft.pending, egui::Button::new(label))
                .clicked()
                || enter;
            if ui.available_width() > 20.0 {
                cancel = ui
                    .add_enabled_ui(!draft.pending, |ui| {
                        controls::close_button(ui, "Cancel · Escape")
                    })
                    .inner
                    .clicked();
            }
        });
        if let Some(error) = self.project.create.as_ref().and_then(|d| d.error.as_ref()) {
            // Keep errors actionable without changing virtual row heights.
            self.notice = error.clone();
        }
        if cancel {
            self.project.create = None;
            self.project.tree = None;
        } else if submit {
            self.submit_explorer_entry(ui.ctx());
        }
    }
    pub(super) fn submit_explorer_entry(&mut self, ctx: &egui::Context) {
        if self.busy || self.closing {
            return;
        }
        let Some(draft) = self.project.create.as_mut() else {
            return;
        };
        if !valid_name(&draft.name) {
            draft.error = Some("Enter a file or folder name without path separators.".into());
            draft.focus = true;
            return;
        }
        let (token, parent, name, directory, rename) = (
            draft.token,
            draft.parent.clone(),
            draft.name.trim().to_owned(),
            draft.directory,
            draft.rename.clone(),
        );
        if rename.is_some() {
            self.opens.cancel();
            if let Err(error) = self.workspace.checkpoint() {
                let d = self.project.create.as_mut().unwrap();
                d.error = Some(error);
                d.focus = true;
                return;
            }
        }
        self.project.create.as_mut().unwrap().pending = true;
        self.job(ctx, move || {
            let result = (|| {
                if let Some(path) = rename {
                    let source = hedit::folder::entry_path(&path)?;
                    let target =
                        hedit::folder::rename_path(source.to_string_lossy().into_owned(), name)?;
                    Ok(EntryOutcome::Renamed(source, PathBuf::from(target)))
                } else {
                    let path = if directory {
                        hedit::folder::create_folder(parent, name)?
                    } else {
                        hedit::folder::create_file(parent, name)?
                    };
                    Ok(EntryOutcome::Created(path, directory))
                }
            })();
            Ok(Action::ExplorerEntry(token, result))
        });
    }
    pub(super) fn finish_explorer_entry(
        &mut self,
        ctx: &egui::Context,
        token: u64,
        outcome: Result<EntryOutcome, String>,
    ) {
        match outcome {
            Ok(outcome) => {
                if self
                    .project
                    .create
                    .as_ref()
                    .is_some_and(|d| d.token == token)
                {
                    self.project.create = None;
                }
                let path = match outcome {
                    EntryOutcome::Created(path, directory) => {
                        if directory {
                            self.project.expanded.insert(path.clone());
                        } else {
                            self.pending_paths.push_back(PathBuf::from(&path));
                        }
                        self.refresh_folder();
                        path
                    }
                    EntryOutcome::Renamed(from, to) => {
                        let path = to.to_string_lossy().into_owned();
                        self.quality_event(ctx, quality::Event::FileChanged(from, Some(to)));
                        path
                    }
                };
                self.explorer.selected = Some(path.clone());
                self.project.reveal = Some((path, Instant::now() + Duration::from_secs(3)));
                self.changed();
            }
            Err(error) => {
                if let Some(d) = self.project.create.as_mut().filter(|d| d.token == token) {
                    d.pending = false;
                    d.error = Some(error);
                    d.focus = true;
                } else {
                    self.error = Some(error);
                }
            }
        }
    }
    pub(super) fn toggle_explorer_node(&mut self, node: &project::tree::Node) {
        if matches!(node.kind, Kind::Root) {
            if let Some(root) = self
                .workspace
                .folders
                .iter_mut()
                .find(|r| r.path == node.path)
            {
                root.collapsed = !root.collapsed;
            }
        } else if node.expanded {
            let origin = match &node.kind {
                Kind::Entry {
                    origin: Some(origin),
                    ..
                } => origin.as_str(),
                _ => node.path.as_str(),
            };
            self.project.expanded.retain(|p| {
                !(Path::new(p).starts_with(origin) && Path::new(&node.path).starts_with(p))
            });
        } else {
            self.project.expanded.insert(node.path.clone());
        }
        self.project.tree = None;
        self.sync_folder_view();
    }
    pub(super) fn explorer_shortcuts(&mut self, ctx: &egui::Context) -> bool {
        if self.busy
            || self.closing
            || self.open_menu.is_some()
            || ctx.memory(|m| m.any_popup_open())
        {
            return false;
        }
        let focused = ctx.memory(|m| m.focused());
        if focused == Some(egui::Id::new("explorer-entry-name")) {
            if tab_switch::take_exact(ctx, Modifiers::NONE, Key::Escape) {
                self.project.create = None;
                self.project.tree = None;
                self.focus_explorer(ctx);
                return true;
            }
            return false;
        }
        if focused != Some(egui::Id::new("explorer-tree"))
            || self.sidebar != workbench_navigation::Sidebar::Files
            || !self.settings.show_sidebar
        {
            return false;
        }
        // Global shortcuts and explicitly configured bindings keep priority in coalesced input.
        let configured = self.configured_bindings();
        let claimed = ctx.input(|i| {
            i.events.iter().any(|event| {
                let egui::Event::Key {
                    key,
                    modifiers,
                    pressed: true,
                    ..
                } = event
                else {
                    return false;
                };
                modifiers.command
                    || modifiers.ctrl
                    || modifiers.alt
                    || configured.iter().filter(|b| !b.editor_only).any(|b| {
                        chords::parse(&b.shortcut)
                            .ok()
                            .and_then(|k| k.first().copied())
                            .is_some_and(|s| {
                                s.logical_key == *key && modifiers.matches_exact(s.modifiers)
                            })
                    })
            })
        });
        drop(configured);
        if claimed {
            return false;
        }
        if tab_switch::take_exact(ctx, Modifiers::NONE, Key::Escape) {
            ctx.memory_mut(|m| m.surrender_focus(egui::Id::new("explorer-tree")));
            self.focus_editor = true;
            return true;
        }
        let Some(tree) = self.project.tree.clone() else {
            return false;
        };
        let valid: Vec<_> = tree
            .rows
            .iter()
            .enumerate()
            .filter(|(_, n)| selectable(&n.kind))
            .map(|(i, _)| i)
            .collect();
        if valid.is_empty() {
            return false;
        }
        let at = valid
            .iter()
            .position(|&i| Some(&tree.rows[i].path) == self.explorer.selected.as_ref())
            .unwrap_or(0);
        let node = &tree.rows[valid[at]];
        let mut target = None;
        for (key, offset) in [
            (Key::ArrowDown, 1isize),
            (Key::ArrowUp, -1),
            (Key::PageDown, 10),
            (Key::PageUp, -10),
        ] {
            if tab_switch::take_exact(ctx, Modifiers::NONE, key) {
                target = Some((at as isize + offset).clamp(0, valid.len() as isize - 1) as usize);
                break;
            }
        }
        if tab_switch::take_exact(ctx, Modifiers::NONE, Key::Home) {
            target = Some(0);
        }
        if tab_switch::take_exact(ctx, Modifiers::NONE, Key::End) {
            target = Some(valid.len() - 1);
        }
        if tab_switch::take_exact(ctx, Modifiers::NONE, Key::ArrowRight) {
            if directory(&node.kind) {
                if !node.expanded {
                    self.toggle_explorer_node(node);
                } else if at + 1 < valid.len() && tree.rows[valid[at + 1]].depth > node.depth {
                    target = Some(at + 1);
                }
            }
            if target.is_none() {
                return true;
            }
        }
        if tab_switch::take_exact(ctx, Modifiers::NONE, Key::ArrowLeft) {
            if directory(&node.kind) && node.expanded {
                self.toggle_explorer_node(node);
                return true;
            }
            target = (0..at)
                .rev()
                .find(|&i| tree.rows[valid[i]].depth < node.depth);
            if target.is_none() {
                return true;
            }
        }
        if let Some(target) = target {
            self.explorer.selected = Some(tree.rows[valid[target]].path.clone());
            self.explorer.scroll = true;
            return true;
        }
        if tab_switch::take_exact(ctx, Modifiers::NONE, Key::F2) {
            if !self.personally_bound(Command::ExplorerRename) {
                self.start_explorer_entry(ctx, node.path.clone(), directory(&node.kind), true);
            }
            return true;
        }
        if tab_switch::take_exact(ctx, Modifiers::NONE, Key::Delete) {
            self.begin_file_action(PathBuf::from(&node.path), quality::FileOperation::Trash);
            return true;
        }
        if tab_switch::take_exact(ctx, Modifiers::NONE, Key::Enter)
            || tab_switch::take_exact(ctx, Modifiers::NONE, Key::Space)
        {
            if directory(&node.kind) {
                self.toggle_explorer_node(node);
            } else {
                self.quality.preview_request = false;
                self.pending_paths.push_back(PathBuf::from(&node.path));
            }
            return true;
        }
        let typed = ctx.input(|i| {
            if i.modifiers.command || i.modifiers.ctrl || i.modifiers.alt {
                String::new()
            } else {
                i.events
                    .iter()
                    .filter_map(|e| {
                        if let egui::Event::Text(t) = e {
                            Some(t.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<String>()
            }
        });
        if !typed.is_empty() && typed.chars().all(|c| !c.is_control()) {
            if self
                .explorer
                .typed_at
                .is_none_or(|t| t.elapsed() > Duration::from_millis(800))
            {
                self.explorer.typed.clear();
            }
            self.explorer.typed.extend(
                typed
                    .chars()
                    .take(32usize.saturating_sub(self.explorer.typed.chars().count())),
            );
            self.explorer.typed_at = Some(Instant::now());
            if let Some(next) = (1..=valid.len())
                .map(|n| (at + n) % valid.len())
                .find(|&i| {
                    tree.rows[valid[i]]
                        .name
                        .to_lowercase()
                        .starts_with(&self.explorer.typed.to_lowercase())
                })
            {
                self.explorer.selected = Some(tree.rows[valid[next]].path.clone());
                self.explorer.scroll = true;
            }
            ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Text(_))));
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests;
