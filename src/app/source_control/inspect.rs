//! Less frequent Git tools share one background query path and existing review UI.
use super::*;
use git::{BlameLine, Comparison, HistoryScope, ReflogEntry, Tag};

pub(super) fn view_name(view: View) -> &'static str {
    match view {
        View::Changes => "Changes",
        View::History => "History",
        View::Branches => "Branches",
        View::Stashes => "Stashes",
        View::Compare => "Compare",
        View::Tags => "Tags",
        View::Recovery => "Recovery",
        View::Blame => "Line history",
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Query {
    History(HistoryScope, usize),
    Compare {
        base: String,
        head: String,
        merge_base: bool,
    },
    Tags,
    Recovery(usize),
    Blame {
        path: PathBuf,
        reference: String,
    },
}
pub(super) enum Data {
    History(Vec<git::Commit>),
    Compare(Comparison),
    Tags(Vec<Tag>),
    Recovery(Vec<ReflogEntry>),
    Blame(Vec<BlameLine>),
}
impl Query {
    pub(super) fn run(&self, git: &Git, root: &Path) -> Result<Data, String> {
        match self {
            Self::History(scope, limit) => {
                git.scoped_history(root, *limit, scope).map(Data::History)
            }
            Self::Compare {
                base,
                head,
                merge_base,
            } => git
                .compare(root, base, head, *merge_base)
                .map(Data::Compare),
            Self::Tags => git.tags(root).map(Data::Tags),
            Self::Recovery(limit) => git.reflog(root, *limit).map(Data::Recovery),
            Self::Blame { path, reference } => git.blame(root, reference, path).map(Data::Blame),
        }
    }
}
pub(super) struct State {
    scope: HistoryScope,
    history_reference: String,
    history_path: String,
    compare_base: String,
    compare_head: String,
    merge_base: bool,
    comparison: Option<Query>,
    blame_path: String,
    blame_reference: String,
    blame: Option<Query>,
    blame_rows: BlameRows,
    pub loaded: Option<Query>,
    pub data: Option<Arc<Data>>,
}
#[derive(Default)]
struct BlameRows {
    source: std::sync::Weak<Data>,
    filter: String,
    rows: Arc<Vec<usize>>,
    width: usize,
}
impl Default for State {
    fn default() -> Self {
        Self {
            scope: HistoryScope::default(),
            history_reference: String::new(),
            history_path: String::new(),
            compare_base: "HEAD".into(),
            compare_head: "HEAD".into(),
            merge_base: true,
            comparison: None,
            blame_path: String::new(),
            blame_reference: "HEAD".into(),
            blame: None,
            blame_rows: BlameRows::default(),
            loaded: None,
            data: None,
        }
    }
}
#[derive(Clone)]
pub(in crate::app) enum NamedAction {
    Branch(String),
    Tag(String),
    Rename(String),
    Track(String),
}
impl NamedAction {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Branch(_) => "Create branch at commit",
            Self::Tag(_) => "Create annotated tag",
            Self::Rename(_) => "Rename branch",
            Self::Track(_) => "Create tracking branch",
        }
    }
    pub fn description(&self) -> &'static str {
        match self {
            Self::Branch(_) => {
                "Keep this commit on a new branch. Your current checkout stays in place."
            }
            Self::Tag(_) => {
                "Give this commit a name and a release message. The tag is created locally."
            }
            Self::Rename(_) => {
                "Choose a new local branch name. Tracking configuration follows the branch."
            }
            Self::Track(_) => "Choose a local branch name to track this remote branch.",
        }
    }
    pub fn button(&self) -> &'static str {
        match self {
            Self::Branch(_) => "Create branch",
            Self::Tag(_) => "Create tag",
            Self::Rename(_) => "Rename",
            Self::Track(_) => "Create branch",
        }
    }
    pub fn operation(&self, name: &str, message: &str) -> Operation {
        match self {
            Self::Branch(oid) => Operation::CreateBranchAt {
                name: name.into(),
                oid: oid.clone(),
            },
            Self::Tag(oid) => Operation::CreateTag {
                name: name.into(),
                oid: oid.clone(),
                message: message.into(),
            },
            Self::Track(remote) => Operation::TrackBranch {
                local: name.into(),
                remote: remote.clone(),
            },
            Self::Rename(old) => Operation::RenameBranch {
                old: old.clone(),
                new: name.into(),
            },
        }
    }
}
impl SourceControl {
    pub(super) fn prepare_inspection_view(&mut self) {
        if self.view == View::Compare && self.inspection.comparison.is_none() {
            let Some(snapshot) = &self.snapshot else {
                return;
            };
            if snapshot.unborn {
                return;
            }
            let base = if !snapshot.upstream.is_empty() {
                snapshot.upstream.clone()
            } else if let Some(branch) = snapshot
                .branches
                .iter()
                .find(|b| !b.current && matches!(b.name.as_str(), "main" | "master"))
            {
                branch.reference.clone()
            } else {
                snapshot
                    .commits
                    .iter()
                    .find(|c| c.oid == snapshot.oid)
                    .and_then(|c| c.parents.first())
                    .cloned()
                    .unwrap_or_else(|| "HEAD".into())
            };
            self.inspection.compare_base = base.clone();
            self.inspection.compare_head = "HEAD".into();
            self.inspection.comparison = Some(Query::Compare {
                base,
                head: "HEAD".into(),
                merge_base: self.inspection.merge_base,
            });
        }
    }
    pub(super) fn inspection_query(&self) -> Option<Query> {
        match self.view {
            View::History if self.inspection.scope != HistoryScope::default() => {
                Some(Query::History(self.inspection.scope.clone(), self.limit))
            }
            View::Compare => self.inspection.comparison.clone(),
            View::Tags => Some(Query::Tags),
            View::Recovery => Some(Query::Recovery(self.limit)),
            View::Blame => self.inspection.blame.clone(),
            _ => None,
        }
    }
    pub(super) fn inspection_data(&self) -> Option<Arc<Data>> {
        (self.inspection.loaded == self.inspection_query())
            .then(|| self.inspection.data.clone())
            .flatten()
    }
    pub(super) fn clear_review(&mut self) {
        self.selection = None;
        self.diff = None;
        self.commit = None;
        self.files.clear();
        self.body.clear();
        self.clear_line_selection();
    }
}
impl HeditApp {
    pub(super) fn git_name_at(&mut self, action: NamedAction) {
        self.source_control.prompt = Some(Prompt::Named {
            action,
            name: String::new(),
            message: String::new(),
        });
    }
    pub(super) fn git_start_comparison(&mut self, ctx: &egui::Context, base: String, head: String) {
        if self.source_control.pending && self.source_control.ux.activity.is_some() {
            return;
        }
        let state = &mut self.source_control;
        state.navigate(View::Compare, true);
        state.clear_review();
        state.filter.clear();
        state.view = View::Compare;
        state.inspection.compare_base = base.clone();
        state.inspection.compare_head = head.clone();
        state.inspection.comparison = Some(Query::Compare {
            base,
            head,
            merge_base: state.inspection.merge_base,
        });
        state.error = None;
        state.request(ctx, None);
    }
    pub(super) fn git_branch_history(&mut self, ctx: &egui::Context, reference: String) {
        if self.source_control.pending && self.source_control.ux.activity.is_some() {
            return;
        }
        let state = &mut self.source_control;
        state.navigate(View::History, true);
        state.clear_review();
        state.view = View::History;
        state.filter.clear();
        state.inspection.history_reference = reference.clone();
        state.inspection.scope.reference = Some(reference);
        state.request(ctx, None);
    }
    pub(super) fn git_file_history(&mut self, ctx: &egui::Context, path: PathBuf) {
        if self.source_control.pending && self.source_control.ux.activity.is_some() {
            return;
        }
        let state = &mut self.source_control;
        state.navigate(View::History, true);
        state.clear_review();
        state.view = View::History;
        state.filter.clear();
        state.inspection.history_path = path.to_string_lossy().into_owned();
        state.inspection.scope.path = Some(path);
        state.request(ctx, None);
    }
    pub(super) fn git_line_history(&mut self, ctx: &egui::Context, path: PathBuf) {
        if self.source_control.pending && self.source_control.ux.activity.is_some() {
            return;
        }
        let state = &mut self.source_control;
        state.navigate(View::Blame, true);
        state.clear_review();
        state.view = View::Blame;
        state.inspection.blame_path = path.to_string_lossy().into_owned();
        state.inspection.blame_reference = "HEAD".into();
        state.inspection.blame = Some(Query::Blame {
            path,
            reference: "HEAD".into(),
        });
        state.request(ctx, None);
    }
    fn git_inspect_commit(&mut self, ctx: &egui::Context, oid: String) {
        if self.source_control.pending && self.source_control.ux.activity.is_some() {
            return;
        }
        let state = &mut self.source_control;
        state.navigate(View::History, true);
        state.clear_review();
        state.view = View::History;
        state.filter.clear();
        state.inspection.scope = HistoryScope {
            reference: Some(oid.clone()),
            path: None,
        };
        state.inspection.history_reference = oid.clone();
        state.inspection.history_path.clear();
        state.commit = Some(oid);
        state.request(ctx, None);
    }
    fn git_active_relative_path(&self) -> Option<PathBuf> {
        let root = self.source_control.root.as_ref()?;
        let path = Path::new(
            self.workspace
                .open
                .get(self.workspace.active)?
                .page
                .info
                .path
                .as_ref()?,
        );
        path.strip_prefix(root).ok().map(Path::to_path_buf)
    }
    pub(super) fn git_inspection_controls(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        snapshot: &Snapshot,
    ) {
        let pending = self.source_control.pending || self.busy || self.closing;
        let active_path = self.git_active_relative_path();
        ui.add_enabled_ui(!pending, |ui| {
            match self.source_control.view {
                View::History => {
                    egui::CollapsingHeader::new("History scope").id_salt("git-history-scope").show(ui, |ui| {
                        let state = &mut self.source_control.inspection;
                        ui.label("Branch or revision · empty means all branches");
                        ui.add(egui::TextEdit::singleline(&mut state.history_reference).hint_text("All branches").desired_width(f32::INFINITY));
                        ui.label("Repository-relative file or folder");
                        ui.add(egui::TextEdit::singleline(&mut state.history_path).hint_text("All files").desired_width(f32::INFINITY));
                        let (mut apply, mut reset, mut active) = (false, false, false);
                        ui.horizontal_wrapped(|ui| {
                            apply = ui.small_button("Apply scope").clicked();
                            reset = ui.small_button("All history").clicked();
                            active = ui.add_enabled(active_path.is_some(), egui::Button::new("Active file").small()).clicked();
                        });
                        if apply {
                            state.scope = HistoryScope { reference: (!state.history_reference.trim().is_empty()).then(|| state.history_reference.trim().into()), path: (!state.history_path.is_empty()).then(|| PathBuf::from(&state.history_path)) };
                        }
                        if reset { state.scope = HistoryScope::default(); state.history_reference.clear(); state.history_path.clear(); }
                        if apply || reset { self.source_control.clear_review(); self.source_control.error = None; self.source_control.request(ctx, None); }
                        if active { if let Some(path) = self.git_active_relative_path() { self.git_file_history(ctx, path); } }
                    });
                    let scope = &self.source_control.inspection.scope;
                    if scope != &HistoryScope::default() {
                        ui.add(egui::Label::new(RichText::new(format!("{} · {}", scope.reference.as_deref().unwrap_or("All branches"), scope.path.as_ref().map_or("All files".into(), |p| p.display().to_string()))).small().color(self.theme.accent.color())).wrap());
                    }
                }
                View::Compare => {
                    let state = &mut self.source_control.inspection;
                    let entered = revision_input(ui, "Base", &mut state.compare_base, snapshot) | revision_input(ui, "Head", &mut state.compare_head, snapshot);
                    ui.checkbox(&mut state.merge_base, "Changes since common ancestor").on_hover_text("Show what Head introduced since the branches diverged. Turn off to compare their current contents directly.");
                    let mut apply = entered;
                    let mut swapped = false;
                    ui.horizontal_wrapped(|ui| {
                        if ui.small_button("Swap").clicked() { std::mem::swap(&mut state.compare_base, &mut state.compare_head); swapped = true; }
                        apply |= ui.add_enabled(!state.compare_base.trim().is_empty() && !state.compare_head.trim().is_empty(), egui::Button::new("Compare")).clicked();
                    });
                    let changed = state.comparison.as_ref() != Some(&Query::Compare { base: state.compare_base.trim().into(), head: state.compare_head.trim().into(), merge_base: state.merge_base });
                    if changed && !apply && !swapped { ui.label(RichText::new("Inputs changed · Compare to update").small().color(self.theme.warning.color())); }
                    if (apply || swapped) && !state.compare_base.trim().is_empty() && !state.compare_head.trim().is_empty() { let (base, head) = (state.compare_base.trim().to_owned(), state.compare_head.trim().to_owned()); self.git_start_comparison(ctx, base, head); }
                }
                View::Blame => {
                    let state = &mut self.source_control.inspection;
                    ui.label("Repository-relative file");
                    ui.add(egui::TextEdit::singleline(&mut state.blame_path).hint_text("src/main.rs").desired_width(f32::INFINITY));
                    revision_input(ui, "Revision", &mut state.blame_reference, snapshot);
                    let (mut apply, mut active) = (false, false);
                    ui.horizontal_wrapped(|ui| {
                        apply = ui.add_enabled(!state.blame_path.is_empty() && !state.blame_reference.trim().is_empty(), egui::Button::new("Show line history")).clicked();
                        active = ui.add_enabled(active_path.is_some(), egui::Button::new("Active file").small()).clicked();
                    });
                    if apply {
                        state.blame = Some(Query::Blame { path: PathBuf::from(&state.blame_path), reference: state.blame_reference.trim().into() });
                        self.source_control.clear_review(); self.source_control.error = None; self.source_control.request(ctx, None);
                    }
                    if active { if let Some(path) = self.git_active_relative_path() { self.git_line_history(ctx, path); } }
                    ui.add(egui::Label::new("Saved commits only. Select a commit ID to inspect the change.").wrap());
                }
                _ => (),
            }
        });
    }
    pub(super) fn git_inspection_sidebar(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        snapshot: &Snapshot,
    ) {
        if self.source_control.view == View::Tags
            && !snapshot.unborn
            && ui.button("Tag HEAD…").clicked()
        {
            self.git_name_at(NamedAction::Tag(snapshot.oid.clone()));
        }
        if self.source_control.view == View::Recovery {
            ui.add(egui::Label::new("The reflog records local branch movements. Recover a commit by giving it a new branch.").wrap());
            if ui
                .add_enabled(
                    !snapshot.unborn && snapshot.operation.is_none(),
                    egui::Button::new("Undo last commit…"),
                )
                .clicked()
            {
                self.git_confirm("Undo last commit", "Move this branch back one commit and keep its changes staged. Working files and unsaved editor text stay in place. This rewrites history; use Revert for shared commits.".into(), Operation::UndoCommit { expected: snapshot.oid.clone() });
            }
            ui.separator();
        }
        let Some(data) = self.source_control.inspection_data() else {
            return;
        };
        let filter = self.source_control.filter.clone();
        match &*data {
            Data::Compare(comparison) => {
                ui.label(format!(
                    "{} ahead · {} behind",
                    comparison.ahead, comparison.behind
                ));
                ui.label(
                    RichText::new(format!("{} changed files", comparison.files.len())).strong(),
                );
                ui.label(
                    RichText::new(format!(
                        "{} → {}",
                        &comparison.base[..8],
                        &comparison.head[..8]
                    ))
                    .small(),
                )
                .on_hover_text(
                    "Review is pinned to these commit IDs. Refresh updates moving branches.",
                );
                self.git_tree_controls(ui);
                self.git_file_tree(
                    ui,
                    ctx,
                    "comparison-files",
                    &comparison.files,
                    DiffKind::Compare {
                        base: comparison.base.clone(),
                        head: comparison.head.clone(),
                    },
                );
                if comparison.files.is_empty() {
                    ui.label("No differences in this comparison.");
                }
            }
            Data::Tags(tags) => {
                for tag in tags.iter().filter(|t| matches_filter(&t.name, &filter)) {
                    ui.push_id(&tag.name, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(&tag.name);
                            ui.menu_button("…", |ui| {
                                if let Some(oid) = &tag.commit {
                                    if ui.button("Inspect commit").clicked() {
                                        self.git_inspect_commit(ctx, oid.clone());
                                        ui.close_menu();
                                    }
                                    if ui.button("Compare with HEAD").clicked() {
                                        self.git_start_comparison(ctx, oid.clone(), "HEAD".into());
                                        ui.close_menu();
                                    }
                                    if ui.button("Branch here…").clicked() {
                                        self.git_name_at(NamedAction::Branch(oid.clone()));
                                        ui.close_menu();
                                    }
                                }
                                if ui.button("Copy tag name").clicked() {
                                    ui.ctx().copy_text(tag.name.clone());
                                    ui.close_menu();
                                }
                                if ui.button("Delete local tag…").clicked() {
                                    self.git_confirm(
                                        "Delete local tag",
                                        format!(
                                            "Delete {} locally? The remote tag stays in place.",
                                            tag.name
                                        ),
                                        Operation::DeleteTag {
                                            name: tag.name.clone(),
                                            expected: tag.object.clone(),
                                        },
                                    );
                                    ui.close_menu();
                                }
                            });
                        });
                    });
                }
                if tags.is_empty() {
                    ui.label("No tags yet. Tag HEAD to mark a release.");
                }
            }
            Data::Recovery(entries) => {
                for entry in entries.iter().filter(|e| {
                    matches_filter(&format!("{} {} {}", e.oid, e.selector, e.subject), &filter)
                }) {
                    ui.push_id(&entry.selector, |ui| {
                        ui.add(egui::Label::new(RichText::new(&entry.subject).strong()).wrap());
                        ui.label(
                            RichText::new(format!("{} · {}", entry.selector, entry.date))
                                .small()
                                .color(self.theme.muted.color()),
                        );
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .small_button(&entry.oid[..8])
                                .on_hover_text("Inspect this commit")
                                .clicked()
                            {
                                self.git_inspect_commit(ctx, entry.oid.clone());
                            }
                            if ui.small_button("Recover branch…").clicked() {
                                self.git_name_at(NamedAction::Branch(entry.oid.clone()));
                            }
                        });
                        ui.separator();
                    });
                }
                if entries.is_empty() {
                    ui.label("No local reflog entries.");
                }
                if entries.len() >= self.source_control.limit
                    && self.source_control.limit < 2000
                    && ui.button("Load more entries").clicked()
                {
                    self.source_control.limit += 200;
                    self.source_control.request(ctx, None);
                }
            }
            _ => (),
        }
    }
    pub(super) fn git_blame_canvas(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(Query::Blame { path, reference }) = self.source_control.inspection.blame.clone()
        else {
            ui.label("Choose a file in the sidebar, or use Line history from a changed file’s context menu.");
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.strong(path.display().to_string());
            ui.label(reference);
        });
        let Some(data) = self.source_control.inspection_data() else {
            ui.label("Loading line history…");
            return;
        };
        let Data::Blame(lines) = &*data else {
            return;
        };
        if lines.is_empty() {
            ui.label("This file is empty at the selected revision.");
            return;
        }
        let style = self.git_text_style(ui, &path);
        let font = style.font;
        let row_height = font.row_height(ui).max(24.0);
        let language = style.language;
        let filter = self.source_control.filter.to_lowercase();
        let cache = &mut self.source_control.inspection.blame_rows;
        if cache.filter != filter
            || cache
                .source
                .upgrade()
                .is_none_or(|old| !Arc::ptr_eq(&old, &data))
        {
            cache.source = Arc::downgrade(&data);
            cache.filter = filter.clone();
            cache.rows = Arc::new(
                lines
                    .iter()
                    .enumerate()
                    .filter_map(|(index, line)| {
                        (filter.is_empty()
                            || format!(
                                "{} {} {} {}",
                                line.oid, line.author, line.summary, line.text
                            )
                            .to_lowercase()
                            .contains(&filter))
                        .then_some(index)
                    })
                    .collect(),
            );
            cache.width = cache
                .rows
                .iter()
                .map(|i| lines[*i].text.chars().take(64 * 1024).count())
                .max()
                .unwrap_or(0);
        }
        let rows = cache.rows.clone();
        let width = cache.width as f32 * font.size * 0.7 + 320.0;
        egui::ScrollArea::both().id_salt("git-blame").show_rows(
            ui,
            row_height,
            rows.len(),
            |ui, range| {
                ui.set_min_width(ui.available_width().max(width));
                for index in &rows[range] {
                    let line = &lines[*index];
                    ui.horizontal(|ui| {
                        ui.add_enabled_ui(!self.source_control.pending, |ui| {
                            if ui
                                .add_sized(
                                    [72.0, row_height],
                                    egui::Button::new(RichText::new(&line.oid[..8]).monospace())
                                        .frame(false),
                                )
                                .on_hover_text(&line.summary)
                                .clicked()
                            {
                                self.git_inspect_commit(ctx, line.oid.clone());
                            }
                        });
                        ui.add_sized(
                            [140.0, row_height],
                            egui::Label::new(
                                RichText::new(&line.author)
                                    .small()
                                    .color(self.theme.muted.color()),
                            )
                            .truncate(),
                        )
                        .on_hover_text(format!("{}\n{}", line.author, line.summary));
                        ui.add_sized(
                            [48.0, row_height],
                            egui::Label::new(
                                RichText::new(line.number.to_string())
                                    .font(font.gutter_font())
                                    .color(self.theme.muted.color()),
                            ),
                        );
                        let text: String = line
                            .text
                            .chars()
                            .take(64 * 1024 / style.tab_size.max(4))
                            .collect();
                        let text =
                            hedit::advanced::Projection::new(&text, style.tab_size, &[]).text;
                        ui.add(
                            egui::Label::new(editor_layout::code_job(
                                &text,
                                language,
                                font,
                                f32::INFINITY,
                                &self.theme,
                                None,
                            ))
                            .selectable(true),
                        );
                    });
                }
            },
        );
    }
}
fn revision_input(ui: &mut egui::Ui, label: &str, value: &mut String, snapshot: &Snapshot) -> bool {
    let mut entered = false;
    ui.label(label);
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 38.0).max(60.0);
        let input = ui.add_sized(
            [width, ui.spacing().interact_size.y],
            egui::TextEdit::singleline(value),
        );
        entered = input.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        ui.push_id(label, |ui| {
            let picker = ui.menu_button("…", |ui| {
                if ui.button("HEAD").clicked() {
                    *value = "HEAD".into();
                    ui.close_menu();
                }
                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .show(ui, |ui| {
                        for branch in &snapshot.branches {
                            if ui
                                .selectable_label(*value == branch.reference, &branch.name)
                                .clicked()
                            {
                                *value = branch.reference.clone();
                                ui.close_menu();
                            }
                        }
                    });
            });
            picker
                .response
                .on_hover_text(format!("Choose {label} revision"))
                .widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        ui.is_enabled(),
                        format!("Choose {label} revision"),
                    )
                });
        });
    });
    entered
}
