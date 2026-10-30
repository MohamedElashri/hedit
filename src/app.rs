mod advanced_edit;
mod auto_complete;
mod bidi_view;
mod build_panel;
mod changes;
mod chords;
mod chrome;
mod clipboard_lines;
mod close;
#[cfg(test)]
mod close_tests;
mod commands;
mod controls;
mod definitions;
mod edit_actions;
#[cfg(test)]
mod editing_tests;
mod editor_layout;
mod find_live;
mod find_tools;
mod fold_gutter;
mod folder_search;
#[cfg(test)]
mod folder_search_tests;
mod font_picker;
mod goto_anything;
#[cfg(test)]
mod goto_tests;
mod macros;
mod markdown;
mod markdown_media;
mod markdown_scroll;
#[cfg(test)]
mod markdown_tests;
#[cfg(test)]
mod overlay_tests;
mod overlays;
mod panes;
mod paste_history;
mod preferences;
mod project;
mod project_settings;
#[cfg(test)]
mod project_settings_tests;
mod quality;
#[cfg(test)]
mod quality_tests;
mod quality_windows;
#[cfg(test)]
mod recovery_tests;
#[cfg(test)]
mod redesign_tests;
#[cfg(test)]
mod rtl_tests;
#[cfg(test)]
mod screenshot_tests;
mod settings_pages;
mod settings_store;
#[cfg(test)]
mod settings_tests;
mod snippet_fields;
mod spell_check;
mod tab_switch;
#[cfg(test)]
mod tab_tests;
mod themes;
mod tree_row;
mod trim_indent;
mod updates;
mod view_commands;
mod wait;

use commands::Command;
use eframe::egui::{self, FontId, Key, Modifiers, RichText};
use hedit::{
    brand,
    debugger::{DebugSession, State},
    document::{Document, Info, Page, PAGE_SIZE},
    session::SessionStore,
    syntax,
    theme::{Rgb, Role, Theme},
    workspace::{Buffer, Workspace},
};
use preferences::PreferencesPage;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

enum Action {
    Quality(Box<quality::Event>),
    Add(Document, Page, usize),
    Activate(u64),
    Reopen(u64, Option<(Page, usize)>),
    Folder(hedit::folder::FolderListing),
    Folders(Vec<hedit::folder::FolderListing>),
    Created(String, bool),
    Page(u64, Page, Option<(usize, usize)>),
    Reloaded(u64, Page, Option<(usize, usize)>),
    History(u64, Page, bool, Vec<hedit::advanced::Selection>),
    Saved(u64, Info),
    SaveTo(u64, PathBuf),
    SaveCancelled,
    Discarded(u64, Option<Page>),
    ThemeImported(Theme),
    DebugProgram(PathBuf),
    /// Open a `path:line` location.
    OpenAt(PathBuf),
    OpenFailed(String),
    /// A font loaded for the `font_face` setting.
    /// A font for `font_face`: its name, file bytes, and face index.
    Font(String, Vec<u8>, u32),
    /// Installed font families, for the font picker.
    FontFamilies(Vec<hedit::fonts::Family>),
    /// System fonts for other scripts are ready.
    FallbackFonts,
    Notice(String),
    Nothing,
}
type SoftState = (u64, Vec<hedit::advanced::Selection>);
const CHECKPOINT_FAILED: &str = "RECOVERY CHECKPOINT FAILED";
const FILE_ERROR_TIME: Duration = Duration::from_secs(5);
/// Job outcomes that are expected answers, shown as notices rather than errors.
const ROUTINE: [&str; 2] = ["No matches found.", "Search cancelled."];

struct Checkpoint {
    generation: u64,
    result: Result<(), String>,
}

/// Wakes the window when another `hedit` process sends files, so an idle
/// window needs no redraw timer. Stops when the app is dropped.
struct RequestWatch(Arc<AtomicBool>);
impl RequestWatch {
    fn start(ctx: &egui::Context, directory: PathBuf) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let ctx = ctx.clone();
        thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                if hedit::ipc::pending(&directory) {
                    ctx.request_repaint();
                }
                thread::sleep(Duration::from_millis(250));
            }
        });
        Self(stop)
    }
}
impl Drop for RequestWatch {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

pub struct HeditApp {
    _requests: RequestWatch,
    quality: quality::Quality,
    workspace: Workspace,
    project: project::ProjectBrowser,
    tx: Sender<Result<Action, String>>,
    rx: Receiver<Result<Action, String>>,
    checkpoint_tx: Sender<Checkpoint>,
    checkpoint_rx: Receiver<Checkpoint>,
    checkpoint_pending: bool,
    checkpoint_worker: Option<thread::JoinHandle<()>>,
    checkpoint_at: Instant,
    generation: u64,
    persisted: u64,
    last_saved: Option<Instant>,
    busy: bool,
    cancel: Arc<AtomicBool>,
    closing: bool,
    close_flow: Option<close::CloseFlow>,
    exit_ready: bool,
    pending_paths: VecDeque<PathBuf>,
    error: Option<String>,
    /// File-open errors expire; keep the message so a newer error is not hidden.
    transient_error: Option<(String, Instant)>,
    notice: String,
    /// The notice on display in the status bar and when it appeared.
    notice_shown: (String, Instant),
    settings: hedit::settings::Settings,
    settings_files: settings_store::SettingsFiles,
    /// The command whose shortcut is being recorded on the Key Bindings page.
    key_capture: Option<Command>,
    /// `hedit -w` requests waiting for their files to close.
    waits: Vec<wait::Wait>,
    build: build_panel::BuildState,
    /// The first key of a shortcut being recorded, and when it was pressed.
    capture_first: Option<(String, Instant)>,
    /// The first key of a two-key shortcut that is waiting for its second.
    chord: Option<egui::KeyboardShortcut>,
    /// Selection history for Undo/Redo selection change: (view, cursors).
    soft_history: (Vec<SoftState>, Vec<SoftState>, Option<SoftState>),
    tab_cycle: tab_switch::TabCycle,
    /// Text copied or cut in hedit, newest first.
    paste_history: Vec<String>,
    /// A list shown in the palette instead of files or commands.
    pick_list: Option<commands::PickList>,
    /// Tab moves through the fields of the snippet inserted last.
    snippet: Option<snippet_fields::SnippetSession>,
    auto: auto_complete::AutoComplete,
    /// The focused editor's caret on screen, for the completion list.
    caret_screen: Option<egui::Rect>,
    /// The view, name range, and pointer position of a name the pointer rests on.
    hover_target: Option<(u64, (usize, usize), egui::Pos2)>,
    hover: Option<definitions::Hover>,
    fold_cache: fold_gutter::FoldCache,
    spell: spell_check::SpellCheck,
    macros: macros::Macros,
    /// Text copied as whole lines, which pastes above the cursor's line.
    line_clip: Option<String>,
    /// The next paste is Paste and indent.
    paste_indent: bool,
    /// Files used recently, newest first.
    recent_files: Vec<String>,
    /// A scroll position for a view, applied without moving its cursor.
    scroll_request: Option<(u64, f64)>,
    /// A line holding only the indentation Enter added.
    auto_indent: Option<trim_indent::AutoIndent>,
    /// Cursors made by Add cursor above/below and the column they keep.
    line_cursor_goal: Option<(Vec<hedit::advanced::Selection>, usize)>,
    live_find: find_live::LiveFind,
    markdown: markdown::Markdown,
    /// Where the cursor was before Go to Anything previewed a result.
    goto_origin: Option<goto_anything::Origin>,
    /// The `font_face` in use, or being loaded.
    font_applied: String,
    /// The `font_face` font file, kept to rebuild the fonts when fallbacks load.
    editor_font: Option<(Vec<u8>, u32)>,
    /// Installed font families, once listed.
    font_families: Option<Vec<hedit::fonts::Family>>,
    /// Open the font picker when the families arrive.
    font_picker_pending: bool,
    /// Installed families are being listed.
    fonts_listing: bool,
    /// An input method is composing text (it sent a non-empty preedit).
    ime_composing: bool,
    key_filter: String,
    panes: panes::Panes,
    rectangle_start: Option<(usize, usize)>,
    extra_layouts: [editor_layout::LayoutCache; panes::MAX_GROUPS - 1],
    projections: [editor_layout::ProjectionCache; panes::MAX_GROUPS],
    distraction_free: bool,
    fullscreen: bool,
    preferences_page: PreferencesPage,
    goto_open: bool,
    goto_focus: bool,
    replace_open: bool,
    commands_only: bool,
    palette_focus: bool,
    palette_index: usize,
    about_open: bool,
    window_title: String,
    find_open: bool,
    find_focus: bool,
    query: String,
    replacement: String,
    palette: bool,
    palette_query: String,
    syntax_query: String,
    syntax_focus: bool,
    syntax_matches: Vec<syntax::Language>,
    syntax_last_query: String,
    editor_layout: editor_layout::LayoutCache,
    goto: String,
    gpu_open: bool,
    debug: Option<DebugSession>,
    debug_program: String,
    debug_args: String,
    breakpoint: String,
    console: String,
    sanitizer: String,
    poll_at: Instant,
    focus_editor: bool,
    theme: Theme,
    preferences_open: bool,
    appearance: themes::Appearance,
    /// The menu-bar menu shown last frame, to highlight its title.
    open_menu: Option<&'static str>,
    rename_target: Option<u64>,
    unsaved_file_name: String,
    unsaved_file_name_focus: bool,
    shelf_open: bool,
    shelf_query: String,
    bookmarks_open: bool,
    updates: updates::Updates,
}
impl HeditApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        workspace: Workspace,
        errors: Vec<String>,
        paths: Vec<PathBuf>,
    ) -> Self {
        Self::from_context(&cc.egui_ctx, cc.storage, workspace, errors, paths)
    }
    fn from_context(
        ctx: &egui::Context,
        storage: Option<&dyn eframe::Storage>,
        workspace: Workspace,
        errors: Vec<String>,
        paths: Vec<PathBuf>,
    ) -> Self {
        let mut errors = errors;
        let theme = match storage.and_then(|s| s.get_string("theme")) {
            Some(json) => match Theme::from_json(&json) {
                Ok(theme) => themes::refresh_saved_theme(theme),
                Err(error) => {
                    errors.push(format!("Saved appearance could not be loaded: {error}"));
                    Theme::default()
                }
            },
            None => Theme::default(),
        };
        theme.apply(ctx);
        // Ctrl/Cmd +, -, and 0 change the editor font, not the whole interface.
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        let appearance = themes::Appearance::load(storage, &theme);
        let (tx, rx) = mpsc::channel();
        let (checkpoint_tx, checkpoint_rx) = mpsc::channel();
        let project = project::ProjectBrowser::new(&workspace.folders);
        let (mut settings, config, settings_files, setting_errors) =
            settings_store::load(storage, workspace.store.directory());
        errors.extend(setting_errors);
        // The session remembers whether this workspace's folders were shown.
        if let Some(folder) = workspace.folders.first() {
            settings.show_folders = folder.visible;
        }
        let mut panes: panes::Panes = storage
            .and_then(|s| eframe::get_value(s, "panes"))
            .unwrap_or_default();
        panes.normalize();
        let mut app = Self {
            _requests: RequestWatch::start(ctx, workspace.store.directory().to_owned()),
            quality: Default::default(),
            project,
            workspace,
            tx,
            rx,
            checkpoint_tx,
            checkpoint_rx,
            checkpoint_pending: false,
            checkpoint_worker: None,
            checkpoint_at: Instant::now() - Duration::from_secs(3),
            generation: 1,
            persisted: 0,
            last_saved: None,
            busy: false,
            cancel: Arc::new(AtomicBool::new(false)),
            closing: false,
            close_flow: None,
            exit_ready: false,
            pending_paths: paths.into(),
            error: if errors.is_empty() {
                None
            } else {
                Some(errors.join("\n"))
            },
            transient_error: None,
            notice: "Ready.".into(),
            notice_shown: ("Ready.".into(), Instant::now()),
            settings,
            settings_files,
            key_capture: None,
            waits: Vec::new(),
            build: Default::default(),
            capture_first: None,
            chord: None,
            soft_history: Default::default(),
            tab_cycle: Default::default(),
            paste_history: Vec::new(),
            pick_list: None,
            snippet: None,
            auto: Default::default(),
            caret_screen: None,
            hover_target: None,
            hover: None,
            fold_cache: Default::default(),
            spell: Default::default(),
            macros: Default::default(),
            line_clip: None,
            paste_indent: false,
            scroll_request: None,
            auto_indent: None,
            recent_files: storage
                .and_then(|s| eframe::get_value(s, "recent_files"))
                .unwrap_or_default(),
            line_cursor_goal: None,
            live_find: Default::default(),
            markdown: Default::default(),
            goto_origin: None,
            font_applied: String::new(),
            editor_font: None,
            font_families: None,
            font_picker_pending: false,
            fonts_listing: false,
            ime_composing: false,
            key_filter: String::new(),
            rectangle_start: None,
            panes,
            extra_layouts: Default::default(),
            projections: Default::default(),
            distraction_free: false,
            fullscreen: false,
            preferences_page: PreferencesPage::Editor,
            goto_open: false,
            goto_focus: false,
            replace_open: false,
            commands_only: false,
            palette_focus: false,
            palette_index: 0,
            about_open: false,
            window_title: String::new(),
            find_open: false,
            find_focus: false,
            query: String::new(),
            replacement: String::new(),
            palette: false,
            palette_query: String::new(),
            goto: String::new(),
            gpu_open: false,
            debug: None,
            debug_program: String::new(),
            debug_args: "[]".into(),
            breakpoint: String::new(),
            console: String::new(),
            sanitizer: "memcheck".into(),
            poll_at: Instant::now(),
            focus_editor: true,
            syntax_query: String::new(),
            syntax_focus: true,
            syntax_matches: syntax::Language::ALL.to_vec(),
            syntax_last_query: String::new(),
            editor_layout: editor_layout::LayoutCache::default(),
            theme,
            preferences_open: false,
            appearance,
            open_menu: None,
            rename_target: None,
            unsaved_file_name: String::new(),
            unsaved_file_name_focus: false,
            shelf_open: false,
            shelf_query: String::new(),
            bookmarks_open: false,
            updates: Default::default(),
        };
        // Tests stay on egui's own fonts unless they load fallbacks themselves.
        if !cfg!(test) {
            settings_store::load_fallback_fonts(app.tx.clone(), ctx);
        }
        app.init_quality(storage);
        app.quality.config = config;
        app.apply_settings();
        let warnings = app.apply_theme_settings(ctx);
        if !warnings.is_empty() {
            let banner = app.error.take().into_iter().chain(warnings);
            app.error = Some(banner.collect::<Vec<_>>().join("\n"));
        }
        app
    }
    /// Whether the window holds nothing worth reopening: no folders, no
    /// closed files, and only empty, unnamed files.
    fn workspace_is_empty(&self) -> bool {
        self.workspace.folders.is_empty()
            && self.workspace.closed.is_empty()
            && self
                .workspace
                .open
                .iter()
                .all(|b| b.page.text.is_empty() && b.page.info.path.is_none())
    }
    fn changed(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }
    fn report(&mut self, result: Result<(), String>) {
        if let Err(error) = result {
            self.error = Some(error);
        } else {
            self.changed();
            self.focus_editor = true;
        }
    }
    fn active(&self) -> &Buffer {
        &self.workspace.open[self.workspace.active]
    }
    fn job(
        &mut self,
        ctx: &egui::Context,
        job: impl FnOnce() -> Result<Action, String> + Send + 'static,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.cancel.store(false, Ordering::Relaxed);
        let sender = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = sender.send(job());
            ctx.request_repaint();
        });
    }
    fn checkpoint(&mut self, ctx: &egui::Context) {
        if self.checkpoint_pending {
            return;
        }
        self.checkpoint_at = Instant::now();
        let store: Arc<SessionStore> = Arc::clone(&self.workspace.store);
        let snapshot = match hedit::session::SessionSnapshot::capture(
            &self.workspace.editor,
            self.workspace.view(),
        ) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.error = Some(format!("{CHECKPOINT_FAILED}\n{error}"));
                self.cancel_close();
                return;
            }
        };
        // Background operations may hold a document lock while scanning a large
        // file. Never block the UI here; the next edit/result or exit also saves
        // project state, while the recovery worker can wait independently.
        let project_state = if self.busy {
            None
        } else {
            self.quality
                .project_path
                .clone()
                .map(|path| (path, self.project_snapshot()))
        };
        let generation = self.generation;
        let sender = self.checkpoint_tx.clone();
        let ctx = ctx.clone();
        self.checkpoint_pending = true;
        self.checkpoint_worker = Some(thread::spawn(move || {
            let result = store.save_snapshot(snapshot).and_then(|()| {
                if let Some((path, state)) = project_state {
                    hedit::projects::atomic_json(&path.with_extension("hedit-workspace"), &state?)
                } else {
                    Ok(())
                }
            });
            let _ = sender.send(Checkpoint { generation, result });
            ctx.request_repaint();
        }));
    }
    fn open(&mut self, ctx: &egui::Context, path: Option<PathBuf>) {
        if path
            .as_ref()
            .is_some_and(|p| p.extension().is_some_and(|e| e == "hedit-project"))
        {
            self.load_project(ctx, path);
            return;
        }
        self.navigation_mark();
        let editor = Arc::clone(&self.workspace.editor);
        let closed_paths: Vec<_> = self
            .workspace
            .closed
            .iter()
            .filter_map(|view| {
                let doc = self.workspace.editor.get(view.id).ok()?;
                let path = doc.lock().ok()?.info().path?;
                Some((path, view.id))
            })
            .collect();
        let open_paths: Vec<_> = self
            .workspace
            .open
            .iter()
            .map(|b| (b.page.info.path.clone(), b.page.info.id))
            .collect();
        let open = move || -> Result<Action, String> {
            let path = match path {
                Some(path) => path,
                None => match rfd::FileDialog::new()
                    .set_title("Open in hedit")
                    .pick_file()
                {
                    Some(p) => p,
                    None => return Ok(Action::Nothing),
                },
            };
            if path.is_dir() {
                return hedit::folder::read_directory(path).map(Action::Folder);
            }
            let (file, line, col) = hedit::cli::location(&path);
            let plain_path = file == path;
            let path = file;
            let path = if path.is_absolute() {
                path
            } else {
                std::env::current_dir()
                    .map_err(|e| e.to_string())?
                    .join(path)
            };
            let canonical = std::fs::canonicalize(&path)
                .unwrap_or(path.clone())
                .to_string_lossy()
                .into_owned();
            if let Some((_, id)) = open_paths
                .iter()
                .find(|(p, _)| p.as_deref() == Some(&canonical))
            {
                if plain_path {
                    return Ok(Action::Activate(*id));
                }
                let shared = editor.get(*id)?;
                let mut doc = shared.lock().map_err(|e| e.to_string())?;
                let offset = doc.location_offset(line, col)?;
                let page = doc.page(offset.saturating_sub(4096))?;
                let cursor = page.text
                    [..(offset - page.start).min(page.text.len() as u64) as usize]
                    .chars()
                    .count();
                return Ok(Action::Page(*id, page, Some((cursor, cursor))));
            }
            if let Some((_, id)) = closed_paths.iter().find(|(p, _)| p == &canonical) {
                let location = if plain_path {
                    None
                } else {
                    let shared = editor.get(*id)?;
                    let mut doc = shared.lock().map_err(|e| e.to_string())?;
                    let offset = doc.location_offset(line, col)?;
                    let page = doc.page(offset.saturating_sub(4096))?;
                    let cursor = page.text[..(offset - page.start) as usize].chars().count();
                    Some((page, cursor))
                };
                return Ok(Action::Reopen(*id, location));
            }
            let mut doc = Document::open_or_new(editor.id(), &path)?;
            let offset = doc.location_offset(line, col)?;
            let page = doc.page(offset.saturating_sub(4096))?;
            let cursor = page.text[..(offset - page.start) as usize].chars().count();
            Ok(Action::Add(doc, page, cursor))
        };
        self.job(ctx, move || Ok(open().unwrap_or_else(Action::OpenFailed)));
    }
    fn load_page(&mut self, ctx: &egui::Context, offset: u64, reload: bool) {
        let editor = Arc::clone(&self.workspace.editor);
        let id = self.active().page.info.id;
        let selection = (self.active().view.anchor, self.active().view.head);
        if reload && self.active().page.info.dirty {
            self.error = Some(
                "This file has unsaved changes. Use Save As to keep them before reloading.".into(),
            );
            return;
        }
        self.job(ctx, move || {
            let shared = editor.get(id)?;
            let mut doc = shared.lock().map_err(|e| e.to_string())?;
            let page = if reload {
                doc.reload(offset)?
            } else {
                let offset = if offset == u64::MAX {
                    doc.info().size.saturating_sub(PAGE_SIZE)
                } else {
                    offset
                };
                doc.load_page(offset)?
            };
            let count = page.text.chars().count();
            Ok(if reload {
                Action::Reloaded(
                    id,
                    page,
                    Some((selection.0.min(count), selection.1.min(count))),
                )
            } else {
                Action::Page(id, page, None)
            })
        });
    }
    fn save_file(&mut self, ctx: &egui::Context, save_as: bool) {
        let id = self.active().view.id;
        if save_as || self.active().page.info.path.is_none() {
            let name = self
                .active()
                .view
                .custom_name
                .clone()
                .unwrap_or_else(|| self.active().view.name.clone());
            let directory = self
                .active()
                .page
                .info
                .path
                .as_ref()
                .and_then(|p| std::path::Path::new(p).parent().map(|p| p.to_path_buf()))
                .or_else(|| {
                    self.workspace
                        .folders
                        .first()
                        .map(|f| PathBuf::from(&f.path))
                });
            self.job(ctx, move || {
                let mut dialog = rfd::FileDialog::new()
                    .set_title("Save file — hedit")
                    .set_file_name(name);
                if let Some(directory) = directory {
                    dialog = dialog.set_directory(directory);
                }
                Ok(match dialog.save_file() {
                    Some(path) => Action::SaveTo(id, path),
                    None => Action::SaveCancelled,
                })
            });
        } else {
            self.save_to_path(ctx, id, None);
        }
    }
    fn save_to_path(&mut self, ctx: &egui::Context, id: u64, target: Option<PathBuf>) {
        let Some(index) = self.workspace.open.iter().position(|b| b.view.id == id) else {
            return;
        };
        self.workspace.active = index;
        if !self.format_before_save_for(target.as_deref()) {
            return;
        }
        let editor = Arc::clone(&self.workspace.editor);
        self.job(ctx, move || {
            let info = editor
                .get(id)?
                .lock()
                .map_err(|e| e.to_string())?
                .save(target)?;
            Ok(Action::Saved(id, info))
        });
    }
    fn history(&mut self, ctx: &egui::Context, redo: bool) {
        let id = self.active().page.info.id;
        let offset = self.active().page.start;
        let revision = self.active().page.info.revision;
        let cursors = self.selections();
        let editor = Arc::clone(&self.workspace.editor);
        self.job(ctx, move || {
            let page = editor
                .get(id)?
                .lock()
                .map_err(|e| e.to_string())?
                .history(redo, offset, revision)?;
            Ok(Action::History(id, page, redo, cursors))
        });
    }
    fn find(&mut self, ctx: &egui::Context) {
        if self.query.is_empty() {
            return;
        }
        if !self.active().page.info.paged
            || self.quality.options.regex
            || !self.quality.options.case_sensitive
            || self.quality.options.whole_word
            || self.quality.selection_only
        {
            self.find_advanced(ctx, false);
            return;
        }
        if self.query.is_empty() {
            return;
        }
        let id = self.active().page.info.id;
        let query = self.query.clone();
        let anchor = self.active().page.start
            + self
                .active()
                .page
                .text
                .chars()
                .take(self.active().view.head)
                .map(char::len_utf8)
                .sum::<usize>() as u64;
        let editor = Arc::clone(&self.workspace.editor);
        let cancel = Arc::clone(&self.cancel);
        self.job(ctx, move || {
            let shared = editor.get(id)?;
            let mut doc = shared.lock().map_err(|e| e.to_string())?;
            let mut start = 0;
            let mut first = None;
            let found = loop {
                if cancel.load(Ordering::Relaxed) {
                    return Err("Search cancelled.".into());
                }
                let scan = doc.scan_search(&query, start, anchor)?;
                if first.is_none() {
                    first = doc.scan_search(&query, start, 0)?.next_offset;
                }
                if let Some(offset) = scan.next_offset {
                    break offset;
                }
                if scan.complete {
                    break first.ok_or("No matches found.")?;
                }
                start = scan.scanned_through;
            };
            let page = doc.page(found.saturating_sub(4096))?;
            let local = (found - page.start) as usize;
            let cursor = page.text[..local].chars().count();
            Ok(Action::Page(
                id,
                page,
                Some((cursor, cursor + query.chars().count())),
            ))
        });
    }
    fn replace_selection(&mut self) {
        self.replace_advanced(false);
    }
    fn goto_line(&mut self, ctx: &egui::Context) {
        let Ok(line) = self.goto.parse::<u64>() else {
            self.error = Some("Enter a positive line number.".into());
            return;
        };
        if line == 0 {
            return;
        }
        self.goto_location(ctx, line, 1);
    }
    fn goto_location(&mut self, ctx: &egui::Context, line: u64, column: u64) {
        self.navigation_mark();
        let id = self.active().page.info.id;
        let editor = Arc::clone(&self.workspace.editor);
        self.job(ctx, move || {
            let shared = editor.get(id)?;
            let mut doc = shared.lock().map_err(|e| e.to_string())?;
            let offset = doc.location_offset(line, column)?;
            let page = doc.page(offset.saturating_sub(4096))?;
            let cursor = page.text[..(offset - page.start) as usize].chars().count();
            Ok(Action::Page(id, page, Some((cursor, cursor))))
        });
    }
    fn receive(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.rx.try_recv() {
            let opened = matches!(
                &event,
                Ok(Action::Add(..) | Action::Activate(..) | Action::Reopen(..) | Action::Page(..))
            );
            // Fonts and Git markers load beside jobs, not as them.
            let side = match &event {
                Ok(Action::Quality(event)) => {
                    matches!(&**event, quality::Event::GitMarkers(..))
                }
                Ok(Action::Font(..) | Action::FallbackFonts | Action::FontFamilies(_)) => true,
                _ => false,
            };
            if !side {
                self.busy = false;
            }
            let reloaded = matches!(&event, Ok(Action::Reloaded(..)));
            match event {
                Ok(Action::Quality(event)) => self.quality_event(ctx, *event),
                Ok(Action::Activate(id)) => {
                    if let Some(index) = self
                        .workspace
                        .open
                        .iter()
                        .position(|b| b.page.info.id == id)
                    {
                        self.workspace.active = index;
                        self.changed();
                        self.focus_editor = true;
                        self.finish_preview();
                        self.navigation_mark();
                    }
                }
                Ok(Action::Reopen(id, location)) => {
                    if let Some(index) = self.workspace.closed.iter().position(|v| v.id == id) {
                        let result = self.workspace.reopen_at(index);
                        if result.is_ok() {
                            if let Some((page, cursor)) = location {
                                self.reveal_markdown_source(ctx);
                                let buffer = &mut self.workspace.open[self.workspace.active];
                                buffer.set_page(page);
                                buffer.view.anchor = cursor;
                                buffer.view.head = cursor;
                                buffer.reveal_cursor = true;
                            }
                            self.finish_preview();
                            self.navigation_mark();
                        } else {
                            self.quality.preview_request = false;
                        }
                        self.report(result);
                    } else {
                        self.quality.preview_request = false;
                    }
                }
                Ok(Action::Folder(listing)) => self.set_folder(listing),
                Ok(Action::Folders(listings)) => {
                    for listing in listings {
                        self.set_folder(listing);
                    }
                }
                Ok(Action::Created(path, directory)) => {
                    self.refresh_folder();
                    if directory {
                        self.project.expanded.insert(path);
                        self.sync_folder_view();
                    } else {
                        self.pending_paths.push_back(path.into());
                    }
                }
                Ok(Action::Add(doc, page, cursor)) => {
                    let result = self.workspace.add(doc, page);
                    if result.is_ok() {
                        let buffer = &mut self.workspace.open[self.workspace.active];
                        buffer.view.anchor = cursor;
                        buffer.view.head = cursor;
                        buffer.reveal_cursor = cursor > 0;
                    }
                    self.report(result);
                    self.apply_format_preferences();
                    self.finish_preview();
                    self.navigation_mark();
                }
                Ok(Action::History(id, page, redo, current)) => {
                    if let Some(index) = self
                        .workspace
                        .open
                        .iter()
                        .position(|b| b.page.info.id == id)
                    {
                        self.workspace.active = index;
                        let buffer = &mut self.workspace.open[index];
                        let selection = if redo {
                            buffer.cursor_redo.pop()
                        } else {
                            buffer.cursor_undo.pop()
                        };
                        if selection.is_some() {
                            if redo {
                                buffer.cursor_undo.push(current);
                            } else {
                                buffer.cursor_redo.push(current);
                            }
                        }
                        buffer.set_page(page);
                        buffer.external = false;
                        if let Some(selection) = selection {
                            self.set_selections(selection);
                        } else {
                            let b = &mut self.workspace.open[index];
                            b.view.anchor = 0;
                            b.view.head = 0;
                        }
                        self.focus_editor = true;
                        self.changed();
                    }
                }
                Ok(Action::Page(id, page, selection) | Action::Reloaded(id, page, selection)) => {
                    if let Some(index) = self
                        .workspace
                        .open
                        .iter()
                        .position(|b| b.page.info.id == id)
                    {
                        self.workspace.active = index;
                        if !reloaded && selection.is_some() {
                            self.reveal_markdown_source(ctx);
                        }
                        let buffer = &mut self.workspace.open[index];
                        if reloaded {
                            buffer.cursor_undo.clear();
                            buffer.cursor_redo.clear();
                            buffer.pending_selection = None;
                            buffer.changes.clear();
                        }
                        buffer.set_page(page);
                        buffer.external = false;
                        if let Some((anchor, head)) = selection {
                            buffer.view.anchor = anchor;
                            buffer.view.head = head;
                            buffer.reveal_cursor = true;
                        } else {
                            buffer.view.anchor = 0;
                            buffer.view.head = 0;
                            buffer.view.scroll_top = 0.0;
                        }
                        self.changed();
                        // Stepping through matches keeps typing in the Find bar.
                        self.focus_editor = !self.find_input_focused(ctx);
                        self.navigation_mark();
                    }
                }
                Ok(Action::SaveTo(id, path)) => {
                    self.save_to_path(ctx, id, Some(path));
                    // Formatting can fail before a save worker starts, after
                    // the native Save As dialog has already returned a path.
                    if !self.busy {
                        self.cancel_close();
                    }
                }
                Ok(Action::SaveCancelled) => {
                    // Cancel a pending tab closure, but let an app shutdown
                    // preserve the unsaved buffer through session recovery.
                    if self.close_flow.is_some() {
                        self.cancel_close();
                    }
                    self.focus_editor = !self.closing;
                }
                Ok(Action::Discarded(id, page)) => {
                    if let Some(index) = self.workspace.open.iter().position(|b| b.view.id == id) {
                        if let Some(page) = page {
                            let buffer = &mut self.workspace.open[index];
                            let mut view = buffer.view.clone();
                            view.page_start = page.start;
                            let count = page.text.chars().count();
                            view.anchor = view.anchor.min(count);
                            view.head = view.head.min(count);
                            *buffer = Buffer::new(page, Some(view));
                        } else if let Err(error) = self.workspace.forget(index) {
                            self.error = Some(error);
                            self.cancel_close();
                        }
                    }
                    self.changed();
                }
                Ok(Action::Saved(id, info)) => {
                    // Compare resolved paths: the config folder may be reached
                    // through a symlink, such as /var → /private/var on macOS.
                    let resolve = |p: PathBuf| std::fs::canonicalize(&p).unwrap_or(p);
                    let settings_file = info.path.as_deref().map(|p| resolve(PathBuf::from(p)))
                        == self.settings_path().map(resolve);
                    let new_path = self
                        .workspace
                        .open
                        .iter()
                        .find(|b| b.view.id == id)
                        .is_some_and(|b| b.page.info.path != info.path);
                    if let Some(buffer) = self
                        .workspace
                        .open
                        .iter_mut()
                        .find(|b| b.page.info.id == id)
                    {
                        buffer.view.name = info.name.clone();
                        buffer.page.info = info;
                        buffer.refresh_name();
                        buffer.external = false;
                    }
                    self.refresh_folder();
                    if new_path && self.active().view.id == id {
                        self.apply_format_preferences();
                    }
                    self.notice = "File saved.".into();
                    self.changed();
                    if settings_file {
                        self.reload_settings(ctx);
                    }
                    self.build_after_save(ctx);
                }
                Ok(Action::ThemeImported(theme)) => self.add_imported_theme(ctx, theme),
                Ok(Action::Font(name, bytes, index)) => {
                    if self.settings.font_face == name {
                        self.editor_font = Some((bytes, index));
                        settings_store::set_editor_font(ctx, self.editor_font.clone());
                    }
                }
                Ok(Action::FontFamilies(families)) => {
                    self.font_families = Some(families);
                    self.fonts_listing = false;
                    if std::mem::take(&mut self.font_picker_pending) {
                        self.open_font_picker(ctx);
                    }
                }
                Ok(Action::FallbackFonts) => {
                    settings_store::set_editor_font(ctx, self.editor_font.clone());
                }
                Ok(Action::OpenAt(path)) => self.pending_paths.push_back(path),
                Ok(Action::OpenFailed(error)) => {
                    self.quality.preview_request = false;
                    if self.quality.incoming.is_some() {
                        self.quality.incoming_error = Some(error.clone());
                    }
                    self.transient_error = Some((error.clone(), Instant::now()));
                    self.error = Some(error);
                    self.cancel_close();
                }
                Ok(Action::DebugProgram(path)) => {
                    self.debug_program = path.to_string_lossy().into_owned();
                }
                Ok(Action::Notice(message)) => self.notice = message,
                Ok(Action::Nothing) => (),
                Err(error)
                    if matches!(self.quality.folder_state, folder_search::State::Searching) =>
                {
                    self.quality.folder_state = if error == "Search cancelled." {
                        folder_search::State::Cancelled
                    } else {
                        folder_search::State::Failed(error)
                    };
                }
                Err(error) if ROUTINE.contains(&error.as_str()) => {
                    self.quality.match_status = error.trim_end_matches('.').into();
                    self.notice = error;
                }
                Err(error) => {
                    if self.quality.incoming.is_some() {
                        self.quality.incoming_error = Some(error.clone());
                    }
                    self.error = Some(error);
                    self.cancel_close();
                }
            }
            if !side && !self.busy {
                self.finish_markdown_link(ctx, opened);
            }
        }
        while let Ok(checkpoint) = self.checkpoint_rx.try_recv() {
            if let Some(worker) = self.checkpoint_worker.take() {
                let _ = worker.join();
            }
            self.checkpoint_pending = false;
            match checkpoint.result {
                Ok(()) => {
                    self.persisted = checkpoint.generation;
                    self.last_saved = Some(Instant::now());
                }
                Err(error) => {
                    self.error = Some(format!("{CHECKPOINT_FAILED}\n{error}\nKeep hedit open and save your files to disk."));
                    self.cancel_close();
                }
            }
        }
        self.expire_error(Instant::now());
        self.advance_close();
        if !self.busy && !self.closing {
            if let Some(path) = self.pending_paths.pop_front() {
                self.open(ctx, Some(path));
            }
        }
        if self.closing
            && self.close_flow.is_none()
            && !self.busy
            && !self.checkpoint_pending
            && !self.updates.installing()
        {
            if self.generation == self.persisted {
                self.exit_ready = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                self.checkpoint(ctx);
            }
        } else if self.generation != self.persisted
            && self.checkpoint_at.elapsed() >= Duration::from_secs(2)
        {
            self.checkpoint(ctx);
        }
    }
    fn expire_error(&mut self, now: Instant) {
        if let Some((message, shown)) = &self.transient_error {
            if self.error.as_ref() != Some(message) {
                self.transient_error = None;
            } else if now.saturating_duration_since(*shown) >= FILE_ERROR_TIME {
                self.error = None;
                self.transient_error = None;
            }
        }
    }
    /// When the next frame is needed without input: soon while work is in
    /// flight, at the next checkpoint or git refresh, or never when idle.
    fn next_wake(&self, ctx: &egui::Context) -> Option<Duration> {
        let working = self.busy
            || (self.closing && self.close_flow.is_none())
            || self.checkpoint_pending
            || self.updates.active()
            || self.quality.incoming.is_some()
            || !self.pending_paths.is_empty()
            || self
                .debug
                .as_ref()
                .is_some_and(|d| d.state != State::Exited);
        if working {
            return Some(Duration::from_millis(50));
        }
        let due = |since: Instant, every: Duration| every.saturating_sub(since.elapsed());
        [
            self.transient_error
                .as_ref()
                .map(|(_, shown)| due(*shown, FILE_ERROR_TIME)),
            (self.generation != self.persisted)
                .then(|| due(self.checkpoint_at, Duration::from_secs(2))),
            // Followed logs and files on disk are checked once a second.
            self.active()
                .follow
                .then(|| due(self.poll_at, Duration::from_secs(1))),
            self.settings_due(),
            self.wait_due(),
            (!self.quality.git_pending && self.git_stale(ctx)).then(|| {
                self.quality
                    .git_at
                    .map_or(Duration::ZERO, |at| due(at, Duration::from_secs(2)))
            }),
        ]
        .into_iter()
        .flatten()
        .min()
    }
    fn poll_external(&mut self, ctx: &egui::Context) {
        if self.busy
            || (self.poll_at.elapsed() < Duration::from_secs(1) && !quality::refocused(ctx))
        {
            return;
        }
        self.poll_at = Instant::now();
        let id = self.active().page.info.id;
        let changed = self
            .workspace
            .editor
            .get(id)
            .ok()
            .and_then(|d| d.try_lock().ok().map(|d| d.external_change().0))
            .unwrap_or(false);
        if changed {
            let buffer = &mut self.workspace.open[self.workspace.active];
            buffer.external = true;
            if !buffer.page.info.dirty {
                let offset = if buffer.follow {
                    u64::MAX
                } else {
                    buffer.page.start
                };
                self.load_page(ctx, offset, true);
            }
        }
    }
    /// Keyboard layouts such as Arabic on GNOME commit each letter through the
    /// input method without composing first. egui only accepts a commit at the
    /// caret where composition began, so after the first letter it dropped
    /// them all; type such commits as plain text instead.
    fn plain_ime_commits(&mut self, ctx: &egui::Context) {
        ctx.input_mut(|input| {
            for event in &mut input.events {
                let egui::Event::Ime(ime) = event else {
                    continue;
                };
                match ime {
                    egui::ImeEvent::Preedit(text) => self.ime_composing = !text.is_empty(),
                    egui::ImeEvent::Commit(text) => {
                        if !std::mem::take(&mut self.ime_composing)
                            && !text.is_empty()
                            && text != "\n"
                            && text != "\r"
                        {
                            *event = egui::Event::Text(std::mem::take(text));
                        }
                    }
                    egui::ImeEvent::Disabled => self.ime_composing = false,
                    egui::ImeEvent::Enabled => {}
                }
            }
        });
    }
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.close_flow.is_some() {
            return;
        }
        self.play_step(ctx);
        self.plain_ime_commits(ctx);
        self.record_input(ctx);
        if self.capture_binding(ctx) {
            return;
        }
        self.track_selection();
        self.track_recent_tabs(ctx);
        self.track_recent_file();
        self.sync_snippet();
        self.trim_auto_indent();
        self.update_completion(ctx);
        self.update_spelling(ctx);
        if !self.busy && !self.closing && self.handle_chords(ctx) {
            return;
        }
        // Escape still closes windows while a background job runs.
        self.escape(ctx);
        if self.busy || self.closing {
            return;
        }
        let cmd = Modifiers::COMMAND;
        let shift = Modifiers { shift: true, ..cmd };
        let take = |m, k| ctx.input_mut(|i| i.consume_key(m, k));
        // Layouts as in Sublime: Alt+Shift+N on Linux, Cmd+Option+N on macOS.
        let layout = if cfg!(target_os = "macos") {
            Modifiers { alt: true, ..cmd }
        } else {
            Modifiers {
                alt: true,
                shift: true,
                ..Modifiers::NONE
            }
        };
        let rows = Modifiers {
            shift: true,
            ..layout
        };
        for (modifiers, key, command) in [
            (layout, Key::Num1, Command::SinglePane),
            (layout, Key::Num2, Command::SplitColumns),
            (layout, Key::Num3, Command::Columns3),
            (layout, Key::Num4, Command::Columns4),
            (layout, Key::Num5, Command::Grid),
            (
                if cfg!(target_os = "macos") {
                    rows
                } else {
                    layout
                },
                if cfg!(target_os = "macos") {
                    Key::Num2
                } else {
                    Key::Num8
                },
                Command::SplitRows,
            ),
            (
                if cfg!(target_os = "macos") {
                    rows
                } else {
                    layout
                },
                if cfg!(target_os = "macos") {
                    Key::Num3
                } else {
                    Key::Num9
                },
                Command::Rows3,
            ),
        ] {
            // Exact modifiers, so Alt+Shift+2 is not taken for Alt+2.
            let pressed = ctx.input_mut(|i| {
                let found = i.events.iter().position(|e| {
                    matches!(e, egui::Event::Key { key: k, pressed: true, modifiers: m, .. } if *k == key && m.matches_exact(modifiers))
                });
                found.map(|n| i.events.remove(n)).is_some()
            });
            if pressed && !self.overridden(command) {
                self.execute(ctx, command);
            }
        }
        // Focus a group with Ctrl+N (Control+N on macOS, where Cmd+N is New).
        let focus = if cfg!(target_os = "macos") {
            Modifiers::CTRL
        } else {
            cmd
        };
        // macOS's Control+Cmd+F shortcuts first, since a Cmd+F or Cmd+Shift+F
        // pattern also matches them.
        if cfg!(target_os = "macos") {
            let control = Modifiers { ctrl: true, ..cmd };
            for (modifiers, command) in [
                (
                    Modifiers {
                        shift: true,
                        ..control
                    },
                    Command::DistractionFree,
                ),
                (control, Command::Fullscreen),
                (Modifiers { alt: true, ..cmd }, Command::Replace),
            ] {
                if !self.overridden(command) && take(modifiers, Key::F) {
                    self.execute(ctx, command);
                }
            }
            if !self.overridden(Command::AllOccurrences) && take(control, Key::G) {
                self.execute(ctx, Command::AllOccurrences);
            }
        }
        self.tab_shortcuts(ctx);
        // Before Ctrl+F2 and F2, which would also take these.
        for (modifiers, command) in [
            (
                Modifiers::COMMAND | Modifiers::SHIFT,
                Command::ClearBookmarks,
            ),
            (Modifiers::ALT, Command::SelectBookmarks),
        ] {
            if !self.overridden(command) && tab_switch::take_exact(ctx, modifiers, Key::F2) {
                self.execute(ctx, command);
            }
        }
        let (record, play) = macros::macro_keys();
        for (modifiers, command) in [(play, Command::PlayMacro), (record, Command::RecordMacro)] {
            if !self.overridden(command) && tab_switch::take_exact(ctx, modifiers, Key::Q) {
                self.execute(ctx, command);
            }
        }
        // Alt+Enter selects every match, from the Find field or the editor.
        if !self.query.is_empty()
            && !self.overridden(Command::SelectAllMatches)
            && tab_switch::take_exact(ctx, Modifiers::ALT, Key::Enter)
        {
            self.execute(ctx, Command::SelectAllMatches);
        }
        self.quality_shortcuts(ctx);
        self.filter_overridden_clipboard(ctx);
        if self.overridden(Command::SelectAll) {
            let _ = take(cmd, Key::A);
        }
        for (modifiers, key, command) in [
            (shift, Key::P, Command::Palette),
            (cmd, Key::P, Command::Switcher),
            (
                Modifiers { alt: true, ..cmd },
                Key::B,
                Command::FolderSidebar,
            ),
            (
                Modifiers { alt: true, ..shift },
                Key::ArrowRight,
                Command::MovePane,
            ),
            (
                Modifiers { alt: true, ..cmd },
                Key::ArrowRight,
                Command::FocusPane,
            ),
            (shift, Key::B, Command::BuildWith),
            (cmd, Key::B, Command::Build),
            (Modifiers::SHIFT, Key::F4, Command::PreviousResult),
            (Modifiers::NONE, Key::F4, Command::NextResult),
            (shift, Key::N, Command::NewWindow),
            (cmd, Key::N, Command::New),
            (shift, Key::O, Command::OpenFolder),
            (cmd, Key::O, Command::Open),
            (shift, Key::S, Command::SaveAs),
            (cmd, Key::S, Command::Save),
            (shift, Key::T, Command::Reopen),
            (cmd, Key::W, Command::Close),
            (cmd, Key::F2, Command::Bookmark),
            (Modifiers::SHIFT, Key::F2, Command::PreviousBookmark),
            (Modifiers::NONE, Key::F2, Command::NextBookmark),
            (shift, Key::R, Command::Rename),
            (cmd, Key::Comma, Command::Settings),
            (cmd, Key::F, Command::Find),
            (focus, Key::Num1, Command::FocusGroup1),
            (focus, Key::Num2, Command::FocusGroup2),
            (focus, Key::Num3, Command::FocusGroup3),
            (focus, Key::Num4, Command::FocusGroup4),
            (cmd, Key::Num0, Command::ResetZoom),
            (cmd, Key::G, Command::Goto),
            (shift, Key::W, Command::Wrap),
        ] {
            if !self.overridden(command) && take(modifiers, key) {
                self.execute(ctx, command);
            }
        }
        // Exactly Ctrl+Q, so Cmd+Option+Q (Wrap paragraph on macOS) does not quit.
        if !self.overridden(Command::Quit) && tab_switch::take_exact(ctx, cmd, Key::Q) {
            self.execute(ctx, Command::Quit);
        }
        if !cfg!(target_os = "macos") {
            if !self.overridden(Command::Replace) && take(cmd, Key::H) {
                self.execute(ctx, Command::Replace);
            }
            if !self.overridden(Command::DistractionFree) && take(Modifiers::SHIFT, Key::F11) {
                self.execute(ctx, Command::DistractionFree);
            } else if !self.overridden(Command::Fullscreen) && take(Modifiers::NONE, Key::F11) {
                self.execute(ctx, Command::Fullscreen);
            }
        }
        // Let single-line search and debugger inputs keep their own undo history.
        let mut editor_id = egui::Id::new((
            "editor",
            self.active().page.info.id,
            self.active().page.start,
        ));
        if self.panes.mode != 0 && self.panes.focused > 0 {
            editor_id = editor_id.with(self.panes.focused);
        }
        if self.focus_editor
            && self.panes.mode != 0
            && !self.markdown_preview_only()
            && !self.preferences_open
            && !self.palette
            && !self.find_focus
            && !self.goto_focus
            && !ctx.memory(|m| m.has_focus(editor_id))
        {
            ctx.memory_mut(|m| m.request_focus(editor_id));
        }
        if !self.markdown_preview_only() && ctx.memory(|m| m.has_focus(editor_id)) {
            self.completion_keys(ctx);
            self.clipboard_events(ctx);
            let line_cursor = if cfg!(target_os = "macos") {
                Modifiers::CTRL | Modifiers::SHIFT
            } else {
                Modifiers::ALT | Modifiers::SHIFT
            };
            let mac = cfg!(target_os = "macos");
            let shifted = Modifiers::COMMAND | Modifiers::SHIFT;
            // Cmd+Up goes to the top of the file on macOS.
            let scroll = if mac {
                Modifiers::CTRL | Modifiers::ALT
            } else {
                Modifiers::COMMAND
            };
            for (modifiers, command) in [
                (shifted, Command::PreviousMisspelling),
                (Modifiers::COMMAND, Command::NextMisspelling),
                (Modifiers::NONE, Command::ToggleSpellCheck),
            ] {
                if !self.overridden(command) && tab_switch::take_exact(ctx, modifiers, Key::F6) {
                    self.execute(ctx, command);
                }
            }
            let command_alt = Modifiers::COMMAND | Modifiers::ALT;
            let mut exact = vec![
                (line_cursor, Key::ArrowUp, Command::AddCursorAbove),
                (line_cursor, Key::ArrowDown, Command::AddCursorBelow),
                (Modifiers::COMMAND, Key::F9, Command::SortLines),
                (scroll, Key::ArrowUp, Command::ScrollLineUp),
                (scroll, Key::ArrowDown, Command::ScrollLineDown),
                (shifted, Key::F3, Command::QuickFindPrevious),
                (Modifiers::COMMAND, Key::F3, Command::QuickFindNext),
                (shifted, Key::E, Command::UseSelectionForReplace),
                (Modifiers::COMMAND, Key::E, Command::UseSelectionForFind),
                (Modifiers::SHIFT, Key::F12, Command::GotoReferences),
                (Modifiers::NONE, Key::F12, Command::GotoDefinition),
                (Modifiers::NONE, Key::F9, Command::SortLinesIgnoreCase),
            ];
            if mac {
                exact.extend([
                    (command_alt, Key::Slash, Command::ToggleBlockComment),
                    (command_alt, Key::Q, Command::WrapLines),
                    (Modifiers::CTRL, Key::T, Command::Transpose),
                ]);
            } else {
                exact.extend([
                    // Shift+/ arrives as ? on many layouts.
                    (shifted, Key::Slash, Command::ToggleBlockComment),
                    (shifted, Key::Questionmark, Command::ToggleBlockComment),
                    (Modifiers::ALT, Key::Q, Command::WrapLines),
                    (Modifiers::COMMAND, Key::T, Command::Transpose),
                    (shifted, Key::Backspace, Command::DeleteToLineStart),
                    (shifted, Key::Delete, Command::DeleteToLineEnd),
                ]);
            }
            for (modifiers, key, command) in exact {
                if !self.overridden(command) && tab_switch::take_exact(ctx, modifiers, key) {
                    self.execute(ctx, command);
                }
            }
            for (modifiers, key, command) in [
                (shift, Key::D, Command::Duplicate),
                (cmd, Key::D, Command::NextOccurrence),
                (shift, Key::L, Command::SplitLines),
                (Modifiers::ALT, Key::F3, Command::AllOccurrences),
                (shift, Key::Space, Command::ExpandSelection),
                (shift, Key::M, Command::ExpandToBrackets),
                (shift, Key::J, Command::ExpandToIndentation),
                (shift, Key::U, Command::SoftRedo),
                (cmd, Key::U, Command::SoftUndo),
                (shift, Key::OpenBracket, Command::Fold),
                (shift, Key::CloseBracket, Command::Unfold),
                (shift, Key::K, Command::DeleteLine),
                (shift, Key::Enter, Command::InsertBefore),
                (cmd, Key::Enter, Command::InsertAfter),
                (shift, Key::ArrowUp, Command::MoveUp),
                (shift, Key::ArrowDown, Command::MoveDown),
                (cmd, Key::L, Command::SelectLine),
                (cmd, Key::J, Command::JoinLines),
                (
                    // Cmd+M minimizes on macOS; Sublime uses Control+M there.
                    if cfg!(target_os = "macos") {
                        Modifiers::CTRL
                    } else {
                        cmd
                    },
                    Key::M,
                    Command::JumpToBracket,
                ),
                (cmd, Key::Slash, Command::ToggleComment),
                (cmd, Key::CloseBracket, Command::Indent),
                (cmd, Key::OpenBracket, Command::Unindent),
            ] {
                if !self.overridden(command) && take(modifiers, key) {
                    self.execute(ctx, command);
                }
            }
            if take(Modifiers::SHIFT, Key::Tab) {
                if !self.next_field(true) {
                    self.execute(ctx, Command::Unindent);
                }
            } else if take(Modifiers::NONE, Key::Tab) && !self.next_field(false) {
                self.insert_indentation();
            }
            self.advanced_input(ctx);
            if !self.settings.auto_indent
                && self.active().page.info.eol == "CRLF"
                && take(Modifiers::NONE, Key::Enter)
            {
                self.insert_crlf();
            }
            if !self.overridden(Command::Redo) && (take(shift, Key::Z) || take(cmd, Key::Y)) {
                self.execute(ctx, Command::Redo);
            } else if !self.overridden(Command::Undo) && take(cmd, Key::Z) {
                self.execute(ctx, Command::Undo);
            }
        }
        if !self.overridden(Command::ZoomIn) && (take(cmd, Key::Equals) || take(cmd, Key::Plus)) {
            self.settings.font_size = (self.settings.font_size + 1.0).min(28.0);
        }
        if !self.overridden(Command::ZoomOut) && take(cmd, Key::Minus) {
            self.settings.font_size = (self.settings.font_size - 1.0).max(10.0);
        }
    }
    fn editor(&mut self, ctx: &egui::Context) {
        self.markdown.retain_documents(ctx, &self.workspace.open);
        self.apply_editor_style(ctx);
        let theme = self.theme.clone();
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme.background.color())
                    .inner_margin(egui::Margin::symmetric(4, 6)),
            )
            .show(ctx, |ui| {
                self.error_banner(ui, &theme);
                self.pane_layout(ui);
            });
    }
    /// One banner above all panes; Escape or its close button clears it.
    fn error_banner(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let Some(error) = self.error.clone() else {
            return;
        };
        let ctx = ui.ctx().clone();
        egui::Frame::new()
            .fill(theme.error.color().gamma_multiply(0.12))
            .inner_margin(10.0)
            .show(ui, |ui| {
                let retry = error.starts_with(CHECKPOINT_FAILED);
                ui.horizontal_top(|ui| {
                    // Leave room on the right for the buttons; the message wraps.
                    let buttons = if retry { 150.0 } else { 0.0 } + 28.0;
                    ui.allocate_ui(
                        egui::vec2((ui.available_width() - buttons).max(120.0), 0.0),
                        |ui| {
                            ui.add(
                                egui::Label::new(RichText::new(&error).color(theme.error.color()))
                                    .wrap(),
                            )
                        },
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        if controls::close_button(ui, "Dismiss · Esc").clicked() {
                            self.error = None;
                            self.transient_error = None;
                        }
                        if retry && ui.button("Retry checkpoint").clicked() {
                            self.checkpoint(&ctx);
                        }
                    });
                });
            });
        ui.add_space(6.0);
    }
    fn editor_pane(&mut self, ui: &mut egui::Ui, pane_id: usize) {
        self.markdown_pane(ui, pane_id);
    }
    fn editor_source_pane(&mut self, ui: &mut egui::Ui, pane_id: usize) {
        if self.distraction_free {
            let mut rect = ui.available_rect_before_wrap();
            let width = ui
                .fonts(|f| f.glyph_width(&FontId::monospace(self.settings.font_size), ' '))
                * 100.0
                + 60.0;
            if rect.width() > width {
                let margin = (rect.width() - width) / 2.0;
                rect.min.x += margin;
                rect.max.x -= margin;
            }
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                self.editor_surface(ui, pane_id)
            });
        } else {
            self.editor_surface(ui, pane_id);
        }
    }
    fn editor_surface(&mut self, ui: &mut egui::Ui, pane_id: usize) {
        let context = ui.ctx().clone();
        let ctx = &context;
        let theme = self.theme.clone();
        let markdown_split = self.markdown_available()
            && self.active().view.markdown_view == hedit::session::MarkdownView::Split;
        let find_matches = self.find_highlights();
        let word_matches = self.selection_highlights();

        if self.active().external {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(
                    theme.warning.color(),
                    "File changed outside hedit. Your open text has been kept.",
                );
                if ui
                    .add_enabled(
                        !self.busy && !self.active().page.info.dirty,
                        egui::Button::new("Reload file"),
                    )
                    .clicked()
                {
                    self.load_page(ctx, self.active().page.start, true);
                }
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Compare changes…"))
                    .clicked()
                {
                    self.compare_disk(ctx, false);
                }
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Save file as…"))
                    .clicked()
                {
                    self.save_file(ctx, true);
                }
            });
        }
        if self.active().page.info.paged {
            ui.add_enabled_ui(!self.busy && !self.closing, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("|‹ Start").clicked() {
                        self.load_page(ctx, 0, false);
                    }
                    if ui.button("‹ Previous").clicked() {
                        self.load_page(
                            ctx,
                            self.active().page.start.saturating_sub(PAGE_SIZE),
                            false,
                        );
                    }
                    if ui.button("Next ›").clicked() {
                        self.load_page(ctx, self.active().page.end, false);
                    }
                    if ui.button("End ›|").clicked() {
                        self.load_page(
                            ctx,
                            self.active().page.info.size.saturating_sub(PAGE_SIZE),
                            false,
                        );
                    }
                    let page = &self.active().page;
                    ui.label(format!(
                        "Bytes {}–{} of {}",
                        page.start, page.end, page.info.size
                    ));
                })
            });
        }
        // The editor's right-click menu, worked out before borrowing the buffer.
        let context_menu: Vec<Vec<(Command, &str, String, bool)>> = [
            &[
                Command::Cut,
                Command::Copy,
                Command::Paste,
                Command::PasteHistory,
            ][..],
            &[Command::SelectAll, Command::ExpandSelection][..],
            &[Command::GotoDefinition, Command::GotoReferences][..],
            &[Command::ToggleComment, Command::ToggleBlockComment][..],
            &[Command::CopyPath, Command::RevealInSidebar][..],
        ]
        .iter()
        .map(|group| {
            group
                .iter()
                .map(|&c| (c, c.label(), self.shortcut_text(c), self.command_enabled(c)))
                .collect()
        })
        .collect();
        let mut context_command = None;
        let mut spell_choice = None;
        let active_view = self.active().view.id;
        let misspelled: Vec<std::ops::Range<usize>> = if self.settings.spell_check {
            self.misspellings(active_view)
                .map(<[_]>::to_vec)
                .unwrap_or_default()
        } else {
            vec![]
        };
        let mut word_menu_at = None;
        let index = self.workspace.active;
        let buffer = &mut self.workspace.open[index];
        let base_id = egui::Id::new(("editor", buffer.page.info.id, buffer.page.start));
        let id = if pane_id == 0 {
            base_id
        } else {
            base_id.with(pane_id)
        };
        let count = buffer.page.text.chars().count();
        buffer.view.anchor = buffer.view.anchor.min(count);
        buffer.view.head = buffer.view.head.min(count);
        buffer.view.folds.retain(|f| {
            f.end <= count && !(f.start < buffer.view.head && buffer.view.head < f.end)
        });
        let projection = self.projections[pane_id].get(
            &buffer.page.text,
            buffer.view.tab_size.unwrap_or(4),
            &buffer.view.folds,
        );
        let mut display = projection.text.clone();
        let original_selection = hedit::advanced::Selection {
            anchor: buffer.view.anchor,
            head: buffer.view.head,
        };
        // Lines with right-to-left text are drawn by hedit; see bidi_view.
        let rtl_layout = if pane_id == 0 {
            &self.editor_layout
        } else {
            &self.extra_layouts[pane_id - 1]
        }
        .last()
        .filter(|(galley, bidi)| bidi.any() && galley.text() == projection.text);
        if let Some((galley, bidi)) = &rtl_layout {
            // Left and Right move across right-to-left text as it appears.
            let display_head = projection.display(buffer.view.head);
            let step = ctx.input(|i| {
                i.events.iter().find_map(|event| match event {
                    egui::Event::Key {
                        key: key @ (Key::ArrowLeft | Key::ArrowRight),
                        pressed: true,
                        modifiers,
                        ..
                    } if !modifiers.ctrl && !modifiers.alt && !modifiers.command => {
                        Some((*key, *modifiers))
                    }
                    _ => None,
                })
            });
            if let Some((key, modifiers)) = step.filter(|_| ctx.memory(|m| m.has_focus(id))) {
                if let Some(next) =
                    bidi_view::visual_step(galley, bidi, display_head, key == Key::ArrowRight)
                {
                    ctx.input_mut(|i| i.consume_key(modifiers, key));
                    buffer.view.head = projection.source(next);
                    if !modifiers.shift {
                        buffer.view.anchor = buffer.view.head;
                    }
                }
            }
        }
        let rtl_text = rtl_layout.is_some();
        {
            let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
            state.clear_undoer();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(projection.display(buffer.view.anchor)),
                    egui::text::CCursor::new(projection.display(buffer.view.head)),
                )));
            state.store(ctx, id);
        }
        if !ctx.input(|i| i.pointer.primary_down()) {
            self.rectangle_start = None;
        }
        if ctx.input(|i| {
            i.pointer.primary_pressed()
                && i.modifiers.alt
                && i.modifiers.shift
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| ui.clip_rect().contains(p))
        }) {
            self.rectangle_start = Some((pane_id, original_selection.head));
        }
        let language = buffer.language();
        let font_size = self.settings.font_size;
        let wrap = self.settings.word_wrap || self.distraction_free;
        let column_width = ui.fonts(|fonts| fonts.glyph_width(&FontId::monospace(font_size), ' '));
        let wrap_limit = if self.settings.wrap_width == 0 {
            if self.distraction_free {
                column_width * 100.0
            } else {
                f32::INFINITY
            }
        } else {
            column_width * f32::from(self.settings.wrap_width)
        };
        // Each group keeps its own layout and highlighting.
        let layout_cache = if pane_id == 0 {
            &mut self.editor_layout
        } else {
            &mut self.extra_layouts[pane_id - 1]
        };
        let row_height = ui.fonts(|f| f.row_height(&FontId::monospace(font_size)));
        let line_height =
            (self.settings.line_padding > 0.0).then_some(row_height + self.settings.line_padding);
        // Like Sublime, the last line can scroll up to the top: the editor is a
        // window's height taller than its text, so clicks there place the cursor.
        let rows = if self.settings.scroll_past_end && !buffer.follow {
            let lines = memchr::memchr_iter(b'\n', buffer.page.text.as_bytes()).count() + 1;
            let visible = (ui.available_height() / line_height.unwrap_or(row_height)) as usize;
            lines + visible.saturating_sub(2)
        } else {
            30
        };
        // Row directions of the latest layout, for drawing right-to-left text.
        let latest_bidi = std::cell::RefCell::new(Arc::default());
        let mut layouter = |ui: &egui::Ui, text: &str, wrap_width: f32| {
            let galley = layout_cache.layout(
                ui,
                text,
                language,
                editor_layout::Font {
                    size: font_size,
                    line_height,
                },
                if wrap {
                    wrap_width.min(wrap_limit)
                } else {
                    f32::INFINITY
                },
                &theme,
            );
            if let Some((_, bidi)) = layout_cache.last() {
                *latest_bidi.borrow_mut() = bidi;
            }
            galley
        };
        let markdown_navigation = buffer.reveal_cursor
            || self
                .scroll_request
                .is_some_and(|(view, _)| view == buffer.view.id)
            || ctx.memory(|m| m.has_focus(id))
                && ctx.input(|input| {
                    input.events.iter().any(|event| {
                        matches!(
                            event,
                            egui::Event::Key { pressed: true, .. }
                                | egui::Event::Text(_)
                                | egui::Event::Ime(egui::ImeEvent::Commit(_))
                        )
                    })
                });
        let mut scroll = egui::ScrollArea::both()
            .id_salt((
                "page-scroll",
                buffer.page.info.id,
                buffer.page.start,
                pane_id,
            ))
            .auto_shrink([false, false])
            .stick_to_bottom(buffer.follow);
        if buffer.restore_cursor {
            scroll = scroll
                .vertical_scroll_offset(buffer.view.scroll_top as f32)
                .horizontal_scroll_offset(buffer.view.scroll_left as f32);
        } else if let Some(top) = self
            .scroll_request
            .take_if(|(view, _)| *view == buffer.view.id)
        {
            // Scrolling alone leaves the cursor where it is, even off screen.
            scroll = scroll.vertical_scroll_offset(top.1 as f32);
        }
        if markdown_split {
            if let Some(top) = self.markdown.scroll.source_offset(buffer.view.id, pane_id) {
                if !markdown_navigation {
                    scroll = scroll.vertical_scroll_offset(top);
                }
            }
        }
        let mut markdown_layout = None;
        let restore = buffer.restore_cursor;
        let gutter = if self.settings.line_numbers {
            let digits = (memchr::memchr_iter(b'\n', buffer.page.text.as_bytes()).count() + 1)
                .to_string()
                .len();
            (digits.max(3) as f32 * font_size * 0.65 + 16.0).max(42.0)
        } else {
            0.0
        };
        let gutter_color = self.theme.muted.color();
        let gutter_background = self.theme.background.color();
        let gutter_text = self.theme.text.color();
        let head_byte = hedit::advanced::byte(&buffer.page.text, buffer.view.head);
        let current_line =
            memchr::memchr_iter(b'\n', &buffer.page.text.as_bytes()[..head_byte]).count() + 1;
        let map_area = ui.available_rect_before_wrap();
        let mut editor_area = map_area;
        if self.settings.show_minimap && !self.distraction_free && map_area.width() > 300.0 {
            editor_area.max.x -= 76.0;
        }
        // Fold arrows: open blocks while the pointer is over the gutter, and
        // folded blocks always.
        let gutter_hovered = ctx
            .pointer_hover_pos()
            .is_some_and(|p| editor_area.contains(p) && p.x < editor_area.left() + gutter + 4.0);
        let fold_heads = if gutter > 0.0 && !buffer.page.info.paged && gutter_hovered {
            self.fold_cache.heads(
                buffer.view.id,
                buffer.page.info.revision,
                buffer.view.tab_size.unwrap_or(4) as usize,
                &buffer.page.text,
            )
        } else {
            Arc::default()
        };
        let folded_heads = if gutter > 0.0 && !buffer.view.folds.is_empty() {
            self.fold_cache.folded(
                buffer.view.id,
                buffer.page.info.revision,
                &buffer.view.folds,
                &buffer.page.text,
            )
        } else {
            Arc::default()
        };
        let mut fold_click: Option<(hedit::advanced::Fold, bool)> = None;
        // Move the text out for this frame instead of copying a page of up to
        // 2 MiB; it goes back, possibly edited, right after the editor is drawn.
        let old = std::mem::take(&mut buffer.page.text);
        let mut edited_text: Option<String> = None;
        let scroll_output = ui
            .scope_builder(egui::UiBuilder::new().max_rect(editor_area), |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(editor_area));
                scroll.show(ui, |ui| {
                    let content_top = ui.max_rect().top();
                    let gutter_rect = egui::Rect::from_min_max(
                        ui.clip_rect().min,
                        egui::pos2(ui.clip_rect().left() + gutter, ui.clip_rect().bottom()),
                    );
                    let gutter_painter = ui.painter().clone();
                    ui.horizontal_top(|ui| {
                        if gutter > 0.0 {
                            ui.add_space(gutter);
                        }
                        ui.set_clip_rect(ui.clip_rect().intersect(egui::Rect::from_min_max(
                            egui::pos2(gutter_rect.right(), ui.clip_rect().top()),
                            ui.clip_rect().max,
                        )));
                        let mut edit = egui::TextEdit::multiline(&mut display)
                            .id(id)
                            .font(FontId::monospace(font_size))
                            .code_editor()
                            .lock_focus(true)
                            .frame(false)
                            .desired_rows(rows)
                            .desired_width(if wrap {
                                ui.available_width()
                            } else {
                                f32::INFINITY
                            })
                            .layouter(&mut layouter)
                            .interactive(
                                !self.busy
                                    && !self.closing
                                    && !buffer.follow
                                    && !self.preferences_open
                                    && !self.palette
                                    && self.rename_target.is_none()
                                    && !self.shelf_open
                                    && !self.bookmarks_open
                                    && !self.goto_open
                                    && !self.about_open
                                    && !self.updates.open
                                    && self.project.create.is_none()
                                    && self.quality.file_action.is_none(),
                            );
                        if !wrap {
                            edit = edit.desired_width(f32::INFINITY);
                        }
                        // Asking again resets egui's key filter, and the next
                        // arrow or Tab press would move focus out of the editor.
                        if self.focus_editor && !ctx.memory(|m| m.has_focus(id)) {
                            ctx.memory_mut(|m| m.request_focus(id));
                        }
                        let line_background = ui.painter().add(egui::Shape::Noop);
                        let selection_background = ui.painter().add(egui::Shape::Noop);
                        let caret_color = ui.visuals().text_cursor.stroke.color;
                        let selection_color = ui.visuals().selection.bg_fill;
                        if rtl_text {
                            // egui places these by character index; draw them below instead.
                            ui.visuals_mut().text_cursor.stroke.color = egui::Color32::TRANSPARENT;
                            ui.visuals_mut().selection.bg_fill = egui::Color32::TRANSPARENT;
                        }
                        let mut output = edit.show(ui);
                        if markdown_split {
                            markdown_layout = Some((
                                Arc::clone(&output.galley),
                                Arc::clone(&projection),
                                output.galley_pos.y - content_top,
                            ));
                        }

                        let bidi_rows: Arc<bidi_view::BidiRows> = latest_bidi.take();
                        if bidi_rows.any() {
                            bidi_view::interact(
                                ui,
                                &mut output,
                                &bidi_rows,
                                selection_background,
                                bidi_view::Look {
                                    caret: egui::Stroke::new(
                                        self.settings.caret_width,
                                        caret_color,
                                    ),
                                    blink: self.settings.caret_blink,
                                    selection: selection_color,
                                },
                            );
                        }
                        if self.settings.highlight_line {
                            if let Some(range) = &output.cursor_range {
                                let caret = output
                                    .galley
                                    .pos_from_cursor(&range.primary)
                                    .translate(output.galley_pos.to_vec2());
                                let rect = egui::Rect::from_min_max(
                                    egui::pos2(ui.clip_rect().left(), caret.top()),
                                    egui::pos2(ui.clip_rect().right(), caret.bottom()),
                                );
                                ui.painter().set(
                                    line_background,
                                    egui::Shape::rect_filled(
                                        rect.intersect(ui.clip_rect()),
                                        0.0,
                                        self.theme.elevated.color().gamma_multiply(0.35),
                                    ),
                                );
                            }
                        }
                        for fold in buffer.view.folds.clone() {
                            let display_start = projection.display(fold.start);
                            if projection.source(display_start) != fold.start {
                                continue;
                            }
                            let cursor = output
                                .galley
                                .from_ccursor(egui::text::CCursor::new(display_start));
                            let position = output
                                .galley
                                .pos_from_cursor(&cursor)
                                .translate(output.galley_pos.to_vec2());
                            if !position.intersects(ui.clip_rect()) {
                                continue;
                            }
                            let lines = old[hedit::advanced::byte(&old, fold.start)
                                ..hedit::advanced::byte(&old, fold.end)]
                                .bytes()
                                .filter(|c| *c == b'\n')
                                .count()
                                .max(1);
                            let button = egui::Rect::from_min_size(
                                position.left_top(),
                                egui::vec2(140.0, position.height()),
                            );
                            if ui
                                .put(
                                    button,
                                    egui::Button::new(format!("▸ {lines} lines folded")).small(),
                                )
                                .clicked()
                            {
                                buffer.view.folds.retain(|f| *f != fold);
                                self.focus_editor = true;
                                self.generation = self.generation.wrapping_add(1);
                            }
                        }
                        if self.settings.draw_indent_guides {
                            let mut first = true;
                            for row in &output.galley.rows {
                                if first
                                    && output.galley_pos.y + row.rect.bottom()
                                        >= ui.clip_rect().top()
                                    && output.galley_pos.y + row.rect.top()
                                        <= ui.clip_rect().bottom()
                                {
                                    let spaces =
                                        row.glyphs.iter().take_while(|g| g.chr == ' ').count();
                                    for col in
                                        (0..spaces)
                                            .step_by(buffer.view.tab_size.unwrap_or(4).clamp(1, 8)
                                                as usize)
                                    {
                                        let x = output.galley_pos.x + column_width * col as f32;
                                        ui.painter().vline(
                                            x,
                                            (output.galley_pos.y + row.rect.top())
                                                ..=(output.galley_pos.y + row.rect.bottom()),
                                            egui::Stroke::new(
                                                1.0,
                                                self.theme.muted.color().gamma_multiply(0.22),
                                            ),
                                        );
                                    }
                                }
                                first = row.ends_with_newline;
                            }
                        }
                        // egui paints the main selection only while the editor has
                        // focus; keep a found match visible while typing in Find.
                        let primary = hedit::advanced::Selection {
                            anchor: buffer.view.anchor,
                            head: buffer.view.head,
                        };
                        let unfocused = (!output.response.has_focus()
                            && primary.start() != primary.end())
                        .then_some(primary);
                        // Find matches are filled; other occurrences of a selected
                        // word are outlined.
                        for (ranges, fill, stroke) in [
                            (
                                &find_matches,
                                self.theme.warning.color().gamma_multiply(0.22),
                                egui::Stroke::new(
                                    1.0,
                                    self.theme.warning.color().gamma_multiply(0.7),
                                ),
                            ),
                            (
                                &word_matches,
                                egui::Color32::TRANSPARENT,
                                egui::Stroke::new(
                                    1.0,
                                    self.theme.muted.color().gamma_multiply(0.8),
                                ),
                            ),
                        ] {
                            if let Some(ranges) = ranges {
                                let shown: Vec<(usize, usize)> = ranges
                                    .iter()
                                    .filter(|&&(a, _)| {
                                        projection.source(projection.display(a)) == a
                                    })
                                    .map(|&(a, b)| (projection.display(a), projection.display(b)))
                                    .collect();
                                find_live::paint_ranges(
                                    ui.painter(),
                                    &output.galley,
                                    &bidi_rows,
                                    output.galley_pos,
                                    ui.clip_rect(),
                                    &shown,
                                    (fill, stroke),
                                );
                            }
                        }
                        if self.settings.match_brackets && primary.start() == primary.end() {
                            let pair =
                                hedit::advanced::matching_bracket(&old, primary.head, 100_000);
                            for at in pair.into_iter().flat_map(|(a, b)| [a, b]) {
                                let display = projection.display(at);
                                if projection.source(display) != at {
                                    continue; // Folded away.
                                }
                                let glyph = bidi_view::glyph_rect(&output.galley, display)
                                    .filter(|r| r.width() > 0.0);
                                if let Some(glyph) = glyph {
                                    ui.painter().rect_stroke(
                                        glyph.translate(output.galley_pos.to_vec2()),
                                        2.0,
                                        egui::Stroke::new(1.0, self.theme.accent.color()),
                                        egui::StrokeKind::Inside,
                                    );
                                }
                            }
                        }
                        for selection in buffer.view.selections.iter().chain(unfocused.iter()) {
                            let start = projection.display(selection.start());
                            let end = projection.display(selection.end());
                            let offset = output.galley_pos.to_vec2();
                            if start == end {
                                let caret =
                                    bidi_view::caret_rect(&output.galley, &bidi_rows, start)
                                        .translate(offset);
                                ui.painter().vline(
                                    caret.left(),
                                    caret.y_range(),
                                    egui::Stroke::new(
                                        self.settings.caret_width,
                                        self.theme.accent.color(),
                                    ),
                                );
                            } else {
                                for rect in
                                    bidi_view::range_rects(&output.galley, &bidi_rows, start, end)
                                {
                                    ui.painter().rect_filled(
                                        rect.translate(offset),
                                        0.0,
                                        self.theme.accent.color().gamma_multiply(0.22),
                                    );
                                }
                            }
                        }
                        for &column in &self.settings.rulers {
                            let x = output.galley_pos.x + column_width * f32::from(column);
                            ui.painter().vline(
                                x,
                                ui.clip_rect().y_range(),
                                egui::Stroke::new(
                                    1.0,
                                    self.theme.muted.color().gamma_multiply(0.25),
                                ),
                            );
                        }
                        if self.settings.draw_white_space {
                            let source_chars: Vec<_> = old.chars().collect();
                            let mut row_start = 0;
                            for row in &output.galley.rows {
                                let top = output.galley_pos.y + row.rect.top();
                                if top + row.rect.height() >= ui.clip_rect().top()
                                    && top <= ui.clip_rect().bottom()
                                {
                                    for (n, glyph) in row.glyphs.iter().enumerate() {
                                        let position = row_start + n;
                                        let source = projection.source(position);
                                        let mut center = output.galley_pos
                                            + glyph.pos.to_vec2()
                                            + egui::vec2(glyph.advance_width / 2.0, -3.0);
                                        if source_chars.get(source) == Some(&'\t') {
                                            if projection.display(source) == position {
                                                let width =
                                                    projection.display(source + 1) - position;
                                                center.x += glyph.advance_width
                                                    * (width.saturating_sub(1)) as f32
                                                    / 2.0;
                                                ui.painter().text(
                                                    center,
                                                    egui::Align2::CENTER_CENTER,
                                                    "→",
                                                    FontId::monospace(font_size - 2.0),
                                                    self.theme.muted.color(),
                                                );
                                            }
                                        } else if glyph.chr == ' ' {
                                            ui.painter().circle_filled(
                                                center,
                                                0.8,
                                                self.theme.muted.color().gamma_multiply(0.65),
                                            );
                                        }
                                    }
                                }
                                row_start += row.glyphs.len() + usize::from(row.ends_with_newline);
                            }
                        }
                        if gutter > 0.0 {
                            gutter_painter.rect_filled(gutter_rect, 0.0, gutter_background);
                            // The text's row is clipped to the text, which would
                            // make the gutter's markers and arrows unreachable by the pointer.
                            let mut gutter_ui =
                                ui.new_child(egui::UiBuilder::new().max_rect(gutter_rect));
                            gutter_ui.set_clip_rect(gutter_rect);
                            let mut display_line = 0;
                            let mut first_row = true;
                            for row in &output.galley.rows {
                                if first_row {
                                    let position = egui::pos2(
                                        gutter_rect.right() - 12.0,
                                        output.galley_pos.y + row.rect.center().y,
                                    );
                                    if gutter_rect
                                        .contains(egui::pos2(gutter_rect.center().x, position.y))
                                    {
                                        let source_line = *projection
                                            .source_lines
                                            .get(display_line)
                                            .unwrap_or(&1);
                                        if self
                                            .quality
                                            .git_lines
                                            .get(&buffer.view.id)
                                            .is_some_and(|lines| lines.contains(&source_line))
                                        {
                                            let mark = egui::Rect::from_min_size(
                                                egui::pos2(
                                                    gutter_rect.left() + 2.0,
                                                    position.y - row.rect.height() / 2.0,
                                                ),
                                                egui::vec2(5.0, row.rect.height()),
                                            );
                                            gutter_painter.rect_filled(
                                                mark,
                                                0.0,
                                                self.theme.warning.color(),
                                            );
                                            if let Some(diff) =
                                                self.quality.git_cache.get(&buffer.view.id)
                                            {
                                                let hunk =
                                                    hedit::intelligence::hunk_at(diff, source_line);
                                                gutter_ui
                                                    .interact(
                                                        mark,
                                                        gutter_ui.id().with((
                                                            "git-hunk",
                                                            buffer.view.id,
                                                            source_line,
                                                        )),
                                                        egui::Sense::hover(),
                                                    )
                                                    .on_hover_ui(|ui| {
                                                        ui.set_max_width(620.0);
                                                        ui.label(RichText::new(hunk).monospace());
                                                    });
                                            }
                                        }
                                        let folded = folded_heads
                                            .iter()
                                            .find(|(line, _)| *line == source_line);
                                        let open = (gutter_hovered && folded.is_none())
                                            .then(|| {
                                                fold_heads
                                                    .iter()
                                                    .find(|(line, _)| *line == source_line)
                                            })
                                            .flatten();
                                        if let Some(&(_, fold)) = folded.or(open) {
                                            let center =
                                                egui::pos2(gutter_rect.right() - 6.0, position.y);
                                            let target = egui::Rect::from_center_size(
                                                center,
                                                egui::vec2(12.0, row.rect.height()),
                                            );
                                            let response = gutter_ui
                                                .interact(
                                                    target,
                                                    gutter_ui.id().with((
                                                        "fold-arrow",
                                                        buffer.view.id,
                                                        source_line,
                                                    )),
                                                    egui::Sense::click(),
                                                )
                                                .on_hover_text(if folded.is_some() {
                                                    "Unfold"
                                                } else {
                                                    "Fold"
                                                });
                                            fold_gutter::arrow(
                                                &gutter_painter,
                                                center,
                                                folded.is_some(),
                                                if response.hovered() {
                                                    gutter_text
                                                } else {
                                                    gutter_color
                                                },
                                            );
                                            if response.clicked() {
                                                fold_click = Some((fold, folded.is_some()));
                                            }
                                        }
                                        gutter_painter.text(
                                            position,
                                            egui::Align2::RIGHT_CENTER,
                                            projection
                                                .source_lines
                                                .get(display_line)
                                                .unwrap_or(&1)
                                                .to_string(),
                                            FontId::monospace(font_size - 1.0),
                                            if projection.source_lines.get(display_line)
                                                == Some(&current_line)
                                            {
                                                gutter_text
                                            } else {
                                                gutter_color
                                            },
                                        );
                                    }
                                }
                                first_row = row.ends_with_newline;
                                if row.ends_with_newline {
                                    display_line += 1;
                                }
                            }
                        }
                        if self.focus_editor {
                            if !output.response.has_focus() {
                                output.response.request_focus();
                            }
                            self.focus_editor = false;
                        }
                        // A pointer resting on a name shows its definitions.
                        let rested = ctx.input(|i| i.pointer.time_since_last_movement());
                        if output.response.hovered()
                            && self.settings.show_definitions
                            && rested <= 0.6
                        {
                            ctx.request_repaint_after(Duration::from_secs_f32(0.61 - rested));
                        }
                        if output.response.hovered()
                            && self.settings.show_definitions
                            && ctx.input(|i| {
                                !i.pointer.any_down() && i.pointer.time_since_last_movement() > 0.6
                            })
                        {
                            if let Some(at) = ctx.pointer_hover_pos() {
                                let relative = at - output.galley_pos;
                                let index = bidi_view::hit(&output.galley, &bidi_rows, relative)
                                    .unwrap_or_else(|| {
                                        output.galley.cursor_from_pos(relative).ccursor.index
                                    });
                                // Only over the text itself, not the space after a line.
                                let caret =
                                    bidi_view::caret_rect(&output.galley, &bidi_rows, index);
                                let close = (caret.center().x - relative.x).abs() < font_size;
                                let source = projection.source(index);
                                // The text is in `old` while the editor is drawn.
                                let word = hedit::advanced::word(&old, source);
                                if close && word.start() < word.end() {
                                    self.hover_target =
                                        Some((buffer.view.id, (word.start(), word.end()), at));
                                }
                            }
                        }
                        if output.response.has_focus() {
                            self.caret_screen = output.cursor_range.and_then(|range| {
                                let rect = bidi_view::caret_rect(
                                    &output.galley,
                                    &bidi_rows,
                                    range.primary.ccursor.index,
                                )
                                .translate(output.galley_pos.to_vec2());
                                ui.clip_rect().contains(rect.center()).then_some(rect)
                            });
                        }
                        if restore && buffer.reveal_cursor {
                            // TextEdit only returns a cursor range when focused.
                            // Find keeps focus in its field, so reveal the stored
                            // selection head in display coordinates instead.
                            let head = output.cursor_range.map_or_else(
                                || projection.display(buffer.view.head),
                                |range| range.primary.ccursor.index,
                            );
                            let rect = bidi_view::caret_rect(&output.galley, &bidi_rows, head)
                                .translate(output.galley_pos.to_vec2());
                            ui.scroll_to_rect(rect, Some(egui::Align::Center));
                        }
                        if let Some(range) = output.cursor_range {
                            let range = range.as_ccursor_range();
                            let previous = (buffer.view.anchor, buffer.view.head);
                            let selection = hedit::advanced::Selection {
                                anchor: range.secondary.index,
                                head: range.primary.index,
                            };
                            if output.response.changed() {
                                let edited = projection.edited(&old, &display, selection);
                                let (a, b, _) =
                                    hedit::workspace::splice(&projection.text, &display);
                                let a = projection.text[..a].chars().count();
                                let b = projection.text[..b].chars().count();
                                let touched = projection.touches_fold(a, b);
                                if !(original_selection.anchor == original_selection.head
                                    && touched)
                                {
                                    edited_text = Some(edited.text);
                                }
                                let selected = edited.selections[0];
                                buffer.view.anchor = selected.anchor;
                                buffer.view.head = selected.head;
                                buffer.view.folds.clear();
                                self.generation = self.generation.wrapping_add(1);
                            } else {
                                buffer.view.anchor = projection.source(selection.anchor);
                                buffer.view.head = projection.source(selection.head);
                            }
                            if output.response.clicked() || output.response.dragged() {
                                let modifiers = ctx.input(|i| i.modifiers);
                                if modifiers.alt && !buffer.page.info.paged {
                                    let head = buffer.view.head;
                                    if modifiers.shift {
                                        let mut ranges = hedit::advanced::rectangle(
                                            edited_text.as_deref().unwrap_or(&old),
                                            self.rectangle_start
                                                .filter(|(pane, _)| *pane == pane_id)
                                                .map_or(
                                                    original_selection.anchor,
                                                    |(_, anchor)| anchor,
                                                ),
                                            head,
                                            buffer.view.tab_size.unwrap_or(4) as usize,
                                        );
                                        if let Some(primary) = ranges.pop() {
                                            buffer.view.anchor = primary.anchor;
                                            buffer.view.head = primary.head;
                                        }
                                        buffer.view.selections = ranges;
                                    } else if output.response.clicked()
                                        && buffer.view.selections.len()
                                            < hedit::advanced::MAX_SELECTIONS - 1
                                    {
                                        buffer.view.selections.push(original_selection);
                                    }
                                } else {
                                    buffer.view.selections.clear();
                                }
                                if let Some(at) = output.response.interact_pointer_pos() {
                                    let cursor =
                                        output.galley.cursor_from_pos(at - output.galley_pos);
                                    let source = projection.source(cursor.ccursor.index);
                                    buffer
                                        .view
                                        .folds
                                        .retain(|f| !(f.start <= source && source <= f.end));
                                }
                            }
                            if previous != (buffer.view.anchor, buffer.view.head) {
                                self.generation = self.generation.wrapping_add(1);
                            }
                        }
                        // Misspelled words on screen get a red underline.
                        if !misspelled.is_empty() {
                            let clip = ui.clip_rect();
                            let top = output
                                .galley
                                .cursor_from_pos(egui::vec2(0.0, clip.top() - output.galley_pos.y))
                                .ccursor
                                .index;
                            let bottom = output
                                .galley
                                .cursor_from_pos(egui::vec2(
                                    f32::MAX,
                                    clip.bottom() - output.galley_pos.y,
                                ))
                                .ccursor
                                .index;
                            let painter = ui.painter();
                            let stroke = egui::Stroke::new(1.2, self.theme.error.color());
                            for range in &misspelled {
                                let (a, b) = (
                                    projection.display(range.start),
                                    projection.display(range.end),
                                );
                                if b < top || a > bottom || a >= b {
                                    continue;
                                }
                                let offset = output.galley_pos.to_vec2();
                                let start = bidi_view::caret_rect(&output.galley, &bidi_rows, a)
                                    .translate(offset);
                                let end = bidi_view::caret_rect(&output.galley, &bidi_rows, b)
                                    .translate(offset);
                                if (start.center().y - end.center().y).abs() > 1.0 {
                                    continue;
                                }
                                let y = start.bottom() - 1.0;
                                let (left, right) =
                                    (start.left().min(end.left()), start.left().max(end.left()));
                                // A gentle wave, as other editors draw it.
                                let mut points = vec![];
                                let mut x = left;
                                let mut up = false;
                                while x < right {
                                    points.push(egui::pos2(x, if up { y - 1.5 } else { y }));
                                    x += 2.0;
                                    up = !up;
                                }
                                points.push(egui::pos2(right, y));
                                painter.add(egui::Shape::line(points, stroke));
                            }
                        }
                        // Right-clicking outside the selection moves the cursor there.
                        if output.response.secondary_clicked() {
                            if let Some(at) = output.response.interact_pointer_pos() {
                                let relative = at - output.galley_pos;
                                let index = bidi_view::hit(&output.galley, &bidi_rows, relative)
                                    .unwrap_or_else(|| {
                                        output.galley.cursor_from_pos(relative).ccursor.index
                                    });
                                let source = projection.source(index);
                                word_menu_at = Some(source);
                                // Until the word is looked up, show no stale entries.
                                self.spell.menu = None;
                                let (a, b) = (
                                    buffer.view.anchor.min(buffer.view.head),
                                    buffer.view.anchor.max(buffer.view.head),
                                );
                                if a == b || source < a || source > b {
                                    buffer.view.anchor = source;
                                    buffer.view.head = source;
                                    buffer.view.selections.clear();
                                    buffer.restore_cursor = true;
                                }
                            }
                        }
                        output.response.context_menu(|ui| {
                            if let Some(menu) = &self.spell.menu {
                                if menu.suggestions.is_empty() {
                                    ui.weak("No suggestions");
                                }
                                for word in &menu.suggestions {
                                    if ui.button(RichText::new(word).strong()).clicked() {
                                        spell_choice =
                                            Some(spell_check::SpellAction::Replace(word.clone()));
                                        ui.close_menu();
                                    }
                                }
                                if ui
                                    .button(format!("Add “{}” to dictionary", menu.word))
                                    .clicked()
                                {
                                    spell_choice = Some(spell_check::SpellAction::Add);
                                    ui.close_menu();
                                }
                                if ui.button(format!("Ignore “{}”", menu.word)).clicked() {
                                    spell_choice = Some(spell_check::SpellAction::Ignore);
                                    ui.close_menu();
                                }
                                ui.separator();
                            }
                            for (n, group) in context_menu.iter().enumerate() {
                                if n > 0 {
                                    ui.separator();
                                }
                                for (command, label, shortcut, enabled) in group {
                                    let button =
                                        egui::Button::new(*label).shortcut_text(shortcut.as_str());
                                    if ui.add_enabled(*enabled, button).clicked() {
                                        context_command = Some(*command);
                                        ui.close_menu();
                                    }
                                }
                            }
                        });
                        // Backend history is authoritative across pages. Do not retain a
                        // second, potentially large TextEdit undo history.
                        output.state.clear_undoer();
                        output.state.store(ctx, id);
                        edited_text.as_ref().is_some_and(|text| *text != old)
                    })
                    .inner
                })
            })
            .inner;
        // Keep the previous text only when it changed; history needs it.
        let previous = match edited_text {
            Some(text) if text != old => {
                buffer.page.text = text;
                Some(old)
            }
            _ => {
                buffer.page.text = old;
                None
            }
        };
        if let Some((fold, folded)) = fold_click.filter(|_| previous.is_none()) {
            if folded {
                buffer.view.folds.retain(|f| *f != fold);
            } else {
                buffer.view.folds.push(fold);
                // A cursor inside would unfold it again; keep it at the header.
                if fold.start < buffer.view.head && buffer.view.head < fold.end {
                    buffer.view.anchor = fold.start;
                    buffer.view.head = fold.start;
                }
                buffer.view.selections.clear();
            }
            buffer.restore_cursor = true;
            self.generation = self.generation.wrapping_add(1);
        }
        let mut minimap_target = None;
        if self.settings.show_minimap && !self.distraction_free && map_area.width() > 300.0 {
            let map = egui::Rect::from_min_max(
                egui::pos2(map_area.right() - 68.0, map_area.top()),
                map_area.max,
            );
            ui.painter().rect_filled(map, 0.0, self.theme.panel.color());
            let lines: Vec<_> = projection.text.lines().collect();
            let rows = lines.len().max(1);
            let samples = (map.height() as usize).min(rows).max(1);
            for n in 0..samples {
                let line = lines.get(n * rows / samples).copied().unwrap_or("");
                let y = map.top() + n as f32 * (map.height() / samples as f32).min(2.0);
                let leading = line
                    .chars()
                    .take_while(|c| c.is_whitespace())
                    .count()
                    .min(100);
                let width = line.chars().count().min(120);
                ui.painter().hline(
                    (map.left() + leading as f32 * 0.45)..=(map.left() + width as f32 * 0.45),
                    y,
                    egui::Stroke::new(0.7, self.theme.muted.color().gamma_multiply(0.5)),
                );
            }
            let content = scroll_output.content_size.y.max(map.height());
            let start = (scroll_output.state.offset.y / content).clamp(0.0, 1.0);
            let fraction = (map.height() / content).min(1.0);
            let viewport = egui::Rect::from_min_size(
                egui::pos2(map.left(), map.top() + start * map.height()),
                egui::vec2(map.width(), (fraction * map.height()).max(8.0)),
            );
            ui.painter().rect_filled(
                viewport.intersect(map),
                0.0,
                self.theme.accent.color().gamma_multiply(0.12),
            );
            let response = ui.interact(
                map,
                egui::Id::new(("minimap", pane_id)),
                egui::Sense::click_and_drag(),
            );
            if let Some(pos) = response.interact_pointer_pos() {
                minimap_target = Some(
                    (((pos.y - map.top()) / map.height()) * content - map.height() / 2.0).max(0.0)
                        as f64,
                );
            }
            if buffer.page.info.paged {
                ui.painter().text(
                    map.left_top(),
                    egui::Align2::LEFT_TOP,
                    "PAGE",
                    FontId::monospace(9.0),
                    self.theme.muted.color(),
                );
            }
        }
        buffer.restore_cursor = false;
        buffer.reveal_cursor = false;
        if buffer.view.scroll_top != scroll_output.state.offset.y as f64
            || buffer.view.scroll_left != scroll_output.state.offset.x as f64
        {
            buffer.view.scroll_top = scroll_output.state.offset.y as f64;
            buffer.view.scroll_left = scroll_output.state.offset.x as f64;
            self.generation = self.generation.wrapping_add(1);
        }
        if let Some(target) = minimap_target {
            buffer.view.scroll_top = target;
            buffer.restore_cursor = true;
        }
        if let Some(old) = previous {
            let b = &mut self.workspace.open[index];
            let mut cursors = b.view.selections.clone();
            cursors.push(original_selection);
            b.pending_selection = Some(cursors);
            match self.workspace.edit(index, &old) {
                Ok(()) => self.changed(),
                Err(error) => {
                    // If an external writer invalidates a paged source, keep
                    // the user's edited view in a new independent unsaved file.
                    let text = self.workspace.open[index].page.text.clone();
                    self.workspace.open[index].page.text = old;
                    let result = (|| {
                        let mut doc = Document::empty(self.workspace.editor.id());
                        doc.replace(0, 0, text, 0)?;
                        let page = doc.page(0)?;
                        self.workspace.add(doc, page)
                    })();
                    self.report(result);
                    self.error = Some(format!(
                        "{error}\nThe edited page has been kept in a new unsaved file."
                    ));
                }
            }
        }
        if let Some((galley, projection, inset)) = markdown_layout {
            self.markdown.scroll.source(
                active_view,
                pane_id,
                markdown_scroll::Source {
                    viewport: markdown_scroll::Viewport::new(&scroll_output, editor_area),
                    galley,
                    projection,
                    inset,
                    revision: self.workspace.open[index].page.info.revision,
                    navigation: markdown_navigation,
                },
            );
        }
        if let Some(at) = word_menu_at {
            if self.settings.spell_check {
                self.word_menu_at(active_view, at);
            }
        }
        if let Some(action) = spell_choice {
            self.focus_editor = true;
            self.spell_action(action);
        }
        if let Some(command) = context_command {
            self.focus_editor = true;
            self.execute(ui.ctx(), command);
        }
    }
    fn gpu(&mut self, ctx: &egui::Context) {
        let theme = self.theme.clone();
        if !hedit::debugger::LOCAL_TOOLS_SUPPORTED || !self.gpu_open {
            return;
        }
        egui::TopBottomPanel::bottom("gpu").resizable(true).default_height(285.0).min_height(160.0).show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("CUDA / GPU").strong().color(theme.accent.color()));
                ui.label(RichText::new("NVIDIA tools · Linux").small().color(theme.muted.color()));
                if let Some(debug) = &self.debug { ui.label(format!("{:?}", debug.state)); }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if controls::close_button(ui, "Hide CUDA panel").clicked() { self.gpu_open = false; }
                });
            });
            let live = self.debug.as_ref().is_some_and(|d| d.state != State::Exited);
            ui.add_enabled_ui(!live, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Executable");
                    ui.add(egui::TextEdit::singleline(&mut self.debug_program).desired_width(280.0).hint_text("/path/to/cuda-program"));
                    if ui.add_enabled(!self.busy, egui::Button::new("Browse…")).clicked() {
                        // Native dialogs block; run it off the UI thread like the others.
                        self.job(ctx, || Ok(rfd::FileDialog::new().pick_file().map_or(Action::Nothing, Action::DebugProgram)));
                    }
                    ui.label("Args (JSON)");
                    ui.add(egui::TextEdit::singleline(&mut self.debug_args).desired_width(180.0).hint_text("[\"arg one\", \"arg2\"]"));
                });
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Tool settings…").clicked() {
                        self.open_preferences(PreferencesPage::Tools);
                    }
                    if ui.button("Start CUDA-GDB").clicked() {
                        let result = serde_json::from_str::<Vec<String>>(&self.debug_args).map_err(|e| format!("Arguments must be a JSON array of strings: {e}"))
                            .and_then(|args| DebugSession::launch(&self.settings.cuda_gdb_path, &PathBuf::from(&self.debug_program), &args, None));
                        match result { Ok(session) => self.debug = Some(session), Err(e) => self.error = Some(e) }
                    }
                    egui::ComboBox::from_id_salt("sanitizer").selected_text(&self.sanitizer).show_ui(ui, |ui| {
                        for tool in ["memcheck", "racecheck", "initcheck", "synccheck"] { ui.selectable_value(&mut self.sanitizer, tool.into(), tool); }
                    });
                    if ui.button("Run sanitizer").clicked() {
                        let result = serde_json::from_str::<Vec<String>>(&self.debug_args).map_err(|e| e.to_string())
                            .and_then(|args| DebugSession::sanitizer(&PathBuf::from(&self.debug_program), &args, &self.sanitizer));
                        match result { Ok(session) => self.debug = Some(session), Err(e) => self.error = Some(e) }
                    }
                    if ui.button("GPU info").clicked() { match DebugSession::gpu_info() { Ok(session) => self.debug = Some(session), Err(e) => self.error = Some(e) } }
                });
            });
            if let Some(debug) = &mut self.debug {
                ui.horizontal_wrapped(|ui| {
                    let mi = debug.mi && debug.state != State::Exited;
                    for (label, command) in [("Run", "-exec-run"), ("Continue", "-exec-continue"), ("Pause", "-exec-interrupt --all"),
                        ("Step", "-exec-step"), ("Next", "-exec-next"), ("Finish", "-exec-finish"), ("Stack", "-stack-list-frames"), ("Locals", "-stack-list-variables --all-values")] {
                        if ui.add_enabled(mi, egui::Button::new(label)).clicked() { if let Err(e) = debug.send(command) { self.error = Some(e); } }
                    }
                    if ui.add_enabled(live, egui::Button::new("Stop")).clicked() { debug.stop(); }
                    if ui.button("Clear output").clicked() { debug.transcript.clear(); }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.breakpoint).desired_width(180.0).hint_text("kernel_name or file.cu:42"));
                    if ui.add_enabled(debug.mi, egui::Button::new("Add breakpoint")).clicked() { if let Err(e) = debug.breakpoint(&self.breakpoint) { self.error = Some(e); } }
                    for (label, command) in [("Devices", "info cuda devices"), ("Kernels", "info cuda kernels"), ("Threads", "info cuda threads"), ("Warps", "info cuda warps")] {
                        if ui.add_enabled(debug.mi, egui::Button::new(label)).clicked() { if let Err(e) = debug.console(command) { self.error = Some(e); } }
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    let response = ui.add(egui::TextEdit::singleline(&mut self.console).desired_width(420.0).hint_text("GDB command: cuda kernel 0, print value, break ..."));
                    let enter = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    if ui.add_enabled(debug.mi, egui::Button::new("Send")).clicked() || enter {
                        if let Err(e) = debug.console(&self.console) { self.error = Some(e); }
                        self.console.clear(); response.request_focus();
                    }
                });
                egui::ScrollArea::vertical().id_salt("debug-output").stick_to_bottom(true).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(&debug.transcript).monospace().size(12.0)).selectable(true));
                });
            } else {
                ui.label(RichText::new("Build with nvcc -g -G, then select the executable. Start loads symbols; Run launches it. CUDA Toolkit tools are optional.").small().color(theme.muted.color()));
            }
        });
    }
    fn open_appearance(&mut self) {
        if !self.preferences_open {
            self.appearance.original = self.theme.clone();
        }
        self.open_preferences(PreferencesPage::Appearance);
    }
    fn import_theme(&mut self, ctx: &egui::Context) {
        self.job(ctx, || {
            let Some(path) = rfd::FileDialog::new()
                .set_title("Import a theme")
                .add_filter("Themes", &["json", "tmTheme"])
                .add_filter("hedit theme", &["json"])
                .add_filter("TextMate or Sublime color scheme", &["tmTheme"])
                .pick_file()
            else {
                return Ok(Action::Nothing);
            };
            let textmate = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("tmtheme"));
            let theme = if textmate {
                let size = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
                if size > 1024 * 1024 {
                    return Err("Color schemes must be at most 1 MiB.".into());
                }
                let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                syntax::grammar::import_tm_theme(&bytes, &stem)?
            } else {
                Theme::load(&path)?
            };
            Ok(Action::ThemeImported(theme))
        });
    }
    fn export_theme(&mut self, ctx: &egui::Context) {
        if let Err(error) = self.theme.validate() {
            self.error = Some(error);
            return;
        }
        let theme = self.theme.clone();
        self.job(ctx, move || {
            let Some(path) = rfd::FileDialog::new()
                .set_title("Export hedit theme")
                .add_filter("hedit theme", &["json"])
                .set_file_name("my-hedit-theme.json")
                .save_file()
            else {
                return Ok(Action::Nothing);
            };
            theme.export(&path)?;
            Ok(Action::Notice(format!(
                "Theme exported to {}",
                path.display()
            )))
        });
    }
    fn open_unsaved_file_name(&mut self) {
        if self.active().page.info.path.is_some() {
            self.error = Some(
                "Only unsaved files can be named. Use Save As to save a copy of a saved file."
                    .into(),
            );
            return;
        }
        self.rename_target = Some(self.active().page.info.id);
        self.unsaved_file_name = self.active().view.custom_name.clone().unwrap_or_default();
        self.unsaved_file_name_focus = true;
    }
    fn toggle_bookmark(&mut self) {
        let result = self.workspace.toggle_bookmark(self.workspace.active);
        match result {
            Ok(added) => {
                self.notice = if added {
                    "Bookmark added."
                } else {
                    "Bookmark removed."
                }
                .into();
                self.changed();
            }
            Err(error) => self.error = Some(error),
        }
    }
    fn adjacent_bookmark(&mut self, ctx: &egui::Context, forward: bool) {
        if let Some(offset) = self
            .workspace
            .adjacent_bookmark(self.workspace.active, forward)
        {
            self.go_to_offset(ctx, offset);
        } else {
            self.notice = "No bookmarks in this file.".into();
        }
    }
    fn go_to_offset(&mut self, ctx: &egui::Context, offset: u64) {
        if !self.quality.replaying {
            self.navigation_mark();
        }
        let id = self.active().page.info.id;
        let editor = Arc::clone(&self.workspace.editor);
        self.job(ctx, move || {
            let shared = editor.get(id)?;
            let mut document = shared.lock().map_err(|e| e.to_string())?;
            let offset = offset.min(document.info().size);
            let page = document.page(offset.saturating_sub(4096))?;
            let mut local = offset
                .saturating_sub(page.start)
                .min(page.text.len() as u64) as usize;
            while !page.text.is_char_boundary(local) {
                local -= 1;
            }
            let cursor = page.text[..local].chars().count();
            Ok(Action::Page(id, page, Some((cursor, cursor))))
        });
    }
    fn buffer_tools(&mut self, ctx: &egui::Context) {
        if let Some(id) = self.rename_target {
            let mut open = true;
            let mut apply = false;
            let mut cancel = false;
            egui::Window::new("Name unsaved file")
                .id(egui::Id::new("unsaved-file-name"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(360.0)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(
                        "Choose a tab name for this unsaved file. This does not save it to disk.",
                    );
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.unsaved_file_name)
                            .char_limit(80)
                            .desired_width(f32::INFINITY)
                            .hint_text("Investigation notes"),
                    );
                    if self.unsaved_file_name_focus {
                        response.request_focus();
                        self.unsaved_file_name_focus = false;
                    }
                    let enter = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    ui.label(
                        RichText::new("Leave blank to use the first nonempty line again.")
                            .small()
                            .color(self.theme.muted.color()),
                    );
                    match controls::dialog_buttons(
                        ui,
                        &self.theme,
                        "Apply",
                        !self.busy && !self.closing,
                        enter,
                    ) {
                        controls::Choice::Confirm => apply = true,
                        controls::Choice::Cancel => cancel = true,
                        controls::Choice::None => {}
                    }
                });
            open &= !cancel;
            if apply && !self.busy && !self.closing {
                if let Some(index) = self
                    .workspace
                    .open
                    .iter()
                    .position(|b| b.page.info.id == id)
                {
                    let result = self
                        .workspace
                        .name_scratchpad(index, &self.unsaved_file_name);
                    if result.is_ok() {
                        self.rename_target = None;
                    }
                    self.report(result);
                } else {
                    self.rename_target = None;
                }
            }
            if !open {
                self.rename_target = None;
                self.focus_editor = true;
            }
        }
        if self.shelf_open {
            let mut open = true;
            let mut restore = None;
            let shown = egui::Window::new("Closed files")
                .open(&mut open)
                .default_width(480.0)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label("Closed files keep their text, names, and bookmarks.");
                    let filter = ui.add(
                        egui::TextEdit::singleline(&mut self.shelf_query)
                            .desired_width(f32::INFINITY)
                            .hint_text("Filter closed files…"),
                    );
                    // Type to filter as soon as the window appears.
                    if !ctx.memory(|m| m.areas().visible_last_frame(&ui.layer_id())) {
                        filter.request_focus();
                    }
                    if self.workspace.closed.is_empty() {
                        ui.label("The shelf is empty. Close a tab to keep it here.");
                    }
                    let filter = self.shelf_query.to_lowercase();
                    let mut matches = 0;
                    egui::ScrollArea::vertical()
                        .max_height(320.0)
                        .show(ui, |ui| {
                            for (index, view) in self.workspace.closed.iter().enumerate().rev() {
                                if !view.name.to_lowercase().contains(&filter) {
                                    continue;
                                }
                                matches += 1;
                                ui.push_id(view.id, |ui| {
                                    ui.horizontal(|ui| {
                                        if ui
                                            .add_enabled(
                                                !self.busy && !self.closing,
                                                egui::Button::new("Reopen"),
                                            )
                                            .clicked()
                                        {
                                            restore = Some(index);
                                        }
                                        ui.label(&view.name);
                                        if !view.bookmarks.is_empty() {
                                            ui.label(
                                                RichText::new(format!(
                                                    "{} bookmarks",
                                                    view.bookmarks.len()
                                                ))
                                                .small()
                                                .color(self.theme.muted.color()),
                                            );
                                        }
                                    })
                                });
                            }
                        });
                    if matches == 0 && !self.workspace.closed.is_empty() {
                        ui.label("No closed files match that name.");
                    }
                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "{} closed files. Unsaved notes are kept beyond the 50-file history limit.",
                            self.workspace.closed.len()
                        ))
                        .small()
                        .color(self.theme.muted.color()),
                    );
                });
            self.shelf_open = open && !overlays::clicked_away(ctx, &shown);
            if let Some(index) = restore {
                let result = self.workspace.reopen_at(index);
                if result.is_ok() {
                    self.shelf_open = false;
                }
                self.report(result);
            }
            if !open {
                self.focus_editor = true;
            }
        }
        if self.bookmarks_open {
            let mut open = true;
            let mut target = None;
            let bookmarks = self.active().view.bookmarks.clone();
            let shown = egui::Window::new("Bookmarks")
                .open(&mut open)
                .default_width(480.0)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(RichText::new(&self.active().view.name).strong());
                    if ui
                        .add_enabled(
                            !self.busy && !self.closing,
                            egui::Button::new(format!(
                                "Toggle at cursor · {}",
                                Command::Bookmark.shortcut()
                            )),
                        )
                        .clicked()
                    {
                        self.toggle_bookmark();
                    }
                    if bookmarks.is_empty() {
                        ui.label(format!(
                            "No bookmarks yet. Mark a position with {}.",
                            Command::Bookmark.shortcut()
                        ));
                    }
                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .show(ui, |ui| {
                            for bookmark in bookmarks {
                                if ui
                                    .add_enabled(
                                        !self.busy && !self.closing,
                                        egui::Button::new(format!(
                                            "{}  ·  byte {}",
                                            bookmark.label, bookmark.offset
                                        )),
                                    )
                                    .clicked()
                                {
                                    target = Some(bookmark.offset);
                                }
                            }
                        });
                    ui.label(
                        RichText::new(
                            "F2 next · Shift+F2 previous. Positions are kept between launches.",
                        )
                        .small()
                        .color(self.theme.muted.color()),
                    );
                });
            self.bookmarks_open = open && !overlays::clicked_away(ctx, &shown);
            if let Some(offset) = target {
                self.bookmarks_open = false;
                self.go_to_offset(ctx, offset);
            }
            if !open {
                self.focus_editor = true;
            }
        }
    }
}
impl eframe::App for HeditApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.updates.poll();
        self.receive(ctx);
        self.poll_incoming(ctx);
        self.project.poll();
        if self.quality.preview.is_some_and(|id| {
            self.workspace
                .open
                .iter()
                .any(|b| b.view.id == id && b.page.info.dirty)
        }) {
            self.quality.preview = None;
        }
        if let Some(debug) = &mut self.debug {
            debug.poll();
        }
        self.poll_external(ctx);
        self.poll_git(ctx);
        if ctx.input(|i| i.viewport().close_requested()) && !self.exit_ready {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request_quit();
        }
        self.sync_appearance(ctx);
        // Follow full screen changes made outside hedit, such as by the window manager.
        if let Some(fullscreen) = ctx.input(|i| i.viewport().fullscreen) {
            self.fullscreen = fullscreen;
        }
        if quality::refocused(ctx) {
            self.reload_settings(ctx);
        }
        self.shortcuts(ctx);
        let title = format!(
            "{}{}{} — hedit",
            self.active().view.name,
            if self.active().page.info.dirty {
                " •"
            } else {
                ""
            },
            match self.workspace.folders.as_slice() {
                [] => String::new(),
                [folder] => format!(
                    " — {}",
                    std::path::Path::new(&folder.path)
                        .file_name()
                        .unwrap_or_else(|| std::ffi::OsStr::new(&folder.path))
                        .to_string_lossy()
                ),
                folders => format!(" — {} folders", folders.len()),
            }
        );
        if self.window_title != title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
        self.poll_build();
        self.track_waits();
        self.chrome(ctx);
        self.gpu(ctx);
        self.search_panel(ctx);
        self.build_panel(ctx);
        self.editor(ctx);
        if !self.busy && !self.closing {
            self.command_palette(ctx);
        }
        self.preferences(ctx);
        self.buffer_tools(ctx);
        self.folder_dialog(ctx);
        self.updates_window(ctx);
        self.quality_windows(ctx);
        self.record_copies(ctx);
        self.close_dialog(ctx);
        let paths: Vec<_> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        self.pending_paths.extend(paths);
        self.persist_settings();
        if let Some(after) = self.next_wake(ctx) {
            ctx.request_repaint_after(after);
        }
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "recent_projects", &self.quality.recent);
        eframe::set_value(storage, "recent_files", &self.recent_files);
        eframe::set_value(storage, "project_path", &self.quality.project_path);
        eframe::set_value(storage, "project_settings", &self.quality.project_settings);
        eframe::set_value(storage, "panes", &self.panes);
        let mut theme = self.theme.clone();
        if theme.name.trim().is_empty() {
            theme.name = "My theme".into();
        }
        if let Ok(json) = theme.to_json() {
            storage.set_string("theme", json);
        }
        self.appearance.save(storage);
        self.flush_settings();
    }
    fn auto_save_interval(&self) -> Duration {
        Duration::from_secs(2)
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.release_waits();
        if let Err(error) = self.workspace.checkpoint() {
            eprintln!("hedit: final recovery checkpoint failed: {error}");
        }
        // An extra window closed with nothing in it is not reopened next time.
        let session = self.workspace.store.directory().to_owned();
        if hedit::windows::is_extra(&session) && self.workspace_is_empty() {
            let _ = std::fs::remove_dir_all(&session);
        }
        if let Some(path) = &self.quality.project_path {
            if let Err(error) = self.project_snapshot().and_then(|state| {
                hedit::projects::atomic_json(&path.with_extension("hedit-workspace"), &state)
            }) {
                eprintln!("hedit: final project workspace checkpoint failed: {error}");
            }
        }
    }
}
impl Drop for HeditApp {
    fn drop(&mut self) {
        if let Some(worker) = self.checkpoint_worker.take() {
            let _ = worker.join();
        }
    }
}
fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}
pub fn icon() -> egui::IconData {
    brand::icon()
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn frame(
        ctx: &egui::Context,
        app: &mut HeditApp,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        frame_size(ctx, app, events, egui::vec2(1280.0, 820.0))
    }
    pub(super) fn frame_size(
        ctx: &egui::Context,
        app: &mut HeditApp,
        events: Vec<egui::Event>,
        size: egui::Vec2,
    ) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            focused: true,
            events,
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            app.shortcuts(ctx);
            app.chrome(ctx);
            app.gpu(ctx);
            app.search_panel(ctx);
            app.poll_build();
            app.build_panel(ctx);
            app.editor(ctx);
            app.command_palette(ctx);
            app.preferences(ctx);
            app.buffer_tools(ctx);
            app.folder_dialog(ctx);
            app.quality_windows(ctx);
            app.record_copies(ctx);
            app.close_dialog(ctx);
        });
        assert!(!output.shapes.is_empty());
        output
    }
    pub(super) fn key(key: Key, modifiers: Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    pub(super) fn finish_app_job(app: &mut HeditApp, ctx: &egui::Context) {
        let started = Instant::now();
        while app.busy || !app.pending_paths.is_empty() {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "background job timed out"
            );
            app.receive(ctx);
            thread::sleep(Duration::from_millis(5));
        }
        assert!(app.error.is_none(), "{:?}", app.error);
    }

    #[test]
    fn folder_tree_opens_nested_files_and_preserves_edits_and_cursor() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir_all(root.join("src")).unwrap();
        let path = root.join("src/main.rs");
        std::fs::write(&path, "fn main() {}\n").unwrap();
        let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.open(&ctx, Some(root.clone()));
        finish_app_job(&mut app, &ctx);
        assert_eq!(
            app.workspace.folders[0].path,
            std::fs::canonicalize(&root).unwrap().to_str().unwrap()
        );
        frame(&ctx, &mut app, vec![]);
        click_label(&ctx, &mut app, "src");
        let started = Instant::now();
        loop {
            let output = frame(&ctx, &mut app, vec![]);
            if output
                .shapes
                .iter()
                .any(|s| text_rect(&s.shape, "main.rs").is_some())
            {
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(5));
        }
        click_label(&ctx, &mut app, "main.rs");
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, "fn main() {}\n");
        let id = app.active().page.info.id;
        let index = app.workspace.active;
        let old = app.active().page.text.clone();
        app.workspace.open[index].page.text = "// unsaved λ\nfn main() {}\n".into();
        app.workspace.edit(index, &old).unwrap();
        app.workspace.open[index].view.head = 5;
        app.open(&ctx, Some(path.clone()));
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.info.id, id);
        assert_eq!(app.active().view.head, 5);
        assert_eq!(app.workspace.open.len(), 2);
        app.workspace.close(index).unwrap();
        app.open(&ctx, Some(path));
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.info.id, id);
        assert!(app.active().page.text.starts_with("// unsaved λ"));
        assert_eq!(app.active().view.head, 5);
        app.execute(&ctx, Command::CloseFolder);
        assert!(app.workspace.folders.is_empty());
        assert_eq!(app.active().page.info.id, id);
        assert!(app.active().page.info.dirty);
    }

    #[test]
    fn folder_palette_finds_unopened_file_and_accepts_enter() {
        use std::path::Path;

        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/unique.rs"), "// found").unwrap();
        let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.open(&ctx, Some(root));
        finish_app_job(&mut app, &ctx);
        app.execute(&ctx, Command::Switcher);
        app.palette_query = "srcunq".into();
        let started = Instant::now();
        loop {
            frame(&ctx, &mut app, vec![]);
            if app.project.search.is_some() {
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            Path::new(
                &app.project
                    .search
                    .as_ref()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .entries[0]
                    .name
            ),
            Path::new("src/unique.rs")
        );
        frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
        finish_app_job(&mut app, &ctx);
        assert!(!app.palette);
        assert_eq!(app.active().page.text, "// found");
        let id = app.active().page.info.id;
        let count = app.workspace.open.len();
        app.execute(&ctx, Command::Switcher);
        app.palette_query = "srcunq".into();
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
        assert!(
            !app.palette,
            "fuzzy matches must also select already open files"
        );
        assert_eq!(app.active().page.info.id, id);
        assert_eq!(app.workspace.open.len(), count);
    }

    #[test]
    fn tabs_stay_above_the_editor_and_leave_the_sidebar_full_height() {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        for size in [egui::vec2(1280.0, 820.0), egui::vec2(720.0, 480.0)] {
            for sidebar in [true, false] {
                app.settings.show_sidebar = sidebar;
                for _ in 0..3 {
                    frame_size(&ctx, &mut app, vec![], size);
                }
                let panel = |name| {
                    egui::containers::panel::PanelState::load(&ctx, egui::Id::new(name))
                        .unwrap()
                        .rect
                };
                let tabs = panel("tabs");
                let menu = panel("menu-bar");
                let status = panel("status");
                assert!((tabs.top() - menu.bottom()).abs() <= 1.0);
                assert!((tabs.right() - size.x).abs() <= 1.0);
                assert_eq!(status.left(), 0.0, "status bar remains full width");
                if sidebar {
                    let side = panel("buffers");
                    assert!(
                        (tabs.left() - side.right()).abs() <= 1.0,
                        "tabs must begin at the editor's left edge"
                    );
                    assert!(
                        (side.top() - menu.bottom()).abs() <= 1.0,
                        "sidebar starts directly below the menu"
                    );
                    assert!((side.bottom() - status.top()).abs() <= 1.0);
                    assert!(!tabs.intersect(side).is_positive());
                } else {
                    assert_eq!(tabs.left(), 0.0);
                }
            }
        }
    }

    #[test]
    fn folder_sidebar_toggles_independently_and_restores_with_multiple_roots() {
        let directory = tempfile::tempdir().unwrap();
        let roots: Vec<_> = ["client", "server"]
            .iter()
            .map(|name| directory.path().join(name))
            .collect();
        for root in &roots {
            std::fs::create_dir(root).unwrap();
        }
        let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], roots.clone());
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.workspace.folders.len(), 2);
        app.open(&ctx, Some(roots[0].clone()));
        finish_app_job(&mut app, &ctx);
        assert_eq!(
            app.workspace.folders.len(),
            2,
            "reopening a root must not duplicate it"
        );
        let output = frame(&ctx, &mut app, vec![]);
        assert!(output
            .shapes
            .iter()
            .any(|s| text_rect(&s.shape, "client").is_some()));
        click_label(&ctx, &mut app, "client");
        assert!(app.workspace.folders[0].collapsed);
        frame(
            &ctx,
            &mut app,
            vec![key(
                Key::B,
                Modifiers {
                    alt: true,
                    ..Modifiers::COMMAND
                },
            )],
        );
        assert!(
            app.settings.show_sidebar,
            "hiding folders keeps the closed-files shortcut"
        );
        assert!(!app.settings.show_folders);
        let output = frame(&ctx, &mut app, vec![]);
        assert!(!output
            .shapes
            .iter()
            .any(|s| text_rect(&s.shape, "client").is_some()));
        assert!(output
            .shapes
            .iter()
            .any(|s| text_rect(&s.shape, "CLOSED FILES    0").is_some()));
        let started = Instant::now();
        while app.checkpoint_pending {
            app.receive(&ctx);
            assert!(started.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(5));
        }
        app.workspace.checkpoint().unwrap();
        let mut storage = MemoryStorage::default();
        eframe::App::save(&mut app, &mut storage);
        drop(app);
        let (workspace, errors) = Workspace::restore(directory.path().join("session")).unwrap();
        assert!(errors.is_empty());
        let mut app = HeditApp::from_context(&ctx, Some(&storage), workspace, vec![], vec![]);
        assert!(!app.settings.show_folders);
        assert_eq!(app.workspace.folders.len(), 2);
        assert!(app.workspace.folders[0].collapsed);
        frame(
            &ctx,
            &mut app,
            vec![key(
                Key::B,
                Modifiers {
                    alt: true,
                    ..Modifiers::COMMAND
                },
            )],
        );
        assert!(app.settings.show_folders);
        // The sidebar toggle is Sublime's Ctrl+K, Ctrl+B; Ctrl+B builds.
        let release = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        frame(&ctx, &mut app, vec![release(Key::B)]);
        frame(
            &ctx,
            &mut app,
            vec![key(Key::K, Modifiers::COMMAND), release(Key::K)],
        );
        frame(
            &ctx,
            &mut app,
            vec![key(Key::B, Modifiers::COMMAND), release(Key::B)],
        );
        assert!(!app.settings.show_sidebar);
        frame(
            &ctx,
            &mut app,
            vec![key(
                Key::B,
                Modifiers {
                    alt: true,
                    ..Modifiers::COMMAND
                },
            )],
        );
        assert!(
            app.settings.show_sidebar && app.settings.show_folders,
            "folder toggle can reveal a hidden sidebar"
        );
    }

    #[test]
    fn quick_open_searches_a_second_root_when_the_folder_sidebar_is_hidden() {
        let directory = tempfile::tempdir().unwrap();
        let roots: Vec<_> = ["client", "server"]
            .iter()
            .map(|name| directory.path().join(name))
            .collect();
        for root in &roots {
            std::fs::create_dir(root).unwrap();
            std::fs::write(
                root.join("main.rs"),
                root.file_name().unwrap().to_str().unwrap(),
            )
            .unwrap();
        }
        let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], roots);
        finish_app_job(&mut app, &ctx);
        app.execute(&ctx, Command::FolderSidebar);
        app.execute(&ctx, Command::Switcher);
        app.palette_query = "server/main".into();
        let started = Instant::now();
        while app.project.search.is_none() {
            frame(&ctx, &mut app, vec![]);
            assert!(started.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(10));
        }
        frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, "server");
        assert!(!app.settings.show_folders);
    }

    #[test]
    fn missing_restored_folder_renders_error_without_losing_unsaved_text() {
        let directory = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
        workspace.folders = vec![hedit::session::FolderView {
            path: directory
                .path()
                .join("missing")
                .to_string_lossy()
                .into_owned(),
            visible: true,
            expanded: vec![],
            collapsed: false,
        }];
        workspace.open[0].page.text = "keep me".into();
        workspace.edit(0, "").unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        frame_size(&ctx, &mut app, vec![], egui::vec2(720.0, 480.0));
        thread::sleep(Duration::from_millis(20));
        frame_size(&ctx, &mut app, vec![], egui::vec2(720.0, 480.0));
        app.execute(&ctx, Command::RefreshFolder);
        app.execute(&ctx, Command::CloseFolder);
        assert_eq!(app.active().page.text, "keep me");
    }
    #[test]
    fn native_ui_accepts_text_and_renders_search_palette_and_cuda_panels() {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        frame(&ctx, &mut app, vec![]);
        frame(
            &ctx,
            &mut app,
            vec![egui::Event::Text("a persistent snippet λ".into())],
        );
        assert_eq!(app.active().page.text, "a persistent snippet λ");
        assert_eq!(
            app.workspace
                .editor
                .get(app.active().page.info.id)
                .unwrap()
                .lock()
                .unwrap()
                .page(0)
                .unwrap()
                .text,
            "a persistent snippet λ"
        );
        app.find_open = true;
        app.gpu_open = true;
        frame(&ctx, &mut app, vec![]);
        app.palette = true;
        frame(&ctx, &mut app, vec![]);
        app.workspace.checkpoint().unwrap();
        drop(app);
        let (restored, errors) = Workspace::restore(directory.path().into()).unwrap();
        assert!(errors.is_empty());
        assert_eq!(
            restored.open[restored.active].page.text,
            "a persistent snippet λ"
        );
    }

    #[test]
    fn native_shortcuts_discard_blank_tabs_and_reopen_nonempty_notes() {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![key(Key::N, Modifiers::COMMAND)]);
        assert_eq!(app.workspace.open.len(), 2);
        frame(&ctx, &mut app, vec![key(Key::W, Modifiers::COMMAND)]);
        assert!(app.workspace.closed.is_empty());
        frame(
            &ctx,
            &mut app,
            vec![key(Key::T, Modifiers::COMMAND | Modifiers::SHIFT)],
        );
        assert_eq!(app.workspace.open.len(), 1);
        frame(&ctx, &mut app, vec![key(Key::N, Modifiers::COMMAND)]);
        frame(
            &ctx,
            &mut app,
            vec![egui::Event::Text("keep this note".into())],
        );
        frame(&ctx, &mut app, vec![key(Key::W, Modifiers::COMMAND)]);
        app.choose_close(&ctx, close::Choice::Discard);
        assert_eq!(app.workspace.closed.len(), 1);
        frame(
            &ctx,
            &mut app,
            vec![key(
                Key::T,
                Modifiers {
                    shift: true,
                    ..Modifiers::COMMAND
                },
            )],
        );
        assert_eq!(app.workspace.open.len(), 2);
        assert!(app.workspace.closed.is_empty());
    }
    #[derive(Default)]
    pub(super) struct MemoryStorage(pub(super) std::collections::HashMap<String, String>);
    #[test]
    fn multiple_cursors_type_delete_undo_and_recover_as_one_document() {
        let directory = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        workspace.open[0].page.text = "α foo β foo".into();
        workspace.edit(0, "").unwrap();
        workspace.open[0].view.anchor = 2;
        workspace.open[0].view.head = 5;
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.execute(&ctx, Command::AllOccurrences);
        assert_eq!(app.selections().len(), 2);
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![egui::Event::Text("🦀".into())]);
        assert_eq!(app.active().page.text, "α 🦀 β 🦀");
        assert_eq!(app.selections().len(), 2);
        app.workspace.checkpoint().unwrap();
        app.execute(&ctx, Command::Undo);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, "α foo β foo");
        app.execute(&ctx, Command::Redo);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, "α 🦀 β 🦀");
        assert_eq!(app.selections().len(), 2);
        app.set_selections(vec![
            hedit::advanced::Selection::caret(3),
            hedit::advanced::Selection::caret(7),
        ]);
        app.workspace.checkpoint().unwrap();
        drop(app);
        let (workspace, errors) = Workspace::restore(directory.path().into()).unwrap();
        assert!(errors.is_empty());
        assert_eq!(workspace.open[workspace.active].view.selections.len(), 1);
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![key(Key::Backspace, Modifiers::NONE)]);
        assert_eq!(app.active().page.text, "α  β ");
    }

    #[test]
    fn move_group_shortcut_exchanges_the_displayed_files() {
        let directory = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        workspace.open[0].page.text = "A".into();
        workspace.edit(0, "").unwrap();
        workspace.new_buffer().unwrap();
        workspace.open[1].page.text = "B".into();
        workspace.edit(1, "").unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.execute(&ctx, Command::SplitColumns);
        frame(&ctx, &mut app, vec![]);
        app.workspace.active = 0;
        frame(&ctx, &mut app, vec![]);
        frame(
            &ctx,
            &mut app,
            vec![key(
                Key::ArrowRight,
                Modifiers {
                    shift: true,
                    alt: true,
                    ..Modifiers::COMMAND
                },
            )],
        );
        assert_eq!(app.panes.focused, 1);
        assert_eq!(app.active().page.text, "A");
        app.execute(&ctx, Command::FocusPane);
        frame(&ctx, &mut app, vec![]);
        assert_eq!(app.active().page.text, "B");
    }

    #[test]
    fn minimap_click_scrolls_and_shift_duplicate_keeps_its_shortcut() {
        let directory = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        workspace.open[0].page.text = "alpha\n".repeat(600);
        workspace.edit(0, "").unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.execute(&ctx, Command::Minimap);
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![]);
        let response = ctx
            .read_response(egui::Id::new(("minimap", 0usize)))
            .unwrap();
        let position = egui::pos2(response.rect.center().x, response.rect.bottom() - 8.0);
        frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        frame(
            &ctx,
            &mut app,
            vec![egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
        );
        assert!(app.active().view.scroll_top > 1000.0);
        frame(&ctx, &mut app, vec![]);
        assert!(app.active().view.scroll_top > 1000.0);
        app.execute(&ctx, Command::Minimap);
        app.focus_editor = true;
        frame(&ctx, &mut app, vec![]);
        let before = app.active().page.text.clone();
        frame(
            &ctx,
            &mut app,
            vec![key(
                Key::D,
                Modifiers {
                    shift: true,
                    ..Modifiers::COMMAND
                },
            )],
        );
        assert_eq!(app.active().page.text.len(), before.len() + 6);
    }

    #[test]
    fn alt_shift_drag_creates_a_rectangle_in_the_native_editor() {
        let directory = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        workspace.open[0].page.text = "abcde\nfghij\nklmno\n".into();
        workspace.edit(0, "").unwrap();
        workspace.open[0].view.anchor = 1;
        workspace.open[0].view.head = 1;
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        frame(&ctx, &mut app, vec![]);
        let id = egui::Id::new(("editor", app.active().page.info.id, 0u64));
        let rect = ctx.read_response(id).unwrap().rect;
        let start = rect.min + egui::vec2(10.0, 8.0);
        let end = rect.min + egui::vec2(26.0, 40.0);
        let modifiers = Modifiers {
            alt: true,
            shift: true,
            ..Modifiers::NONE
        };
        let run = |app: &mut HeditApp, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 820.0),
                    )),
                    modifiers,
                    events,
                    focused: true,
                    ..Default::default()
                },
                |ctx| {
                    app.shortcuts(ctx);
                    app.chrome(ctx);
                    app.editor(ctx);
                },
            )
        };
        run(
            &mut app,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers,
                },
            ],
        );
        run(&mut app, vec![egui::Event::PointerMoved(end)]);
        assert_eq!(app.selections().len(), 3);
        assert_eq!(app.active().page.text, "abcde\nfghij\nklmno\n");
    }

    #[test]
    fn split_views_share_edits_and_undo_but_keep_separate_cursors() {
        let directory = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        workspace.open[0].page.text = "one\ntwo\nthree\n".into();
        workspace.edit(0, "").unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.execute(&ctx, Command::SplitColumns);
        frame(&ctx, &mut app, vec![]);
        app.workspace.open[0].view.anchor = 4;
        app.workspace.open[0].view.head = 4;
        app.execute(&ctx, Command::FocusPane);
        assert_eq!(app.active().view.head, 0);
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![egui::Event::Text("α".into())]);
        assert_eq!(app.active().page.text, "αone\ntwo\nthree\n");
        assert_eq!(app.workspace.open.len(), 1);
        app.execute(&ctx, Command::FocusPane);
        frame(&ctx, &mut app, vec![]);
        assert_eq!(app.active().view.head, 5);
        app.execute(&ctx, Command::Undo);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, "one\ntwo\nthree\n");
        app.execute(&ctx, Command::SplitRows);
        frame(&ctx, &mut app, vec![]);
        app.workspace.checkpoint().unwrap();
        let mut storage = MemoryStorage::default();
        eframe::App::save(&mut app, &mut storage);
        drop(app);
        let (workspace, errors) = Workspace::restore(directory.path().into()).unwrap();
        assert!(errors.is_empty());
        let mut app = HeditApp::from_context(&ctx, Some(&storage), workspace, vec![], vec![]);
        assert_eq!(app.panes.mode, 2);
        frame(&ctx, &mut app, vec![]);
        app.execute(&ctx, Command::SinglePane);
        frame(&ctx, &mut app, vec![]);
        assert_eq!(app.workspace.open.len(), 1);
    }

    #[test]
    fn smart_indentation_tab_stops_and_folds_never_expand_saved_tabs() {
        let directory = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        workspace.open[0].page.text = "\tα();\n".into();
        workspace.edit(0, "").unwrap();
        workspace.open[0].view.tab_size = Some(8);
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![key(Key::ArrowRight, Modifiers::NONE)]);
        assert_eq!(app.active().view.head, 1);
        frame(&ctx, &mut app, vec![egui::Event::Text("β".into())]);
        assert_eq!(app.active().page.text, "\tβα();\n");
        app.workspace.open[0].page.text = "fn f() {\n    α();\n}\n".into();
        let old = "\tβα();\n";
        app.workspace.edit(0, old).unwrap();
        app.workspace.open[0].view.anchor = 0;
        app.workspace.open[0].view.head = 0;
        let source = app.active().page.text.clone();
        app.execute(&ctx, Command::FoldAll);
        frame(&ctx, &mut app, vec![]);
        assert!(!app.active().view.folds.is_empty());
        assert_eq!(app.active().page.text, source);
        app.execute(&ctx, Command::Minimap);
        frame(&ctx, &mut app, vec![]);
        assert_eq!(app.active().page.text, source);
        app.execute(&ctx, Command::SelectAll);
        frame(&ctx, &mut app, vec![]);
        let output = frame(&ctx, &mut app, vec![egui::Event::Copy]);
        assert!(output
            .platform_output
            .commands
            .iter()
            .any(|c| matches!(c,egui::OutputCommand::CopyText(text) if text==&source)));
        app.execute(&ctx, Command::UnfoldAll);
        frame(&ctx, &mut app, vec![]);
        assert!(app.active().view.folds.is_empty());
    }
    impl eframe::Storage for MemoryStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.into(), value);
        }
        fn flush(&mut self) {}
    }
    #[test]
    fn edit_actions_use_backend_undo_and_respect_read_only_and_paged_buffers() {
        let directory = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        workspace.open[0].page.text = "α();\nβ();\n".into();
        workspace.edit(0, "").unwrap();
        workspace.open[0].set_language(Some(syntax::Language::Rust));
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.execute(&ctx, Command::Duplicate);
        assert_eq!(app.active().page.text, "α();\nα();\nβ();\n");
        assert!(app.active().page.info.dirty);
        app.execute(&ctx, Command::Undo);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, "α();\nβ();\n");
        app.execute(&ctx, Command::Redo);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, "α();\nα();\nβ();\n");
        let original = app.active().page.text.clone();
        for paged in [false, true] {
            app.workspace.open[0].follow = !paged;
            app.workspace.open[0].page.info.paged = paged;
            for command in [
                Command::DeleteLine,
                Command::Indent,
                Command::ToggleComment,
                Command::TrimWhitespace,
                Command::ToLf,
                Command::ToCrlf,
            ] {
                assert!(!app.command_enabled(command));
                app.execute(&ctx, command);
                assert_eq!(app.active().page.text, original);
            }
        }
    }

    #[test]
    fn line_ending_conversion_undo_typing_save_and_recovery_keep_bytes_and_policy_together() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("text.txt");
        std::fs::write(&file, "\u{feff}α\nβ\n").unwrap();
        let session = directory.path().join("session");
        let (workspace, _) = Workspace::restore(session.clone()).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.open(&ctx, Some(file.clone()));
        finish_app_job(&mut app, &ctx);
        app.workspace.open[app.workspace.active].view.anchor = 3;
        app.workspace.open[app.workspace.active].view.head = 2;
        app.execute(&ctx, Command::ToCrlf);
        assert_eq!(app.active().page.text, "α\r\nβ\r\n");
        assert_eq!(app.active().page.info.eol, "CRLF");
        assert_eq!((app.active().view.anchor, app.active().view.head), (4, 3));
        app.execute(&ctx, Command::Undo);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, "α\nβ\n");
        assert_eq!(app.active().page.info.eol, "LF");
        app.execute(&ctx, Command::Redo);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.info.eol, "CRLF");
        let index = app.workspace.active;
        let length = app.active().page.text.chars().count();
        app.workspace.open[index].view.anchor = length;
        app.workspace.open[index].view.head = length;
        app.workspace.open[index].restore_cursor = true;
        app.focus_editor = true;
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
        assert_eq!(app.active().page.text, "α\r\nβ\r\n\r\n");
        app.execute(&ctx, Command::Save);
        finish_app_job(&mut app, &ctx);
        assert_eq!(
            std::fs::read_to_string(file).unwrap(),
            "\u{feff}α\r\nβ\r\n\r\n"
        );
        app.workspace.checkpoint().unwrap();
        drop(app);
        let (restored, errors) = Workspace::restore(session).unwrap();
        assert!(errors.is_empty());
        assert_eq!(restored.open[restored.active].page.info.eol, "CRLF");
        assert_eq!(restored.open[restored.active].page.text, "α\r\nβ\r\n\r\n");
    }

    #[test]
    fn column_wrapping_changes_layout_without_changing_text() {
        let directory = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let source = "x".repeat(200);
        workspace.open[0].page.text = source.clone();
        workspace.edit(0, "").unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        let rows = |output: &egui::FullOutput| {
            output
                .shapes
                .iter()
                .find_map(|s| {
                    if let egui::Shape::Text(text) = &s.shape {
                        if text.galley.job.text == source {
                            return Some(text.galley.rows.len());
                        }
                    }
                    None
                })
                .unwrap()
        };
        app.execute(&ctx, Command::WrapWindow);
        let window = rows(&frame(&ctx, &mut app, vec![]));
        app.execute(&ctx, Command::Wrap80);
        app.execute(&ctx, Command::Ruler80);
        let column = rows(&frame(&ctx, &mut app, vec![]));
        assert!(column > window);
        assert_eq!(app.active().page.text, source);
        assert!(app.command_selected(Command::Ruler80));
        app.execute(&ctx, Command::Wrap);
        assert_eq!(rows(&frame(&ctx, &mut app, vec![])), 1);
    }

    #[test]
    fn editor_shortcuts_indent_by_policy_and_leave_search_inputs_alone() {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.execute(&ctx, Command::Indent2);
        // The editor keeps Tab for itself once it has held focus for a frame.
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![key(Key::Tab, Modifiers::NONE)]);
        assert_eq!(app.active().page.text, "  ");
        app.execute(&ctx, Command::IndentTabs);
        frame(&ctx, &mut app, vec![key(Key::Tab, Modifiers::NONE)]);
        assert_eq!(app.active().page.text, "  \t");
        frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::COMMAND)]);
        assert_eq!(app.active().page.text, "  \t\n  \t");
        app.execute(&ctx, Command::Find);
        frame(&ctx, &mut app, vec![]);
        let original = app.active().page.text.clone();
        frame(
            &ctx,
            &mut app,
            vec![key(
                Key::D,
                Modifiers {
                    shift: true,
                    ..Modifiers::COMMAND
                },
            )],
        );
        assert_eq!(app.active().page.text, original);
    }

    #[test]
    fn view_preferences_survive_restart_and_distraction_free_restores_layout() {
        let directory = tempfile::tempdir().unwrap();
        // View settings now live in settings.json in the configuration folder.
        settings_store::TEST_CONFIG_DIR
            .with(|dir| *dir.borrow_mut() = Some(directory.path().join("config")));
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.execute(&ctx, Command::Tabs);
        app.execute(&ctx, Command::CurrentLine);
        app.execute(&ctx, Command::Whitespace);
        app.execute(&ctx, Command::Ruler100);
        app.execute(&ctx, Command::Wrap80);
        let flags = (
            app.settings.show_sidebar,
            app.settings.show_tab_bar,
            app.settings.show_status_bar,
        );
        app.execute(&ctx, Command::DistractionFree);
        let output = frame(&ctx, &mut app, vec![]);
        assert!(!output
            .shapes
            .iter()
            .any(|s| text_rect(&s.shape, "CLOSED FILES    0").is_some()));
        assert_eq!(
            (
                app.settings.show_sidebar,
                app.settings.show_tab_bar,
                app.settings.show_status_bar
            ),
            flags
        );
        app.execute(&ctx, Command::DistractionFree);
        let output = frame(&ctx, &mut app, vec![]);
        assert!(output
            .shapes
            .iter()
            .any(|s| text_rect(&s.shape, "CLOSED FILES    0").is_some()));
        let mut storage = MemoryStorage::default();
        eframe::App::save(&mut app, &mut storage);
        drop(app);
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let restored = HeditApp::from_context(&ctx, Some(&storage), workspace, vec![], vec![]);
        assert!(!restored.settings.show_tab_bar);
        assert!(!restored.settings.highlight_line);
        assert!(restored.settings.show_status_bar && restored.settings.draw_white_space);
        assert_eq!(restored.settings.rulers, [100]);
        assert_eq!(restored.settings.wrap_width, 80);
        assert!(restored.settings.word_wrap);
        assert!(!restored.distraction_free);
        let file = std::fs::read_to_string(directory.path().join("config/settings.json")).unwrap();
        assert!(file.contains("\"rulers\": [\n    100\n  ]"), "{file}");
        settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn unsupported_platforms_hide_cuda_tools_but_keep_cuda_editing() {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        assert!(!app.command_enabled(Command::Gpu));
        assert!(!app.command_enabled(Command::ToolSettings));
        app.execute(&ctx, Command::Gpu);
        assert!(!app.gpu_open);
        app.open_preferences(PreferencesPage::Tools);
        assert_eq!(app.preferences_page, PreferencesPage::General);
        app.workspace.open[0].set_language(Some(syntax::Language::Cuda));
        assert_eq!(app.active().language(), syntax::Language::Cuda);
        assert_eq!(
            Command::Open.shortcut(),
            if cfg!(target_os = "macos") {
                "Cmd+O"
            } else {
                "Ctrl+O"
            }
        );
    }
    #[test]
    fn appearance_follows_system_and_saves_custom_themes() {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        let dark = hedit::theme::builtin("Dracula").unwrap();
        let light = hedit::theme::builtin("GitHub Light").unwrap();
        app.choose_theme(&ctx, dark.clone());
        app.set_follow_system(&ctx, true);
        app.appearance.light = light.clone();
        let system = |app: &mut HeditApp, theme| {
            let input = egui::RawInput {
                system_theme: Some(theme),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| app.sync_appearance(ctx));
        };
        system(&mut app, egui::Theme::Light);
        assert_eq!(app.theme, light);
        assert!(!ctx.style().visuals.dark_mode);
        system(&mut app, egui::Theme::Dark);
        assert_eq!(app.theme, dark);
        // Editing a built-in marks it modified and updates the displayed slot.
        let before = app.theme.clone();
        app.theme.accent = Rgb::new(1, 2, 3);
        app.theme_edited(&ctx, &before);
        assert_eq!(app.theme.name, "Dracula (modified)");
        assert_eq!(app.appearance.dark, app.theme);
        let library = directory.path().join("themes");
        app.appearance.dir = Some(library.clone());
        app.save_theme(&ctx, "Midnight".into(), None);
        assert_eq!(app.error, None);
        assert_eq!(app.appearance.library.len(), 1);
        assert_eq!(app.theme.name, "Midnight");
        assert!(library.join("midnight.json").is_file());
        // Reusing a saved name is refused rather than overwriting.
        app.save_theme(&ctx, "midnight".into(), None);
        assert!(app.error.take().unwrap().contains("already named"));
        let mut storage = MemoryStorage::default();
        eframe::App::save(&mut app, &mut storage);
        let saved = app.theme.clone();
        drop(app);
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let app = HeditApp::from_context(&ctx, Some(&storage), workspace, vec![], vec![]);
        assert!(app.appearance.follow_system);
        assert_eq!(app.appearance.dark, saved);
        assert_eq!(app.appearance.light, light);
    }
    #[test]
    fn custom_appearance_survives_preferences_restart() {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        let mut custom = Theme::paper();
        custom.name = "My paper".into();
        custom.accent = Rgb::new(109, 65, 180);
        app.set_theme(&ctx, custom.clone());
        app.open_appearance();
        frame(&ctx, &mut app, vec![]);
        let mut storage = MemoryStorage::default();
        eframe::App::save(&mut app, &mut storage);
        drop(app);
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let app = HeditApp::from_context(&ctx, Some(&storage), workspace, vec![], vec![]);
        assert_eq!(app.theme, custom);
        assert_eq!(
            ctx.style().visuals.text_cursor.stroke.color,
            custom.accent.color()
        );
        assert!(!ctx.style().visuals.dark_mode);
    }
    #[test]
    fn invalid_saved_theme_keeps_editor_usable_and_reports_error() {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let ctx = egui::Context::default();
        let mut storage = MemoryStorage::default();
        eframe::Storage::set_string(&mut storage, "theme", "broken JSON".into());
        let mut app = HeditApp::from_context(&ctx, Some(&storage), workspace, vec![], vec![]);
        assert_eq!(app.theme, Theme::default());
        assert!(app.error.as_deref().unwrap().contains("Saved appearance"));
        frame(&ctx, &mut app, vec![]);
    }

    pub(super) fn text_rect(shape: &egui::Shape, label: &str) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.galley.rect.translate(text.pos.to_vec2()))
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| text_rect(shape, label)),
            _ => None,
        }
    }
    fn filled_rect_at(shape: &egui::Shape, point: egui::Pos2, fill: egui::Color32) -> bool {
        match shape {
            egui::Shape::Rect(rect) => rect.fill == fill && rect.rect.contains(point),
            egui::Shape::Vec(shapes) => shapes.iter().any(|s| filled_rect_at(s, point, fill)),
            _ => false,
        }
    }
    #[test]
    fn hovered_menu_titles_and_entries_are_highlighted() {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        let size = egui::vec2(1280.0, 820.0);
        for theme in hedit::theme::builtins() {
            app.set_theme(&ctx, theme.clone());
            // Menus fade in; skip the animation so fills are fully opaque.
            ctx.all_styles_mut(|style| style.animation_time = 0.0);
            let hover = theme.hover().color();
            let find = |output: &egui::FullOutput, label: &str| {
                output
                    .shapes
                    .iter()
                    .find_map(|s| text_rect(&s.shape, label))
                    .unwrap()
                    .center()
            };
            let highlighted = |output: &egui::FullOutput, point| {
                output
                    .shapes
                    .iter()
                    .any(|s| filled_rect_at(&s.shape, point, hover))
            };
            let output = frame_size(&ctx, &mut app, vec![], size);
            let tools = find(&output, "Tools");
            frame_size(&ctx, &mut app, vec![egui::Event::PointerMoved(tools)], size);
            let output = frame_size(&ctx, &mut app, vec![], size);
            assert!(highlighted(&output, tools), "{} menu title", theme.name);
            click_label(&ctx, &mut app, "File");
            let output = frame_size(&ctx, &mut app, vec![], size);
            let item = find(&output, "New file");
            frame_size(&ctx, &mut app, vec![egui::Event::PointerMoved(item)], size);
            let output = frame_size(&ctx, &mut app, vec![], size);
            if let Some(dir) = std::env::var_os("HEDIT_PREVIEW_DIR") {
                let svg = crate::preview::export(
                    &output.shapes,
                    [size.x, size.y],
                    theme.background.color(),
                );
                let slug = hedit::theme::slug(&theme.name);
                std::fs::write(
                    PathBuf::from(dir).join(format!("{slug}-menu-hover.svg")),
                    svg,
                )
                .unwrap();
            }
            assert!(highlighted(&output, item), "{} menu entry", theme.name);
            let file = find(&output, "File");
            assert!(highlighted(&output, file), "{} open menu title", theme.name);
            // Unavailable entries, such as Undo with nothing to undo, still
            // show which row the pointer is on, with a fainter box.
            let reopen = find(&output, "Reopen closed tab");
            frame_size(
                &ctx,
                &mut app,
                vec![egui::Event::PointerMoved(reopen)],
                size,
            );
            let output = frame_size(&ctx, &mut app, vec![], size);
            let faint = theme.hover().color().gamma_multiply(0.6);
            assert!(
                output
                    .shapes
                    .iter()
                    .any(|s| filled_rect_at(&s.shape, reopen, faint)),
                "{} disabled menu entry",
                theme.name
            );
            if let Some(dir) = std::env::var_os("HEDIT_PREVIEW_DIR") {
                let svg = crate::preview::export(
                    &output.shapes,
                    [size.x, size.y],
                    theme.background.color(),
                );
                let slug = hedit::theme::slug(&theme.name);
                std::fs::write(
                    PathBuf::from(dir).join(format!("{slug}-menu-hover-disabled.svg")),
                    svg,
                )
                .unwrap();
            }
            frame_size(
                &ctx,
                &mut app,
                vec![key(Key::Escape, Modifiers::NONE)],
                size,
            );
        }
    }
    pub(super) fn click_label(ctx: &egui::Context, app: &mut HeditApp, label: &str) {
        click_label_sized(ctx, app, label, egui::vec2(1280.0, 820.0));
    }
    fn click_label_sized(ctx: &egui::Context, app: &mut HeditApp, label: &str, size: egui::Vec2) {
        let output = frame_size(ctx, app, vec![], size);
        let rect = output
            .shapes
            .iter()
            .find_map(|shape| text_rect(&shape.shape, label))
            .unwrap_or_else(|| panic!("Missing visible label: {label}"));
        click_at(ctx, app, rect.center(), size);
    }
    /// Press and release the primary button at `position` over two frames.
    pub(super) fn click_at(
        ctx: &egui::Context,
        app: &mut HeditApp,
        position: egui::Pos2,
        size: egui::Vec2,
    ) {
        frame_size(
            ctx,
            app,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ],
            size,
        );
        frame_size(
            ctx,
            app,
            vec![egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
            size,
        );
    }
    #[test]
    fn status_bar_selects_syntax_and_returns_to_automatic_without_editing_text() {
        for size in [egui::vec2(1280.0, 820.0), egui::vec2(720.0, 480.0)] {
            let directory = tempfile::tempdir().unwrap();
            let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
            let ctx = egui::Context::default();
            let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
            frame_size(&ctx, &mut app, vec![], size);
            frame_size(
                &ctx,
                &mut app,
                vec![egui::Event::Text("print('keep λ')".into())],
                size,
            );
            let generation = app.generation;
            click_label_sized(&ctx, &mut app, "Plain text ⏷", size);
            let dismissed = frame_size(
                &ctx,
                &mut app,
                vec![key(Key::Escape, Modifiers::NONE)],
                size,
            );
            assert!(!dismissed.shapes.iter().any(|shape| text_rect(
                &shape.shape,
                "Syntax for this file"
            )
            .is_some()));
            assert_eq!(app.generation, generation);
            assert!(app.active().view.language_override.is_none());
            click_label_sized(&ctx, &mut app, "Plain text ⏷", size);
            let output = frame_size(
                &ctx,
                &mut app,
                vec![egui::Event::Text("python".into())],
                size,
            );
            assert_eq!(app.syntax_query, "python");
            assert_eq!(app.active().page.text, "print('keep λ')");
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            for label in ["Syntax for this file", "Automatic (Plain text)", "Python"] {
                let rect = output
                    .shapes
                    .iter()
                    .find_map(|shape| text_rect(&shape.shape, label))
                    .unwrap_or_else(|| panic!("Missing syntax menu label: {label}"));
                assert!(
                    screen.contains_rect(rect),
                    "Clipped syntax menu label: {label}"
                );
            }
            click_label_sized(&ctx, &mut app, "Python", size);
            assert_eq!(app.active().language(), syntax::Language::Python);
            assert_eq!(
                app.active().view.language_override.as_deref(),
                Some("python")
            );
            assert!(app.generation > generation);
            assert_eq!(app.active().page.text, "print('keep λ')");
            let output = frame_size(&ctx, &mut app, vec![], size);
            assert!(
                output.shapes.iter().any(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == "print('keep λ')" => {
                        text.galley
                            .job
                            .sections
                            .iter()
                            .any(|section| section.format.color == app.theme.string.color())
                    }
                    _ => false,
                }),
                "Editor did not apply the selected syntax"
            );
            // Syntax selection restores typing focus without modifying text.
            frame_size(
                &ctx,
                &mut app,
                vec![egui::Event::Text(" # more".into())],
                size,
            );
            assert_eq!(app.active().page.text, "print('keep λ') # more");
            click_label_sized(&ctx, &mut app, "Python ⏷", size);
            click_label_sized(&ctx, &mut app, "Automatic (Plain text)", size);
            assert_eq!(app.active().language(), syntax::Language::PlainText);
            assert!(app.active().view.language_override.is_none());
            assert_eq!(app.active().page.text, "print('keep λ') # more");
            click_label_sized(&ctx, &mut app, "Plain text ⏷", size);
            let output = frame_size(
                &ctx,
                &mut app,
                vec![egui::Event::Text("no-such-syntax".into())],
                size,
            );
            assert!(output.shapes.iter().any(|shape| text_rect(
                &shape.shape,
                "No matching syntax"
            )
            .is_some()));
            frame_size(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)], size);
            assert_eq!(app.active().page.text, "print('keep λ') # more");
            assert!(app.active().view.language_override.is_none());
            frame_size(
                &ctx,
                &mut app,
                vec![key(Key::Escape, Modifiers::NONE)],
                size,
            );
            click_label_sized(&ctx, &mut app, "Plain text ⏷", size);
            frame_size(&ctx, &mut app, vec![egui::Event::Text(".hpp".into())], size);
            frame_size(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)], size);
            assert_eq!(app.active().language(), syntax::Language::Cpp);
            assert_eq!(app.active().page.text, "print('keep λ') # more");
        }
    }
    #[test]
    fn syntax_picker_hover_and_filter_reuse_editor_layout_and_keep_popup_geometry() {
        fn editor_galley(shape: &egui::Shape, source: &str) -> Option<Arc<egui::Galley>> {
            match shape {
                egui::Shape::Text(text) if text.galley.job.text == source => {
                    Some(Arc::clone(&text.galley))
                }
                egui::Shape::Vec(shapes) => {
                    shapes.iter().find_map(|shape| editor_galley(shape, source))
                }
                _ => None,
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let source = "__global__ void kernel() { int n = 42; } // λ\n".repeat(4000);
        workspace.open[0].page.text.clone_from(&source);
        workspace.edit(0, "").unwrap();
        workspace.open[0].set_language(Some(syntax::Language::Cuda));
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        let size = egui::vec2(720.0, 480.0);
        frame_size(&ctx, &mut app, vec![], size);
        let first = frame_size(&ctx, &mut app, vec![], size);
        let galley = first
            .shapes
            .iter()
            .find_map(|shape| editor_galley(&shape.shape, &source))
            .unwrap();
        click_label_sized(&ctx, &mut app, "CUDA ⏷", size);
        for _ in 0..3 {
            frame_size(&ctx, &mut app, vec![], size);
        }
        let popup_id = egui::Id::new("syntax-popup");
        let rect = ctx.memory(|memory| memory.area_rect(popup_id)).unwrap();
        assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(rect));
        for event in [
            egui::Event::PointerMoved(rect.center()),
            egui::Event::Text("python".into()),
        ] {
            let frame = frame_size(&ctx, &mut app, vec![event], size);
            let current = frame
                .shapes
                .iter()
                .find_map(|shape| editor_galley(&shape.shape, &source))
                .unwrap();
            assert!(
                Arc::ptr_eq(&galley, &current),
                "Menu interaction rebuilt unchanged editor layout"
            );
            let current_rect = ctx.memory(|memory| memory.area_rect(popup_id)).unwrap();
            assert!((current_rect.min - rect.min).length() < 0.5);
            assert!((current_rect.size() - rect.size()).length() < 0.5);
        }
        assert_eq!(app.syntax_matches, [syntax::Language::Python]);
        let selected = frame_size(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)], size);
        let changed = selected
            .shapes
            .iter()
            .find_map(|shape| editor_galley(&shape.shape, &source))
            .unwrap();
        assert!(
            !Arc::ptr_eq(&galley, &changed),
            "Changing syntax did not invalidate the editor layout"
        );
        assert_eq!(app.active().language(), syntax::Language::Python);
        assert_eq!(app.active().page.text, source);
    }
    #[test]
    fn menus_open_unified_preferences_and_keep_text_safe() {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        frame(&ctx, &mut app, vec![]);
        frame(
            &ctx,
            &mut app,
            vec![egui::Event::Text("keep these notes".into())],
        );
        click_label(&ctx, &mut app, "Preferences");
        click_label(&ctx, &mut app, "Settings…");
        assert!(app.preferences_open);
        click_label(&ctx, &mut app, "Editor");
        click_label(&ctx, &mut app, "Wrap long lines");
        assert!(app.settings.word_wrap);
        click_label(&ctx, &mut app, "Appearance");
        app.appearance.query = "paper".into();
        click_label(&ctx, &mut app, "Hedit Paper");
        assert_eq!(app.theme, Theme::paper());
        if hedit::debugger::LOCAL_TOOLS_SUPPORTED {
            click_label(&ctx, &mut app, "CUDA Tools");
        }
        click_label(&ctx, &mut app, "Recovery");
        assert_eq!(app.active().page.text, "keep these notes");
        frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
        assert!(!app.preferences_open);
    }
    #[test]
    fn preferences_restore_layout_and_debugger_path() {
        let directory = tempfile::tempdir().unwrap();
        settings_store::TEST_CONFIG_DIR
            .with(|dir| *dir.borrow_mut() = Some(directory.path().join("config")));
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.settings.word_wrap = true;
        app.settings.show_sidebar = false;
        app.settings.line_numbers = false;
        app.settings.font_size = 19.0;
        app.settings.cuda_gdb_path = "/opt/cuda/bin/cuda-gdb".into();
        let mut storage = MemoryStorage::default();
        eframe::App::save(&mut app, &mut storage);
        drop(app);
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let app = HeditApp::from_context(&ctx, Some(&storage), workspace, vec![], vec![]);
        assert!(app.settings.word_wrap);
        assert!(!app.settings.show_sidebar);
        assert!(!app.settings.line_numbers);
        assert_eq!(app.settings.font_size, 19.0);
        assert_eq!(app.settings.cuda_gdb_path, "/opt/cuda/bin/cuda-gdb");
        settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
    }
    #[test]
    fn command_palette_accepts_typed_command_with_enter() {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        frame(&ctx, &mut app, vec![]);
        frame(
            &ctx,
            &mut app,
            vec![key(
                Key::P,
                Modifiers {
                    shift: true,
                    ..Modifiers::COMMAND
                },
            )],
        );
        frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::Text("color theme".into()),
                key(Key::Enter, Modifiers::NONE),
            ],
        );
        // The command opens the quick theme switcher, which previews as you move.
        assert!(app.palette);
        let original = app.theme.clone();
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![key(Key::ArrowDown, Modifiers::NONE)]);
        let previewed = app.theme.clone();
        assert_ne!(previewed, original);
        assert_eq!(&previewed, &hedit::theme::builtins()[1]);
        frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
        assert!(!app.palette);
        assert_eq!(app.theme, original);
        // Typing filters, and Enter keeps the highlighted theme.
        app.execute(&ctx, Command::Appearance);
        frame(&ctx, &mut app, vec![]);
        frame(
            &ctx,
            &mut app,
            vec![egui::Event::Text("solarized light".into())],
        );
        frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
        assert!(!app.palette);
        assert_eq!(app.theme.name, "Solarized Light");
        assert!(!app.theme.dark);
        assert_eq!(app.active().page.text, "");
    }
    #[test]
    fn menu_cut_keeps_unicode_and_backend_consistent_and_guards_follow() {
        let directory = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        workspace.open[0].page.text = "Aλ🦀Z".into();
        workspace.edit(0, "").unwrap();
        workspace.open[0].view.anchor = 1;
        workspace.open[0].view.head = 3;
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.execute(&ctx, Command::Cut);
        assert_eq!(app.active().page.text, "AZ");
        let id = app.active().page.info.id;
        assert_eq!(
            app.workspace
                .editor
                .get(id)
                .unwrap()
                .lock()
                .unwrap()
                .page(0)
                .unwrap()
                .text,
            "AZ"
        );
        app.workspace.open[0].follow = true;
        app.execute(&ctx, Command::Cut);
        assert!(!app.command_enabled(Command::Paste));
        assert!(!app.command_enabled(Command::Undo));
        assert_eq!(app.active().page.text, "AZ");
    }

    #[test]
    fn find_and_goto_submit_with_enter_without_editing_the_document() {
        fn finish_job(app: &mut HeditApp, ctx: &egui::Context) {
            for _ in 0..200 {
                app.receive(ctx);
                if !app.busy {
                    return;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            panic!("Navigation job did not complete");
        }
        let directory = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let text = "first λ\nsecond\nthird";
        workspace.open[0].page.text = text.into();
        workspace.edit(0, "").unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![key(Key::F, Modifiers::COMMAND)]);
        frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::Text("second".into()),
                key(Key::Enter, Modifiers::NONE),
            ],
        );
        assert!(app.busy, "Find should submit on Enter");
        finish_job(&mut app, &ctx);
        assert_eq!(
            app.active()
                .page
                .text
                .chars()
                .skip(app.active().view.anchor)
                .take(app.active().view.head - app.active().view.anchor)
                .collect::<String>(),
            "second"
        );
        frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
        frame(&ctx, &mut app, vec![key(Key::G, Modifiers::COMMAND)]);
        frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::Text("3".into()),
                key(Key::Enter, Modifiers::NONE),
            ],
        );
        assert!(app.busy, "Go to line should submit on Enter");
        assert!(!app.goto_open);
        finish_job(&mut app, &ctx);
        assert_eq!(
            app.active()
                .page
                .text
                .chars()
                .take(app.active().view.head)
                .filter(|&c| c == '\n')
                .count(),
            2
        );
        assert_eq!(app.active().page.text, text);
    }

    #[test]
    fn compact_preferences_keep_title_and_footer_visible() {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        let size = egui::vec2(720.0, 480.0);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        for preset in hedit::theme::builtins() {
            app.set_theme(&ctx, preset.clone());
            for page in [
                PreferencesPage::Editor,
                PreferencesPage::Appearance,
                PreferencesPage::Tools,
                PreferencesPage::Recovery,
            ] {
                app.open_preferences(page);
                for _ in 0..3 {
                    frame_size(&ctx, &mut app, vec![], size);
                }
                let output = frame_size(&ctx, &mut app, vec![], size);
                for label in ["Preferences", "Open settings.json"] {
                    let rects: Vec<_> = output
                        .shapes
                        .iter()
                        .filter_map(|shape| text_rect(&shape.shape, label))
                        .collect();
                    assert!(!rects.is_empty());
                    assert!(
                        rects.iter().all(|rect| screen.contains_rect(*rect)),
                        "Clipped {label}: {page:?}"
                    );
                }
            }
        }
    }
    #[test]
    fn gutter_skips_wrap_continuations_and_stays_fixed_on_horizontal_scroll() {
        let directory = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        workspace.open[0].page.text = format!("{}\nsecond line\nthird line", "x".repeat(350));
        workspace.edit(0, "").unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.settings.word_wrap = true;
        let mut output = frame(&ctx, &mut app, vec![]);
        for _ in 0..3 {
            output = frame(&ctx, &mut app, vec![]);
        }
        let position = |output: &egui::FullOutput, label| {
            output
                .shapes
                .iter()
                .find_map(|shape| text_rect(&shape.shape, label))
                .unwrap()
        };
        assert!(
            position(&output, "2").top() - position(&output, "1").top()
                > app.settings.font_size * 2.0
        );
        app.settings.word_wrap = false;
        frame(&ctx, &mut app, vec![]);
        let unscrolled = frame(&ctx, &mut app, vec![]);
        app.workspace.open[0].view.scroll_left = 120.0;
        app.workspace.open[0].restore_cursor = true;
        let scrolled = frame(&ctx, &mut app, vec![]);
        assert!((position(&unscrolled, "1").left() - position(&scrolled, "1").left()).abs() < 1.0);
    }

    #[test]
    fn expanded_edit_and_view_submenus_are_usable_at_compact_sizes() {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        let size = egui::vec2(720.0, 480.0);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        for (menu, submenu, command) in [
            ("Edit", "Line", Command::Duplicate),
            ("Edit", "Text", Command::TrimWhitespace),
            ("Edit", "Indentation", Command::Indent2),
            ("View", "Editor", Command::Whitespace),
            ("View", "Side bar", Command::Sidebar),
            ("View", "Font size", Command::ResetZoom),
            ("View", "Ruler", Command::Ruler80),
            ("View", "Word wrap column", Command::Wrap100),
            ("Edit", "Line endings", Command::ToCrlf),
        ] {
            frame_size(
                &ctx,
                &mut app,
                vec![key(Key::Escape, Modifiers::NONE)],
                size,
            );
            click_label_sized(&ctx, &mut app, menu, size);
            click_label_sized(&ctx, &mut app, submenu, size);
            let output = frame_size(&ctx, &mut app, vec![], size);
            let rect = output
                .shapes
                .iter()
                .find_map(|s| text_rect(&s.shape, command.label()))
                .unwrap();
            assert!(screen.contains_rect(rect), "Clipped {}", command.label());
            click_label_sized(&ctx, &mut app, command.label(), size);
        }
        assert_eq!(app.active().view.tab_size, Some(2));
        assert!(app.settings.draw_white_space);
        assert!(!app.settings.show_sidebar);
        assert_eq!(app.active().page.info.eol, "CRLF");
        assert_eq!(app.settings.rulers, [80]);
        assert_eq!(app.settings.wrap_width, 100);
    }

    #[test]
    #[ignore = "writes Edit/View SVG previews to HEDIT_PREVIEW_DIR"]
    fn export_menu_previews() {
        let output =
            PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").expect("set HEDIT_PREVIEW_DIR"));
        std::fs::create_dir_all(&output).unwrap();
        for (slug, preset) in [("ink", Theme::default()), ("paper", Theme::paper())] {
            for (name, menu, submenu) in [
                ("line", "Edit", "Line"),
                ("indentation", "Edit", "Indentation"),
                ("view", "View", "Editor"),
                ("ruler", "View", "Ruler"),
                ("wrap-column", "View", "Word wrap column"),
                ("line-endings", "Edit", "Line endings"),
                ("columns", "", ""),
                ("distraction", "", ""),
                ("split", "", ""),
                ("grid", "", ""),
                ("folded", "", ""),
                ("multiple", "", ""),
            ] {
                let directory = tempfile::tempdir().unwrap();
                let (mut workspace, _) =
                    Workspace::restore(directory.path().join("session")).unwrap();
                workspace.open[0].page.text = "fn main() {\n    let message = \"Hello, hedit\";\n\tprintln!(\"{message}\");\n}\n".into();
                workspace.edit(0, "").unwrap();
                workspace.open[0].set_language(Some(syntax::Language::Rust));
                let ctx = egui::Context::default();
                let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
                app.set_theme(&ctx, preset.clone());
                app.settings.draw_white_space = true;
                if name == "split" {
                    app.execute(&ctx, Command::SplitColumns);
                    app.execute(&ctx, Command::Minimap);
                }
                if name == "grid" {
                    app.execute(&ctx, Command::Grid);
                }
                if name == "folded" {
                    app.execute(&ctx, Command::FoldAll);
                    app.execute(&ctx, Command::Minimap);
                }
                if name == "multiple" {
                    let old = app.active().page.text.clone();
                    app.workspace.open[0].page.text =
                        "let value = α;\nprintln!(\"{value}\");\nvalue += 1;".into();
                    app.workspace.edit(0, &old).unwrap();
                    app.workspace.open[0].view.anchor = 4;
                    app.workspace.open[0].view.head = 9;
                    app.execute(&ctx, Command::AllOccurrences);
                }
                if name == "columns" {
                    app.execute(&ctx, Command::Sidebar);
                    app.execute(&ctx, Command::Ruler80);
                    app.execute(&ctx, Command::Wrap80);
                    let old = app.active().page.text.clone();
                    app.workspace.open[0].page.text = format!(
                        "{}\n// {}",
                        old,
                        "long text to check wrapping at the chosen column. ".repeat(6)
                    );
                    app.workspace.edit(0, &old).unwrap();
                }
                let size = if matches!(name, "columns" | "split" | "grid") {
                    egui::vec2(1280.0, 820.0)
                } else {
                    egui::vec2(720.0, 480.0)
                };
                frame_size(&ctx, &mut app, vec![], size);
                if name == "distraction" {
                    app.execute(&ctx, Command::DistractionFree);
                } else if !menu.is_empty() {
                    click_label_sized(&ctx, &mut app, menu, size);
                    click_label_sized(&ctx, &mut app, submenu, size);
                }
                let mut last = frame_size(&ctx, &mut app, vec![], size);
                for _ in 0..3 {
                    last = frame_size(&ctx, &mut app, vec![], size);
                }
                std::fs::write(
                    output.join(format!("menus-{slug}-{name}.svg")),
                    crate::preview::export(
                        &last.shapes,
                        [size.x, size.y],
                        app.theme.background.color(),
                    ),
                )
                .unwrap();
            }
        }
    }

    #[test]
    #[ignore = "writes folder SVG previews to HEDIT_PREVIEW_DIR"]
    fn export_folder_previews() {
        let output =
            PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").expect("set HEDIT_PREVIEW_DIR"));
        std::fs::create_dir_all(&output).unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for (slug, preset) in [("ink", Theme::default()), ("paper", Theme::paper())] {
            let directory = tempfile::tempdir().unwrap();
            let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
            let ctx = egui::Context::default();
            let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
            app.set_theme(&ctx, preset.clone());
            app.open(&ctx, Some(root.clone()));
            finish_app_job(&mut app, &ctx);
            app.open(&ctx, Some(root.join("examples/cuda/vector_add.cu")));
            finish_app_job(&mut app, &ctx);
            for path in ["src", "src/app", "examples", "examples/cuda"] {
                app.project
                    .expanded
                    .insert(root.join(path).to_string_lossy().into_owned());
            }
            for (name, size) in [
                ("tree", [1280.0, 820.0]),
                ("compact", [720.0, 480.0]),
                ("quick-open", [1280.0, 820.0]),
                ("multiple-roots", [1280.0, 820.0]),
                ("hidden-folders", [720.0, 480.0]),
            ] {
                if name == "quick-open" {
                    app.execute(&ctx, Command::Switcher);
                    app.palette_query = "app".into();
                }
                if name == "multiple-roots" {
                    app.palette = false;
                    let other = directory.path().join("another-project");
                    std::fs::create_dir_all(other.join("src")).unwrap();
                    std::fs::write(other.join("src/main.rs"), "fn main() {}\n").unwrap();
                    app.open(&ctx, Some(other.clone()));
                    finish_app_job(&mut app, &ctx);
                    app.workspace.folders[0].collapsed = true;
                    app.project
                        .expanded
                        .insert(other.join("src").to_string_lossy().into_owned());
                }
                if name == "hidden-folders" {
                    app.execute(&ctx, Command::FolderSidebar);
                }
                let mut last = None;
                for _ in 0..40 {
                    last = Some(frame_size(
                        &ctx,
                        &mut app,
                        vec![],
                        egui::vec2(size[0], size[1]),
                    ));
                    thread::sleep(Duration::from_millis(10));
                }
                std::fs::write(
                    output.join(format!("folder-{slug}-{name}.svg")),
                    crate::preview::export(
                        &last.unwrap().shapes,
                        size,
                        app.theme.background.color(),
                    ),
                )
                .unwrap();
            }
        }
    }

    #[test]
    #[ignore = "manual headless CPU timing for syntax picker with a large file"]
    fn profile_syntax_picker() {
        let directory = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let source =
            "__global__ void kernel(float* out) { out[threadIdx.x] = 42.0f; } // notes λ\n"
                .repeat(3000);
        workspace.open[0].page.text.clone_from(&source);
        workspace.edit(0, "").unwrap();
        workspace.open[0].set_language(Some(syntax::Language::Cuda));
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        click_label(&ctx, &mut app, "CUDA ⏷");
        let start = Instant::now();
        for _ in 0..20 {
            frame(
                &ctx,
                &mut app,
                vec![egui::Event::PointerMoved(egui::pos2(1100.0, 600.0))],
            );
        }
        eprintln!(
            "Syntax picker with {} bytes: {:.2} ms/frame (20 hover frames)",
            source.len(),
            start.elapsed().as_secs_f64() * 1000.0 / 20.0
        );
    }
    #[test]
    #[ignore = "writes SVG layout previews to HEDIT_PREVIEW_DIR"]
    fn export_theme_previews() {
        let output =
            PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").expect("set HEDIT_PREVIEW_DIR"));
        std::fs::create_dir_all(&output).unwrap();
        for preset in hedit::theme::builtins() {
            let directory = tempfile::tempdir().unwrap();
            let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
            let mut doc = Document::open(
                workspace.editor.id(),
                &PathBuf::from("examples/cuda/vector_add.cu"),
            )
            .unwrap();
            let page = doc.page(0).unwrap();
            workspace.add(doc, page).unwrap();
            let ctx = egui::Context::default();
            let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
            app.set_theme(&ctx, preset.clone());
            let slug = preset.name.to_lowercase().replace(' ', "-");
            let mut render_time = 0.0;
            for (name, size, page) in [
                ("editor", [1280.0, 820.0], None),
                ("syntax", [1280.0, 820.0], None),
                ("compact-syntax", [720.0, 480.0], None),
                (
                    "appearance",
                    [1280.0, 820.0],
                    Some(PreferencesPage::Appearance),
                ),
                ("compact", [720.0, 480.0], None),
                (
                    "compact-appearance",
                    [720.0, 480.0],
                    Some(PreferencesPage::Appearance),
                ),
                (
                    "customize",
                    [1280.0, 820.0],
                    Some(PreferencesPage::Appearance),
                ),
                ("switcher", [1280.0, 820.0], None),
                (
                    "preferences-editor",
                    [1280.0, 820.0],
                    Some(PreferencesPage::Editor),
                ),
                (
                    "preferences-general",
                    [1280.0, 820.0],
                    Some(PreferencesPage::General),
                ),
                (
                    "preferences-formatting",
                    [1280.0, 820.0],
                    Some(PreferencesPage::Formatting),
                ),
                (
                    "preferences-keys",
                    [1280.0, 820.0],
                    Some(PreferencesPage::Keys),
                ),
                (
                    "preferences-snippets",
                    [1280.0, 820.0],
                    Some(PreferencesPage::Snippets),
                ),
                (
                    "preferences-folders",
                    [720.0, 480.0],
                    Some(PreferencesPage::Folders),
                ),
                (
                    "preferences-tools",
                    [720.0, 480.0],
                    Some(PreferencesPage::Tools),
                ),
                (
                    "preferences-recovery",
                    [720.0, 480.0],
                    Some(PreferencesPage::Recovery),
                ),
                ("search", [1280.0, 820.0], None),
                ("build", [1280.0, 820.0], None),
                ("cuda", [1280.0, 820.0], None),
                ("dialog", [1280.0, 820.0], None),
            ] {
                frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
                app.preferences_open = page.is_some();
                app.preferences_page = page.unwrap_or(PreferencesPage::Editor);
                app.find_open = name == "search";
                app.query = if name == "search" {
                    "count".into()
                } else {
                    String::new()
                };
                app.replace_open = name == "search";
                app.gpu_open = name == "cuda";
                app.appearance.customizing = name == "customize";
                app.build.open = name == "build";
                app.goto_open = name == "dialog";
                app.goto = if name == "dialog" {
                    "42".into()
                } else {
                    String::new()
                };
                app.error = (name == "dialog")
                    .then(|| "Could not save vector_add.cu: permission denied.".into());
                if name == "build" {
                    app.build.name = "CUDA (nvcc)".into();
                    app.build.status =
                        "[Finished in 1.4s with exit code 1] · 2 locations; F4 jumps to the next"
                            .into();
                    app.build.output = "vector_add.cu(6): error: identifier \"i2\" is undefined\nvector_add.cu(17): warning: variable \"x\" was declared but never referenced\n2 errors detected in the compilation of \"vector_add.cu\".\n[Finished in 1.4s with exit code 1]".into();
                    app.build.locations = hedit::build::locations(
                        &app.build.output,
                        &hedit::build::defaults()[0].file_regex,
                        std::path::Path::new("examples/cuda"),
                        0,
                    );
                    app.build.current = Some(0);
                }
                if name == "switcher" {
                    app.open_theme_palette();
                }
                if name.ends_with("syntax") {
                    click_label_sized(&ctx, &mut app, "CUDA ⏷", egui::vec2(size[0], size[1]));
                }
                // Multiple frames let egui settle sizing/scrollbar geometry.
                let mut last = None;
                for _ in 0..3 {
                    render_time += 0.25;
                    let result = ctx.run(
                        egui::RawInput {
                            time: Some(render_time),
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(size[0], size[1]),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            app.chrome(ctx);
                            app.gpu(ctx);
                            app.search_panel(ctx);
                            app.build_panel(ctx);
                            app.editor(ctx);
                            app.command_palette(ctx);
                            app.preferences(ctx);
                        },
                    );
                    last = Some(result);
                }
                let svg = crate::preview::export(
                    &last.unwrap().shapes,
                    size,
                    app.theme.background.color(),
                );
                std::fs::write(output.join(format!("{slug}-{name}.svg")), svg).unwrap();
            }
            app.theme
                .export(&output.join(format!("{slug}.json")))
                .unwrap();
        }
    }
    #[test]
    fn buffer_tools_render_and_shortcuts_keep_metadata() {
        let directory = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        workspace.open[0].page.text = "notes to keep".into();
        workspace.edit(0, "").unwrap();
        workspace.name_scratchpad(0, "Named notes").unwrap();
        workspace.toggle_bookmark(0).unwrap();
        workspace.close(0).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.shelf_open = true;
        frame(&ctx, &mut app, vec![]);
        app.shelf_open = false;
        app.workspace.reopen_at(0).unwrap();
        app.bookmarks_open = true;
        frame(&ctx, &mut app, vec![]);
        app.bookmarks_open = false;
        app.open_unsaved_file_name();
        frame(&ctx, &mut app, vec![]);
        assert_eq!(app.unsaved_file_name, "Named notes");
        assert_eq!(app.active().view.bookmarks.len(), 1);
    }
}
