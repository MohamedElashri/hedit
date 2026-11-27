use super::*;
use hedit::{intelligence, projects, search};
use serde::{Deserialize, Deserializer, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub(super) enum BindingCommand {
    Builtin(Command),
    Extension(#[serde(deserialize_with = "deserialize_extension_id")] String),
}

fn deserialize_extension_id<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    let id = String::deserialize(deserializer)?;
    if extensions::valid_command_id(&id) {
        Ok(id)
    } else {
        Err(serde::de::Error::custom("Invalid extension command ID"))
    }
}

impl From<Command> for BindingCommand {
    fn from(command: Command) -> Self {
        Self::Builtin(command)
    }
}

impl BindingCommand {
    pub(super) fn builtin(&self) -> Option<Command> {
        match self {
            Self::Builtin(command) => Some(*command),
            Self::Extension(_) => None,
        }
    }
}

impl PartialEq<Command> for BindingCommand {
    fn eq(&self, other: &Command) -> bool {
        self.builtin() == Some(*other)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Binding {
    pub command: BindingCommand,
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
    /// Saved macros, in macros.json.
    pub macros: Vec<macros::Macro>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            keybindings: vec![],
            snippets: intelligence::default_snippets(),
            macros: vec![],
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
    pub folder_query: String,
    pub saved_searches: folder_search::Saved,
    pub folder_initialized: bool,
    pub folder_generation: u64,
    pub folder_rows: folder_search::Rows,
    pub folder_replacement: String,
    pub folder_options: search::Options,
    pub folder_replace: bool,
    pub folder_context: bool,
    pub folder_collapsed: std::collections::HashSet<PathBuf>,
    pub folder_state: folder_search::State,
    pub folder_task: Option<folder_search::Task>,
    pub folder_revisions: Vec<(u64, u64)>,
    pub folder_scope: Option<PathBuf>,
    pub folder_roots: Vec<PathBuf>,
    pub folder_snapshot: Option<(String, String, search::Options)>,
    pub folder_regex: Option<regex::Regex>,
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
    pub project_editor: project_settings::Editor,
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
    /// The view, revision, saved state, and comparison the markers are for.
    pub git_key: Option<(u64, u64, bool, String)>,
    /// The text each view's change markers compare against.
    pub git_base: std::collections::BTreeMap<u64, String>,
    pub git_lines: std::collections::BTreeMap<u64, std::collections::BTreeSet<usize>>,
    pub file_action: Option<FileAction>,
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
    pub preview_group: Option<u64>,
    pub preview_request: bool,
    pub tab_drag: Option<u64>,
    pub tab_drag_group: Option<usize>,
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
impl LastFind {
    pub(super) fn matches(&self, buffer: &Buffer, query: &str, options: &search::Options) -> bool {
        self.id == buffer.view.id
            && self.revision == buffer.page.info.revision
            && self.query == query
            && self.options == *options
            && self.start == self.end
            && self.start == buffer.cursor_offset() as usize
    }
    pub(super) fn at(
        buffer: &hedit::workspace::Buffer,
        query: &str,
        options: &search::Options,
    ) -> Self {
        Self {
            id: buffer.view.id,
            revision: buffer.page.info.revision,
            query: query.to_owned(),
            options: options.clone(),
            start: buffer.page.start as usize
                + hedit::advanced::byte(&buffer.page.text, buffer.view.anchor),
            end: buffer.page.start as usize
                + hedit::advanced::byte(&buffer.page.text, buffer.view.head),
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FileOperation {
    Rename,
    Move,
    Duplicate,
    Trash,
    Delete,
}
pub(super) struct FileAction {
    pub path: PathBuf,
    pub operation: FileOperation,
    pub skip_trash_confirmation: bool,
}
pub(super) enum Event {
    /// Change markers: view, revision, mode, diff, and the text compared against.
    GitMarkers(u64, u64, String, String, Option<String>),
    Found(u64, Page, usize, usize, usize, String, search::Options),
    Results(search::Results),
    Applied(Vec<(u64, Page)>, usize, Vec<String>, Vec<search::Restore>),
    Restored(usize, Vec<String>, Vec<search::Restore>),
    Symbols(Vec<intelligence::Symbol>),
    /// Definitions (false) or references (true) of a name.
    Locations(bool, String, Vec<definitions::Location>),
    Counterparts(u64, PathBuf, Vec<PathBuf>),
    FileReferences(u64, Option<PathBuf>, String, Vec<PathBuf>),
    TagSelections(
        u64,
        u64,
        Vec<hedit::advanced::Selection>,
        Vec<hedit::advanced::Selection>,
    ),
    Diff(u64, u64, String, String, bool),
    FileChanged(PathBuf, Option<PathBuf>),
    FileRemovalFailed(String, Vec<(u64, hedit::document::RemovalState)>),
    ProjectLoaded(
        PathBuf,
        projects::Project,
        Option<projects::State>,
        Vec<(Document, Page, hedit::session::TabView)>,
    ),
    ProjectSaved(PathBuf),
}
/// Save-time cleanup preserves lines and only changes their trailing whitespace
/// and separators. Keep the cursor on the same line and clamp it to the body.
fn cursor_after_save_cleanup(before: &str, after: &str, cursor: usize) -> usize {
    let mut remaining = cursor;
    let mut result = 0;
    let mut after = after.split_inclusive('\n');
    for line in before.split_inclusive('\n') {
        let next = after.next().unwrap_or("");
        let length = line.chars().count();
        let has_newline = line.ends_with('\n');
        if remaining < length || !has_newline {
            let body = next
                .strip_suffix("\r\n")
                .or_else(|| next.strip_suffix('\n'))
                .unwrap_or(next);
            return result + remaining.min(body.chars().count());
        }
        remaining -= length;
        result += next.chars().count();
    }
    result
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
            Event::Counterparts(view, source, found) => {
                self.show_counterparts(ctx, view, source, found);
            }
            Event::FileReferences(view, document, reference, found) => {
                self.show_file_references(ctx, view, document, reference, found);
            }
            Event::TagSelections(view, revision, before, after) => {
                if self.active().view.id == view
                    && self.active().page.info.revision == revision
                    && self.selections() == before
                {
                    if after == before {
                        self.notice = "No larger enclosing HTML/XML element found.".into();
                    } else {
                        self.set_selections(after);
                    }
                }
            }
            Event::GitMarkers(id, revision, mode, diff, base) => {
                self.quality.git_pending = false;
                if mode == self.settings.mini_diff
                    && self
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
                    self.reveal_markdown_source(ctx);
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
                self.quality.folder_generation = self.quality.folder_generation.wrapping_add(1);
                self.quality.folder_state = folder_search::State::Ready;
                self.quality.folder_regex = self
                    .quality
                    .folder_snapshot
                    .as_ref()
                    .and_then(|(q, _, options)| options.compile(q).ok());
                self.quality.results = results;
                self.quality.result_current = None;
                self.quality.result_shown = None;
                self.quality.results_newer = true;
                self.quality.preview_diff = None;
                self.quality.preview_rendered = None;
            }
            Event::Applied(pages, count, mut errors, restore) => {
                let paged_edited = pages.len();
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
                let edited = edited + paged_edited;
                self.notice = match edited {
                    0 => format!("Updated {count} files."),
                    _ => format!(
                        "Updated {count} files on disk and {edited} open files (unsaved; undo in each file)."
                    ),
                };
                self.quality.folder_state = folder_search::State::Applied(self.notice.clone());
                self.changed();
                if !errors.is_empty() {
                    self.error = Some(errors.join("\n"));
                }
            }
            Event::Restored(count, errors, pending) => {
                self.quality.replace_undo = pending;
                self.notice = format!("Restored {count} files.");
                self.quality.folder_state = folder_search::State::Applied(self.notice.clone());
                if !errors.is_empty() {
                    self.error = Some(errors.join("\n"));
                }
            }
            Event::FileRemovalFailed(error, states) => {
                self.rollback_file_removal(states);
                self.refresh_folder();
                self.error = Some(error);
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
                if git && self.settings.mini_diff != "off" {
                    self.quality
                        .git_lines
                        .insert(id, intelligence::changed_lines(&self.quality.diff));
                }
            }
            Event::FileChanged(from, to) => {
                if let Some(relative) = self
                    .quality
                    .folder_scope
                    .as_ref()
                    .and_then(|path| path.strip_prefix(&from).ok())
                {
                    let scope = to.as_ref().map(|path| path.join(relative));
                    self.set_folder_search_scope(scope);
                }
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
                let saved_layout = state.is_some();
                let state = state.unwrap_or_default();
                self.opens.cancel();
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
                if saved_layout {
                    self.settings.word_wrap = state.wrap;
                }
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
                let indentation = format.indent_style.map(|style| {
                    if matches!(style.as_str(), "tab" | "tabs") {
                        "tabs"
                    } else {
                        "spaces"
                    }
                });
                if indentation.is_some()
                    || b.format_indent
                        .is_some_and(|prior| b.view.indent.as_deref() == Some(prior))
                {
                    b.view.indent = indentation.map(str::to_owned);
                }
                if format.indent_size.is_some()
                    || b.format_tab_size
                        .is_some_and(|prior| b.view.tab_size == Some(prior))
                {
                    b.view.tab_size = Some(format.indent_size.unwrap_or(4));
                }
                b.format_indent = indentation;
                b.format_tab_size = format.indent_size;
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
        let charset = format
            .charset
            .as_deref()
            .map(|charset| {
                let bom = charset == "utf-8-bom";
                let charset = if bom { "UTF-8" } else { charset };
                hedit::text_encoding::canonical(charset).map(|encoding| (encoding, bom))
            })
            .transpose();
        let charset = match charset {
            Ok(charset) => charset,
            Err(error) => {
                self.error = Some(error);
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
            let info = &self.active().page.info;
            if charset.is_some_and(|(encoding, bom)| {
                info.encoding != encoding || (encoding == "UTF-8" && info.bom != bom)
            }) {
                self.error = Some("Save-time encoding conversion requires a fully loaded file. Disable the encoding rule to save this paged file unchanged.".into());
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
        if let Some((encoding, bom)) = charset {
            if let Err(error) = hedit::text_encoding::encode(&text, encoding, bom) {
                self.error = Some(error);
                return false;
            }
        }
        if text != self.active().page.text
            || crlf.is_some_and(|c| c != (self.active().page.info.eol == "CRLF"))
        {
            let buffer = self.active();
            let anchor = cursor_after_save_cleanup(&buffer.page.text, &text, buffer.view.anchor);
            let head = cursor_after_save_cleanup(&buffer.page.text, &text, buffer.view.head);
            if !self.apply_edit(hedit::editing::Edit { text, anchor, head }, crlf) {
                return false;
            }
        }
        if let Some((charset, needs_bom)) = charset {
            let shared = self.workspace.editor.get(self.active().view.id).unwrap();
            let mut doc = shared.lock().unwrap();
            if let Err(e) = doc.set_encoding(charset) {
                self.error = Some(e);
                return false;
            }
            if doc.info().encoding == "UTF-8" {
                if let Err(error) = doc.set_bom(needs_bom) {
                    self.error = Some(error);
                    return false;
                }
            }
            self.workspace.open[self.workspace.active].page.info = doc.info();
        }
        true
    }
    pub(super) fn find_advanced(&mut self, ctx: &egui::Context, previous: bool) {
        if !self.active().page.info.paged {
            self.navigate_find(ctx, previous);
            return;
        }
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
            let a = opening::page_cursor(&page, selected.start as u64)?;
            let z = opening::page_cursor(&page, selected.end as u64)?;
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
        let selection_scope = if self.quality.selection_only {
            let Some((_, start, end)) = self.quality.scope.filter(|(view, start, end)| {
                *view == id
                    && start < end
                    && text.is_char_boundary(*start)
                    && text.is_char_boundary(*end)
            }) else {
                self.notice = "Select text in this tab before replacing in a selection.".into();
                return;
            };
            Some(start..end)
        } else {
            None
        };
        let scope = if all {
            selection_scope.clone().unwrap_or(0..text.len())
        } else {
            hedit::advanced::byte(text, self.active().view.anchor.min(self.active().view.head))
                ..hedit::advanced::byte(
                    text,
                    self.active().view.anchor.max(self.active().view.head),
                )
        };
        if selection_scope
            .is_some_and(|selection| scope.start < selection.start || scope.end > selection.end)
        {
            self.notice = "Find a match inside the selected text before replacing.".into();
            return;
        }
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
    pub(super) fn reset_folder_search_results(&mut self) {
        self.quality.folder_task = None;
        self.quality.folder_generation = self.quality.folder_generation.wrapping_add(1);
        self.quality.results = Default::default();
        self.quality.folder_state = folder_search::State::Idle;
        self.quality.folder_regex = None;
        self.quality.result_current = None;
        self.quality.result_shown = None;
        self.quality.results_newer = false;
        self.quality.folder_collapsed.clear();
        self.quality.folder_snapshot = None;
        self.quality.preview_query = None;
        self.quality.preview_diff = None;
        self.quality.preview_rendered = None;
    }

    pub(super) fn folder_search(&mut self, ctx: &egui::Context, preview: bool) {
        if self.busy {
            return;
        }
        let roots = self.folder_search_roots();
        if self.quality.folder_scope.is_some() && roots.is_empty() {
            self.notice = "Clear the search scope or reopen its workspace folder.".into();
            return;
        }
        self.reset_folder_search_results();
        self.quality.folder_state = folder_search::State::Searching;
        let (query, replacement, filter, options) = (
            self.quality.folder_query.clone(),
            self.quality.folder_replacement.clone(),
            self.quality.filter.clone(),
            self.quality.folder_options.clone(),
        );
        self.quality.folder_snapshot = Some((query.clone(), filter.clone(), options.clone()));
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
                let path = b.page.info.path.as_ref()?;
                Some((search_path(path), b.page.text.clone()))
            })
            .collect();
        // Search holds immutable snapshots, never the UI's mutation lock.
        self.quality.folder_revisions = self
            .workspace
            .open
            .iter()
            .map(|b| (b.view.id, b.page.info.revision))
            .collect();
        self.quality.folder_roots = roots.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        self.quality.folder_task = Some(folder_search::Task {
            cancel: cancel.clone(),
            rx,
        });
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = search::folders(
                &roots,
                &filter,
                &query,
                preview.then_some(replacement.as_str()),
                &options,
                &open,
                &cancel,
            );
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
    pub(super) fn apply_folder_replacements(&mut self, ctx: &egui::Context) {
        if self.busy || !self.folder_preview_valid() || self.quality.results.truncated {
            self.notice = "Generate a fresh, complete replacement preview before applying.".into();
            return;
        }
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
                    .as_deref()
                    .is_some_and(|path| search_path(path) == plan.path)
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
                b.page.info.path.as_ref().map(|p| {
                    (
                        search_path(p),
                        b.view.id,
                        b.page.info.dirty,
                        b.follow,
                        b.page.start,
                    )
                })
            })
            .collect::<Vec<_>>();
        self.job(ctx, move || {
            let mut count = 0;
            let mut pages = Vec::new();
            let mut errors = Vec::new();
            let mut restore = vec![];
            for plan in plans {
                let result = (|| {
                    if let Some((_, id, dirty, follow, page_start)) =
                        open.iter().find(|(p, _, _, _, _)| p == &plan.path)
                    {
                        if *dirty || *follow {
                            return Err(format!(
                                "{}: open buffer has unsaved changes or follows a log; skipped.",
                                plan.path.display()
                            ));
                        }
                        let shared = editor.get(*id)?;
                        let mut doc = shared.lock().map_err(|e| e.to_string())?;
                        if doc.text(hedit::text_encoding::DECODE_LIMIT)? != plan.before {
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
                        let page = candidate.page(*page_start)?;
                        *doc = candidate;
                        pages.push((*id, page));
                        Ok(false)
                    } else {
                        restore.push(search::apply_recorded(&plan)?);
                        Ok(true)
                    }
                })();
                match result {
                    Ok(wrote_disk) => count += usize::from(wrote_disk),
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
        if self.busy || self.closing || self.quality.replace_undo.is_empty() {
            return;
        }
        let records = std::mem::take(&mut self.quality.replace_undo);
        // A file opened and edited since keeps its edits; it is not reverted
        // underneath them.
        let edited: Vec<PathBuf> = self
            .workspace
            .open
            .iter()
            .filter(|b| b.page.info.dirty)
            .filter_map(|b| b.page.info.path.as_deref().map(search_path))
            .collect();
        self.job(ctx, move || {
            let mut count = 0;
            let mut errors = vec![];
            let mut pending = Vec::new();
            for record in records {
                if edited.contains(&record.path) {
                    errors.push(format!(
                        "{} has unsaved changes in hedit; it was left as it is.",
                        record.path.display()
                    ));
                    pending.push(record);
                    continue;
                }
                match search::restore(&record) {
                    Ok(()) => count += 1,
                    Err(e) => {
                        errors.push(e);
                        pending.push(record);
                    }
                }
            }
            Ok(Action::Quality(Box::new(Event::Restored(
                count, errors, pending,
            ))))
        });
    }
    pub(super) fn compare_disk(&mut self, ctx: &egui::Context, git: bool) {
        let b = self.active();
        let Some(path) = b.page.info.path.clone() else {
            self.error = Some("Save the file first.".into());
            return;
        };
        let encoding = b.page.info.encoding.clone();
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
            let (disk, _) = hedit::text_encoding::decode(&bytes, &encoding)?;
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
                bom: b.page.info.dirty.then_some(b.page.info.bom),
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
            projects::save(&path, &project, &state)?;
            Ok(Action::Quality(Box::new(Event::ProjectSaved(path))))
        });
    }
    pub(super) fn load_project(&mut self, ctx: &egui::Context, path: Option<PathBuf>) {
        self.opens.cancel();
        if let Some(current) = self.quality.project_path.clone() {
            match self
                .project_snapshot()
                .and_then(|s| projects::save_workspace(&current, &s))
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
            let (project, state) = projects::load_with_workspace(&path)?;
            let mut tabs = Vec::new();
            for tab in state.iter().flat_map(|state| &state.tabs) {
                let mut doc = if let Some(path) = &tab.path {
                    Document::open_or_new_with_encoding(editor.id(), path, &tab.encoding)?
                } else {
                    Document::empty(editor.id())
                };
                if let Some(text) = &tab.text {
                    doc.replace(0, doc.info().size, text.clone(), doc.info().revision)?;
                }
                if doc.info().encoding != tab.encoding {
                    doc.set_encoding(&tab.encoding)?;
                }
                if let Some(bom) = tab.bom {
                    doc.set_bom(bom)?;
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
        let available = |&(_, (id, _)): &(usize, &(u64, u64))| {
            self.workspace
                .open
                .iter()
                .any(|buffer| buffer.view.id == *id)
                || self.workspace.closed.iter().any(|view| view.id == *id)
        };
        let mut navigation = self.quality.navigation.iter().enumerate();
        let target = if forward {
            navigation.nth(self.quality.navigation_index);
            navigation.find(available)
        } else {
            navigation
                .take(self.quality.navigation_index)
                .rev()
                .find(available)
        };
        let Some((next, &(id, offset))) = target else {
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
        self.insert_snippet_with(&snippet.body, before, |body, selection| {
            Some(intelligence::expand_fields(body, selection))
        });
    }

    pub(super) fn insert_snippet_with(
        &mut self,
        body: &str,
        before: usize,
        expand: impl FnOnce(&str, &str) -> Option<intelligence::Expansion>,
    ) {
        if self.active().follow || self.active().page.info.paged {
            return;
        }
        let b = self.active();
        let old = &b.page.text;
        let start = snippet_fields::insertion_byte(old, b.view.anchor.min(b.view.head));
        let end = snippet_fields::insertion_byte(old, b.view.anchor.max(b.view.head));
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
        let body = snippet_fields::indent_body(&body.replace("\r\n", "\n"), &indent, &unit);
        // Convert before expanding fields so their character positions include
        // every carriage return in the inserted text.
        let body = if b.page.info.eol == "CRLF" {
            body.replace('\n', "\r\n")
        } else {
            body
        };
        let Some(expansion) = expand(&body, &old[start..end]) else {
            self.error = Some("The expanded extension snippet is too large.".into());
            return;
        };
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
        let before = &b.page.text[..snippet_fields::insertion_byte(&b.page.text, b.view.head)];
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
    pub(super) fn begin_file_action(
        &mut self,
        ctx: &egui::Context,
        path: PathBuf,
        operation: FileOperation,
    ) {
        if self.busy || self.closing {
            return;
        }
        self.quality.file_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if self.workspace.remote.is_some() {
            self.quality.file_name = if operation == FileOperation::Move {
                String::new()
            } else {
                percent_encoding::percent_decode_str(&self.quality.file_name)
                    .decode_utf8_lossy()
                    .into_owned()
            };
        }
        self.quality.file_action = Some(FileAction {
            path,
            operation,
            skip_trash_confirmation: false,
        });
        if operation == FileOperation::Trash && !self.settings.confirm_trash {
            self.perform_file_action(ctx);
        }
    }
    pub(super) fn perform_file_action(&mut self, ctx: &egui::Context) {
        if self.busy || self.closing {
            return;
        }
        if let Err(error) = self.workspace.editor.check_membership_writable() {
            self.error = Some(error);
            return;
        }
        let Some(FileAction {
            path,
            operation,
            skip_trash_confirmation,
        }) = self.quality.file_action.take()
        else {
            return;
        };
        if operation == FileOperation::Trash && skip_trash_confirmation {
            self.settings.confirm_trash = false;
            self.flush_settings();
        }
        if operation != FileOperation::Duplicate {
            self.opens.cancel();
        }
        if let Some(remote) = self.workspace.remote.clone() {
            let name = self.quality.file_name.clone();
            self.job(ctx, move || {
                remote.flush()?;
                let path = hedit::folder::entry_path(&path)?;
                let to = match operation {
                    FileOperation::Rename => Some(PathBuf::from(hedit::folder::rename_path(
                        path.to_string_lossy().into_owned(),
                        name,
                    )?)),
                    FileOperation::Move => {
                        let target = remote.path(&name)?;
                        Some(hedit::folder::move_path(&path, &target)?)
                    }
                    FileOperation::Duplicate => {
                        let target = hedit::folder::duplicate_sibling(&path, &name)?;
                        return Ok(Action::Created(
                            target.to_string_lossy().into_owned(),
                            false,
                        ));
                    }
                    FileOperation::Trash => {
                        hedit::folder::trash_path(&path)?;
                        None
                    }
                    FileOperation::Delete => {
                        hedit::folder::delete_path(&path)?;
                        None
                    }
                };
                Ok(Action::Quality(Box::new(Event::FileChanged(path, to))))
            });
            return;
        }
        // Documents and roots use resolved paths. Normalize the source before
        // mutation, while its ancestors still exist (e.g. macOS /var aliases).
        let path = match hedit::folder::entry_path(&path) {
            Ok(path) => path,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        if operation == FileOperation::Delete
            && self
                .workspace
                .store
                .directory()
                .canonicalize()
                .is_ok_and(|dir| dir.starts_with(&path) || path.starts_with(&dir))
        {
            self.error = Some("This deletion would remove the active recovery data.".into());
            return;
        }
        let name = self.quality.file_name.clone();
        if matches!(operation, FileOperation::Trash | FileOperation::Delete) {
            let mut preserved = Vec::new();
            let checkpoint = (|| {
                for view in self
                    .workspace
                    .open
                    .iter()
                    .map(|b| &b.view)
                    .chain(&self.workspace.closed)
                {
                    let shared = self.workspace.editor.get(view.id)?;
                    let mut doc = shared.lock().map_err(|error| error.to_string())?;
                    if let Some(source) =
                        doc.info().path.filter(|p| Path::new(p).starts_with(&path))
                    {
                        preserved.push((view.id, source, doc.prepare_removal()?));
                    }
                }
                self.workspace.checkpoint()
            })();
            if let Err(error) = checkpoint {
                self.rollback_file_removal(preserved.into_iter().map(|(id, _, state)| (id, state)));
                self.error = Some(error);
                return;
            }
            self.refresh_removal_buffer_info();
            self.job(ctx, move || {
                let result = if operation == FileOperation::Trash {
                    hedit::folder::trash_path(&path)
                } else {
                    hedit::folder::delete_path(&path)
                };
                let event = match result {
                    Ok(()) => Event::FileChanged(path, None),
                    Err(error) => {
                        let remaining = preserved
                            .into_iter()
                            .filter(|(_, source, _)| {
                                !std::fs::symlink_metadata(source).is_err_and(|error| {
                                    error.kind() == std::io::ErrorKind::NotFound
                                })
                            })
                            .map(|(id, _, state)| (id, state))
                            .collect();
                        Event::FileRemovalFailed(error, remaining)
                    }
                };
                Ok(Action::Quality(Box::new(event)))
            });
            return;
        }
        // Snapshot first so moving a source preserves recoverable buffers.
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
                FileOperation::Trash | FileOperation::Delete => {
                    unreachable!("removal handled separately")
                }
            };
            Ok(Action::Quality(Box::new(Event::FileChanged(path, to))))
        });
    }
    fn refresh_removal_buffer_info(&mut self) {
        for buffer in &mut self.workspace.open {
            if let Ok(shared) = self.workspace.editor.get(buffer.view.id) {
                buffer.page.info = shared.lock().unwrap().info();
            }
        }
        self.changed();
    }

    fn rollback_file_removal(
        &mut self,
        states: impl IntoIterator<Item = (u64, hedit::document::RemovalState)>,
    ) {
        for (id, state) in states {
            if let Ok(shared) = self.workspace.editor.get(id) {
                shared.lock().unwrap().rollback_removal(state);
            }
        }
        self.refresh_removal_buffer_info();
    }

    pub(super) fn poll_incoming(&mut self, ctx: &egui::Context) {
        if self.closing {
            return;
        }
        if self.quality.incoming.is_some() {
            if !self.busy
                && !self.opens.pending()
                && self.pending_paths.is_empty()
                && !self.checkpoint_pending
            {
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
        if self.busy || self.opens.pending() || !self.pending_paths.is_empty() {
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
        self.request_activation(ctx, incoming.request.activation.clone());
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
                Ok(Action::Add(Box::new(doc), page, 0))
            });
        }
    }
}

impl HeditApp {
    /// Share eligibility and timing between the worker and the frame scheduler.
    pub(super) fn git_refresh_delay(&self, ctx: &egui::Context) -> Option<Duration> {
        if self.workspace.remote.is_some()
            || self.settings.mini_diff == "off"
            || self.quality.git_pending
            || self.busy
            || self.closing
        {
            return None;
        }
        let b = self.active();
        if b.page.info.path.is_none() || b.page.info.paged {
            return None;
        }
        if refocused(ctx) {
            return Some(Duration::ZERO);
        }
        let same = self.quality.git_key.as_ref().is_some_and(|key| {
            key.0 == b.view.id
                && key.1 == b.page.info.revision
                && key.2 == b.page.info.dirty
                && key.3 == self.settings.mini_diff
        });
        let age = self.quality.git_at.map_or(Duration::MAX, |at| at.elapsed());
        if same && age < Duration::from_secs(30) {
            // Recheck the baseline on the next input or watcher update.
            // A fresh marker cache should neither force an idle frame nor run Git.
            return None;
        }
        Some(Duration::from_secs(2).saturating_sub(age))
    }
    pub(super) fn poll_git(&mut self, ctx: &egui::Context) {
        if self.settings.mini_diff == "off" {
            self.quality.git_key = None;
            self.quality.git_at = None;
            self.quality.git_lines.clear();
            self.quality.git_cache.clear();
            self.quality.git_base.clear();
            return;
        }
        if self.git_refresh_delay(ctx) != Some(Duration::ZERO) {
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
        let encoding = b.page.info.encoding.clone();
        self.quality.git_at = Some(Instant::now());
        self.quality.git_pending = true;
        self.quality.git_key = Some((id, revision, dirty, mode.clone()));
        thread::spawn(move || {
            let path = Path::new(&path);
            // "auto" compares with Git's HEAD, or the saved file outside Git.
            // The saved file in the encoding the editor reads it with.
            let saved = || {
                // Mini Diff only runs for a non-paged view. Allow its full
                // disk representation, including twice the UTF-8 length for
                // ASCII stored as UTF-16 and a byte order mark.
                let file = hedit_core::file_io::open_regular(path).ok()?;
                let bytes =
                    hedit_core::file_io::read_bounded(file, 2 * hedit::document::SMALL_LIMIT + 3)
                        .ok()?;
                hedit::text_encoding::decode(&bytes, &encoding)
                    .or_else(|_| {
                        hedit::text_encoding::decode(&bytes, hedit::text_encoding::detect(&bytes))
                    })
                    .ok()
                    .map(|(text, _)| text)
            };
            let base = match mode.as_str() {
                "saved" => saved(),
                _ => intelligence::git_base(path).ok().or_else(saved),
            };
            // Git may store LF where the file on disk uses CRLF; compare like
            // with like, so only real changes are marked and reverted.
            let base = base.map(|base| {
                if text.contains("\r\n") && !base.contains('\r') {
                    base.replace('\n', "\r\n")
                } else {
                    base
                }
            });
            let diff = base
                .as_deref()
                .filter(|base| *base != text)
                .map(|base| hedit::search::diff(base, &text, path))
                .unwrap_or_default();
            let _ = tx.send(Ok(Action::Quality(Box::new(Event::GitMarkers(
                id, revision, mode, diff, base,
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

fn search_path(path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if hedit_core::backend::is_remote(&p) {
        p
    } else {
        // Drafts already have a canonical parent even before the file exists.
        p.canonicalize().unwrap_or(p)
    }
}
