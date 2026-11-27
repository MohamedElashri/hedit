//! Shared Git navigation, compact repository controls, and task feedback.
use super::*;

const SCM_ICON_SIZE: f32 = 14.0;

pub(super) fn scm_icon(
    ui: &egui::Ui,
    icon: hedit::icons::Icon,
    color: egui::Color32,
) -> egui::Image<'static> {
    let size = egui::Vec2::splat(SCM_ICON_SIZE);
    let texture = icon.texture(ui.ctx(), size.x);
    egui::Image::new((texture.id(), size)).tint(color)
}

pub(super) fn git_icon_button(
    ui: &mut egui::Ui,
    glyph: &str,
    label: &str,
    enabled: bool,
) -> egui::Response {
    let button = ui.add_enabled(enabled, egui::Button::new(glyph).small().frame(false));
    button.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled() && enabled, label)
    });
    button.on_hover_text(label).on_disabled_hover_text(label)
}

#[derive(Default)]
pub(super) struct State {
    locations: BTreeMap<View, Location>,
    return_view: Option<View>,
    branch_filter: String,
    pub(super) branch_menu_open: bool,
    pub(super) collapsed_changes: BTreeSet<&'static str>,
    pub(super) reveal_output: bool,
    pub(super) graph_open: bool,
    pub(super) show_filter: bool,
    pub(super) focus_filter: bool,
    queue: ReviewQueue,
    commit_after_input: Option<u64>,
    commit_field_pass: Option<u64>,
    pub activity: Option<&'static str>,
    pub completion: Option<&'static str>,
    pub notice: Option<&'static str>,
    pub prompt_visible: bool,
}
#[derive(Default)]
struct ReviewQueue {
    key: Option<(View, String, String, Option<String>)>,
    snapshot: Option<Arc<Snapshot>>,
    inspection: Option<Arc<inspect::Data>>,
    rows: Arc<Vec<Selection>>,
}
fn same_arc<T>(a: &Option<Arc<T>>, b: &Option<Arc<T>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}
#[derive(Default)]
struct Location {
    filter: String,
    selection: Option<Selection>,
    commit: Option<String>,
    commit_file_filter: String,
}
pub(super) struct ActionText {
    pub running: &'static str,
    pub done: &'static str,
    pub confirm: &'static str,
}
pub(super) fn action_text(op: &Operation) -> ActionText {
    let (running, done, confirm) = match op {
        Operation::Stage(_) => ("Staging files…", "Files staged", "Stage files"),
        Operation::Unstage(_) => ("Unstaging files…", "Files unstaged", "Unstage files"),
        Operation::Hunk { selection, .. } | Operation::Lines { selection, .. } => {
            if selection.kind == DiffKind::Staged {
                ("Unstaging changes…", "Changes unstaged", "Unstage changes")
            } else {
                ("Staging changes…", "Changes staged", "Stage changes")
            }
        }
        Operation::Commit { amend: true, .. } => {
            ("Amending commit…", "Commit amended", "Amend commit")
        }
        Operation::Commit { .. } => ("Creating commit…", "Commit created", "Commit staged"),
        Operation::CommitMerge { .. } => ("Committing merge…", "Merge committed", "Commit merge"),
        Operation::StageCommit { .. } => (
            "Staging and committing…",
            "Commit created",
            "Stage and Commit",
        ),
        Operation::Switch(_) | Operation::TrackBranch { .. } => {
            ("Switching branch…", "Branch switched", "Switch branch")
        }
        Operation::CreateBranch(_) | Operation::CreateBranchAt { .. } => {
            ("Creating branch…", "Branch created", "Create branch")
        }
        Operation::RenameBranch { .. } => ("Renaming branch…", "Branch renamed", "Rename branch"),
        Operation::DeleteBranch(_) => ("Deleting branch…", "Branch deleted", "Delete branch"),
        Operation::SetUpstream { .. } => ("Setting upstream…", "Upstream updated", "Set upstream"),
        Operation::Merge(_) => ("Merging branch…", "Branch merged", "Merge branch"),
        Operation::Rebase(_) => ("Rebasing branch…", "Rebase completed", "Rebase branch"),
        Operation::CherryPick(_) => ("Applying commit…", "Commit applied", "Cherry-pick commit"),
        Operation::Revert(_) => (
            "Reverting commit…",
            "Revert commit created",
            "Revert commit",
        ),
        Operation::Continue(_) => (
            "Continuing operation…",
            "Git operation continued",
            "Continue",
        ),
        Operation::Abort(_) => (
            "Aborting operation…",
            "Git operation aborted",
            "Abort operation",
        ),
        Operation::Stash(_) => ("Saving stash…", "Changes stashed", "Stash changes"),
        Operation::ApplyStash(_) => ("Applying stash…", "Stash applied", "Apply stash"),
        Operation::DropStash { .. } => ("Dropping stash…", "Stash dropped", "Drop stash"),
        Operation::CreateTag { .. } => ("Creating tag…", "Tag created", "Create tag"),
        Operation::DeleteTag { .. } => ("Deleting tag…", "Local tag deleted", "Delete local tag"),
        Operation::UndoCommit { .. } => (
            "Undoing commit…",
            "Commit undone · changes remain staged",
            "Undo commit",
        ),
        Operation::Fetch => ("Fetching remotes…", "Remote references updated", "Fetch"),
        Operation::Pull => ("Pulling changes…", "Branch updated", "Pull"),
        Operation::Push => ("Pushing commits…", "Push completed", "Push"),
        Operation::Sync => (
            "Synchronizing changes…",
            "Changes synchronized",
            "Sync Changes",
        ),
        Operation::Publish { .. } => ("Publishing branch…", "Branch published", "Publish branch"),
        Operation::AddRemote { .. } => ("Adding remote…", "Remote added", "Add remote"),
        Operation::RemoveRemote(_) => ("Removing remote…", "Remote removed", "Remove remote"),
        Operation::Discard { .. } => (
            "Restoring file…",
            "Working file restored",
            "Discard changes",
        ),
        Operation::Resolve { .. } => (
            "Restoring conflict side…",
            "Conflict side restored · review and stage the file",
            "Replace working file",
        ),
        Operation::Init => (
            "Initializing repository…",
            "Repository initialized",
            "Initialize repository",
        ),
    };
    ActionText {
        running,
        done,
        confirm,
    }
}

impl SourceControl {
    /// Each view remembers where the user was. Contextual links offer one step back.
    pub(super) fn visit(&mut self, ctx: &egui::Context, view: View, contextual: bool) {
        self.navigate(view, contextual);
        self.prepare_inspection_view();
        self.request(ctx, None);
    }
    pub(super) fn navigate(&mut self, view: View, contextual: bool) {
        if self.view != view {
            self.ux.return_view = contextual.then_some(self.view);
            self.ux.locations.insert(
                self.view,
                Location {
                    filter: self.filter.clone(),
                    selection: self.selection.clone(),
                    commit: self.commit.clone(),
                    commit_file_filter: self.commit_file_filter.clone(),
                },
            );
            let location = self.ux.locations.remove(&view).unwrap_or_default();
            self.clear_review();
            self.view = view;
            self.filter = location.filter;
            self.selection = location.selection;
            self.commit = location.commit;
            self.commit_file_filter = location.commit_file_filter;
        } else if !contextual {
            self.ux.return_view = None;
        }
    }
    fn cached_review_queue(&self) -> Option<Arc<Vec<Selection>>> {
        let cache = &self.ux.queue;
        let same_key = cache
            .key
            .as_ref()
            .is_some_and(|(view, filter, file_filter, commit)| {
                *view == self.view
                    && filter == &self.filter
                    && file_filter == &self.commit_file_filter
                    && commit == &self.commit
            });
        (same_key
            && same_arc(&cache.snapshot, &self.snapshot)
            && same_arc(&cache.inspection, &self.inspection_data()))
        .then(|| cache.rows.clone())
    }
    pub(super) fn review_queue(&mut self) -> Arc<Vec<Selection>> {
        let key = (
            self.view,
            self.filter.clone(),
            self.commit_file_filter.clone(),
            self.commit.clone(),
        );
        let data = self.inspection_data();
        if let Some(rows) = self.cached_review_queue() {
            return rows;
        }
        let rows = Arc::new(self.build_review_queue());
        self.ux.queue = ReviewQueue {
            key: Some(key),
            snapshot: self.snapshot.clone(),
            inspection: data,
            rows: rows.clone(),
        };
        rows
    }
    fn build_review_queue(&self) -> Vec<Selection> {
        let mut result = Vec::new();
        let mut append = |files: &[FileStatus], kind: DiffKind| {
            let mut paths: Vec<_> = files
                .iter()
                .filter(|f| matches_filter(&f.path.to_string_lossy(), &self.filter))
                .map(|f| Selection {
                    path: f.path.clone(),
                    kind: if kind == DiffKind::Working && f.untracked() {
                        DiffKind::Untracked
                    } else {
                        kind.clone()
                    },
                })
                .collect();
            paths.sort_by(|a, b| a.path.cmp(&b.path));
            result.extend(paths);
        };
        match self.view {
            View::Changes => {
                if let Some(snapshot) = &self.snapshot {
                    for category in 0..3 {
                        let files: Vec<_> = snapshot
                            .files
                            .iter()
                            .filter(|f| match category {
                                0 => f.conflict,
                                1 => f.staged(),
                                _ => f.working(),
                            })
                            .cloned()
                            .collect();
                        append(
                            &files,
                            match category {
                                0 => DiffKind::Conflict,
                                1 => DiffKind::Staged,
                                _ => DiffKind::Working,
                            },
                        );
                    }
                }
            }
            View::History => {
                if let Some(oid) = &self.commit {
                    let files: Vec<_> = self
                        .files
                        .iter()
                        .filter(|f| {
                            matches_filter(&f.path.to_string_lossy(), &self.commit_file_filter)
                        })
                        .cloned()
                        .collect();
                    // The sidebar's commit filter matches commits, not these files.
                    for file in files {
                        result.push(Selection {
                            path: file.path,
                            kind: DiffKind::Commit(oid.clone()),
                        });
                    }
                    result.sort_by(|a, b| a.path.cmp(&b.path));
                }
            }
            View::Compare => {
                if let Some(data) = self.inspection_data() {
                    if let inspect::Data::Compare(c) = &*data {
                        append(
                            &c.files,
                            DiffKind::Compare {
                                base: c.base.clone(),
                                head: c.head.clone(),
                            },
                        );
                    }
                }
            }
            _ => (),
        }
        result
    }
    pub(in crate::app) fn can_move_review(&self, next: bool) -> bool {
        if self.ux.activity.is_some() || self.prompt.is_some() {
            return false;
        }
        let queue = self
            .cached_review_queue()
            .unwrap_or_else(|| Arc::new(self.build_review_queue()));
        let index = self
            .selection
            .as_ref()
            .and_then(|s| queue.iter().position(|item| item == s));
        match index {
            Some(i) if next => i + 1 < queue.len(),
            Some(i) => i > 0,
            None => next && !queue.is_empty(),
        }
    }
    pub(in crate::app) fn can_commit(&self) -> bool {
        self.commit_blocker().is_none()
    }
    fn commit_blocker(&self) -> Option<&'static str> {
        let Some(snapshot) = &self.snapshot else {
            return Some("Open a Git repository to commit.");
        };
        if self.pending {
            Some(if self.ux.activity.is_some() {
                "Wait for the current Git action."
            } else {
                "Refreshing repository status…"
            })
        } else if self.prompt.is_some() {
            Some("Finish the open Git dialog.")
        } else if snapshot.files.iter().any(|file| file.conflict) {
            Some("Resolve and stage conflicts before committing.")
        } else if snapshot.operation.is_some() && !snapshot.merging() {
            Some("Use Continue to finish the current Git operation.")
        } else if self.amend && snapshot.unborn {
            Some("Create the first commit before amending.")
        } else if !snapshot.merging()
            && !self.amend
            && !snapshot
                .files
                .iter()
                .any(|file| file.staged() || file.working())
        {
            Some("There are no changes to commit.")
        } else if self.draft.trim().is_empty() {
            Some("Enter a commit message.")
        } else {
            None
        }
    }
    pub(in crate::app) fn move_review(&mut self, ctx: &egui::Context, next: bool) {
        let queue = self.review_queue();
        let index = self
            .selection
            .as_ref()
            .and_then(|selected| queue.iter().position(|s| s == selected));
        let target = match index {
            Some(i) if next => queue.get(i + 1),
            Some(i) => i.checked_sub(1).and_then(|i| queue.get(i)),
            None if next => queue.first(),
            _ => None,
        };
        if let Some(selection) = target {
            self.select(ctx, selection.clone());
        }
    }
}

impl HeditApp {
    pub(in crate::app) fn git_sidebar_visible(&self) -> bool {
        self.sidebar == workbench_navigation::Sidebar::SourceControl
            && self.settings.show_sidebar
            && !self.distraction_free
    }

    pub(in crate::app) fn git_commit_context(&self) -> bool {
        self.source_control.view == View::Changes
            && (self.git_sidebar_visible() || self.source_control.open)
    }

    pub(super) fn git_track_remote(
        &mut self,
        ctx: &egui::Context,
        snapshot: &Snapshot,
        branch: &git::Branch,
    ) {
        if let Some(local) = snapshot
            .branches
            .iter()
            .find(|local| !local.remote && local.upstream == branch.reference)
        {
            if !local.current {
                self.git_action(ctx, Operation::Switch(local.name.clone()));
            }
            return;
        }
        let remote = snapshot
            .remotes
            .iter()
            .filter(|remote| branch.name.starts_with(&format!("{remote}/")))
            .max_by_key(|remote| remote.len());
        let name = remote
            .and_then(|remote| branch.name.strip_prefix(&format!("{remote}/")))
            .unwrap_or(&branch.name);
        self.source_control.prompt = Some(Prompt::Named {
            action: inspect::NamedAction::Track(branch.reference.clone()),
            name: name.into(),
            message: String::new(),
        });
    }
    pub(super) fn git_repository_header(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        use hedit::icons::{self, Icon};

        let pending = self.source_control.pending || self.busy || self.closing;
        let snapshot = self.source_control.snapshot.clone();
        ui.horizontal(|ui| {
            if self.settings.show_activity_bar {
                ui.small(RichText::new("Source Control").strong());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let menu = icons::button(ui, Icon::More, "More Actions");
                egui::Popup::menu(&menu)
                    .show(|ui| self.git_repository_actions(ui, ctx, snapshot.as_deref(), pending));
                if ui
                    .add_enabled_ui(!pending, |ui| icons::button(ui, Icon::Refresh, "Refresh"))
                    .inner
                    .clicked()
                {
                    self.source_control.error = None;
                    self.prepare_source_control(ctx);
                }
            });
        });
        if self.source_control.repos.len() > 1
            || (snapshot.is_none() && !self.source_control.repos.is_empty())
        {
            let mut selected = None;
            ui.add_enabled_ui(!pending, |ui| {
                egui::ComboBox::from_id_salt("git-repository")
                    .width(ui.available_width())
                    .truncate()
                    .selected_text(
                        self.source_control
                            .root
                            .as_deref()
                            .map(repo_name)
                            .unwrap_or_else(|| "Select Repository".into()),
                    )
                    .show_ui(ui, |ui| {
                        for root in &self.source_control.repos {
                            let name = repo_name(root);
                            let label = if self
                                .source_control
                                .repos
                                .iter()
                                .any(|other| other != root && repo_name(other) == name)
                            {
                                root.display().to_string()
                            } else {
                                name
                            };
                            if ui
                                .selectable_label(
                                    self.source_control.root.as_ref() == Some(root),
                                    label,
                                )
                                .on_hover_text(root.display().to_string())
                                .clicked()
                            {
                                selected = Some(root.clone());
                            }
                        }
                    });
            });
            if let Some(root) = selected {
                self.source_control.change_root(ctx, root);
            }
        }
    }

    fn git_repository_actions(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        snapshot: Option<&Snapshot>,
        pending: bool,
    ) {
        if let Some(snapshot) = snapshot {
            ui.add_enabled_ui(!pending && self.source_control.prompt.is_none(), |ui| {
                let tracking =
                    !snapshot.upstream.is_empty() && !snapshot.upstream_missing && !snapshot.unborn;
                let available = snapshot.operation.is_none();
                if ui
                    .add_enabled(!snapshot.remotes.is_empty(), egui::Button::new("Fetch"))
                    .clicked()
                {
                    self.git_action(ctx, Operation::Fetch);
                    ui.close();
                }
                if ui
                    .add_enabled(
                        tracking && available && snapshot.ahead == 0,
                        egui::Button::new("Pull"),
                    )
                    .clicked()
                {
                    self.git_action(ctx, Operation::Pull);
                    ui.close();
                }
                if ui
                    .add_enabled(
                        tracking && available && snapshot.behind == 0,
                        egui::Button::new("Push"),
                    )
                    .clicked()
                {
                    self.git_action(ctx, Operation::Push);
                    ui.close();
                }
                ui.separator();
                ui.menu_button("Commit", |ui| {
                    let staged = snapshot.files.iter().any(FileStatus::staged);
                    for (label, amend) in [("Commit Staged", false), ("Commit (Amend)", true)] {
                        if ui
                            .add_enabled(
                                available && if amend { !snapshot.unborn } else { staged },
                                egui::Button::new(label),
                            )
                            .clicked()
                        {
                            self.git_show_changes(ctx);
                            self.source_control.amend = amend;
                            self.git_submit_current_message(ctx);
                            ui.close();
                        }
                    }
                    if ui
                        .add_enabled(
                            available
                                && snapshot
                                    .commits
                                    .first()
                                    .is_some_and(|commit| !commit.parents.is_empty()),
                            egui::Button::new("Undo Last Commit…"),
                        )
                        .clicked()
                    {
                        self.git_confirm(
                            "Undo Last Commit",
                            "Undo the last commit and keep its changes staged?".into(),
                            Operation::UndoCommit {
                                expected: snapshot.oid.clone(),
                            },
                        );
                        ui.close();
                    }
                });
                ui.menu_button("Branch", |ui| {
                    if ui.button("Create Branch…").clicked() {
                        self.source_control.prompt = Some(Prompt::Branch(String::new()));
                        ui.close();
                    }
                    if ui.button("Manage Branches").clicked() {
                        self.source_control.visit(ctx, View::Branches, false);
                        ui.close();
                    }
                });
                ui.menu_button("Stash", |ui| {
                    if ui
                        .add_enabled(
                            available && !snapshot.unborn && !snapshot.files.is_empty(),
                            egui::Button::new("Stash Changes…"),
                        )
                        .clicked()
                    {
                        self.source_control.prompt = Some(Prompt::Stash(String::new()));
                        ui.close();
                    }
                    if ui.button("Manage Stashes").clicked() {
                        self.source_control.visit(ctx, View::Stashes, false);
                        ui.close();
                    }
                });
                if ui.button("Add Remote…").clicked() {
                    self.source_control.prompt = Some(Prompt::Remote {
                        name: "origin".into(),
                        url: String::new(),
                    });
                    ui.close();
                }
            });
            ui.separator();
            ui.add_enabled_ui(self.source_control.prompt.is_none(), |ui| {
                ui.menu_button("View & Sort", |ui| {
                    for (tree, label) in [(false, "View as List"), (true, "View as Tree")] {
                        if ui
                            .selectable_value(&mut self.source_control.tree_view, tree, label)
                            .clicked()
                        {
                            ui.close();
                        }
                    }
                    if ui.button("Expand All").clicked() {
                        self.expand_git_trees();
                        ui.close();
                    }
                    if ui.button("Filter…").clicked() {
                        self.source_control.ux.show_filter = true;
                        self.source_control.ux.focus_filter = true;
                        ui.close();
                    }
                });
                ui.menu_button("History", |ui| {
                    for (view, label) in [
                        (View::History, "Source Control Graph"),
                        (View::Compare, "Compare Revisions"),
                        (View::Tags, "Tags"),
                        (View::Recovery, "Reflog"),
                        (View::Blame, "Line History"),
                    ] {
                        if ui.button(label).clicked() {
                            if view != View::History {
                                self.source_control.show_review();
                            }
                            self.source_control.visit(ctx, view, false);
                            ui.close();
                        }
                    }
                });
            });
            ui.separator();
        }

        if ui
            .toggle_value(&mut self.source_control.output_open, "Show Git Output")
            .clicked()
        {
            self.source_control.ux.reveal_output = self.source_control.output_open;
            ui.close();
        }
        ui.add_enabled_ui(!pending, |ui| {
            if ui.button("Open Repository…").clicked() {
                self.browse_git_repository(ctx);
                ui.close();
            }
            if ui.button("Clone Repository…").clicked() {
                self.source_control.prompt = Some(Prompt::Clone {
                    url: String::new(),
                    destination: String::new(),
                });
                ui.close();
            }
        });
    }

    pub(in crate::app) fn git_status_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(snapshot) = self.source_control.snapshot.clone() else {
            return;
        };
        let pending = self.source_control.pending
            || self.busy
            || self.closing
            || self.source_control.prompt.is_some();
        let branch = if snapshot.branch == "(detached)" {
            snapshot.oid.chars().take(8).collect()
        } else {
            snapshot.branch.clone()
        };
        let branch_label = if branch.chars().count() > 24 {
            format!("{}…", branch.chars().take(23).collect::<String>())
        } else {
            branch.clone()
        };
        branch_icon(ui, self.theme.muted.color());
        self.git_branch_picker(ui, ctx, &snapshot, branch_label, &branch, pending);
        if self.source_control.pending {
            ui.spinner().on_hover_text(
                self.source_control
                    .ux
                    .activity
                    .unwrap_or("Refreshing repository"),
            );
        } else {
            self.git_remote_action(ui, ctx, &snapshot, pending, false);
        }
    }

    fn git_show_changes(&mut self, ctx: &egui::Context) {
        if self.source_control.view != View::Changes {
            self.source_control.navigate(View::Changes, false);
            ctx.request_repaint();
        }
    }

    fn git_branch_picker(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        snapshot: &Snapshot,
        label: String,
        full_name: &str,
        pending: bool,
    ) {
        let first = !self.source_control.ux.branch_menu_open;
        let menu = ui
            .add_enabled_ui(!pending, |ui| {
                ui.menu_button(RichText::new(label).strong(), |ui| {
                    ui.set_min_width(230.0);
                    if first {
                        self.source_control.ux.branch_filter.clear();
                    }
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut self.source_control.ux.branch_filter)
                            .hint_text("Find a branch…")
                            .desired_width(230.0),
                    );
                    if first {
                        field.request_focus();
                    }
                    if ui.button("New branch…").clicked() {
                        self.source_control.prompt = Some(Prompt::Branch(String::new()));
                        ui.close();
                        return None;
                    }
                    let matches: Vec<_> = snapshot
                        .branches
                        .iter()
                        .filter(|branch| {
                            matches_filter(&branch.name, &self.source_control.ux.branch_filter)
                        })
                        .collect();
                    let submit = (field.has_focus() || field.lost_focus())
                        && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter));
                    if submit {
                        if let [branch] = matches.as_slice() {
                            ui.close();
                            return Some(*branch);
                        }
                        field.request_focus();
                    }
                    if matches.is_empty() && !self.source_control.ux.branch_filter.is_empty() {
                        ui.label("No matching branches");
                    }
                    let mut scroll = egui::ScrollArea::vertical().max_height(240.0);
                    if first || field.changed() {
                        scroll = scroll.vertical_scroll_offset(0.0);
                    }
                    scroll
                        .show(ui, |ui| {
                            let mut selected = None;
                            for remote in [false, true] {
                                let mut branches = matches
                                    .iter()
                                    .copied()
                                    .filter(|branch| branch.remote == remote)
                                    .peekable();
                                if branches.peek().is_none() {
                                    continue;
                                }
                                ui.label(
                                    RichText::new(if remote {
                                        "Remote branches"
                                    } else {
                                        "Local branches"
                                    })
                                    .small()
                                    .color(self.theme.muted.color()),
                                );
                                for branch in branches {
                                    if ui.selectable_label(branch.current, &branch.name)
                                        .on_hover_text(if remote {
                                            "Switch to a local branch that tracks this remote branch"
                                        } else {
                                            "Switch to this branch"
                                        })
                                        .clicked() {
                                        selected = Some(branch);
                                        ui.close();
                                    }
                                }
                            }
                            selected
                        })
                        .inner
                })
            })
            .inner;
        let open = menu.inner.is_some();
        if self.source_control.ux.branch_menu_open != open {
            self.source_control.ux.branch_menu_open = open;
            ctx.request_repaint();
        }
        menu.response
            .on_hover_text(format!("{full_name}\nSwitch or create a branch"));
        match menu.inner.flatten() {
            Some(branch) if branch.remote => self.git_track_remote(ctx, snapshot, branch),
            Some(branch) if !branch.current => {
                self.git_action(ctx, Operation::Switch(branch.name.clone()));
            }
            _ => {}
        }
    }

    fn git_remote_action(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        snapshot: &Snapshot,
        pending: bool,
        primary: bool,
    ) {
        if snapshot.unborn
            || snapshot.branch == "(detached)"
            || snapshot.remotes.is_empty()
            || snapshot.operation.is_some()
        {
            return;
        }
        use hedit::icons::Icon;

        let publish = snapshot.upstream.is_empty() || snapshot.upstream_missing;
        let diverged = snapshot.ahead > 0 && snapshot.behind > 0;
        let action = if publish {
            "Publish Branch"
        } else if diverged {
            "Compare Changes"
        } else {
            "Sync Changes"
        };
        let counts = format!("{}↓ {}↑", snapshot.behind, snapshot.ahead);
        let title = if primary {
            format!("{action} {counts}")
        } else if snapshot.ahead == 0 && snapshot.behind == 0 {
            String::new()
        } else {
            counts.clone()
        };
        let button = if publish {
            egui::Button::new(action)
        } else {
            let icon = if diverged { Icon::Split } else { Icon::Refresh };
            let foreground = if primary {
                ui.visuals().selection.stroke.color
            } else {
                ui.visuals().text_color()
            };
            let image = scm_icon(ui, icon, foreground);
            if title.is_empty() {
                egui::Button::image(image)
            } else {
                egui::Button::image_and_text(image, title)
            }
        };
        let tooltip = if snapshot.upstream_missing {
            format!("Upstream branch missing. Publish {}", snapshot.branch)
        } else if publish {
            format!("Publish {}", snapshot.branch)
        } else if diverged {
            format!(
                "Branches have diverged. Compare {} with HEAD",
                snapshot.upstream
            )
        } else {
            format!("Sync Changes with {}. {counts}", snapshot.upstream)
        };
        let button = button.frame(primary).fill(if primary {
            self.theme.accent.color()
        } else {
            egui::Color32::TRANSPARENT
        });
        let response = ui
            .add_enabled_ui(!pending, |ui| {
                if primary {
                    ui.add_sized([ui.available_width(), 26.0], button)
                } else {
                    ui.add(button.small())
                }
            })
            .inner;
        if !primary {
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, !pending, action));
        }
        let response = response.on_hover_text(tooltip);
        if publish {
            egui::Popup::menu(&response).show(|ui| {
                for remote in &snapshot.remotes {
                    if ui.button(remote).clicked() {
                        self.git_action(
                            ctx,
                            Operation::Publish {
                                remote: remote.clone(),
                                branch: snapshot.branch.clone(),
                            },
                        );
                        ui.close();
                    }
                }
            });
        } else if response.clicked() {
            if diverged {
                if !self.git_sidebar_visible() {
                    self.show_git_sidebar(ctx);
                }
                self.git_start_comparison(ctx, snapshot.upstream.clone(), "HEAD".into());
            } else {
                self.git_confirm(
                    "Sync Changes",
                    format!(
                        "Pull from {} and push to the configured destination?",
                        snapshot.upstream
                    ),
                    Operation::Sync,
                );
            }
        }
    }

    pub(super) fn git_filter(&mut self, ui: &mut egui::Ui) {
        let hint = match self.source_control.view {
            View::History => "Find commits, authors, IDs…",
            View::Branches => "Find branches…",
            View::Tags => "Find tags…",
            View::Recovery => "Find reflog entries…",
            View::Blame => "Find code, authors, commits…",
            _ => "Filter files…",
        };
        ui.horizontal(|ui| {
            let clear = !self.source_control.filter.is_empty();
            let width = (ui.available_width() - 36.0).max(50.0);
            let field = ui.add_sized(
                [width, ui.spacing().interact_size.y],
                egui::TextEdit::singleline(&mut self.source_control.filter).hint_text(hint),
            );
            if std::mem::take(&mut self.source_control.ux.focus_filter) {
                field.request_focus();
            }
            if controls::close_button(
                ui,
                if clear {
                    "Clear filter"
                } else {
                    "Close filter"
                },
            )
            .clicked()
            {
                self.source_control.ux.show_filter = clear;
                self.source_control.filter.clear();
            }
        });
    }
    pub(in crate::app) fn git_submit_commit(&mut self, ctx: &egui::Context) {
        // A text event can arrive with the shortcut in the same input batch.
        // Let the commit field consume it before capturing the message.
        if self.git_sidebar_visible()
            && ctx.memory(|memory| memory.has_focus(egui::Id::unique("git-commit-message")))
            && self.source_control.ux.commit_field_pass != Some(ctx.cumulative_pass_nr())
            && ctx.input(|i| {
                i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Text(_)
                            | egui::Event::Paste(_)
                            | egui::Event::Ime(egui::ImeEvent::Commit(_))
                    )
                })
            })
        {
            self.source_control.ux.commit_after_input = Some(ctx.cumulative_pass_nr());
            return;
        }
        self.git_submit_current_message(ctx);
    }
    fn git_submit_current_message(&mut self, ctx: &egui::Context) {
        self.git_submit_message(ctx, None);
    }
    fn git_submit_message(&mut self, ctx: &egui::Context, after: Option<Operation>) {
        if !self.git_commit_context()
            || self.busy
            || self.closing
            || !self.source_control.can_commit()
        {
            return;
        }
        if let Some(snapshot) = self
            .source_control
            .snapshot
            .as_ref()
            .filter(|snapshot| snapshot.merging())
        {
            self.git_action(
                ctx,
                Operation::CommitMerge {
                    message: self.source_control.draft.clone(),
                    expected_head: snapshot.oid.clone(),
                },
            );
            return;
        }
        if !self.source_control.amend {
            let Some(snapshot) = self.source_control.snapshot.clone() else {
                return;
            };
            if !snapshot.files.iter().any(FileStatus::staged) {
                let paths: BTreeSet<_> = snapshot
                    .files
                    .iter()
                    .filter(|file| file.working())
                    .flat_map(|file| staging_paths(file, &DiffKind::Working))
                    .collect();
                let detail = if after.is_some() {
                    format!(
                        "Stage and commit {} changed files, then run the selected remote action?",
                        paths.len()
                    )
                } else {
                    format!(
                        "There are no staged changes. Stage and commit {} changed files?",
                        paths.len()
                    )
                };
                self.git_confirm_then(
                    "No Staged Changes",
                    detail,
                    Operation::StageCommit {
                        paths: paths.into_iter().collect(),
                        message: self.source_control.draft.clone(),
                    },
                    after,
                );
                return;
            }
        }
        let op = Operation::Commit {
            message: self.source_control.draft.clone(),
            amend: self.source_control.amend,
        };
        if self.source_control.amend {
            self.git_confirm("Amend last commit", "Replace HEAD with the staged changes and this message. This rewrites its commit ID.".into(), op);
        } else {
            self.git_action_then(ctx, op, after);
        }
    }
    pub(super) fn git_commit_box(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        snapshot: &Snapshot,
    ) {
        use hedit::icons::Icon;

        let message = if self.source_control.amend && snapshot.operation.is_none() {
            "Message for amended commit"
        } else {
            "Message"
        };
        ui.add(
            egui::TextEdit::multiline(&mut self.source_control.draft)
                .id(egui::Id::unique("git-commit-message"))
                .hint_text(message)
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        )
        .on_hover_text(format!(
            "{} to commit",
            self.shortcut_text(Command::GitCommit)
        ));
        self.source_control.ux.commit_field_pass = Some(ctx.cumulative_pass_nr());
        if self.source_control.ux.commit_after_input.take() == Some(ctx.cumulative_pass_nr()) {
            self.git_submit_current_message(ctx);
        }

        let ready = !self.source_control.pending
            && !self.busy
            && !self.closing
            && self.source_control.prompt.is_none();
        let has_changes = snapshot
            .files
            .iter()
            .any(|file| file.staged() || file.working() || file.conflict);
        let remote =
            !snapshot.remotes.is_empty() && !snapshot.unborn && snapshot.branch != "(detached)";
        if !has_changes
            && !self.source_control.amend
            && self.source_control.draft.trim().is_empty()
            && snapshot.operation.is_none()
            && remote
            && (snapshot.upstream.is_empty()
                || snapshot.upstream_missing
                || snapshot.ahead > 0
                || snapshot.behind > 0)
        {
            self.git_remote_action(ui, ctx, snapshot, !ready, true);
            return;
        }

        let accent = self.theme.accent.color();
        let foreground = ui.visuals().selection.stroke.color;
        let height = hedit::ui_style::Metrics::for_ui(ui).control;
        let label = if snapshot.operation.is_some() {
            "Continue"
        } else if self.source_control.amend {
            "Commit (Amend)"
        } else {
            "Commit"
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            let menu_width = height.max(SCM_ICON_SIZE + 2.0 * ui.spacing().button_padding.x);
            let width = (ui.available_width() - menu_width - ui.spacing().item_spacing.x).max(0.0);
            let can_continue =
                snapshot.operation.is_some() && !snapshot.files.iter().any(|file| file.conflict);
            let commit = ui.add_enabled(
                ready && (self.source_control.can_commit() || can_continue),
                egui::Button::image_and_text(
                    scm_icon(ui, Icon::Check, foreground),
                    RichText::new(label).color(foreground),
                )
                .fill(accent)
                .min_size(egui::vec2(width, height)),
            );
            if commit
                .on_disabled_hover_text(
                    self.source_control
                        .commit_blocker()
                        .unwrap_or("Wait for Git"),
                )
                .clicked()
            {
                if let Some(operation) = snapshot
                    .operation
                    .as_ref()
                    .filter(|_| !snapshot.merging() || self.source_control.draft.trim().is_empty())
                {
                    self.git_action(ctx, Operation::Continue(operation.clone()));
                } else {
                    self.git_submit_current_message(ctx);
                }
            }
            let choices = ui.add_enabled(
                ready,
                egui::Button::image(scm_icon(ui, Icon::ChevronDown, foreground))
                    .fill(accent)
                    .min_size(egui::vec2(menu_width, height)),
            );
            choices.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Button, ready, "Commit Actions")
            });
            let choices = choices.on_hover_text("Commit Actions");
            egui::Popup::menu(&choices).show(|ui| {
                if let Some(operation) = &snapshot.operation {
                    if ui.button("Abort Operation…").clicked() {
                        self.git_confirm(
                            "Abort Git Operation",
                            "Restore the state Git recorded before the operation?".into(),
                            Operation::Abort(operation.clone()),
                        );
                        ui.close();
                    }
                    return;
                }
                if ui
                    .selectable_label(!self.source_control.amend, "Commit")
                    .clicked()
                {
                    self.source_control.amend = false;
                    self.git_submit_current_message(ctx);
                    ui.close();
                }
                if ui
                    .add_enabled(
                        !snapshot.unborn,
                        egui::Button::new("Commit (Amend)").selected(self.source_control.amend),
                    )
                    .clicked()
                {
                    self.source_control.amend = true;
                    self.git_submit_current_message(ctx);
                    ui.close();
                }
                let can_push = remote
                    && !snapshot.upstream.is_empty()
                    && !snapshot.upstream_missing
                    && snapshot.behind == 0;
                let has_message = !self.source_control.draft.trim().is_empty();
                for (label, after) in [
                    ("Commit & Push", Operation::Push),
                    ("Commit & Sync", Operation::Sync),
                ] {
                    if ui
                        .add_enabled(
                            can_push && has_message && has_changes,
                            egui::Button::new(label),
                        )
                        .clicked()
                    {
                        self.source_control.amend = false;
                        self.git_submit_message(ctx, Some(after));
                        ui.close();
                    }
                }
            });
        });
        let unsaved = self.git_unsaved(None);
        if !unsaved.is_empty() {
            ui.label(
                RichText::new(format!("{} unsaved · Git uses saved files", unsaved.len()))
                    .small()
                    .color(self.theme.warning.color()),
            );
        }
    }
    pub(in crate::app) fn git_review_shortcuts(&mut self, ctx: &egui::Context) -> bool {
        let sidebar = self.git_sidebar_visible();
        if (!self.source_control.open && !sidebar)
            || self.busy
            || self.closing
            || self.transient_overlay_open()
            || egui::Popup::is_any_open(ctx)
        {
            return false;
        }
        let commit_focused = sidebar
            && ctx.memory(|memory| memory.has_focus(egui::Id::unique("git-commit-message")));
        if self.git_commit_context()
            && (self.source_control.open || commit_focused)
            && tab_switch::take_exact(ctx, Modifiers::COMMAND, Key::Enter)
        {
            if !self.overridden(Command::GitCommit) {
                self.git_submit_commit(ctx);
            }
            return true;
        }
        if !self.source_control.open {
            return false;
        }
        for (modifiers, key, command) in [
            (Modifiers::ALT, Key::ArrowDown, Command::GitNextFile),
            (Modifiers::ALT, Key::ArrowUp, Command::GitPreviousFile),
        ] {
            if tab_switch::take_exact(ctx, modifiers, key) {
                if !self.overridden(command) {
                    self.execute(ctx, command);
                }
                return true;
            }
        }
        false
    }
    pub(super) fn git_review_navigation(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let queue = self.source_control.review_queue();
        let index = self
            .source_control
            .selection
            .as_ref()
            .and_then(|s| queue.iter().position(|v| v == s));
        if self.source_control.ux.return_view.is_none() && index.is_none() {
            return;
        }
        let ready = !self.busy
            && !self.closing
            && self.source_control.ux.activity.is_none()
            && self.source_control.prompt.is_none();
        ui.horizontal_wrapped(|ui| {
            if let Some(view) = self.source_control.ux.return_view {
                if ui
                    .add_enabled(
                        ready,
                        egui::Button::new(format!("Back to {}", inspect::view_name(view))).small(),
                    )
                    .clicked()
                {
                    self.source_control.visit(ctx, view, false);
                }
            }
            if let Some(index) = index {
                ui.label(
                    RichText::new(format!("File {} of {}", index + 1, queue.len()))
                        .size(11.0)
                        .color(self.theme.muted.color()),
                );
                if queue.len() > 1 {
                    if ui
                        .add_enabled(ready && index > 0, egui::Button::new("Previous").small())
                        .on_hover_text(format!(
                            "Previous file · {}",
                            self.shortcut_text(Command::GitPreviousFile)
                        ))
                        .clicked()
                    {
                        self.execute(ctx, Command::GitPreviousFile);
                    }
                    if ui
                        .add_enabled(
                            ready && index + 1 < queue.len(),
                            egui::Button::new("Next").small(),
                        )
                        .on_hover_text(format!(
                            "Next file · {}",
                            self.shortcut_text(Command::GitNextFile)
                        ))
                        .clicked()
                    {
                        self.execute(ctx, Command::GitNextFile);
                    }
                }
            }
        });
    }
    pub(super) fn git_review_feedback(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.source_control.error.is_none() {
            if let Some(notice) = self.source_control.ux.notice {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(notice)
                            .size(11.0)
                            .color(self.theme.success.color()),
                    );
                    if controls::close_button(ui, "Dismiss Git result").clicked() {
                        self.source_control.ux.notice = None;
                    }
                });
            }
        }
        let operation = self
            .source_control
            .snapshot
            .as_ref()
            .and_then(|s| s.operation.clone());
        if let Some(operation) = operation {
            let conflicts = self
                .source_control
                .snapshot
                .as_ref()
                .is_some_and(|s| s.files.iter().any(|f| f.conflict));
            egui::Frame::new()
                .fill(self.theme.warning.color().gamma_multiply(0.08))
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(&operation);
                        if self.source_control.view != View::Changes
                            && ui.small_button("Resolve in Changes").clicked()
                        {
                            self.source_control.visit(ctx, View::Changes, false);
                        }
                        if !(self.git_sidebar_visible()
                            && self.source_control.view == View::Changes)
                            && ui
                                .add_enabled(
                                    !conflicts && !self.source_control.pending,
                                    egui::Button::new("Continue").small(),
                                )
                                .clicked()
                        {
                            self.git_action(ctx, Operation::Continue(operation.clone()));
                        }
                        if !(self.git_sidebar_visible()
                            && self.source_control.view == View::Changes)
                            && ui
                                .add_enabled(
                                    !self.source_control.pending,
                                    egui::Button::new("Abort…").small(),
                                )
                                .clicked()
                        {
                            self.git_confirm(
                                "Abort Git operation",
                                "Restore the state Git recorded when this operation began?".into(),
                                Operation::Abort(operation),
                            );
                        }
                    });
                    if conflicts {
                        ui.label("Resolve conflicts, save, and stage each resolved file.");
                    }
                });
        }
    }
    pub(super) fn git_empty_review(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.25).max(16.0));
            if self.source_control.snapshot.is_none() {
                let title = if self.source_control.pending {
                    "Reading repository…"
                } else {
                    "Open a Git repository to get started"
                };
                ui.add(egui::Label::new(RichText::new(title).size(17.0)).wrap());
                if !self.source_control.pending && ui.button("Open repository…").clicked() {
                    self.browse_git_repository(ctx);
                }
                return;
            }
            let queue = self.source_control.review_queue();
            let title = match self.source_control.view {
                View::Changes if !queue.is_empty() => {
                    format!("{} changes ready to review", queue.len())
                }
                View::Changes
                    if self
                        .source_control
                        .snapshot
                        .as_ref()
                        .is_some_and(|s| !s.files.is_empty()) =>
                {
                    "No files match this filter".into()
                }
                View::Changes => "Working tree clean".into(),
                View::History if self.source_control.commit.is_some() => {
                    "Select a file from this commit".into()
                }
                View::History => "Select a commit to inspect its changes".into(),
                View::Compare => "Compare branches, tags, or commits".into(),
                View::Branches => "Work on a branch".into(),
                View::Tags => "Mark a release with a tag".into(),
                View::Recovery => "Recover earlier work on a new branch".into(),
                View::Stashes => "Keep unfinished work for later".into(),
                View::Blame => "See who changed each line".into(),
            };
            ui.add(egui::Label::new(RichText::new(title).size(17.0)).wrap());
            ui.add_space(8.0);
            if let Some(first) = queue.first() {
                if ui
                    .add_enabled(
                        !self.source_control.pending,
                        egui::Button::new("Review changes"),
                    )
                    .clicked()
                {
                    self.source_control.select(ctx, first.clone());
                }
            } else {
                match self.source_control.view {
                    View::Changes if !self.source_control.filter.is_empty() => {
                        if ui.button("Clear filter").clicked() {
                            self.source_control.filter.clear();
                        }
                    }
                    View::Changes => {
                        if ui.button("View history").clicked() {
                            self.source_control.visit(ctx, View::History, false);
                        }
                    }
                    View::Branches => {
                        if ui
                            .add_enabled(
                                !self.source_control.pending,
                                egui::Button::new("New branch…"),
                            )
                            .clicked()
                        {
                            self.source_control.prompt = Some(Prompt::Branch(String::new()));
                        }
                    }
                    View::History => {
                        ui.label("Choose a commit in the sidebar, then select a file.");
                    }
                    View::Compare => {
                        ui.label("Choose Base and Head in the sidebar, then Compare.");
                    }
                    _ => {
                        ui.label("Choose an item in the sidebar to continue.");
                    }
                }
            }
        });
    }
}
