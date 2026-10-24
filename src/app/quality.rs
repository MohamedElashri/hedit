use super::*;
use hedit::{intelligence, projects, search};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Binding {
    pub command: Command,
    pub shortcut: String,
    #[serde(default)]
    pub editor_only: bool,
}
/// Key bindings and snippets, saved as keybindings.json and snippets.json.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Config {
    pub keybindings: Vec<Binding>,
    pub snippets: Vec<intelligence::Snippet>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            keybindings: vec![],
            snippets: intelligence::default_snippets(),
        }
    }
}
#[derive(Default)]
pub(super) struct Quality {
    pub options: search::Options,
    pub selection_only: bool,
    pub scope: Option<(u64, usize, usize)>,
    pub match_status: String,
    pub last_find: Option<LastFind>,
    pub folders_open: bool,
    pub filter: String,
    pub results: search::Results,
    /// Open files changed by the last replacement, and why others were not.
    pub open_applied: (usize, Vec<String>),
    /// The match F4 went to last, and whether these results are newer than
    /// the build's, so F4 goes through them.
    pub result_current: Option<usize>,
    /// The match the results list scrolled to last.
    pub result_shown: Option<usize>,
    pub results_newer: bool,
    /// How to undo the last replacement in folders, file by file.
    pub replace_undo: Vec<search::Restore>,
    pub preview_query: Option<(String, String, String, search::Options)>,
    pub preview_diff: Option<usize>,
    pub preview_rendered: Option<(usize, String)>,
    pub recent_open: bool,
    pub settings_project: bool,
    pub project_path: Option<PathBuf>,
    pub project_settings: projects::Defaults,
    pub recent: Vec<PathBuf>,
    pub symbols_open: bool,
    pub symbols: Vec<intelligence::Symbol>,
    pub symbol_query: String,
    pub symbols_project: bool,
    pub diff_open: bool,
    pub diff: String,
    pub diff_target: Option<(u64, u64, String)>,
    pub git_diff: bool,
    pub git_at: Option<Instant>,
    pub git_cache: std::collections::BTreeMap<u64, String>,
    pub git_pending: bool,
    /// The buffer and revision the current markers were computed for.
    /// The view, revision, saved state, and comparison the markers are for.
    pub git_key: Option<(u64, u64, bool, String)>,
    /// The text each view's change markers compare against.
    pub git_base: std::collections::BTreeMap<u64, String>,
    pub git_lines: std::collections::BTreeMap<u64, std::collections::BTreeSet<usize>>,
    pub file_action: Option<(PathBuf, FileOperation)>,
    pub file_name: String,
    pub config: Config,
    pub paste_requested: bool,
    pub settings_open: bool,
    pub settings_json: String,
    pub encoding_open: bool,
    pub encoding: String,
    pub snippet_focus: bool,
    pub symbol_focus: bool,
    pub folder_focus: bool,
    pub snippets_open: bool,
    pub snippet_query: String,
    pub preview: Option<u64>,
    pub preview_request: bool,
    pub tab_drag: Option<u64>,
    pub navigation: Vec<(u64, u64)>,
    pub navigation_index: usize,
    pub replaying: bool,
    pub incoming: Option<hedit::ipc::Incoming>,
    pub incoming_error: Option<String>,
}
pub(super) struct LastFind {
    id: u64,
    revision: u64,
    query: String,
    options: search::Options,
    start: usize,
    end: usize,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FileOperation {
    Rename,
    Move,
    Duplicate,
    Trash,
}
pub(super) enum Event {
    /// Change markers: view, revision, diff, and the text compared against.
    GitMarkers(u64, u64, String, Option<String>),
    Found(u64, Page, usize, usize, usize, String, search::Options),
    Results(search::Results),
    Applied(Vec<(u64, Page)>, usize, Vec<String>, Vec<search::Restore>),
    Restored(usize, Vec<String>),
    Symbols(Vec<intelligence::Symbol>),
    /// Definitions (false) or references (true) of a name.
    Locations(bool, String, Vec<definitions::Location>),
    Diff(u64, u64, String, String, bool),
    FileChanged(PathBuf, Option<PathBuf>),
    ProjectLoaded(
        PathBuf,
        projects::Project,
        projects::State,
        Vec<(Document, Page, hedit::session::TabView)>,
    ),
    ProjectSaved(PathBuf),
}
impl HeditApp {
    pub(super) fn init_quality(&mut self, storage: Option<&dyn eframe::Storage>) {
        self.quality.recent = storage
            .and_then(|s| eframe::get_value(s, "recent_projects"))
            .unwrap_or_default();
        self.quality.project_path = storage.and_then(|s| eframe::get_value(s, "project_path"));
        self.quality.project_settings = storage
            .and_then(|s| eframe::get_value(s, "project_settings"))
            .unwrap_or_default();
        if let Some(path) = &self.quality.project_path {
            if let Ok((project, _)) = projects::load(path) {
                self.quality.project_settings = project.settings;
            }
        }
    }
    pub(super) fn quality_event(&mut self, ctx: &egui::Context, event: Event) {
        match event {
            Event::GitMarkers(id, revision, diff, base) => {
                self.quality.git_pending = false;
                if self
                    .workspace
                    .open
                    .iter()
                    .any(|b| b.view.id == id && b.page.info.revision == revision)
                {
                    self.quality
                        .git_lines
                        .insert(id, intelligence::changed_lines(&diff));
                    self.quality.git_cache.insert(id, diff);
                    match base {
                        Some(base) => self.quality.git_base.insert(id, base),
                        None => self.quality.git_base.remove(&id),
                    };
                }
            }
            Event::Found(id, page, a, z, count, query, options) => {
                self.quality.last_find = Some(LastFind {
                    id,
                    revision: page.info.revision,
                    query,
                    options,
                    start: page.start as usize + hedit::advanced::byte(&page.text, a),
                    end: page.start as usize + hedit::advanced::byte(&page.text, z),
                });
                if let Some(index) = self.workspace.open.iter().position(|b| b.view.id == id) {
                    self.navigation_mark();
                    self.workspace.active = index;
                    let b = &mut self.workspace.open[index];
                    b.set_page(page);
                    b.view.anchor = a;
                    b.view.head = z;
                    b.reveal_cursor = true;
                    self.quality.match_status = format!("{count} matches");
                    self.navigation_mark();
                    self.changed();
                    self.focus_editor = !self.find_input_focused(ctx);
                }
            }
            Event::Results(results) => {
                self.quality.results = results;
                self.quality.result_current = None;
                self.quality.results_newer = true;
                self.quality.preview_diff = None;
                self.quality.preview_rendered = None;
            }
            Event::Applied(pages, count, mut errors, restore) => {
                for (id, page) in pages {
                    if let Some(b) = self.workspace.open.iter_mut().find(|b| b.view.id == id) {
                        b.set_page(page);
                    }
                }
                // Replacements in open files were made already; add them up.
                let (edited, open_errors) = std::mem::take(&mut self.quality.open_applied);
                errors.extend(open_errors);
                self.quality.replace_undo = restore;
                self.quality.results = Default::default();
                self.quality.preview_query = None;
                self.notice = match edited {
                    0 => format!("Updated {count} files."),
                    _ => format!(
                        "Updated {count} files on disk and {edited} open files (unsaved; undo in each file)."
                    ),
                };
                self.changed();
                if !errors.is_empty() {
                    self.error = Some(errors.join("\n"));
                }
            }
            Event::Restored(count, errors) => {
                self.notice = format!("Restored {count} files.");
                if !errors.is_empty() {
                    self.error = Some(errors.join("\n"));
                }
            }
            Event::Symbols(symbols) => self.quality.symbols = symbols,
            Event::Locations(references, name, found) => {
                self.show_locations(ctx, references, name, found)
            }
            Event::Diff(id, revision, disk, diff, git) => {
                self.quality.diff = diff;
                self.quality.diff_target = Some((id, revision, disk));
                self.quality.diff_open = true;
                self.quality.git_diff = git;
                if git {
                    self.quality
                        .git_lines
                        .insert(id, intelligence::changed_lines(&self.quality.diff));
                }
            }
            Event::FileChanged(from, to) => {
                if let Some(to) = to {
                    for b in &mut self.workspace.open {
                        if let Some(path) = &b.page.info.path {
                            if let Ok(relative) = Path::new(path).strip_prefix(&from) {
                                let target = if relative.as_os_str().is_empty() {
                                    to.clone()
                                } else {
                                    to.join(relative)
                                };
                                let shared = self.workspace.editor.get(b.view.id).unwrap();
                                match shared.lock().unwrap().retarget(target) {
                                    Ok(info) => {
                                        b.page.info = info;
                                        b.refresh_name();
                                    }
                                    Err(e) => self.error = Some(e),
                                };
                            }
                        }
                    }
                    for view in &mut self.workspace.closed {
                        if let Ok(shared) = self.workspace.editor.get(view.id) {
                            let mut doc = shared.lock().unwrap();
                            if let Some(path) = doc.info().path {
                                if let Ok(relative) = Path::new(&path).strip_prefix(&from) {
                                    match doc.retarget(if relative.as_os_str().is_empty() {
                                        to.clone()
                                    } else {
                                        to.join(relative)
                                    }) {
                                        Ok(info) => view.name = info.name,
                                        Err(e) => self.error = Some(e),
                                    }
                                }
                            }
                        }
                    }
                    for folder in &mut self.workspace.folders {
                        if let Ok(relative) = Path::new(&folder.path).strip_prefix(&from) {
                            folder.path = if relative.as_os_str().is_empty() {
                                to.clone()
                            } else {
                                to.join(relative)
                            }
                            .to_string_lossy()
                            .into_owned();
                            folder.expanded.clear();
                        }
                    }
                } else {
                    self.workspace
                        .folders
                        .retain(|f| !Path::new(&f.path).starts_with(&from));
                    for b in &mut self.workspace.open {
                        if b.page
                            .info
                            .path
                            .as_ref()
                            .is_some_and(|p| Path::new(p).starts_with(&from))
                        {
                            b.external = true;
                        }
                    }
                }
                self.project = project::ProjectBrowser::new(&self.workspace.folders);
                self.refresh_folder();
                self.changed();
            }
            Event::ProjectSaved(path) => {
                self.quality.project_path = Some(path.clone());
                self.recent_project(path);
                self.notice = "Project and workspace saved.".into();
            }
            Event::ProjectLoaded(path, project, state, tabs) => {
                let preserve = self
                    .workspace
                    .open
                    .iter()
                    .filter(|b| {
                        b.page.info.dirty || b.page.info.path.is_none() && !b.page.text.is_empty()
                    })
                    .count()
                    + self
                        .workspace
                        .closed
                        .iter()
                        .filter(|v| {
                            self.workspace
                                .editor
                                .get(v.id)
                                .ok()
                                .and_then(|d| {
                                    d.lock()
                                        .ok()
                                        .map(|d| d.info().dirty || d.info().path.is_none())
                                })
                                .unwrap_or(true)
                        })
                        .count();
                if preserve > 50 {
                    self.error=Some("The closed-file shelf cannot preserve all current unsaved tabs. Save some files before switching projects.".into());
                    return;
                }
                if let Err(e) = self.workspace.checkpoint() {
                    self.error = Some(e);
                    return;
                }
                let count = self.workspace.open.len();
                for _ in 0..count {
                    if self.workspace.open[0].page.info.dirty
                        || self.workspace.open[0].page.info.path.is_none()
                            && !self.workspace.open[0].page.text.is_empty()
                    {
                        if let Err(e) = self.workspace.close(0) {
                            self.error = Some(e);
                            return;
                        }
                    } else {
                        let b = self.workspace.open.remove(0);
                        let _ = self.workspace.editor.remove(b.view.id);
                    }
                }
                if self.workspace.open.is_empty() {
                    if let Err(e) = self.workspace.new_buffer() {
                        self.error = Some(e);
                        return;
                    }
                }
                if !tabs.is_empty() {
                    if let Some(b) = self.workspace.open.pop() {
                        let _ = self.workspace.editor.remove(b.view.id);
                    }
                }
                let mut ids = std::collections::HashMap::new();
                for (doc, page, mut view) in tabs {
                    ids.insert(view.id, (doc.id, doc.info().revision));
                    view.id = doc.id;
                    let b = Buffer::new(page, Some(view));
                    if let Err(e) = self.workspace.editor.insert(doc) {
                        self.error = Some(e);
                        return;
                    }
                    self.workspace.open.push(b);
                }
                self.workspace.active = state
                    .active
                    .min(self.workspace.open.len().saturating_sub(1));
                self.workspace.folders = project
                    .folders
                    .iter()
                    .map(|p| hedit::session::FolderView {
                        path: p.to_string_lossy().into_owned(),
                        visible: true,
                        expanded: vec![p.to_string_lossy().into_owned()],
                        collapsed: false,
                    })
                    .collect();
                self.project = project::ProjectBrowser::new(&self.workspace.folders);
                self.settings.show_folders = true;
                self.settings.show_sidebar = true;
                self.quality.project_settings = project.settings;
                self.quality.project_path = Some(path.clone());
                self.recent_project(path);
                self.panes = Default::default();
                self.split(state.pane_mode.min(panes::LAYOUTS - 1));
                if let Some(index) = state.secondary.filter(|i| *i < self.workspace.open.len()) {
                    self.panes.set_secondary(self.workspace.open[index].view.id);
                }
                if let Some(views) = state.pane_views {
                    if let Ok(mut panes) = serde_json::from_value::<panes::Panes>(views) {
                        panes.remap(&ids);
                        self.panes = panes;
                    }
                }
                self.settings.word_wrap = state.wrap;
                if state.font_size.is_finite() && state.font_size >= 10.0 {
                    self.settings.font_size = state.font_size.min(28.0);
                }
                self.quality.preview = None;
                self.quality.navigation.clear();
                self.changed();
                self.focus_editor = true;
            }
        }
        ctx.request_repaint();
    }
    pub(super) fn recent_project(&mut self, path: PathBuf) {
        self.quality.recent.retain(|p| p != &path);
        self.quality.recent.insert(0, path);
        self.quality.recent.truncate(20);
    }
    pub(super) fn formatting(&self) -> Result<projects::Formatting, String> {
        self.formatting_for(None)
    }
    pub(super) fn formatting_for(
        &self,
        target: Option<&Path>,
    ) -> Result<projects::Formatting, String> {
        self.formatting_with(target, &self.settings.defaults())
    }
    /// Formatting from `defaults`, then the project, then .editorconfig.
    fn formatting_with(
        &self,
        target: Option<&Path>,
        defaults: &projects::Defaults,
    ) -> Result<projects::Formatting, String> {
        let b = self.active();
        let path = target
            .map(PathBuf::from)
            .or_else(|| b.page.info.path.as_deref().map(PathBuf::from))
            .unwrap_or_else(|| {
                self.workspace
                    .folders
                    .first()
                    .map(|f| PathBuf::from(&f.path).join("untitled"))
                    .unwrap_or_else(|| PathBuf::from("untitled"))
            });
        let language = target
            .filter(|_| b.language_override().is_none())
            .map_or_else(
                || b.language(),
                |p| syntax::Language::detect(&p.file_name().unwrap_or_default().to_string_lossy()),
            );
        projects::formatting(
            &path,
            language.id(),
            defaults,
            Some(&self.quality.project_settings),
        )
    }
    /// Set the active buffer's indentation from settings, the indentation the
    /// file already uses (with `detect_indentation`), and then project and
    /// .editorconfig rules, each overriding the previous.
    pub(super) fn apply_format_preferences(&mut self) {
        let explicit = self.formatting_with(None, &projects::Defaults::default());
        match self.formatting() {
            Ok(mut format) => {
                let detected = self
                    .settings
                    .detect_indentation
                    .then(|| hedit::advanced::found_indentation(&self.active().page.text))
                    .flatten();
                if let (Some((tabs, width)), Ok(explicit)) = (detected, &explicit) {
                    if explicit.indent_style.is_none() {
                        format.indent_style = Some(if tabs { "tabs" } else { "spaces" }.into());
                    }
                    if explicit.indent_size.is_none() && width.is_some() {
                        format.indent_size = width;
                    }
                }
                let b = &mut self.workspace.open[self.workspace.active];
                if let Some(style) = format.indent_style {
                    b.view.indent = Some(
                        if matches!(style.as_str(), "tab" | "tabs") {
                            "tabs"
                        } else {
                            "spaces"
                        }
                        .into(),
                    );
                }
                if let Some(size) = format.indent_size {
                    b.view.tab_size = Some(size);
                }
            }
            Err(e) => self.error = Some(e),
        }
    }
    pub(super) fn format_before_save_for(&mut self, target: Option<&Path>) -> bool {
        if self.active().follow {
            return true;
        }
        let format = match self.formatting_for(target) {
            Ok(f) => f,
            Err(e) => {
                self.error = Some(e);
                return false;
            }
        };
        if self.active().page.info.paged {
            if format.insert_final_newline == Some(true)
                || format.trim_trailing_whitespace == Some(true)
                || format
                    .end_of_line
                    .as_ref()
                    .is_some_and(|e| !e.eq_ignore_ascii_case(self.active().page.info.eol))
            {
                self.error=Some("Save-time formatting requires a fully loaded file. Disable these rules to save this paged file unchanged.".into());
                return false;
            }
            return true;
        }
        let mut text = projects::clean(
            &self.active().page.text,
            &format,
            self.active().page.info.eol,
        );
        let crlf = format.end_of_line.as_ref().map(|s| s == "crlf");
        if let Some(crlf) = crlf {
            text = hedit::editing::apply(
                &text,
                0,
                0,
                if crlf {
                    hedit::editing::Operation::ToCrlf
                } else {
                    hedit::editing::Operation::ToLf
                },
                4,
                false,
                None,
            )
            .text;
        }
        if text != self.active().page.text
            || crlf.is_some_and(|c| c != (self.active().page.info.eol == "CRLF"))
        {
            let n = text.chars().count();
            if !self.apply_edit(
                hedit::editing::Edit {
                    text,
                    anchor: self.active().view.anchor.min(n),
                    head: self.active().view.head.min(n),
                },
                crlf,
            ) {
                return false;
            }
        }
        if let Some(charset) = format.charset {
            let needs_bom = charset == "utf-8-bom";
            let charset = if charset == "utf-8-bom" {
                "UTF-8"
            } else {
                &charset
            };
            let shared = self.workspace.editor.get(self.active().view.id).unwrap();
            let result = shared.lock().unwrap().set_encoding(charset);
            if let Err(e) = result {
                self.error = Some(e);
                return false;
            }
            if needs_bom {
                shared.lock().unwrap().set_bom(true);
            }
            self.workspace.open[self.workspace.active].page.info = shared.lock().unwrap().info();
        }
        true
    }
    pub(super) fn find_advanced(&mut self, ctx: &egui::Context, previous: bool) {
        let id = self.active().view.id;
        let query = self.query.clone();
        let options = self.quality.options.clone();
        let anchor = self.active().cursor_offset();
        let skip_zero = self.quality.last_find.as_ref().is_some_and(|last| {
            last.id == id
                && last.revision == self.active().page.info.revision
                && last.query == query
                && last.options == options
                && last.start == anchor as usize
                && last.end == last.start
        });
        let old_anchor = self.active().page.start
            + hedit::advanced::byte(&self.active().page.text, self.active().view.anchor) as u64;
        let scope = if self.quality.selection_only {
            self.quality
                .scope
                .filter(|(i, _, _)| *i == id)
                .map(|(_, a, z)| a..z)
        } else {
            None
        };
        let editor = Arc::clone(&self.workspace.editor);
        self.job(ctx, move || {
            let shared = editor.get(id)?;
            let mut doc = shared.lock().map_err(|e| e.to_string())?;
            let text = doc.text(32 * 1024 * 1024)?;
            let scope = scope.unwrap_or(0..text.len());
            let found = search::matches(&text, &query, &options, scope)?;
            let count = found.len();
            let selected = if previous {
                found
                    .iter()
                    .rev()
                    .find(|m| (m.start as u64) < old_anchor)
                    .or_else(|| found.last())
            } else {
                found
                    .iter()
                    .find(|m| {
                        (m.start as u64) >= anchor
                            && !(skip_zero && m.is_empty() && m.start as u64 == anchor)
                    })
                    .or_else(|| found.first())
            }
            .ok_or("No matches found.")?;
            let page = doc.page((selected.start as u64).saturating_sub(4096))?;
            if selected.end as u64 > page.end {
                return Err(
                    "The match extends beyond the editable page; narrow the pattern.".into(),
                );
            }
            let a = page.text[..selected.start - page.start as usize]
                .chars()
                .count();
            let z = page.text[..selected.end - page.start as usize]
                .chars()
                .count();
            Ok(Action::Quality(Box::new(Event::Found(
                id, page, a, z, count, query, options,
            ))))
        });
    }
    pub(super) fn replace_advanced(&mut self, all: bool) {
        if self.active().follow || self.busy || self.closing {
            return;
        }
        if self.active().page.info.paged {
            self.error=Some("Advanced replacement requires a fully loaded file; literal find still scans large files.".into());
            return;
        }
        let text = &self.active().page.text;
        let id = self.active().view.id;
        let scope = if all {
            if self.quality.selection_only {
                self.quality
                    .scope
                    .filter(|(i, _, _)| *i == id)
                    .map(|(_, a, z)| a..z)
                    .unwrap_or(0..text.len())
            } else {
                0..text.len()
            }
        } else {
            hedit::advanced::byte(text, self.active().view.anchor.min(self.active().view.head))
                ..hedit::advanced::byte(
                    text,
                    self.active().view.anchor.max(self.active().view.head),
                )
        };
        let result = (|| {
            let found = search::matches(text, &self.query, &self.quality.options, scope.clone())?;
            if !all && (found.len() != 1 || found[0] != scope) {
                return Err("Find a matching selection before replacing.".into());
            }
            search::replace(
                text,
                &self.query,
                &self.replacement,
                &self.quality.options,
                scope.clone(),
            )
        })();
        match result {
            Ok((text, count)) => {
                let n = text.chars().count();
                let cursor = self.active().view.anchor.min(n);
                self.apply_edit(
                    hedit::editing::Edit {
                        text,
                        anchor: cursor,
                        head: cursor,
                    },
                    None,
                );
                self.quality.match_status = format!("Replaced {count} matches");
                self.quality.scope = None;
                self.quality.selection_only = false;
            }
            Err(e) => self.error = Some(e),
        }
    }
    pub(super) fn folder_search(&mut self, ctx: &egui::Context, preview: bool) {
        if self.busy {
            return;
        }
        self.quality.results = Default::default();
        self.quality.preview_diff = None;
        self.quality.preview_rendered = None;
        let roots = self
            .workspace
            .folders
            .iter()
            .map(|f| PathBuf::from(&f.path))
            .collect::<Vec<_>>();
        let (query, replacement, filter, options) = (
            self.query.clone(),
            self.replacement.clone(),
            self.quality.filter.clone(),
            self.quality.options.clone(),
        );
        self.quality.preview_query = preview.then(|| {
            (
                query.clone(),
                replacement.clone(),
                filter.clone(),
                options.clone(),
            )
        });
        // Search what open files hold now, saved or not.
        let open: std::collections::HashMap<PathBuf, String> = self
            .workspace
            .open
            .iter()
            .filter(|b| !b.page.info.paged && !b.follow)
            .filter_map(|b| {
                let path = std::fs::canonicalize(b.page.info.path.as_ref()?).ok()?;
                Some((path, b.page.text.clone()))
            })
            .collect();
        let cancel = Arc::clone(&self.cancel);
        self.job(ctx, move || {
            search::folders(
                &roots,
                &filter,
                &query,
                preview.then_some(replacement.as_str()),
                &options,
                &open,
                &cancel,
            )
            .map(|r| Action::Quality(Box::new(Event::Results(r))))
        });
    }
    pub(super) fn apply_folder_replacements(&mut self, ctx: &egui::Context) {
        let plans = std::mem::take(&mut self.quality.results.plans);
        // Files open in the editor change there, as one undo step each, and
        // stay unsaved; only files that are not open are written.
        let (open_plans, plans): (Vec<_>, Vec<_>) = plans
            .into_iter()
            .filter(|p| p.selected)
            .partition(|p| p.open);
        let mut edited = 0;
        let mut open_errors = vec![];
        for plan in open_plans {
            let index = self.workspace.open.iter().position(|b| {
                b.page
                    .info
                    .path
                    .as_ref()
                    .and_then(|p| std::fs::canonicalize(p).ok())
                    == Some(plan.path.clone())
            });
            let Some(index) = index else {
                open_errors.push(format!(
                    "{}: the file was closed; search again.",
                    plan.path.display()
                ));
                continue;
            };
            let b = &self.workspace.open[index];
            if b.follow || b.page.info.paged || b.page.text != plan.before {
                open_errors.push(format!(
                    "{}: the open text changed since the preview; search again.",
                    plan.path.display()
                ));
                continue;
            }
            let old = std::mem::replace(&mut self.workspace.open[index].page.text, plan.after);
            match self.workspace.edit(index, &old) {
                Ok(()) => {
                    let b = &mut self.workspace.open[index];
                    let length = b.page.text.chars().count();
                    b.view.anchor = b.view.anchor.min(length);
                    b.view.head = b.view.head.min(length);
                    b.view.selections.clear();
                    b.view.folds.clear();
                    b.restore_cursor = true;
                    edited += 1;
                }
                Err(e) => {
                    self.workspace.open[index].page.text = old;
                    open_errors.push(e);
                }
            }
        }
        self.quality.open_applied = (edited, open_errors);
        self.changed();
        let editor = Arc::clone(&self.workspace.editor);
        let open = self
            .workspace
            .open
            .iter()
            .filter_map(|b| {
                b.page
                    .info
                    .path
                    .as_ref()
                    .map(|p| (PathBuf::from(p), b.view.id, b.page.info.dirty, b.follow))
            })
            .collect::<Vec<_>>();
        self.job(ctx, move || {
            let mut count = 0;
            let mut pages = Vec::new();
            let mut errors = Vec::new();
            let mut restore = vec![];
            for plan in plans {
                let result = (|| {
                    if let Some((_, id, dirty, follow)) =
                        open.iter().find(|(p, _, _, _)| p == &plan.path)
                    {
                        if *dirty || *follow {
                            return Err(format!(
                                "{}: open buffer has unsaved changes or follows a log; skipped.",
                                plan.path.display()
                            ));
                        }
                        let shared = editor.get(*id)?;
                        let mut doc = shared.lock().map_err(|e| e.to_string())?;
                        if doc.text(search::FILE_LIMIT)? != plan.before {
                            return Err(format!(
                                "{}: open text differs from the preview.",
                                plan.path.display()
                            ));
                        }
                        let current = std::fs::read(&plan.path).map_err(|e| e.to_string())?;
                        use sha2::{Digest, Sha256};
                        if <[u8; 32]>::from(Sha256::digest(current)) != plan.hash {
                            return Err("A file changed since preview; search again.".into());
                        }
                        let mut candidate = doc.clone();
                        let size = candidate.info().size;
                        let revision = candidate.info().revision;
                        candidate.replace(0, size, plan.after, revision)?;
                        candidate.save(None)?;
                        let page = candidate.page(0)?;
                        *doc = candidate;
                        pages.push((*id, page));
                    } else {
                        restore.push(search::apply_recorded(&plan)?);
                    }
                    Ok(())
                })();
                match result {
                    Ok(()) => count += 1,
                    Err(e) => errors.push(e),
                }
            }
            Ok(Action::Quality(Box::new(Event::Applied(
                pages, count, errors, restore,
            ))))
        });
    }
    /// Put back the files the last replacement wrote, where unchanged since.
    pub(super) fn undo_folder_replacements(&mut self, ctx: &egui::Context) {
        let records = std::mem::take(&mut self.quality.replace_undo);
        // A file opened and edited since keeps its edits; it is not reverted
        // underneath them.
        let edited: Vec<PathBuf> = self
            .workspace
            .open
            .iter()
            .filter(|b| b.page.info.dirty)
            .filter_map(|b| std::fs::canonicalize(b.page.info.path.as_ref()?).ok())
            .collect();
        self.job(ctx, move || {
            let mut count = 0;
            let mut errors = vec![];
            for record in &records {
                if edited.contains(&record.path) {
                    errors.push(format!(
                        "{} has unsaved changes in hedit; it was left as it is.",
                        record.path.display()
                    ));
                    continue;
                }
                match search::restore(record) {
                    Ok(()) => count += 1,
                    Err(e) => errors.push(e),
                }
            }
            Ok(Action::Quality(Box::new(Event::Restored(count, errors))))
        });
    }
    pub(super) fn compare_disk(&mut self, ctx: &egui::Context, git: bool) {
        let b = self.active();
        let Some(path) = b.page.info.path.clone() else {
            self.error = Some("Save the file first.".into());
            return;
        };
        let id = b.view.id;
        let revision = b.page.info.revision;
        let editor = Arc::clone(&self.workspace.editor);
        self.job(ctx, move || {
            let text = editor
                .get(id)?
                .lock()
                .map_err(|e| e.to_string())?
                .text(16 * 1024 * 1024)?;
            let path = PathBuf::from(path);
            if git {
                let diff = intelligence::git_diff(&path, &text)?;
                return Ok(Action::Quality(Box::new(Event::Diff(
                    id,
                    revision,
                    String::new(),
                    diff,
                    true,
                ))));
            }
            if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 32 * 1024 * 1024 {
                return Err("Disk comparison supports files up to 32 MiB.".into());
            }
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            let (disk, _) =
                hedit::text_encoding::decode(&bytes, hedit::text_encoding::detect(&bytes))?;
            let diff = search::diff(&disk, &text, &path);
            Ok(Action::Quality(Box::new(Event::Diff(
                id, revision, disk, diff, false,
            ))))
        });
    }
    pub(super) fn project_snapshot(&self) -> Result<projects::State, String> {
        if self.workspace.open.len() > 200 {
            return Err("Project workspace snapshots support up to 200 tabs.".into());
        }
        let mut tabs = Vec::new();
        let mut size = 0;
        for b in &self.workspace.open {
            let text = if b.page.info.dirty || b.page.info.path.is_none() {
                let doc = self.workspace.editor.get(b.view.id)?;
                let text = doc
                    .lock()
                    .map_err(|e| e.to_string())?
                    .text(16 * 1024 * 1024)?;
                size += text.len();
                Some(text)
            } else {
                None
            };
            if size > 32 * 1024 * 1024 {
                return Err("Project workspace text exceeds 32 MiB; save some files first.".into());
            }
            tabs.push(projects::Tab {
                path: b.page.info.path.as_ref().map(PathBuf::from),
                text,
                view: b.view.clone(),
                encoding: b.page.info.encoding.clone(),
            });
        }
        Ok(projects::State {
            tabs,
            active: self.workspace.active,
            pane_mode: self.panes.mode,
            secondary: self
                .panes
                .secondary_id()
                .and_then(|id| self.workspace.open.iter().position(|b| b.view.id == id)),
            wrap: self.settings.word_wrap,
            font_size: self.settings.font_size,
            pane_views: serde_json::to_value(&self.panes).ok(),
        })
    }
    pub(super) fn save_project(&mut self, ctx: &egui::Context, choose: bool) {
        let state = match self.project_snapshot() {
            Ok(s) => s,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let mut project = projects::Project {
            folders: self
                .workspace
                .folders
                .iter()
                .map(|f| PathBuf::from(&f.path))
                .collect(),
            settings: self.quality.project_settings.clone(),
        };
        let path = if choose {
            None
        } else {
            self.quality.project_path.clone()
        };
        self.job(ctx, move || {
            let path = match path.or_else(|| {
                rfd::FileDialog::new()
                    .set_title("Save project")
                    .set_file_name("workspace.hedit-project")
                    .save_file()
            }) {
                Some(p) => p,
                None => return Ok(Action::Nothing),
            };
            let path = if path.is_absolute() {
                path
            } else {
                std::env::current_dir()
                    .map_err(|e| e.to_string())?
                    .join(path)
            };
            if let Some(parent) = path.parent() {
                for folder in &mut project.folders {
                    if let Ok(relative) = folder.strip_prefix(parent) {
                        *folder = relative.to_path_buf();
                    }
                }
            }
            projects::atomic_json(&path.with_extension("hedit-workspace"), &state)?;
            projects::atomic_json(&path, &project)?;
            Ok(Action::Quality(Box::new(Event::ProjectSaved(path))))
        });
    }
    pub(super) fn load_project(&mut self, ctx: &egui::Context, path: Option<PathBuf>) {
        if let Some(current) = self.quality.project_path.clone() {
            match self
                .project_snapshot()
                .and_then(|s| projects::atomic_json(&current.with_extension("hedit-workspace"), &s))
            {
                Ok(()) => (),
                Err(e) => {
                    self.error = Some(e);
                    return;
                }
            }
        }
        let editor = Arc::clone(&self.workspace.editor);
        self.job(ctx, move || {
            let path = match path.or_else(|| {
                rfd::FileDialog::new()
                    .add_filter("hedit project", &["hedit-project"])
                    .pick_file()
            }) {
                Some(p) => p,
                None => return Ok(Action::Nothing),
            };
            let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
            let (project, state) = projects::load(&path)?;
            let mut tabs = Vec::new();
            for tab in &state.tabs {
                let mut doc = if let Some(path) = &tab.path {
                    Document::open_or_new(editor.id(), path)?
                } else {
                    Document::empty(editor.id())
                };
                if let Some(text) = &tab.text {
                    doc.replace(0, doc.info().size, text.clone(), doc.info().revision)?;
                }
                if doc.info().encoding != tab.encoding {
                    doc.set_encoding(&tab.encoding)?;
                }
                let page = doc.page(tab.view.page_start)?;
                tabs.push((doc, page, tab.view.clone()));
            }
            Ok(Action::Quality(Box::new(Event::ProjectLoaded(
                path, project, state, tabs,
            ))))
        });
    }
    pub(super) fn list_symbols(&mut self, ctx: &egui::Context) {
        self.quality.symbols_open = true;
        self.quality.symbol_focus = true;
        self.focus_editor = false;
        let project = self.quality.symbols_project;
        let roots = self
            .workspace
            .folders
            .iter()
            .map(|f| PathBuf::from(&f.path))
            .collect::<Vec<_>>();
        let current = self.active().page.text.clone();
        let path = self
            .active()
            .page
            .info
            .path
            .clone()
            .map(PathBuf::from)
            .unwrap_or_default();
        let cancel = Arc::clone(&self.cancel);
        let overrides = self
            .workspace
            .open
            .iter()
            .filter(|b| !b.page.info.paged)
            .filter_map(|b| {
                b.page
                    .info
                    .path
                    .as_ref()
                    .map(|p| (PathBuf::from(p), b.page.text.clone()))
            })
            .collect::<std::collections::HashMap<_, _>>();
        self.job(ctx, move || {
            let mut symbols = intelligence::symbols(&current, &path);
            if project {
                for file in search::walk(&roots, "", &cancel)? {
                    if file == path {
                        continue;
                    }
                    let text = if let Some(text) = overrides.get(&file) {
                        text.clone()
                    } else {
                        if std::fs::metadata(&file).map_or(true, |m| m.len() > search::FILE_LIMIT) {
                            continue;
                        }
                        let Ok(bytes) = std::fs::read(&file) else {
                            continue;
                        };
                        let Ok((text, _)) = hedit::text_encoding::decode(
                            &bytes,
                            hedit::text_encoding::detect(&bytes),
                        ) else {
                            continue;
                        };
                        text
                    };
                    symbols.extend(intelligence::symbols(&text, &file));
                    if symbols.len() >= 20_000 {
                        break;
                    }
                }
            }
            Ok(Action::Quality(Box::new(Event::Symbols(symbols))))
        });
    }
    pub(super) fn navigation_mark(&mut self) {
        let at = (self.active().view.id, self.active().cursor_offset());
        if self.quality.replaying {
            self.quality.replaying = false;
            return;
        }
        if self.quality.navigation.get(self.quality.navigation_index) == Some(&at) {
            return;
        }
        self.quality
            .navigation
            .truncate(self.quality.navigation_index + 1);
        self.quality.navigation.push(at);
        if self.quality.navigation.len() > 200 {
            self.quality.navigation.remove(0);
        }
        self.quality.navigation_index = self.quality.navigation.len().saturating_sub(1);
    }
    pub(super) fn navigate(&mut self, ctx: &egui::Context, forward: bool) {
        if !forward
            && self.quality.navigation.last().copied()
                != Some((self.active().view.id, self.active().cursor_offset()))
        {
            self.navigation_mark();
        }
        let next = if forward {
            self.quality.navigation_index + 1
        } else {
            self.quality.navigation_index.saturating_sub(1)
        };
        let Some(&(id, offset)) = self.quality.navigation.get(next) else {
            return;
        };
        self.quality.navigation_index = next;
        if let Some(index) = self.workspace.closed.iter().position(|v| v.id == id) {
            if let Err(e) = self.workspace.reopen_at(index) {
                self.error = Some(e);
                return;
            }
        }
        if let Some(index) = self.workspace.open.iter().position(|b| b.view.id == id) {
            self.workspace.active = index;
            self.quality.replaying = true;
            self.go_to_offset(ctx, offset);
        }
    }
    /// Insert a snippet over the selection and the `before` bytes ahead of it,
    /// such as its typed trigger.
    pub(super) fn insert_snippet(&mut self, snippet: intelligence::Snippet, before: usize) {
        if self.active().follow || self.active().page.info.paged {
            return;
        }
        let b = self.active();
        let old = &b.page.text;
        let start = hedit::advanced::byte(old, b.view.anchor.min(b.view.head));
        let end = hedit::advanced::byte(old, b.view.anchor.max(b.view.head));
        let start = start.saturating_sub(before);
        if !old.is_char_boundary(start) {
            return;
        }
        let line = old[..start].rfind('\n').map_or(0, |n| n + 1);
        let indent: String = old[line..]
            .chars()
            .take_while(|&c| c == ' ' || c == '\t')
            .collect();
        let unit = if b.view.indent.as_deref() == Some("tabs") {
            "\t".to_owned()
        } else {
            " ".repeat(b.view.tab_size.unwrap_or(4) as usize)
        };
        let body = snippet_fields::indent_body(&snippet.body, &indent, &unit);
        let expansion = intelligence::expand_fields(&body, &old[start..end]);
        let base = old[..start].chars().count();
        let cursor = base + expansion.fields.last().map_or(0, |f| f[0].0);
        let mut text = old.clone();
        text.replace_range(start..end, &expansion.text);
        let inserted = self.apply_edit(
            hedit::editing::Edit {
                text,
                anchor: cursor,
                head: cursor,
            },
            None,
        );
        if inserted && expansion.fields.len() > 1 {
            self.start_snippet(base, expansion.fields);
        }
    }
    pub(super) fn expand_trigger(&mut self) -> bool {
        if self.active().view.anchor != self.active().view.head
            || !self.active().view.selections.is_empty()
        {
            return false;
        }
        let b = self.active();
        let before = &b.page.text[..hedit::advanced::byte(&b.page.text, b.view.head)];
        let snippet = self
            .quality
            .config
            .snippets
            .iter()
            .find(|s| {
                !s.trigger.is_empty()
                    && before.ends_with(&s.trigger)
                    && s.language.as_deref().is_none_or(|l| l == b.language().id())
                    && before[..before.len() - s.trigger.len()]
                        .chars()
                        .last()
                        .is_none_or(|c| !c.is_alphanumeric() && c != '_')
            })
            .cloned();
        if let Some(snippet) = snippet {
            let before = snippet.trigger.len();
            self.insert_snippet(snippet, before);
            true
        } else {
            false
        }
    }
    pub(super) fn begin_file_action(&mut self, path: PathBuf, operation: FileOperation) {
        self.quality.file_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        self.quality.file_action = Some((path, operation));
    }
    pub(super) fn perform_file_action(&mut self, ctx: &egui::Context) {
        let Some((path, operation)) = self.quality.file_action.take() else {
            return;
        };
        // Documents and roots use resolved paths. Normalize the source before
        // mutation, while its ancestors still exist (e.g. macOS /var aliases).
        let path = match hedit::folder::entry_path(&path) {
            Ok(path) => path,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        let name = self.quality.file_name.clone();
        if operation == FileOperation::Trash {
            for view in self
                .workspace
                .open
                .iter()
                .map(|b| &b.view)
                .chain(&self.workspace.closed)
            {
                if let Ok(shared) = self.workspace.editor.get(view.id) {
                    let mut doc = shared.lock().unwrap();
                    if doc
                        .info()
                        .path
                        .as_ref()
                        .is_some_and(|p| Path::new(p).starts_with(&path))
                    {
                        doc.preserve_as_unsaved();
                    }
                }
            }
            for b in &mut self.workspace.open {
                if let Ok(shared) = self.workspace.editor.get(b.view.id) {
                    b.page.info = shared.lock().unwrap().info();
                }
            }
            self.changed();
        }
        // Snapshot first: moving/trashing a source must never invalidate unsaved pieces.
        if let Err(e) = self.workspace.checkpoint() {
            self.error = Some(e);
            return;
        }
        self.job(ctx, move || {
            let to = match operation {
                FileOperation::Rename => Some(PathBuf::from(hedit::folder::rename_path(
                    path.to_string_lossy().into_owned(),
                    name,
                )?)),
                FileOperation::Move => {
                    let Some(parent) = rfd::FileDialog::new()
                        .set_title("Move to folder")
                        .pick_folder()
                    else {
                        return Ok(Action::Nothing);
                    };
                    Some(hedit::folder::move_path(
                        &path,
                        &parent.join(path.file_name().ok_or("Invalid name.")?),
                    )?)
                }
                FileOperation::Duplicate => {
                    let target = hedit::folder::duplicate_sibling(&path, &name)?;
                    return Ok(Action::Created(
                        target.to_string_lossy().into_owned(),
                        target.is_dir(),
                    ));
                }
                FileOperation::Trash => {
                    hedit::folder::trash_path(&path)?;
                    None
                }
            };
            Ok(Action::Quality(Box::new(Event::FileChanged(path, to))))
        });
    }
    pub(super) fn poll_incoming(&mut self, ctx: &egui::Context) {
        if self.closing {
            return;
        }
        if self.quality.incoming.is_some() {
            if !self.busy && self.pending_paths.is_empty() && !self.checkpoint_pending {
                if self.generation != self.persisted {
                    self.checkpoint(ctx);
                    return;
                }
                let incoming = self.quality.incoming.take().unwrap();
                let result = self.quality.incoming_error.take().map_or(Ok(()), Err);
                let _ = incoming.finish(result);
            }
            return;
        }
        if self.busy || !self.pending_paths.is_empty() {
            return;
        }
        let Some(incoming) = hedit::ipc::receive(self.workspace.store.directory()) else {
            return;
        };
        let incoming = match incoming {
            Ok(i) => i,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        if incoming.request.new_buffer {
            let result = self.workspace.new_buffer();
            self.report(result);
        }
        let spool = incoming.request.stdin.as_ref().map(|p| p.path());
        let files: Vec<PathBuf> = incoming.request.files.iter().map(|p| p.path()).collect();
        if let Some(wait) = &incoming.request.wait {
            self.wait_for(&files, wait.path());
        }
        self.pending_paths.extend(files);
        self.quality.incoming = Some(incoming);
        self.changed();
        if let Some(path) = spool {
            let editor = Arc::clone(&self.workspace.editor);
            self.job(ctx, move || {
                let mut doc = Document::from_stdin(editor.id(), &path)?;
                let page = doc.page(0)?;
                Ok(Action::Add(doc, page, 0))
            });
        }
    }
}

impl HeditApp {
    /// Whether git markers are out of date: the text changed, the window came
    /// back into focus (a commit may have happened), or they are 30 s old.
    pub(super) fn git_stale(&self, ctx: &egui::Context) -> bool {
        let b = self.active();
        b.page.info.path.is_some()
            && !b.page.info.paged
            && (self.quality.git_key.as_ref()
                != Some(&(
                    b.view.id,
                    b.page.info.revision,
                    b.page.info.dirty,
                    self.settings.mini_diff.clone(),
                ))
                || refocused(ctx)
                || self
                    .quality
                    .git_at
                    .is_none_or(|t| t.elapsed() >= Duration::from_secs(30)))
    }
    pub(super) fn poll_git(&mut self, ctx: &egui::Context) {
        if self.quality.git_pending
            || self.busy
            || self.closing
            || !self.git_stale(ctx)
            || (!refocused(ctx)
                && self
                    .quality
                    .git_at
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(2)))
        {
            return;
        }
        let b = self.active();
        let Some(path) = b.page.info.path.clone() else {
            return;
        };
        let id = b.view.id;
        let revision = b.page.info.revision;
        let dirty = b.page.info.dirty;
        let text = b.page.text.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let mode = self.settings.mini_diff.clone();
        self.quality.git_at = Some(Instant::now());
        self.quality.git_pending = true;
        self.quality.git_key = Some((id, revision, dirty, mode.clone()));
        thread::spawn(move || {
            let path = Path::new(&path);
            // "auto" compares with Git's HEAD, or the saved file outside Git.
            let saved = || {
                let bytes = std::fs::read(path).ok()?;
                hedit::text_encoding::decode(&bytes, hedit::text_encoding::detect(&bytes))
                    .ok()
                    .map(|(text, _)| text)
            };
            let base = match mode.as_str() {
                "off" => None,
                "saved" => saved(),
                _ => intelligence::git_base(path).ok().or_else(saved),
            };
            let diff = base
                .as_deref()
                .map(|base| hedit::search::diff(base, &text, path))
                .unwrap_or_default();
            let _ = tx.send(Ok(Action::Quality(Box::new(Event::GitMarkers(
                id, revision, diff, base,
            )))));
            ctx.request_repaint();
        });
    }
}

/// True on the frame the window regains keyboard focus.
pub(super) fn refocused(ctx: &egui::Context) -> bool {
    ctx.input(|i| {
        i.events
            .iter()
            .any(|e| matches!(e, egui::Event::WindowFocused(true)))
    })
}
