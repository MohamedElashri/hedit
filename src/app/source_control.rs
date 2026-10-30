//! The native Git workspace. Git and directory dialogs never block painting.
use super::*;
use hedit::git::{self, Diff, DiffKind, FileStatus, Git, Operation, Selection, Snapshot};
use std::{collections::BTreeMap, path::Path};
#[cfg(test)]
#[path = "source_control_tests.rs"]
mod tests;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum View {
    #[default]
    Changes,
    History,
    Branches,
    Stashes,
}
pub(super) enum Prompt {
    Confirm {
        title: String,
        detail: String,
        operation: Operation,
    },
    Branch(String),
    Stash(String),
    Clone {
        url: String,
        destination: String,
    },
    Remote {
        name: String,
        url: String,
    },
}
struct Reply {
    repositories: Vec<PathBuf>,
    root: Option<PathBuf>,
    snapshot: Result<Snapshot, String>,
    selection: Option<Selection>,
    diff: Option<Result<Diff, String>>,
    commit: Option<String>,
    files: Vec<FileStatus>,
    body: String,
    outcome: Option<Result<String, String>>,
    committed: Option<String>,
}
pub(super) struct SourceControl {
    pub open: bool,
    pub view: View,
    pub prompt: Option<Prompt>,
    pub pending: bool,
    root: Option<PathBuf>,
    repos: Vec<PathBuf>,
    snapshot: Option<Arc<Snapshot>>,
    selection: Option<Selection>,
    diff: Option<Arc<Diff>>,
    commit: Option<String>,
    files: Vec<FileStatus>,
    body: String,
    drafts: BTreeMap<PathBuf, String>,
    draft: String,
    amend: bool,
    filter: String,
    limit: usize,
    error: Option<String>,
    output: VecDeque<String>,
    output_open: bool,
    split_diff: bool,
    last_refresh: Instant,
    refresh: bool,
    cancel: Arc<AtomicBool>,
    tx: Sender<Reply>,
    rx: Receiver<Reply>,
}
impl Default for SourceControl {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            open: false,
            view: View::default(),
            prompt: None,
            pending: false,
            root: None,
            repos: vec![],
            snapshot: None,
            selection: None,
            diff: None,
            commit: None,
            files: vec![],
            body: String::new(),
            drafts: BTreeMap::new(),
            draft: String::new(),
            amend: false,
            filter: String::new(),
            limit: 200,
            error: None,
            output: VecDeque::new(),
            output_open: false,
            split_diff: true,
            last_refresh: Instant::now(),
            refresh: false,
            cancel: Arc::new(AtomicBool::new(false)),
            tx,
            rx,
        }
    }
}
impl Drop for SourceControl {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl SourceControl {
    pub(super) fn load(storage: Option<&dyn eframe::Storage>) -> Self {
        let drafts: BTreeMap<String, String> = storage
            .and_then(|s| eframe::get_value(s, "git-commit-drafts"))
            .unwrap_or_default();
        let mut state = Self::default();
        state.drafts.extend(
            drafts
                .into_iter()
                .map(|(root, message)| (PathBuf::from(root), message)),
        );
        state
    }
    pub(super) fn save(&self, storage: &mut dyn eframe::Storage) {
        let mut drafts = self.drafts.clone();
        if let Some(root) = &self.root {
            drafts.insert(root.clone(), self.draft.clone());
        }
        let drafts: BTreeMap<_, _> = drafts
            .into_iter()
            .filter(|(_, message)| !message.is_empty())
            .filter_map(|(root, message)| root.to_str().map(|root| (root.to_owned(), message)))
            .collect();
        eframe::set_value(storage, "git-commit-drafts", &drafts);
    }
    pub(super) fn due(&self) -> Option<Duration> {
        if self.open && !self.pending && self.root.is_some() && self.prompt.is_none() {
            Some(if self.refresh {
                Duration::ZERO
            } else {
                Duration::from_secs(4).saturating_sub(self.last_refresh.elapsed())
            })
        } else {
            None
        }
    }
    fn request(&mut self, ctx: &egui::Context, operation: Option<Operation>) {
        if self.pending {
            self.refresh = true;
            return;
        }
        let Some(directory) = self.root.clone() else {
            return;
        };
        self.pending = true;
        self.refresh = false;
        if operation.is_some() {
            self.error = None;
        }
        self.cancel = Arc::new(AtomicBool::new(false));
        let git = Git {
            cancel: self.cancel.clone(),
        };
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let selection = self.selection.clone();
        let commit = self.commit.clone();
        let limit = self.limit;
        thread::spawn(move || {
            let root = git.discover(&directory);
            let target = root.as_deref().unwrap_or(&directory).to_owned();
            let committed = match &operation {
                Some(Operation::Commit { message, .. }) => Some(message.clone()),
                _ => None,
            };
            let outcome = operation.map(|op| git.execute(&target, &op));
            let root = git.discover(&directory);
            // Cancellation may have happened after a successful mutation. A later
            // explicit refresh reads the actual repository instead of guessing.
            let snapshot = root
                .as_ref()
                .map_err(Clone::clone)
                .and_then(|root| git.snapshot(root, limit));
            let files = commit
                .as_ref()
                .and_then(|id| git.commit_files(&target, id).ok())
                .unwrap_or_default();
            let body = commit
                .as_ref()
                .and_then(|id| git.commit_message(&target, id).ok())
                .unwrap_or_default();
            let diff = selection.as_ref().map(|s| git.diff(&target, s));
            let _ = tx.send(Reply {
                repositories: vec![],
                root: root.ok(),
                snapshot,
                selection,
                diff,
                commit,
                files,
                body,
                outcome,
                committed,
            });
            ctx.request_repaint();
        });
    }
    fn change_root(&mut self, ctx: &egui::Context, root: PathBuf) {
        if self.pending {
            return;
        }
        if let Some(old) = &self.root {
            self.drafts.insert(old.clone(), self.draft.clone());
        }
        self.draft = self.drafts.get(&root).cloned().unwrap_or_default();
        self.root = Some(root);
        self.snapshot = None;
        self.selection = None;
        self.diff = None;
        self.commit = None;
        self.files.clear();
        self.body.clear();
        self.amend = false;
        self.request(ctx, None);
    }
    fn select(&mut self, ctx: &egui::Context, selection: Selection) {
        if self.selection.as_ref() != Some(&selection) {
            self.selection = Some(selection);
            self.diff = None;
            self.request(ctx, None);
        }
    }
    fn log(&mut self, message: String) {
        if self.output.len() == 40 {
            self.output.pop_front();
        }
        self.output.push_back(message);
    }
}

impl HeditApp {
    pub(super) fn open_source_control(&mut self, ctx: &egui::Context) {
        self.close_transient(ctx);
        self.source_control.open = true;
        self.focus_editor = false;
        ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
        if self.source_control.root.is_none() && !self.source_control.pending {
            let mut directories: Vec<_> = self
                .workspace
                .folders
                .iter()
                .map(|f| PathBuf::from(&f.path))
                .collect();
            if let Some(path) = &self.active().page.info.path {
                if let Some(parent) = Path::new(path).parent() {
                    directories.insert(0, parent.to_owned());
                }
            }
            directories.sort();
            directories.dedup();
            if !directories.is_empty() {
                self.discover_source_control(ctx, directories);
            }
        } else {
            self.source_control.refresh = true;
        }
    }
    fn discover_source_control(&mut self, ctx: &egui::Context, directories: Vec<PathBuf>) {
        if self.source_control.pending {
            return;
        }
        self.source_control.pending = true;
        self.source_control.cancel = Arc::new(AtomicBool::new(false));
        let git = Git {
            cancel: self.source_control.cancel.clone(),
        };
        let tx = self.source_control.tx.clone();
        let ctx = ctx.clone();
        // Use a separate result channel for the directory picker/discovery so
        // no filesystem probing or Git runs on the UI thread.
        thread::spawn(move || {
            let mut roots = Vec::new();
            let mut error = None;
            for directory in &directories {
                match git.discover(directory) {
                    Ok(root) => {
                        if !roots.contains(&root) {
                            roots.push(root);
                        }
                    }
                    Err(e) => error = Some(e),
                }
            }
            let root = roots
                .first()
                .cloned()
                .or_else(|| directories.first().cloned());
            let snapshot = root
                .as_ref()
                .ok_or_else(|| "Choose a repository folder to begin.".into())
                .and_then(|r| git.snapshot(r, 200));
            let body = String::new();
            let snapshot = snapshot.map_err(|e| error.unwrap_or(e));
            let _ = tx.send(Reply {
                repositories: roots,
                root,
                snapshot,
                selection: None,
                diff: None,
                commit: None,
                files: vec![],
                body,
                outcome: None,
                committed: None,
            });
            ctx.request_repaint();
        });
    }
    fn browse_git_repository(&mut self, ctx: &egui::Context) {
        if self.source_control.pending {
            return;
        }
        self.source_control.pending = true;
        let tx = self.source_control.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let directory = rfd::FileDialog::new()
                .set_title("Open Git repository")
                .pick_folder();
            let git = Git::default();
            let root = directory
                .as_ref()
                .and_then(|d| git.discover(d).ok())
                .or(directory);
            let snapshot = root
                .as_ref()
                .ok_or_else(|| "Repository selection cancelled.".into())
                .and_then(|r| git.snapshot(r, 200));
            let _ = tx.send(Reply {
                repositories: vec![],
                root,
                snapshot,
                selection: None,
                diff: None,
                commit: None,
                files: vec![],
                body: String::new(),
                outcome: None,
                committed: None,
            });
            ctx.request_repaint();
        });
    }
    pub(super) fn poll_source_control(&mut self, ctx: &egui::Context) {
        while let Ok(reply) = self.source_control.rx.try_recv() {
            let state = &mut self.source_control;
            state.pending = false;
            state.last_refresh = Instant::now();
            if let Some(root) = reply.root {
                if state.root.as_ref() != Some(&root) {
                    if let Some(old) = &state.root {
                        state.drafts.insert(old.clone(), state.draft.clone());
                    }
                    state.draft = state.drafts.get(&root).cloned().unwrap_or_default();
                    state.root = Some(root.clone());
                    state.selection = None;
                    state.diff = None;
                    state.commit = None;
                    state.snapshot = None;
                    state.files.clear();
                    state.body.clear();
                    state.error = None;
                    state.amend = false;
                }
                if !state.repos.contains(&root) {
                    state.repos.push(root);
                }
            }
            if let Some(outcome) = reply.outcome {
                match outcome {
                    Ok(message) => {
                        state.log(message);
                        if reply.committed.as_ref() == Some(&state.draft) {
                            state.draft.clear();
                            state.amend = false;
                        }
                    }
                    Err(error) => {
                        state.log(error.clone());
                        state.error = Some(error);
                    }
                }
            }
            match reply.snapshot {
                Ok(snapshot) => {
                    state.snapshot = Some(Arc::new(snapshot));
                    for root in reply.repositories {
                        if !state.repos.contains(&root) {
                            state.repos.push(root);
                        }
                    }
                    if state.selection == reply.selection {
                        if let Some(diff) = reply.diff {
                            match diff {
                                Ok(diff) => state.diff = Some(Arc::new(diff)),
                                Err(error) => {
                                    state.diff = None;
                                    state.error = Some(error);
                                }
                            }
                        }
                    }
                    if state.commit == reply.commit && reply.commit.is_some() {
                        state.files = reply.files;
                        state.body = reply.body;
                    }
                }
                Err(error) => {
                    if state.error.is_none() {
                        state.error = Some(error);
                    }
                }
            }
        }
        if self.source_control.due() == Some(Duration::ZERO) {
            self.source_control.request(ctx, None);
        }
    }
    fn git_unsaved(&self, paths: Option<&[PathBuf]>) -> Vec<String> {
        let Some(root) = &self.source_control.root else {
            return vec![];
        };
        self.workspace
            .open
            .iter()
            .filter(|b| b.page.info.dirty)
            .filter_map(|b| {
                let path = b.page.info.path.as_ref()?;
                let relative = Path::new(path).strip_prefix(root).ok()?;
                if paths.is_none_or(|p| p.iter().any(|p| p == relative)) {
                    Some(b.view.name.clone())
                } else {
                    None
                }
            })
            .collect()
    }
    fn git_action(&mut self, ctx: &egui::Context, operation: Operation) {
        if self.source_control.pending || self.busy || self.closing {
            return;
        }
        let unsaved = if operation.changes_worktree() {
            self.git_unsaved(None)
        } else if let Operation::Stage(paths) = &operation {
            self.git_unsaved(Some(paths))
        } else if let Operation::Hunk { selection, .. } = &operation {
            self.git_unsaved(Some(std::slice::from_ref(&selection.path)))
        } else {
            vec![]
        };
        if !unsaved.is_empty() {
            self.source_control.error = Some(format!("Save or close these edited files before this Git action: {}. Your editor text has been kept.", unsaved.join(", ")));
            return;
        }
        self.source_control.request(ctx, Some(operation));
    }
    fn git_confirm(&mut self, title: &str, detail: String, operation: Operation) {
        self.source_control.prompt = Some(Prompt::Confirm {
            title: title.into(),
            detail,
            operation,
        });
    }
    pub(super) fn source_control_sidebar(&mut self, ctx: &egui::Context) {
        let theme = self.theme.clone();
        egui::SidePanel::left("source-control")
            .default_width(310.0)
            .min_width(260.0)
            .max_width(520.0)
            .frame(
                egui::Frame::new()
                    .fill(theme.panel.color())
                    .inner_margin(egui::Margin::symmetric(8, 10)),
            )
            .show(ctx, |ui| {
                if controls::workspace_switcher(
                    ui,
                    &theme,
                    true,
                    &self.shortcut_text(Command::SourceControl),
                ) == Some(false)
                {
                    self.source_control.open = false;
                    self.focus_editor = true;
                    ctx.memory_mut(|m| {
                        if let Some(id) = m.focused() {
                            m.surrender_focus(id);
                        }
                    });
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Repository")
                            .small()
                            .color(theme.muted.color()),
                    );
                    ui.add_enabled_ui(!self.source_control.pending, |ui| {
                        ui.menu_button("…", |ui| {
                            if ui.button("Clone repository…").clicked() {
                                self.source_control.prompt = Some(Prompt::Clone {
                                    url: String::new(),
                                    destination: String::new(),
                                });
                                ui.close_menu();
                            }
                            if ui.button("Open repository…").clicked() {
                                self.browse_git_repository(ctx);
                                ui.close_menu();
                            }
                        });
                    });
                    if ui
                        .add_enabled(
                            !self.source_control.pending,
                            egui::Button::new("Refresh").small(),
                        )
                        .clicked()
                    {
                        self.source_control.request(ctx, None);
                    }
                });
                ui.add_space(10.0);
                let pending = self.source_control.pending || self.busy || self.closing;
                let mut chosen = None;
                ui.add_enabled_ui(!pending, |ui| {
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("git-repository")
                            .width((ui.available_width() - 40.0).max(100.0))
                            .selected_text(
                                self.source_control
                                    .root
                                    .as_deref()
                                    .map(repo_name)
                                    .unwrap_or_else(|| "Repository".into()),
                            )
                            .show_ui(ui, |ui| {
                                for root in &self.source_control.repos {
                                    if ui
                                        .selectable_label(
                                            self.source_control.root.as_ref() == Some(root),
                                            repo_name(root),
                                        )
                                        .on_hover_text(root.display().to_string())
                                        .clicked()
                                    {
                                        chosen = Some(root.clone());
                                    }
                                }
                            });
                        if ui
                            .button("…")
                            .on_hover_text("Open another repository")
                            .clicked()
                        {
                            self.browse_git_repository(ctx);
                        }
                    });
                });
                if let Some(root) = chosen {
                    self.source_control.change_root(ctx, root);
                }
                let Some(snapshot) = self.source_control.snapshot.clone() else {
                    ui.add_space(18.0);
                    ui.label("Open a repository to review and commit changes.");
                    if !pending && ui.button("Open repository…").clicked() {
                        self.browse_git_repository(ctx);
                    }
                    if self.source_control.root.is_some()
                        && !pending
                        && ui.button("Initialize repository here…").clicked()
                    {
                        self.git_confirm(
                            "Initialize repository",
                            format!(
                                "Create a Git repository in {}?",
                                self.source_control.root.as_ref().unwrap().display()
                            ),
                            Operation::Init,
                        );
                    }
                    return;
                };
                ui.add_space(7.0);
                ui.horizontal_wrapped(|ui| {
                    branch_icon(ui, theme.accent.color());
                    ui.label(
                        RichText::new(if snapshot.branch == "(detached)" {
                            format!("Detached · {}", &snapshot.oid[..snapshot.oid.len().min(8)])
                        } else {
                            snapshot.branch.clone()
                        })
                        .strong(),
                    );
                    if !snapshot.upstream.is_empty() {
                        ui.label(
                            RichText::new(format!("↑{} ↓{}", snapshot.ahead, snapshot.behind))
                                .color(theme.muted.color()),
                        )
                        .on_hover_text(&snapshot.upstream);
                    }
                });
                ui.add_space(6.0);
                ui.add_enabled_ui(!pending && !snapshot.remotes.is_empty(), |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .small_button("Fetch")
                            .on_hover_text("Fetch all remotes and prune deleted remote references")
                            .clicked()
                        {
                            self.git_action(ctx, Operation::Fetch);
                        }
                        if ui
                            .add_enabled(
                                !snapshot.upstream.is_empty(),
                                egui::Button::new("Pull").small(),
                            )
                            .on_hover_text("Pull upstream using fast-forward only")
                            .clicked()
                        {
                            self.git_action(ctx, Operation::Pull);
                        }
                        if ui
                            .add_enabled(
                                !snapshot.upstream.is_empty(),
                                egui::Button::new("Push").small(),
                            )
                            .on_hover_text("Push using your Git configuration")
                            .clicked()
                        {
                            self.git_action(ctx, Operation::Push);
                        }
                        if snapshot.upstream.is_empty()
                            && !snapshot.unborn
                            && snapshot.branch != "(detached)"
                        {
                            ui.menu_button("Publish", |ui| {
                                for remote in &snapshot.remotes {
                                    if ui.button(remote).clicked() {
                                        self.git_action(
                                            ctx,
                                            Operation::Publish {
                                                remote: remote.clone(),
                                                branch: snapshot.branch.clone(),
                                            },
                                        );
                                        ui.close_menu();
                                    }
                                }
                            });
                        }
                    });
                });
                ui.add_space(10.0);
                let mut view = self.source_control.view;
                ui.horizontal_wrapped(|ui| {
                    for (v, label) in [
                        (View::Changes, "Changes"),
                        (View::History, "History"),
                        (View::Branches, "Branches"),
                        (View::Stashes, "Stashes"),
                    ] {
                        ui.selectable_value(&mut view, v, label);
                    }
                });
                if view != self.source_control.view {
                    self.source_control.view = view;
                    self.source_control.filter.clear();
                    self.source_control.selection = None;
                    self.source_control.diff = None;
                    if view != View::History {
                        self.source_control.commit = None;
                    }
                }
                ui.separator();
                if self.source_control.view == View::Changes {
                    self.git_commit_box(ui, ctx, &snapshot);
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.source_control.filter)
                        .hint_text("Filter…")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .id_salt("git-sidebar-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.add_enabled_ui(!pending && self.source_control.prompt.is_none(), |ui| {
                            match self.source_control.view {
                                View::Changes => self.git_changes(ui, ctx, &snapshot),
                                View::History => self.git_history(ui, ctx, &snapshot),
                                View::Branches => self.git_branches(ui, ctx, &snapshot),
                                View::Stashes => self.git_stashes(ui, ctx, &snapshot),
                            }
                        });
                    });
            });
    }
    fn git_commit_box(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, snapshot: &Snapshot) {
        if let Some(operation) = &snapshot.operation {
            egui::Frame::new().fill(self.theme.accent.color().gamma_multiply(0.08)).inner_margin(8.0).show(ui, |ui| {
                ui.strong(operation);
                ui.label("Resolve conflicts in the editor, save, and stage each resolved file.");
                ui.add_enabled_ui(!self.source_control.pending, |ui| { ui.horizontal(|ui| {
                    if ui.add_enabled(!snapshot.files.iter().any(|f| f.conflict), egui::Button::new("Continue")).clicked() { self.git_action(ctx, Operation::Continue(operation.clone())); }
                    if ui.button("Abort…").clicked() { self.git_confirm("Abort Git operation", "Abort the current operation and restore the state Git recorded when it began?".into(), Operation::Abort(operation.clone())); }
                }); });
            });
        }
        ui.add(
            egui::TextEdit::multiline(&mut self.source_control.draft)
                .id_salt("git-commit-message")
                .hint_text("Commit message\n\nDescribe the change…")
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        let staged = snapshot.files.iter().filter(|f| f.staged()).count();
        let conflicts = snapshot.files.iter().any(|f| f.conflict);
        let ready = !self.source_control.pending
            && !self.busy
            && !self.closing
            && !conflicts
            && !self.source_control.draft.trim().is_empty()
            && (staged > 0 || self.source_control.amend);
        ui.horizontal(|ui| {
            if ui.add_enabled(ready, egui::Button::new(format!("Commit staged ({staged})")).fill(self.theme.accent.color().gamma_multiply(0.18))).clicked() {
                let op = Operation::Commit { message: self.source_control.draft.clone(), amend: self.source_control.amend };
                if self.source_control.amend { self.git_confirm("Amend last commit", "Replace the current HEAD commit with the staged changes and this message. This rewrites its commit ID.".into(), op); }
                else { self.git_action(ctx, op); }
            }
            ui.add_enabled(!snapshot.unborn && !self.source_control.pending, egui::Checkbox::new(&mut self.source_control.amend, "Amend"));
        });
        let unsaved = self.git_unsaved(None);
        if !unsaved.is_empty() {
            ui.label(
                RichText::new(format!(
                    "{} unsaved file(s) · Git uses saved text",
                    unsaved.len()
                ))
                .small()
                .color(self.theme.muted.color()),
            );
        }
        ui.add_space(10.0);
    }
    fn git_changes(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, snapshot: &Snapshot) {
        for (title, category) in [("Conflicts", 0), ("Staged Changes", 1), ("Changes", 2)] {
            let files: Vec<_> = snapshot
                .files
                .iter()
                .filter(|f| match category {
                    0 => f.conflict,
                    1 => f.staged(),
                    _ => f.working(),
                })
                .collect();
            if files.is_empty() {
                continue;
            }
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{title}  {}", files.len()))
                        .strong()
                        .small(),
                );
                if category > 0
                    && ui
                        .small_button(if category == 1 {
                            "Unstage all"
                        } else {
                            "Stage all"
                        })
                        .clicked()
                {
                    let paths = files.iter().flat_map(|f| f.paths()).collect();
                    self.git_action(
                        ctx,
                        if category == 1 {
                            Operation::Unstage(paths)
                        } else {
                            Operation::Stage(paths)
                        },
                    );
                }
            });
            for file in files {
                if !matches_filter(&file.path.to_string_lossy(), &self.source_control.filter) {
                    continue;
                }
                let kind = if category == 0 {
                    DiffKind::Conflict
                } else if category == 1 {
                    DiffKind::Staged
                } else if file.untracked() {
                    DiffKind::Untracked
                } else {
                    DiffKind::Working
                };
                self.git_file_row(ui, ctx, file, kind, category == 0);
            }
            ui.add_space(12.0);
        }
        if snapshot.files.is_empty() {
            ui.add_space(16.0);
            ui.label("Working tree clean");
            ui.label(
                RichText::new("Your saved files match the latest commit.")
                    .small()
                    .color(self.theme.muted.color()),
            );
        }
    }
    fn git_file_row(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        file: &FileStatus,
        kind: DiffKind,
        conflict: bool,
    ) {
        let selection = Selection {
            path: file.path.clone(),
            kind: kind.clone(),
        };
        let selected = self.source_control.selection.as_ref() == Some(&selection);
        let code = if conflict {
            '!'
        } else if kind == DiffKind::Staged {
            file.index
        } else {
            file.worktree
        };
        let color = status_color(code, &self.theme);
        ui.push_id((&file.path, format!("{kind:?}")), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(code.to_string()).monospace().color(color));
                let width = (ui.available_width() - 38.0).max(60.0);
                let label = file
                    .path
                    .file_name()
                    .unwrap_or(file.path.as_os_str())
                    .to_string_lossy();
                let (rect, row) =
                    ui.allocate_exact_size(egui::vec2(width, 23.0), egui::Sense::click());
                if selected || row.hovered() {
                    ui.painter().rect_filled(
                        rect,
                        3.0,
                        if selected {
                            self.theme.selection.color()
                        } else {
                            self.theme.hover().color()
                        },
                    );
                }
                let galley = ui.painter().layout(
                    label.into_owned(),
                    FontId::proportional(13.0),
                    ui.visuals().text_color(),
                    f32::INFINITY,
                );
                ui.painter()
                    .with_clip_rect(rect.intersect(ui.clip_rect()))
                    .galley(
                        egui::pos2(rect.left() + 5.0, rect.center().y - galley.size().y / 2.0),
                        galley,
                        ui.visuals().text_color(),
                    );
                let row = row.on_hover_text(file.original.as_ref().map_or_else(
                    || file.path.display().to_string(),
                    |old| format!("{} → {}", old.display(), file.path.display()),
                ));
                if row.clicked() {
                    self.source_control.select(ctx, selection);
                }
                if row.double_clicked() {
                    self.git_open_file(ctx, &file.path);
                }
                row.context_menu(|ui| {
                    if ui.button("Open in editor").clicked() {
                        self.git_open_file(ctx, &file.path);
                        ui.close_menu();
                    }
                });
                if !matches!(kind, DiffKind::Commit(_))
                    && ui
                        .small_button(if kind == DiffKind::Staged { "−" } else { "+" })
                        .on_hover_text(if conflict {
                            "Mark resolved: stage saved file"
                        } else if kind == DiffKind::Staged {
                            "Unstage file"
                        } else {
                            "Stage file"
                        })
                        .clicked()
                {
                    self.git_action(
                        ctx,
                        if kind == DiffKind::Staged {
                            Operation::Unstage(file.paths())
                        } else {
                            Operation::Stage(file.paths())
                        },
                    );
                }
            });
            if let Some(parent) = file.path.parent().filter(|p| !p.as_os_str().is_empty()) {
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("     {}", parent.display()))
                            .size(10.0)
                            .color(self.theme.muted.color()),
                    )
                    .truncate(),
                );
            }
        });
    }
    fn git_open_file(&mut self, ctx: &egui::Context, path: &Path) {
        if let Some(root) = &self.source_control.root {
            let path = root.join(path);
            self.source_control.open = false;
            self.open(ctx, Some(path));
        }
    }
    fn git_history(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, snapshot: &Snapshot) {
        for (index, commit) in snapshot.commits.iter().enumerate() {
            if !matches_filter(
                &format!("{} {} {}", commit.subject, commit.author, commit.oid),
                &self.source_control.filter,
            ) {
                continue;
            }
            let selected = self.source_control.commit.as_ref() == Some(&commit.oid);
            ui.push_id(&commit.oid, |ui| {
                let response = egui::Frame::new()
                    .fill(if selected {
                        self.theme.selection.color()
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                    .inner_margin(7.0)
                    .show(ui, |ui| {
                        ui.horizontal_top(|ui| {
                            let graph = &snapshot.graph[index];
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2((graph.width as f32 * 12.0 + 8.0).min(84.0), 36.0),
                                egui::Sense::hover(),
                            );
                            ui.vertical(|ui| {
                                ui.add(
                                    egui::Label::new(RichText::new(&commit.subject).strong())
                                        .wrap(),
                                );
                                ui.label(
                                    RichText::new(format!("{} · {}", commit.author, commit.date))
                                        .small()
                                        .color(self.theme.muted.color()),
                                );
                                if !commit.refs.is_empty() {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&commit.refs)
                                                .small()
                                                .color(self.theme.accent.color()),
                                        )
                                        .wrap(),
                                    );
                                }
                            });
                            rect
                        })
                        .inner
                    });
                let graph_rect = response.inner;
                let row_rect = response.response.rect;
                if self.source_control.filter.is_empty() {
                    paint_graph(
                        ui,
                        snapshot.graph.get(index).unwrap(),
                        graph_rect,
                        row_rect,
                        &self.theme,
                    );
                }
                let response = response.response;
                let click =
                    ui.interact(response.rect, ui.id().with("commit"), egui::Sense::click());
                if click.clicked() && !selected {
                    self.source_control.commit = Some(commit.oid.clone());
                    self.source_control.selection = None;
                    self.source_control.diff = None;
                    self.source_control.files.clear();
                    self.source_control.body.clear();
                    self.source_control.request(ctx, None);
                }
            });
        }
        if snapshot.commits.len() >= self.source_control.limit
            && self.source_control.limit < 2000
            && ui.button("Load more commits").clicked()
        {
            self.source_control.limit += 200;
            self.source_control.request(ctx, None);
        }
        if snapshot.unborn {
            ui.label("No commits yet.");
        }
    }
    fn git_branches(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, snapshot: &Snapshot) {
        if ui.button("New branch…").clicked() {
            self.source_control.prompt = Some(Prompt::Branch(String::new()));
        }
        ui.add_space(8.0);
        let filter = self.source_control.filter.clone();
        for remote in [false, true] {
            ui.strong(if remote {
                "Remote branches"
            } else {
                "Local branches"
            });
            for branch in snapshot
                .branches
                .iter()
                .filter(|b| b.remote == remote && matches_filter(&b.name, &filter))
            {
                ui.push_id(&branch.reference, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(if branch.current { "●" } else { "○" }).color(self.theme.accent.color()));
                        ui.label(RichText::new(&branch.name).strong()).on_hover_text(format!("{}\n{}", branch.oid, branch.upstream));
                        ui.menu_button("…", |ui| {
                            if !branch.current && ui.button(if remote { "Create tracking branch" } else { "Switch branch" }).clicked() {
                                self.git_action(ctx, if remote { Operation::TrackBranch { local: branch.name.split_once('/').map_or(branch.name.as_str(), |(_, n)| n).to_owned(), remote: branch.name.clone() } } else { Operation::Switch(branch.name.clone()) }); ui.close_menu();
                            }
                            if !branch.current && ui.button("Merge into current branch…").clicked() {
                                self.git_confirm("Merge branch", format!("Merge {} into {}? Git will stop if there are conflicts.", branch.name, snapshot.branch), Operation::Merge(branch.reference.clone())); ui.close_menu();
                            }
                            if !branch.current && !remote && ui.button("Delete branch…").clicked() {
                                self.git_confirm("Delete branch", format!("Delete {}? Git will refuse if it contains unmerged commits.", branch.name), Operation::DeleteBranch(branch.name.clone())); ui.close_menu();
                            }
                        });
                    });
                });
            }
            ui.add_space(12.0);
        }
        ui.strong("Remotes");
        for remote in &snapshot.remotes {
            ui.horizontal(|ui| {
                ui.label(remote);
                if ui.small_button("Remove…").clicked() { self.git_confirm("Remove remote", format!("Remove the configuration and tracking references for {remote}? The remote server is unchanged."), Operation::RemoveRemote(remote.clone())); }
            });
        }
        if ui.button("Add remote…").clicked() {
            self.source_control.prompt = Some(Prompt::Remote {
                name: "origin".into(),
                url: String::new(),
            });
        }
    }
    fn git_stashes(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, snapshot: &Snapshot) {
        if ui
            .add_enabled(
                !snapshot.files.is_empty(),
                egui::Button::new("Stash changes…"),
            )
            .clicked()
        {
            self.source_control.prompt = Some(Prompt::Stash(String::new()));
        }
        ui.add_space(8.0);
        for stash in &snapshot.stashes {
            if !matches_filter(&stash.subject, &self.source_control.filter) {
                continue;
            }
            ui.push_id(&stash.oid, |ui| {
                ui.strong(&stash.name);
                ui.add(egui::Label::new(&stash.subject).wrap());
                ui.horizontal(|ui| {
                    if ui
                        .button("Apply")
                        .on_hover_text(
                            "Restore saved changes and their index state; keep the stash",
                        )
                        .clicked()
                    {
                        self.git_action(ctx, Operation::ApplyStash(stash.oid.clone()));
                    }
                    if ui.button("Drop…").clicked() {
                        self.git_confirm(
                            "Drop stash",
                            format!("Permanently drop {}: {}?", stash.name, stash.subject),
                            Operation::DropStash {
                                name: stash.name.clone(),
                                oid: stash.oid.clone(),
                            },
                        );
                    }
                });
                ui.separator();
            });
        }
        if snapshot.stashes.is_empty() {
            ui.label("No stashed changes.");
        }
    }
    pub(super) fn source_control_editor(&mut self, ctx: &egui::Context) {
        let theme = self.theme.clone();
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme.background.color())
                    .inner_margin(14.0),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(match self.source_control.view {
                        View::Changes => "Changes",
                        View::History => "History",
                        View::Branches => "Branches",
                        View::Stashes => "Stashes",
                    });
                    if self.source_control.pending {
                        ui.spinner();
                        ui.label(RichText::new("Git is working…").color(theme.muted.color()));
                        if ui.small_button("Cancel").clicked() {
                            self.source_control.cancel.store(true, Ordering::Relaxed);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.toggle_value(&mut self.source_control.output_open, "Git output");
                    });
                });
                if let Some(error) = self.source_control.error.clone() {
                    egui::Frame::new()
                        .fill(theme.error.color().gamma_multiply(0.10))
                        .inner_margin(10.0)
                        .show(ui, |ui| {
                            ui.horizontal_top(|ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(error).color(theme.error.color()),
                                    )
                                    .wrap(),
                                );
                                if controls::close_button(ui, "Dismiss Git message").clicked() {
                                    self.source_control.error = None;
                                }
                            });
                        });
                }
                if self.source_control.output_open {
                    egui::ScrollArea::vertical()
                        .id_salt("git-output")
                        .max_height(150.0)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for text in &self.source_control.output {
                                ui.add(
                                    egui::Label::new(RichText::new(text).monospace().size(11.0))
                                        .wrap()
                                        .selectable(true),
                                );
                                ui.separator();
                            }
                        });
                }
                ui.separator();
                if self.source_control.view == View::History {
                    if let Some(oid) = self.source_control.commit.clone() {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(&oid)
                                    .monospace()
                                    .size(11.0)
                                    .color(theme.muted.color()),
                            );
                            if ui.small_button("Copy ID").clicked() {
                                ui.ctx().copy_text(oid.clone());
                            }
                            ui.add_enabled_ui(!self.source_control.pending, |ui| {
                                if ui.small_button("Cherry-pick…").clicked() {
                                    self.git_confirm(
                                        "Cherry-pick commit",
                                        format!(
                                            "Apply commit {} to the current branch?",
                                            &oid[..8]
                                        ),
                                        Operation::CherryPick(oid.clone()),
                                    );
                                }
                                if ui.small_button("Revert…").clicked() {
                                    self.git_confirm(
                                        "Revert commit",
                                        format!("Create a new commit that reverses {}?", &oid[..8]),
                                        Operation::Revert(oid.clone()),
                                    );
                                }
                            });
                        });
                        egui::ScrollArea::vertical()
                            .id_salt("git-commit-body")
                            .max_height(100.0)
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(&self.source_control.body)
                                        .wrap()
                                        .selectable(true),
                                );
                            });
                        ui.separator();
                        egui::SidePanel::left("git-commit-files")
                            .default_width(210.0)
                            .min_width(140.0)
                            .resizable(true)
                            .show_inside(ui, |ui| {
                                ui.strong(format!("Files · {}", self.source_control.files.len()));
                                egui::ScrollArea::vertical().show(ui, |ui| {
                                    for file in self.source_control.files.clone() {
                                        ui.add_enabled_ui(!self.source_control.pending, |ui| {
                                            self.git_file_row(
                                                ui,
                                                ctx,
                                                &file,
                                                DiffKind::Commit(oid.clone()),
                                                false,
                                            );
                                        });
                                    }
                                });
                            });
                    }
                }
                self.git_diff_canvas(ui, ctx);
            });
        self.git_prompt(ctx);
    }
    fn git_diff_canvas(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let theme = self.theme.clone();
        let Some(selection) = self.source_control.selection.clone() else {
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height() * 0.30).max(20.0));
                branch_icon(ui, theme.muted.color());
                ui.add_space(12.0);
                ui.label(
                    RichText::new(match self.source_control.view {
                        View::Changes => "Select a file to review its changes",
                        View::History => "Select a commit, then a file to inspect its diff",
                        View::Branches => "Manage local and remote branches in the sidebar",
                        View::Stashes => "Save unfinished work and restore it when you need it",
                    })
                    .size(17.0),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Your open files stay available in Files.")
                        .color(theme.muted.color()),
                );
            });
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(selection.path.display().to_string()).strong());
            ui.label(
                RichText::new(match selection.kind {
                    DiffKind::Staged => "INDEX ↔ HEAD",
                    DiffKind::Commit(_) => "COMMIT ↔ PARENT",
                    DiffKind::Untracked => "UNTRACKED",
                    DiffKind::Working => "WORKING TREE ↔ INDEX",
                    DiffKind::Conflict => "MERGE CONFLICT",
                })
                .size(10.0)
                .color(theme.muted.color()),
            );
            if ui.small_button("Open file").clicked() {
                self.git_open_file(ctx, &selection.path);
            }
        });
        let Some(diff) = self.source_control.diff.clone() else {
            ui.label("Loading diff…");
            return;
        };
        if let Some(conflict) = &diff.conflict {
            self.git_conflict_canvas(ui, ctx, &selection, conflict);
            return;
        }
        let pending = self.source_control.pending
            || self.busy
            || self.closing
            || self.source_control.prompt.is_some();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("+{}", diff.additions)).color(status_color('A', &theme)));
            ui.label(RichText::new(format!("−{}", diff.deletions)).color(status_color('D', &theme)));
            ui.selectable_value(&mut self.source_control.split_diff, false, "Unified");
            ui.add_enabled_ui(ui.available_width()>360.0, |ui| { ui.selectable_value(&mut self.source_control.split_diff, true, "Side by side"); });
            ui.add_enabled_ui(!pending, |ui| {
                if !matches!(selection.kind, DiffKind::Commit(_)) {
                    let staged = selection.kind == DiffKind::Staged;
                    if ui.small_button(if staged { "Unstage file" } else { "Stage file" }).clicked() {
                        let paths = self.source_control.snapshot.as_ref().and_then(|s| s.files.iter().find(|f| f.path == selection.path)).map_or_else(|| vec![selection.path.clone()], FileStatus::paths);
                        self.git_action(ctx, if staged { Operation::Unstage(paths) } else { Operation::Stage(paths) });
                    }
                    if selection.kind == DiffKind::Working && !diff.patch.is_empty() && ui.small_button("Discard working changes…").clicked() {
                        self.git_confirm("Discard working changes", format!("Restore {} from the index? The current file is backed up in the session's git-backups folder before it is replaced.", selection.path.display()), Operation::Discard { selection: selection.clone(), expected: diff.patch.clone(), backup: self.workspace.store.directory().join("git-backups") });
                    }
                }
                if ui.small_button("Copy diff").clicked() { ui.ctx().copy_text(diff.patch.clone()); }
            });
        });
        ui.separator();
        if diff.lines.is_empty() {
            ui.label("No changes in this comparison. Refresh or choose another file.");
            return;
        }
        if self.source_control.split_diff && ui.available_width() >= 660.0 {
            self.git_split_diff(ui, ctx, &selection, &diff, pending);
            return;
        }
        let font = FontId::monospace(self.settings.font_size.clamp(11.0, 17.0));
        let height = ui.fonts(|f| f.row_height(&font)).max(16.0) + 6.0;
        let wide = diff
            .lines
            .iter()
            .map(|l| l.text.chars().count())
            .max()
            .unwrap_or(0)
            .min(16_384) as f32
            * font.size
            * 0.65
            + 140.0;
        egui::ScrollArea::both()
            .id_salt((
                "git-diff-canvas",
                &selection.path,
                format!("{:?}", selection.kind),
            ))
            .auto_shrink([false, false])
            .show_rows(ui, height, diff.lines.len(), |ui, rows| {
                ui.set_min_width(ui.available_width().max(wide));
                for row in rows {
                    let line = &diff.lines[row];
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), height),
                        egui::Sense::click(),
                    );
                    let color = match line.kind {
                        '+' => status_color('A', &theme),
                        '-' => status_color('D', &theme),
                        '@' => theme.accent.color(),
                        _ => theme.text.color(),
                    };
                    if matches!(line.kind, '+' | '-' | '@') {
                        ui.painter().rect_filled(
                            rect,
                            0.0,
                            color.gamma_multiply(if line.kind == '@' { 0.08 } else { 0.10 }),
                        );
                    }
                    let left = rect.left();
                    let center = rect.center().y;
                    for (offset, number) in [(43.0, line.old), (87.0, line.new)] {
                        if let Some(number) = number {
                            ui.painter().text(
                                egui::pos2(left + offset, center),
                                egui::Align2::RIGHT_CENTER,
                                number.to_string(),
                                FontId::monospace(11.0),
                                theme.muted.color(),
                            );
                        }
                    }
                    ui.painter().line_segment(
                        [
                            egui::pos2(left + 98.0, rect.top()),
                            egui::pos2(left + 98.0, rect.bottom()),
                        ],
                        egui::Stroke::new(1.0, theme.border.color()),
                    );
                    // Bound one pathological line; Copy diff retains the complete patch.
                    let text: String = line.text.chars().take(16_384).collect();
                    ui.painter().text(
                        egui::pos2(left + 108.0, center),
                        egui::Align2::LEFT_CENTER,
                        text.replace('\t', "    "),
                        font.clone(),
                        color,
                    );
                    if line.kind == '@'
                        && line.hunk.and_then(|h| diff.hunk_patch(h)).is_some()
                        && !pending
                    {
                        let button = egui::Rect::from_min_size(
                            egui::pos2(ui.clip_rect().right() - 120.0, rect.top()),
                            egui::vec2(112.0, height),
                        );
                        if ui
                            .put(
                                button,
                                egui::Button::new(if selection.kind == DiffKind::Staged {
                                    "Unstage hunk"
                                } else {
                                    "Stage hunk"
                                })
                                .small(),
                            )
                            .clicked()
                        {
                            self.git_action(
                                ctx,
                                Operation::Hunk {
                                    selection: selection.clone(),
                                    expected: diff.patch.clone(),
                                    hunk: line.hunk.unwrap(),
                                },
                            );
                        }
                    }
                    response.context_menu(|ui| {
                        if ui.button("Copy line").clicked() {
                            ui.ctx().copy_text(line.text.clone());
                            ui.close_menu();
                        }
                        if ui.button("Copy diff").clicked() {
                            ui.ctx().copy_text(diff.patch.clone());
                            ui.close_menu();
                        }
                    });
                }
            });
    }
    fn git_prompt(&mut self, ctx: &egui::Context) {
        let Some(mut prompt) = self.source_control.prompt.take() else {
            return;
        };
        let title = match &prompt {
            Prompt::Confirm { title, .. } => title.as_str(),
            Prompt::Branch(_) => "Create branch",
            Prompt::Stash(_) => "Stash changes",
            Prompt::Clone { .. } => "Clone repository",
            Prompt::Remote { .. } => "Add remote",
        }
        .to_owned();
        let mut keep = true;
        let mut operation = None;
        let mut create = false;
        egui::Modal::new(egui::Id::new("git-prompt")).show(ctx, |ui| {
            ui.set_width(410.0_f32.min(ctx.screen_rect().width() - 48.0));
            ui.strong(title);
            ui.add_space(10.0);
            match &mut prompt {
                Prompt::Confirm {
                    detail,
                    operation: op,
                    ..
                } => {
                    ui.add(egui::Label::new(detail.as_str()).wrap());
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                        if ui.button("Confirm").clicked() {
                            keep = false;
                            operation = Some(op.clone());
                        }
                    });
                }
                Prompt::Branch(name) | Prompt::Stash(name) => {
                    ui.add(
                        egui::TextEdit::singleline(name)
                            .hint_text("Name or description")
                            .desired_width(380.0),
                    );
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                        if ui
                            .add_enabled(!name.trim().is_empty(), egui::Button::new("Create"))
                            .clicked()
                        {
                            keep = false;
                            create = true;
                        }
                    });
                }
                Prompt::Clone { url, destination } => {
                    ui.label("Repository URL or local path");
                    ui.add(egui::TextEdit::singleline(url).desired_width(380.0));
                    ui.label("Full path to a new destination folder");
                    ui.add(egui::TextEdit::singleline(destination).desired_width(380.0));
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                        if ui
                            .add_enabled(
                                !url.trim().is_empty() && !destination.trim().is_empty(),
                                egui::Button::new("Clone"),
                            )
                            .clicked()
                        {
                            keep = false;
                            create = true;
                        }
                    });
                }
                Prompt::Remote { name, url } => {
                    ui.label("Remote name");
                    ui.text_edit_singleline(name);
                    ui.label("Remote URL");
                    ui.add(egui::TextEdit::singleline(url).desired_width(380.0));
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                        if ui
                            .add_enabled(
                                !name.trim().is_empty() && !url.trim().is_empty(),
                                egui::Button::new("Add remote"),
                            )
                            .clicked()
                        {
                            keep = false;
                            create = true;
                        }
                    });
                }
            }
        });
        if keep {
            self.source_control.prompt = Some(prompt);
        } else {
            match &prompt {
                Prompt::Branch(name) if create => {
                    operation = Some(Operation::CreateBranch(name.trim().into()))
                }
                Prompt::Stash(name) if create => {
                    operation = Some(Operation::Stash(name.trim().into()))
                }
                Prompt::Clone { url, destination } if create => {
                    self.git_clone(ctx, url.trim().into(), PathBuf::from(destination.trim()))
                }
                Prompt::Remote { name, url } if create => {
                    operation = Some(Operation::AddRemote {
                        name: name.trim().into(),
                        url: url.trim().into(),
                    })
                }
                _ => (),
            }
            if let Some(op) = operation {
                self.git_action(ctx, op);
            }
        }
    }
    fn git_split_diff(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        selection: &Selection,
        diff: &Diff,
        pending: bool,
    ) {
        let theme = self.theme.clone();
        let font = FontId::monospace(self.settings.font_size.clamp(11.0, 17.0));
        let height = ui.fonts(|f| f.row_height(&font)).max(16.0) + 6.0;
        ui.columns(2, |columns| {
            columns[0].label(RichText::new("Before").small().color(theme.muted.color()));
            columns[1].label(RichText::new("After").small().color(theme.muted.color()));
        });
        let half = (ui.available_width() / 2.0).max(260.0);
        let wide = diff
            .lines
            .iter()
            .map(|l| l.text.chars().count())
            .max()
            .unwrap_or(0)
            .min(16_384) as f32
            * font.size
            * 0.65
            + 70.0;
        egui::ScrollArea::both()
            .id_salt((
                "git-split-diff",
                &selection.path,
                format!("{:?}", selection.kind),
            ))
            .auto_shrink([false, false])
            .show_rows(ui, height, diff.split_rows.len(), |ui, rows| {
                ui.set_min_width((wide + half).max(half * 2.0));
                for row in rows {
                    let (old, new) = diff.split_rows[row];
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), height),
                        egui::Sense::hover(),
                    );
                    for (column, index) in [(0, old), (1, new)] {
                        let scroll = rect.left() - ui.clip_rect().left();
                        let cell = egui::Rect::from_min_size(
                            egui::pos2(ui.clip_rect().left() + column as f32 * half, rect.top()),
                            egui::vec2(half, height),
                        );
                        let Some(index) = index else {
                            ui.painter().rect_filled(cell, 0.0, theme.panel.color());
                            continue;
                        };
                        let line = &diff.lines[index];
                        let color = match line.kind {
                            '+' => status_color('A', &theme),
                            '-' => status_color('D', &theme),
                            '@' => theme.accent.color(),
                            _ => theme.text.color(),
                        };
                        if matches!(line.kind, '+' | '-' | '@') {
                            ui.painter()
                                .rect_filled(cell, 0.0, color.gamma_multiply(0.10));
                        }
                        let number = if column == 0 { line.old } else { line.new };
                        if let Some(number) = number {
                            ui.painter().text(
                                egui::pos2(cell.left() + 40.0, cell.center().y),
                                egui::Align2::RIGHT_CENTER,
                                number.to_string(),
                                FontId::monospace(11.0),
                                theme.muted.color(),
                            );
                        }
                        let text = if matches!(line.kind, '+' | '-' | ' ') {
                            line.text.get(1..).unwrap_or("")
                        } else {
                            &line.text
                        };
                        let text: String = text.chars().take(16_384).collect();
                        ui.painter()
                            .with_clip_rect(
                                egui::Rect::from_min_max(
                                    cell.min + egui::vec2(45.0, 0.0),
                                    cell.max,
                                )
                                .intersect(ui.clip_rect()),
                            )
                            .text(
                                egui::pos2(cell.left() + 50.0 + scroll, cell.center().y),
                                egui::Align2::LEFT_CENTER,
                                text.replace('\t', "    "),
                                font.clone(),
                                color,
                            );
                        if column == 1
                            && line.kind == '@'
                            && line.hunk.and_then(|h| diff.hunk_patch(h)).is_some()
                            && !pending
                        {
                            let button = egui::Rect::from_min_size(
                                egui::pos2(ui.clip_rect().right() - 120.0, cell.top()),
                                egui::vec2(112.0, height),
                            );
                            if ui
                                .put(
                                    button,
                                    egui::Button::new(if selection.kind == DiffKind::Staged {
                                        "Unstage hunk"
                                    } else {
                                        "Stage hunk"
                                    })
                                    .small(),
                                )
                                .clicked()
                            {
                                self.git_action(
                                    ctx,
                                    Operation::Hunk {
                                        selection: selection.clone(),
                                        expected: diff.patch.clone(),
                                        hunk: line.hunk.unwrap(),
                                    },
                                );
                            }
                        }
                    }
                    ui.painter().line_segment(
                        [
                            egui::pos2(ui.clip_rect().left() + half, rect.top()),
                            egui::pos2(ui.clip_rect().left() + half, rect.bottom()),
                        ],
                        egui::Stroke::new(1.0, theme.border.color()),
                    );
                }
            });
    }
    fn git_conflict_canvas(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        selection: &Selection,
        conflict: &git::Conflict,
    ) {
        ui.label("Review both sides, then open the file to combine changes. Save and stage the resolved result.");
        let rebase = self
            .source_control
            .snapshot
            .as_ref()
            .and_then(|s| s.operation.as_deref())
            == Some("Rebase in progress");
        if rebase {
            ui.label(RichText::new("During rebase, Current is the branch being rebased onto; Incoming is the commit being replayed.").small().color(self.theme.muted.color()));
        }
        let enabled = !self.source_control.pending && self.source_control.prompt.is_none();
        ui.add_enabled_ui(enabled, |ui| { ui.horizontal_wrapped(|ui| {
            if ui.button("Open to resolve").clicked() { self.git_open_file(ctx, &selection.path); }
            for (ours, label, content) in [(true,"Use Current…",&conflict.ours),(false,"Use Incoming…",&conflict.theirs)] {
                if ui.add_enabled(content.is_some(), egui::Button::new(label)).clicked() {
                    self.git_confirm("Replace conflict file", format!("Replace {} with the complete {} version? The working file is backed up first. Review and stage it afterward to mark it resolved.", selection.path.display(),if ours { "Current" } else { "Incoming" }), Operation::Resolve { selection: selection.clone(), ours, expected: conflict.expected.clone(), backup: self.workspace.store.directory().join("git-backups") });
                }
            }
            if ui.button("Stage resolved file").clicked() { self.git_action(ctx, Operation::Stage(vec![selection.path.clone()])); }
        }); });
        ui.separator();
        if let Some(base) = &conflict.base {
            ui.collapsing("Common ancestor", |ui| {
                conflict_text(ui, "base", base, 110.0);
            });
        }
        let height = ui.available_height();
        if ui.available_width() > 660.0 {
            ui.columns(2, |columns| {
                for (index, label, text) in [
                    (0, "Current · ours", &conflict.ours),
                    (1, "Incoming · theirs", &conflict.theirs),
                ] {
                    columns[index].strong(label);
                    conflict_text(
                        &mut columns[index],
                        label,
                        text.as_deref().unwrap_or("File deleted on this side."),
                        height - 28.0,
                    );
                }
            });
        } else {
            for (label, text) in [
                ("Current · ours", &conflict.ours),
                ("Incoming · theirs", &conflict.theirs),
            ] {
                ui.strong(label);
                conflict_text(
                    ui,
                    label,
                    text.as_deref().unwrap_or("File deleted on this side."),
                    (height - 56.0) / 2.0,
                );
            }
        }
    }
    fn git_clone(&mut self, ctx: &egui::Context, url: String, destination: PathBuf) {
        if self.source_control.pending {
            return;
        }
        self.source_control.pending = true;
        self.source_control.cancel = Arc::new(AtomicBool::new(false));
        let git = Git {
            cancel: self.source_control.cancel.clone(),
        };
        let tx = self.source_control.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let cloned = git.clone_repository(&url, &destination);
            let root = cloned.as_ref().ok().cloned();
            let outcome = Some(
                cloned
                    .as_ref()
                    .map(|r| format!("Cloned repository into {}", r.display()))
                    .map_err(Clone::clone),
            );
            let snapshot = cloned.and_then(|root| git.snapshot(&root, 200));
            let _ = tx.send(Reply {
                repositories: vec![],
                root,
                snapshot,
                selection: None,
                diff: None,
                commit: None,
                files: vec![],
                body: String::new(),
                outcome,
                committed: None,
            });
            ctx.request_repaint();
        });
    }
}
fn repo_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}
fn conflict_text(ui: &mut egui::Ui, id: &str, text: &str, height: f32) {
    let lines: Vec<_> = text.lines().collect();
    let font = FontId::monospace(12.0);
    let row_height = ui.fonts(|f| f.row_height(&font)) + 4.0;
    egui::ScrollArea::both()
        .id_salt(("git-conflict", id))
        .max_height(height.max(40.0))
        .auto_shrink([false, false])
        .show_rows(ui, row_height, lines.len(), |ui, rows| {
            for index in rows {
                ui.add(
                    egui::Label::new(
                        RichText::new(lines[index].chars().take(16_384).collect::<String>())
                            .font(font.clone()),
                    )
                    .extend()
                    .selectable(true),
                );
            }
        });
}
fn matches_filter(value: &str, filter: &str) -> bool {
    value.to_lowercase().contains(&filter.to_lowercase())
}
fn status_color(code: char, theme: &Theme) -> egui::Color32 {
    let dark = theme.background.color().r() < 128;
    match code {
        'A' | '?' => {
            if dark {
                egui::Color32::from_rgb(126, 201, 146)
            } else {
                egui::Color32::from_rgb(34, 116, 57)
            }
        }
        'D' | '!' | 'U' => {
            if dark {
                egui::Color32::from_rgb(235, 132, 132)
            } else {
                egui::Color32::from_rgb(170, 53, 53)
            }
        }
        _ => theme.accent.color(),
    }
}
fn branch_icon(ui: &mut egui::Ui, color: egui::Color32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 22.0), egui::Sense::hover());
    let a = egui::pos2(r.left() + 4.0, r.top() + 4.0);
    let b = egui::pos2(a.x, r.bottom() - 4.0);
    let c = egui::pos2(r.right() - 3.0, r.top() + 6.0);
    let p = ui.painter();
    let stroke = egui::Stroke::new(1.5, color);
    p.line_segment([a, b], stroke);
    p.line_segment([b, c], stroke);
    for point in [a, b, c] {
        p.circle_filled(point, 2.5, color);
    }
}
fn paint_graph(
    ui: &egui::Ui,
    row: &git::GraphRow,
    graph: egui::Rect,
    rect: egui::Rect,
    theme: &Theme,
) {
    let x = |lane: usize| graph.left() + (lane as f32 * 12.0 + 4.0).min(graph.width() - 3.0);
    let y = graph.top() + 8.0;
    let stroke = |lane: usize| {
        egui::Stroke::new(
            1.5,
            [
                theme.accent.color(),
                status_color('A', theme),
                status_color('D', theme),
                theme.muted.color(),
            ][lane % 4],
        )
    };
    let p = ui.painter();
    if row.incoming {
        p.line_segment(
            [
                egui::pos2(x(row.node), rect.top()),
                egui::pos2(x(row.node), y),
            ],
            stroke(row.node),
        );
    }
    for &(from, to, parent) in &row.edges {
        if !parent {
            p.line_segment(
                [egui::pos2(x(from), rect.top()), egui::pos2(x(from), y)],
                stroke(from),
            );
        }
        p.line_segment(
            [egui::pos2(x(from), y), egui::pos2(x(to), rect.bottom())],
            stroke(from),
        );
    }
    p.circle_filled(egui::pos2(x(row.node), y), 3.5, stroke(row.node).color);
}
