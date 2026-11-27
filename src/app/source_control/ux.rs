//! Shared Git navigation, compact repository controls, and task feedback.
use super::*;

#[derive(Default)]
pub(super) struct State {
    locations: BTreeMap<View, Location>,
    return_view: Option<View>,
    branch_filter: String,
    pub(super) branch_menu_open: bool,
    pub(super) collapsed_changes: BTreeSet<&'static str>,
    pub(super) reveal_output: bool,
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
            && !snapshot.files.iter().any(FileStatus::staged)
        {
            Some("Stage files or selected lines to include them.")
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
        let mut chosen = None;
        ui.horizontal(|ui| {
            let control = hedit::ui_style::Metrics::for_ui(ui).control;
            let width =
                (ui.available_width() - 38.0 - control - ui.spacing().item_spacing.x).max(60.0);
            ui.add_enabled_ui(!pending, |ui| {
                let label = |root: &Path| {
                    let name = repo_name(root);
                    if self
                        .source_control
                        .repos
                        .iter()
                        .any(|other| other != root && repo_name(other) == name)
                    {
                        root.display().to_string()
                    } else {
                        name
                    }
                };
                egui::ComboBox::from_id_salt("git-repository")
                    .width(width)
                    .truncate()
                    .selected_text(
                        self.source_control
                            .root
                            .as_deref()
                            .map(label)
                            .unwrap_or_else(|| "Choose repository".into()),
                    )
                    .show_ui(ui, |ui| {
                        for root in &self.source_control.repos {
                            if ui
                                .selectable_label(
                                    self.source_control.root.as_ref() == Some(root),
                                    label(root),
                                )
                                .on_hover_text(root.display().to_string())
                                .clicked()
                            {
                                chosen = Some(root.clone());
                            }
                        }
                    });
            });
            if ui
                .add_enabled_ui(!pending, |ui| {
                    icons::button(ui, Icon::Refresh, "Refresh Git status")
                })
                .inner
                .clicked()
            {
                self.source_control.error = None;
                self.prepare_source_control(ctx);
            }
            ui.menu_button("…", |ui| {
                if ui
                    .toggle_value(&mut self.source_control.output_open, "Git output")
                    .clicked()
                {
                    self.source_control.ux.reveal_output = self.source_control.output_open;
                    ui.close();
                }
                ui.add_enabled_ui(!pending, |ui| {
                    if ui.button("Open repository…").clicked() {
                        self.browse_git_repository(ctx);
                        ui.close();
                    }
                    if ui.button("Clone repository…").clicked() {
                        self.source_control.prompt = Some(Prompt::Clone {
                            url: String::new(),
                            destination: String::new(),
                        });
                        ui.close();
                    }
                });
            })
            .response
            .on_hover_text("Repository actions");
        });
        if let Some(root) = chosen {
            self.source_control.change_root(ctx, root);
            return;
        }
        let Some(snapshot) = snapshot else {
            return;
        };
        ui.horizontal(|ui| {
            branch_icon(ui, self.theme.accent.color());
            let name = if snapshot.branch == "(detached)" {
                format!("Detached · {}", &snapshot.oid[..snapshot.oid.len().min(8)])
            } else {
                snapshot.branch.clone()
            };
            let short = if name.chars().count() > 28 {
                format!("{}…", name.chars().take(27).collect::<String>())
            } else {
                name.clone()
            };
            self.git_branch_picker(ui, ctx, &snapshot, short, &name, pending);
            if self.source_control.pending && self.source_control.ux.activity.is_none() {
                ui.spinner().on_hover_text("Refreshing repository");
            }
        });
        self.git_sync_controls(ui, ctx, &snapshot, pending);
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

    fn git_sync_controls(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        snapshot: &Snapshot,
        pending: bool,
    ) {
        let muted = self.theme.muted.color();
        ui.horizontal_wrapped(|ui| {
            if snapshot.remotes.is_empty() {
                ui.label(RichText::new("Local repository").small().color(muted));
                if ui
                    .add_enabled(!pending, egui::Button::new("Add remote…").small())
                    .clicked()
                {
                    self.source_control.prompt = Some(Prompt::Remote {
                        name: "origin".into(),
                        url: String::new(),
                    });
                }
                return;
            }
            let can_sync = !pending && snapshot.operation.is_none();
            if snapshot.upstream_missing {
                ui.label(
                    RichText::new("Upstream branch missing")
                        .small()
                        .color(self.theme.warning.color()),
                )
                .on_hover_text(&snapshot.upstream);
            }
            if snapshot.upstream.is_empty() || snapshot.upstream_missing || snapshot.unborn {
                if !snapshot.unborn && snapshot.branch != "(detached)" {
                    ui.add_enabled_ui(can_sync, |ui| {
                        ui.menu_button("Publish branch", |ui| {
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
                    });
                } else if snapshot.unborn {
                    ui.label(
                        RichText::new("Make the first commit to publish")
                            .small()
                            .color(muted),
                    );
                } else {
                    ui.label(
                        RichText::new("Switch to a branch to publish")
                            .small()
                            .color(muted),
                    );
                }
            } else {
                match (snapshot.ahead, snapshot.behind) {
                    (0, 0) => {
                        ui.label(RichText::new("Up to date").small().color(muted))
                            .on_hover_text(format!(
                                "{}\nBased on the last fetch",
                                snapshot.upstream
                            ));
                        ui.add_enabled_ui(can_sync, |ui| {
                            ui.menu_button("Sync…", |ui| {
                                if ui.button("Pull").clicked() {
                                    self.git_action(ctx, Operation::Pull);
                                    ui.close();
                                }
                                if ui.button("Push").clicked() {
                                    self.git_action(ctx, Operation::Push);
                                    ui.close();
                                }
                            });
                        });
                    }
                    (ahead, 0) => {
                        if ui
                            .add_enabled(
                                can_sync,
                                egui::Button::new(format!("Push {ahead}")).small(),
                            )
                            .on_hover_text("Push to the configured destination")
                            .clicked()
                        {
                            self.git_action(ctx, Operation::Push);
                        }
                    }
                    (0, behind) => {
                        if ui
                            .add_enabled(
                                can_sync,
                                egui::Button::new(format!("Pull {behind}")).small(),
                            )
                            .on_hover_text(format!("Fast-forward from {}", snapshot.upstream))
                            .clicked()
                        {
                            self.git_action(ctx, Operation::Pull);
                        }
                    }
                    (ahead, behind) => {
                        ui.label(
                            RichText::new("Branches have diverged")
                                .small()
                                .color(self.theme.warning.color()),
                        )
                        .on_hover_text(format!(
                            "{ahead} ahead, {behind} behind {}",
                            snapshot.upstream
                        ));
                        if ui
                            .add_enabled(!pending, egui::Button::new("Compare").small())
                            .clicked()
                        {
                            self.git_start_comparison(
                                ctx,
                                snapshot.upstream.clone(),
                                "HEAD".into(),
                            );
                        }
                    }
                }
            }
            if ui
                .add_enabled(!pending, egui::Button::new("Fetch").small())
                .on_hover_text("Update remote references")
                .clicked()
            {
                self.git_action(ctx, Operation::Fetch);
            }
        });
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
            let width = (ui.available_width() - if clear { 30.0 } else { 0.0 }).max(50.0);
            ui.add_sized(
                [width, ui.spacing().interact_size.y],
                egui::TextEdit::singleline(&mut self.source_control.filter).hint_text(hint),
            );
            if clear && controls::close_button(ui, "Clear filter").clicked() {
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
        let op = Operation::Commit {
            message: self.source_control.draft.clone(),
            amend: self.source_control.amend,
        };
        if self.source_control.amend {
            self.git_confirm("Amend last commit", "Replace HEAD with the staged changes and this message. This rewrites its commit ID.".into(), op);
        } else {
            self.git_action(ctx, op);
        }
    }
    pub(super) fn git_commit_box(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        snapshot: &Snapshot,
    ) {
        let staged = snapshot.files.iter().filter(|f| f.staged()).count();
        ui.add(
            egui::TextEdit::multiline(&mut self.source_control.draft)
                .id(egui::Id::unique("git-commit-message"))
                .hint_text("Commit message")
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        self.source_control.ux.commit_field_pass = Some(ctx.cumulative_pass_nr());
        if self.source_control.ux.commit_after_input.take() == Some(ctx.cumulative_pass_nr()) {
            self.git_submit_current_message(ctx);
        }
        ui.horizontal(|ui| {
            let ready = self.command_enabled(Command::GitCommit);
            let label = if snapshot.merging() {
                "Commit merge".into()
            } else if self.source_control.amend {
                "Amend last commit".into()
            } else {
                format!("Commit staged ({staged})")
            };
            let width = (ui.available_width() - 38.0).max(80.0);
            let commit = ui
                .add_enabled_ui(ready, |ui| {
                    ui.add_sized(
                        [width, 28.0],
                        egui::Button::new(label)
                            .fill(self.theme.accent.color().gamma_multiply(0.18)),
                    )
                })
                .inner;
            if commit
                .on_hover_text(format!(
                    "{} · {}",
                    if snapshot.merging() {
                        "Commit the merge"
                    } else {
                        "Commit staged changes"
                    },
                    self.shortcut_text(Command::GitCommit)
                ))
                .clicked()
            {
                self.git_submit_current_message(ctx);
            }
            ui.add_enabled_ui(!self.source_control.pending, |ui| {
                ui.menu_button("…", |ui| {
                    ui.add_enabled(
                        !snapshot.unborn && snapshot.operation.is_none(),
                        egui::Checkbox::new(&mut self.source_control.amend, "Amend last commit"),
                    );
                });
            });
        });
        let hint = if self.busy || self.closing {
            "Wait for the current editor task."
        } else {
            self.source_control
                .commit_blocker()
                .unwrap_or(if snapshot.merging() {
                    "Finish the merge with this message."
                } else if self.source_control.amend {
                    "Replaces the last commit after confirmation."
                } else {
                    "Only staged changes will be committed."
                })
        };
        ui.add(
            egui::Label::new(
                RichText::new(hint)
                    .size(11.0)
                    .color(self.theme.muted.color()),
            )
            .wrap(),
        );
        let unsaved = self.git_unsaved(None);
        if !unsaved.is_empty() {
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "{} unsaved file(s) · Git uses saved text",
                        unsaved.len()
                    ))
                    .small()
                    .color(self.theme.warning.color()),
                )
                .wrap(),
            );
        }
        ui.add_space(8.0);
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
                        if ui
                            .add_enabled(
                                !conflicts && !self.source_control.pending,
                                egui::Button::new("Continue").small(),
                            )
                            .clicked()
                        {
                            self.git_action(ctx, Operation::Continue(operation.clone()));
                        }
                        if ui
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
