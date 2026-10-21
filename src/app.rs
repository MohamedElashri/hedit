mod chrome;
mod commands;
mod editor_layout;
mod preferences;
mod project;
mod tree_row;
mod updates;

use commands::Command;
use eframe::egui::{self, FontId, Key, Modifiers, RichText};
use hedit::{
    brand,
    debugger::{DebugSession, State},
    document::{Document, Info, Page, PAGE_SIZE},
    session::SessionStore,
    syntax,
    theme::{Preset, Rgb, Role, Theme},
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
    Add(Document, Page, usize),
    Activate(u64),
    Reopen(u64, Option<(Page, usize)>),
    Folder(hedit::folder::FolderListing),
    Folders(Vec<hedit::folder::FolderListing>),
    Created(String, bool),
    Page(u64, Page, Option<(usize, usize)>),
    Saved(u64, Info),
    ThemeImported(Theme),
    Notice(String),
    Nothing,
}
struct Checkpoint {
    generation: u64,
    result: Result<(), String>,
}

pub struct HeditApp {
    workspace: Workspace,
    project: project::ProjectBrowser,
    tx: Sender<Result<Action, String>>,
    rx: Receiver<Result<Action, String>>,
    checkpoint_tx: Sender<Checkpoint>,
    checkpoint_rx: Receiver<Checkpoint>,
    checkpoint_pending: bool,
    checkpoint_at: Instant,
    generation: u64,
    persisted: u64,
    last_saved: Option<Instant>,
    busy: bool,
    cancel: Arc<AtomicBool>,
    closing: bool,
    exit_ready: bool,
    pending_paths: VecDeque<PathBuf>,
    error: Option<String>,
    notice: String,
    wrap: bool,
    font_size: f32,
    sidebar: bool,
    folder_sidebar: bool,
    line_numbers: bool,
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
    debugger: String,
    breakpoint: String,
    console: String,
    sanitizer: String,
    poll_at: Instant,
    focus_editor: bool,
    theme: Theme,
    preferences_open: bool,
    appearance_original: Theme,
    theme_hex: Vec<String>,
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
                Ok(theme) => theme,
                Err(error) => {
                    errors.push(format!("Saved appearance could not be loaded: {error}"));
                    Theme::default()
                }
            },
            None => Theme::default(),
        };
        theme.apply(ctx);
        let appearance_original = theme.clone();
        let theme_hex = Role::ALL
            .into_iter()
            .map(|r| theme.color(r).hex())
            .collect();
        let (tx, rx) = mpsc::channel();
        let (checkpoint_tx, checkpoint_rx) = mpsc::channel();
        let wrap = storage
            .and_then(|s| eframe::get_value(s, "wrap"))
            .unwrap_or(false);
        let font_size = storage
            .and_then(|s| eframe::get_value::<f32>(s, "font_size"))
            .filter(|size| size.is_finite())
            .unwrap_or(14.0)
            .clamp(10.0, 28.0);
        let project = project::ProjectBrowser::new(&workspace.folders);
        let sidebar = storage
            .and_then(|s| eframe::get_value(s, "sidebar"))
            .unwrap_or(true);
        let folder_sidebar = workspace
            .folders
            .first()
            .map(|f| f.visible)
            .or_else(|| storage.and_then(|s| eframe::get_value(s, "folder_sidebar")))
            .unwrap_or(true);
        Self {
            project,
            workspace,
            tx,
            rx,
            checkpoint_tx,
            checkpoint_rx,
            checkpoint_pending: false,
            checkpoint_at: Instant::now() - Duration::from_secs(3),
            generation: 1,
            persisted: 0,
            last_saved: None,
            busy: false,
            cancel: Arc::new(AtomicBool::new(false)),
            closing: false,
            exit_ready: false,
            pending_paths: paths.into(),
            error: if errors.is_empty() {
                None
            } else {
                Some(errors.join("\n"))
            },
            notice: "Ready.".into(),
            wrap,
            font_size,
            sidebar,
            folder_sidebar,
            line_numbers: storage
                .and_then(|s| eframe::get_value(s, "line_numbers"))
                .unwrap_or(true),
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
            debugger: storage
                .and_then(|s| s.get_string("debugger"))
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "cuda-gdb".into()),
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
            appearance_original,
            theme_hex,
            rename_target: None,
            unsaved_file_name: String::new(),
            unsaved_file_name_focus: false,
            shelf_open: false,
            shelf_query: String::new(),
            bookmarks_open: false,
            updates: updates::Updates::new(
                storage
                    .and_then(|s| eframe::get_value(s, "update_github_cli"))
                    .unwrap_or(false),
            ),
        }
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
        let editor = Arc::clone(&self.workspace.editor);
        let store: Arc<SessionStore> = Arc::clone(&self.workspace.store);
        let view = self.workspace.view();
        let generation = self.generation;
        let sender = self.checkpoint_tx.clone();
        let ctx = ctx.clone();
        self.checkpoint_pending = true;
        self.checkpoint_at = Instant::now();
        thread::spawn(move || {
            let result = store.save(&editor, view);
            let _ = sender.send(Checkpoint { generation, result });
            ctx.request_repaint();
        });
    }
    fn open(&mut self, ctx: &egui::Context, path: Option<PathBuf>) {
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
        self.job(ctx, move || {
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
        });
    }
    fn load_page(&mut self, ctx: &egui::Context, offset: u64, reload: bool) {
        let editor = Arc::clone(&self.workspace.editor);
        let id = self.active().page.info.id;
        let path = self.active().page.info.path.clone();
        if reload && self.active().page.info.dirty {
            self.error = Some(
                "This file has unsaved changes. Use Save As to keep them before reloading.".into(),
            );
            return;
        }
        self.job(ctx, move || {
            let shared = editor.get(id)?;
            let mut doc = shared.lock().map_err(|e| e.to_string())?;
            if reload {
                let path = path.ok_or("This unsaved file has no disk copy to reload.")?;
                let replacement = Document::open(id, &PathBuf::from(path))?;
                *doc = replacement;
            }
            let offset = if offset == u64::MAX {
                doc.info().size.saturating_sub(PAGE_SIZE)
            } else {
                offset
            };
            let page = doc.load_page(offset)?;
            Ok(Action::Page(id, page, None))
        });
    }
    fn save_file(&mut self, ctx: &egui::Context, save_as: bool) {
        let id = self.active().page.info.id;
        let name = self
            .active()
            .view
            .custom_name
            .clone()
            .unwrap_or_else(|| self.active().page.info.name.clone());
        let choose = save_as || self.active().page.info.path.is_none();
        let editor = Arc::clone(&self.workspace.editor);
        let directory = self
            .workspace
            .folders
            .iter()
            .find(|f| {
                self.active()
                    .page
                    .info
                    .path
                    .as_ref()
                    .is_some_and(|path| std::path::Path::new(path).starts_with(&f.path))
            })
            .or_else(|| self.workspace.folders.first())
            .map(|f| f.path.clone());
        self.job(ctx, move || {
            let target = if choose {
                let mut dialog = rfd::FileDialog::new();
                if let Some(directory) = directory {
                    dialog = dialog.set_directory(directory);
                }
                match dialog
                    .set_title("Save file — hedit")
                    .set_file_name(name)
                    .save_file()
                {
                    Some(path) => Some(path),
                    None => return Ok(Action::Nothing),
                }
            } else {
                None
            };
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
        let editor = Arc::clone(&self.workspace.editor);
        self.job(ctx, move || {
            let page = editor
                .get(id)?
                .lock()
                .map_err(|e| e.to_string())?
                .history(redo, offset, revision)?;
            Ok(Action::Page(id, page, None))
        });
    }
    fn find(&mut self, ctx: &egui::Context) {
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
        let index = self.workspace.active;
        let buffer = &mut self.workspace.open[index];
        let start = buffer.view.anchor.min(buffer.view.head);
        let end = buffer.view.anchor.max(buffer.view.head);
        let byte_start = buffer
            .page
            .text
            .char_indices()
            .nth(start)
            .map_or(buffer.page.text.len(), |(i, _)| i);
        let byte_end = buffer
            .page
            .text
            .char_indices()
            .nth(end)
            .map_or(buffer.page.text.len(), |(i, _)| i);
        if self.query.is_empty() || buffer.page.text[byte_start..byte_end] != self.query {
            self.notice = "Find a matching selection before replacing.".into();
            return;
        }
        let old = buffer.page.text.clone();
        buffer
            .page
            .text
            .replace_range(byte_start..byte_end, &self.replacement);
        buffer.view.anchor = start;
        buffer.view.head = start + self.replacement.chars().count();
        buffer.restore_cursor = true;
        let result = self.workspace.edit(index, &old);
        self.report(result);
    }
    fn goto_line(&mut self, ctx: &egui::Context) {
        let Ok(line) = self.goto.parse::<u64>() else {
            self.error = Some("Enter a positive line number.".into());
            return;
        };
        if line == 0 {
            return;
        }
        let id = self.active().page.info.id;
        let editor = Arc::clone(&self.workspace.editor);
        self.job(ctx, move || {
            let shared = editor.get(id)?;
            let mut doc = shared.lock().map_err(|e| e.to_string())?;
            let offset = doc.location_offset(line, 1)?;
            let page = doc.page(offset.saturating_sub(4096))?;
            let cursor = page.text[..(offset - page.start) as usize].chars().count();
            Ok(Action::Page(id, page, Some((cursor, cursor))))
        });
    }
    fn receive(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.rx.try_recv() {
            self.busy = false;
            match event {
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
                    }
                }
                Ok(Action::Reopen(id, location)) => {
                    if let Some(index) = self.workspace.closed.iter().position(|v| v.id == id) {
                        let result = self.workspace.reopen_at(index);
                        if result.is_ok() {
                            if let Some((page, cursor)) = location {
                                let buffer = &mut self.workspace.open[self.workspace.active];
                                buffer.set_page(page);
                                buffer.view.anchor = cursor;
                                buffer.view.head = cursor;
                                buffer.reveal_cursor = true;
                            }
                        }
                        self.report(result);
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
                }
                Ok(Action::Page(id, page, selection)) => {
                    if let Some(index) = self
                        .workspace
                        .open
                        .iter()
                        .position(|b| b.page.info.id == id)
                    {
                        self.workspace.active = index;
                        let buffer = &mut self.workspace.open[index];
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
                        self.focus_editor = true;
                    }
                }
                Ok(Action::Saved(id, info)) => {
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
                    self.notice = "File saved.".into();
                    self.changed();
                }
                Ok(Action::ThemeImported(theme)) => {
                    self.set_theme(ctx, theme);
                    self.notice = "Theme imported.".into();
                }
                Ok(Action::Notice(message)) => self.notice = message,
                Ok(Action::Nothing) => (),
                Err(error) => {
                    self.error = Some(error);
                    self.closing = false;
                }
            }
        }
        while let Ok(checkpoint) = self.checkpoint_rx.try_recv() {
            self.checkpoint_pending = false;
            match checkpoint.result {
                Ok(()) => {
                    self.persisted = checkpoint.generation;
                    self.last_saved = Some(Instant::now());
                }
                Err(error) => {
                    self.error = Some(format!("RECOVERY CHECKPOINT FAILED\n{error}\nKeep hedit open and save your files to disk."));
                    self.closing = false;
                }
            }
        }
        if !self.busy && !self.closing {
            if let Some(path) = self.pending_paths.pop_front() {
                self.open(ctx, Some(path));
            }
        }
        if self.closing && !self.busy && !self.checkpoint_pending && !self.updates.installing() {
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
    fn poll_external(&mut self, ctx: &egui::Context) {
        if self.busy || self.poll_at.elapsed() < Duration::from_secs(1) {
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
            if buffer.follow && !buffer.page.info.dirty {
                self.load_page(ctx, u64::MAX, true);
            }
        }
    }
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.busy || self.closing {
            return;
        }
        let cmd = Modifiers::COMMAND;
        let shift = Modifiers { shift: true, ..cmd };
        let take = |m, k| ctx.input_mut(|i| i.consume_key(m, k));
        for (modifiers, key, command) in [
            (shift, Key::P, Command::Palette),
            (cmd, Key::P, Command::Switcher),
            (
                Modifiers { alt: true, ..cmd },
                Key::B,
                Command::FolderSidebar,
            ),
            (cmd, Key::B, Command::Sidebar),
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
            (cmd, Key::H, Command::Replace),
            (cmd, Key::G, Command::Goto),
            (shift, Key::W, Command::Wrap),
        ] {
            if take(modifiers, key) {
                self.execute(ctx, command);
            }
        }
        // Let single-line search and debugger inputs keep their own undo history.
        let editor_id = egui::Id::new((
            "editor",
            self.active().page.info.id,
            self.active().page.start,
        ));
        if ctx.memory(|m| m.has_focus(editor_id)) {
            if take(shift, Key::Z) || take(cmd, Key::Y) {
                self.execute(ctx, Command::Redo);
            } else if take(cmd, Key::Z) {
                self.execute(ctx, Command::Undo);
            }
        }
        if take(cmd, Key::Equals) || take(cmd, Key::Plus) {
            self.font_size = (self.font_size + 1.0).min(28.0);
        }
        if take(cmd, Key::Minus) {
            self.font_size = (self.font_size - 1.0).max(10.0);
        }
        // Leave Escape visible to egui so open menus can dismiss themselves too.
        if ctx.input(|input| input.key_pressed(Key::Escape)) {
            self.palette = false;
            self.find_open = false;
            self.goto_open = false;
            self.preferences_open = false;
            self.about_open = false;
            self.updates.open = false;
            self.rename_target = None;
            self.shelf_open = false;
            self.bookmarks_open = false;
            self.project.create = None;
            self.focus_editor = true;
        }
    }
    fn editor(&mut self, ctx: &egui::Context) {
        let theme = self.theme.clone();
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme.background.color())
                    .inner_margin(egui::Margin::symmetric(12, 10)),
            )
            .show(ctx, |ui| {
                if let Some(error) = self.error.clone() {
                    egui::Frame::new()
                        .fill(theme.error.color().gamma_multiply(0.12))
                        .inner_margin(10.0)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(&error).color(theme.error.color()));
                                if ui.button("Dismiss").clicked() {
                                    self.error = None;
                                }
                                if ui.button("Retry checkpoint").clicked() {
                                    self.checkpoint(ctx);
                                }
                            });
                        });
                }
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
                let index = self.workspace.active;
                let buffer = &mut self.workspace.open[index];
                let id = egui::Id::new(("editor", buffer.page.info.id, buffer.page.start));
                if buffer.restore_cursor {
                    let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
                    state.clear_undoer();
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(buffer.view.anchor),
                            egui::text::CCursor::new(buffer.view.head),
                        )));
                    state.store(ctx, id);
                }
                let old = buffer.page.text.clone();
                let language = buffer.language();
                let font_size = self.font_size;
                let wrap = self.wrap;
                let layout_cache = &mut self.editor_layout;
                let mut layouter = move |ui: &egui::Ui, text: &str, wrap_width: f32| {
                    layout_cache.layout(
                        ui,
                        text,
                        language,
                        font_size,
                        if wrap { wrap_width } else { f32::INFINITY },
                        &theme,
                    )
                };
                let mut scroll = egui::ScrollArea::both()
                    .id_salt(("page-scroll", buffer.page.info.id, buffer.page.start))
                    .auto_shrink([false, false])
                    .stick_to_bottom(buffer.follow);
                if buffer.restore_cursor {
                    scroll = scroll
                        .vertical_scroll_offset(buffer.view.scroll_top as f32)
                        .horizontal_scroll_offset(buffer.view.scroll_left as f32);
                }
                let restore = buffer.restore_cursor;
                let gutter = if self.line_numbers {
                    let digits = (buffer
                        .page
                        .text
                        .bytes()
                        .filter(|&byte| byte == b'\n')
                        .count()
                        + 1)
                    .to_string()
                    .len();
                    (digits.max(3) as f32 * font_size * 0.65 + 16.0).max(42.0)
                } else {
                    0.0
                };
                let gutter_color = self.theme.muted.color();
                let gutter_background = self.theme.background.color();
                let gutter_text = self.theme.text.color();
                let current_line = buffer
                    .page
                    .text
                    .chars()
                    .take(buffer.view.head)
                    .filter(|&c| c == '\n')
                    .count()
                    + 1;
                let scroll_output = scroll.show(ui, |ui| {
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
                        let mut edit = egui::TextEdit::multiline(&mut buffer.page.text)
                            .id(id)
                            .font(FontId::monospace(font_size))
                            .code_editor()
                            .lock_focus(true)
                            .frame(false)
                            .desired_rows(30)
                            .desired_width(if self.wrap {
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
                                    && !self.updates.open,
                            );
                        if !self.wrap {
                            edit = edit.desired_width(f32::INFINITY);
                        }
                        if self.focus_editor {
                            ctx.memory_mut(|m| m.request_focus(id));
                        }
                        let mut output = edit.show(ui);
                        if gutter > 0.0 {
                            gutter_painter.rect_filled(gutter_rect, 0.0, gutter_background);
                            let mut line = 1;
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
                                        gutter_painter.text(
                                            position,
                                            egui::Align2::RIGHT_CENTER,
                                            line.to_string(),
                                            FontId::monospace(font_size - 1.0),
                                            if line == current_line {
                                                gutter_text
                                            } else {
                                                gutter_color
                                            },
                                        );
                                    }
                                }
                                first_row = row.ends_with_newline;
                                if row.ends_with_newline {
                                    line += 1;
                                }
                            }
                        }
                        if self.focus_editor {
                            output.response.request_focus();
                            self.focus_editor = false;
                        }
                        if restore && buffer.reveal_cursor {
                            if let Some(range) = output.cursor_range {
                                let rect = output
                                    .galley
                                    .pos_from_cursor(&range.primary)
                                    .translate(output.galley_pos.to_vec2());
                                ui.scroll_to_rect(rect, Some(egui::Align::Center));
                            }
                        }
                        if let Some(range) = output.cursor_range {
                            let range = range.as_ccursor_range();
                            let previous = (buffer.view.anchor, buffer.view.head);
                            buffer.view.anchor = range.secondary.index;
                            buffer.view.head = range.primary.index;
                            if previous != (buffer.view.anchor, buffer.view.head) {
                                self.generation = self.generation.wrapping_add(1);
                            }
                        }
                        // Backend history is authoritative across pages. Do not retain a
                        // second, potentially large TextEdit undo history.
                        output.state.clear_undoer();
                        output.state.store(ctx, id);
                        output.response.changed()
                    })
                    .inner
                });
                buffer.restore_cursor = false;
                buffer.reveal_cursor = false;
                if buffer.view.scroll_top != scroll_output.state.offset.y as f64
                    || buffer.view.scroll_left != scroll_output.state.offset.x as f64
                {
                    buffer.view.scroll_top = scroll_output.state.offset.y as f64;
                    buffer.view.scroll_left = scroll_output.state.offset.x as f64;
                    self.generation = self.generation.wrapping_add(1);
                }
                if scroll_output.inner {
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
            });
    }
    fn gpu(&mut self, ctx: &egui::Context) {
        let theme = self.theme.clone();
        if !self.gpu_open {
            return;
        }
        egui::TopBottomPanel::bottom("gpu").resizable(true).default_height(285.0).min_height(160.0).show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("CUDA / GPU").strong().color(theme.accent.color()));
                ui.label(RichText::new("NVIDIA tools · Linux").small().color(theme.muted.color()));
                if let Some(debug) = &self.debug { ui.label(format!("{:?}", debug.state)); }
                if ui.small_button("Close panel").clicked() { self.gpu_open = false; }
            });
            let live = self.debug.as_ref().is_some_and(|d| d.state != State::Exited);
            ui.add_enabled_ui(!live, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Executable");
                    ui.add(egui::TextEdit::singleline(&mut self.debug_program).desired_width(280.0).hint_text("/path/to/cuda-program"));
                    if ui.button("Browse…").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_file() { self.debug_program = path.to_string_lossy().into_owned(); }
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
                            .and_then(|args| DebugSession::launch(&self.debugger, &PathBuf::from(&self.debug_program), &args, None));
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
            self.appearance_original = self.theme.clone();
        }
        self.open_preferences(PreferencesPage::Appearance);
    }
    fn set_theme(&mut self, ctx: &egui::Context, theme: Theme) {
        self.theme_hex = Role::ALL
            .into_iter()
            .map(|r| theme.color(r).hex())
            .collect();
        self.theme = theme;
        self.theme.apply(ctx);
    }
    fn import_theme(&mut self, ctx: &egui::Context) {
        self.job(ctx, || {
            let Some(path) = rfd::FileDialog::new()
                .set_title("Import hedit theme")
                .add_filter("hedit theme", &["json"])
                .pick_file()
            else {
                return Ok(Action::Nothing);
            };
            Ok(Action::ThemeImported(Theme::load(&path)?))
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
                    apply = ui
                        .add_enabled(!self.busy && !self.closing, egui::Button::new("Apply name"))
                        .clicked()
                        || enter;
                });
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
            egui::Window::new("Closed files")
                .open(&mut open)
                .default_width(480.0)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label("Closed files keep their text, names, and bookmarks.");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.shelf_query)
                            .desired_width(f32::INFINITY)
                            .hint_text("Filter closed files…"),
                    );
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
                            "{} of 50 shelf slots used. Unsaved text is never silently evicted.",
                            self.workspace.closed.len()
                        ))
                        .small()
                        .color(self.theme.muted.color()),
                    );
                });
            self.shelf_open = open;
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
            egui::Window::new("Bookmarks")
                .open(&mut open)
                .default_width(480.0)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(RichText::new(&self.active().view.name).strong());
                    if ui
                        .add_enabled(
                            !self.busy && !self.closing,
                            egui::Button::new("Toggle at cursor · Ctrl+F2"),
                        )
                        .clicked()
                    {
                        self.toggle_bookmark();
                    }
                    if bookmarks.is_empty() {
                        ui.label("No bookmarks yet. Mark a position with Ctrl+F2.");
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
            self.bookmarks_open = open;
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
        self.project.poll();
        if let Some(debug) = &mut self.debug {
            debug.poll();
        }
        self.poll_external(ctx);
        if ctx.input(|i| i.viewport().close_requested()) && !self.exit_ready {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.closing = true;
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
        self.chrome(ctx);
        self.gpu(ctx);
        self.search_panel(ctx);
        self.editor(ctx);
        if !self.busy && !self.closing {
            self.command_palette(ctx);
        }
        self.preferences(ctx);
        self.buffer_tools(ctx);
        self.folder_dialog(ctx);
        self.updates_window(ctx);
        let paths: Vec<_> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        self.pending_paths.extend(paths);
        ctx.request_repaint_after(Duration::from_millis(
            if self.debug.is_some() || self.busy || self.updates.active() {
                50
            } else {
                200
            },
        ));
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "wrap", &self.wrap);
        eframe::set_value(storage, "font_size", &self.font_size);
        eframe::set_value(storage, "sidebar", &self.sidebar);
        eframe::set_value(storage, "folder_sidebar", &self.folder_sidebar);
        eframe::set_value(storage, "line_numbers", &self.line_numbers);
        storage.set_string("debugger", self.debugger.clone());
        eframe::set_value(storage, "update_github_cli", &self.updates.use_github_cli);
        let mut theme = self.theme.clone();
        if theme.name.trim().is_empty() {
            theme.name = "My theme".into();
        }
        if let Ok(json) = theme.to_json() {
            storage.set_string("theme", json);
        }
    }
    fn auto_save_interval(&self) -> Duration {
        Duration::from_secs(2)
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Err(error) = self.workspace.checkpoint() {
            eprintln!("hedit: final recovery checkpoint failed: {error}");
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

    fn frame(
        ctx: &egui::Context,
        app: &mut HeditApp,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        frame_size(ctx, app, events, egui::vec2(1280.0, 820.0))
    }
    fn frame_size(
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
            app.editor(ctx);
            app.command_palette(ctx);
            app.preferences(ctx);
            app.buffer_tools(ctx);
            app.folder_dialog(ctx);
        });
        assert!(!output.shapes.is_empty());
        output
    }
    fn key(key: Key, modifiers: Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    fn finish_app_job(app: &mut HeditApp, ctx: &egui::Context) {
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
        assert_eq!(app.workspace.folders[0].path, root.to_str().unwrap());
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
            app.project
                .search
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .entries[0]
                .name,
            "src/unique.rs"
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
                app.sidebar = sidebar;
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
        assert!(app.sidebar, "hiding folders keeps the open-files sidebar");
        assert!(!app.folder_sidebar);
        let output = frame(&ctx, &mut app, vec![]);
        assert!(!output
            .shapes
            .iter()
            .any(|s| text_rect(&s.shape, "client").is_some()));
        assert!(output
            .shapes
            .iter()
            .any(|s| text_rect(&s.shape, "OPEN FILES").is_some()));
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
        assert!(!app.folder_sidebar);
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
        assert!(app.folder_sidebar);
        frame(&ctx, &mut app, vec![key(Key::B, Modifiers::COMMAND)]);
        assert!(!app.sidebar);
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
            app.sidebar && app.folder_sidebar,
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
        assert!(!app.folder_sidebar);
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
    fn native_shortcuts_create_and_reopen_buffers() {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![key(Key::N, Modifiers::COMMAND)]);
        assert_eq!(app.workspace.open.len(), 2);
        frame(&ctx, &mut app, vec![key(Key::W, Modifiers::COMMAND)]);
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
    struct MemoryStorage(std::collections::HashMap<String, String>);
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
    fn custom_appearance_survives_preferences_restart() {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        let mut custom = Preset::Paper.theme();
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

    fn text_rect(shape: &egui::Shape, label: &str) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.galley.rect.translate(text.pos.to_vec2()))
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| text_rect(shape, label)),
            _ => None,
        }
    }
    fn click_label(ctx: &egui::Context, app: &mut HeditApp, label: &str) {
        click_label_sized(ctx, app, label, egui::vec2(1280.0, 820.0));
    }
    fn click_label_sized(ctx: &egui::Context, app: &mut HeditApp, label: &str, size: egui::Vec2) {
        let output = frame_size(ctx, app, vec![], size);
        let rect = output
            .shapes
            .iter()
            .find_map(|shape| text_rect(&shape.shape, label))
            .unwrap_or_else(|| panic!("Missing visible label: {label}"));
        let position = rect.center();
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
            click_label_sized(&ctx, &mut app, "Plain text ▾", size);
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
            click_label_sized(&ctx, &mut app, "Plain text ▾", size);
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
            click_label_sized(&ctx, &mut app, "Python ▾", size);
            click_label_sized(&ctx, &mut app, "Automatic (Plain text)", size);
            assert_eq!(app.active().language(), syntax::Language::PlainText);
            assert!(app.active().view.language_override.is_none());
            assert_eq!(app.active().page.text, "print('keep λ') # more");
            click_label_sized(&ctx, &mut app, "Plain text ▾", size);
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
            click_label_sized(&ctx, &mut app, "Plain text ▾", size);
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
        click_label_sized(&ctx, &mut app, "CUDA ▾", size);
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
        click_label(&ctx, &mut app, "Word wrap");
        assert!(app.wrap);
        click_label(&ctx, &mut app, "Appearance");
        click_label(&ctx, &mut app, "Hedit Paper");
        assert_eq!(app.theme, Preset::Paper.theme());
        click_label(&ctx, &mut app, "CUDA Tools");
        click_label(&ctx, &mut app, "Recovery");
        assert_eq!(app.active().page.text, "keep these notes");
        frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
        assert!(!app.preferences_open);
    }
    #[test]
    fn preferences_restore_layout_and_debugger_path() {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.wrap = true;
        app.sidebar = false;
        app.line_numbers = false;
        app.font_size = 19.0;
        app.debugger = "/opt/cuda/bin/cuda-gdb".into();
        let mut storage = MemoryStorage::default();
        eframe::App::save(&mut app, &mut storage);
        drop(app);
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let app = HeditApp::from_context(&ctx, Some(&storage), workspace, vec![], vec![]);
        assert!(app.wrap);
        assert!(!app.sidebar);
        assert!(!app.line_numbers);
        assert_eq!(app.font_size, 19.0);
        assert_eq!(app.debugger, "/opt/cuda/bin/cuda-gdb");
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
        assert!(!app.palette);
        assert!(app.preferences_open);
        assert_eq!(app.preferences_page, PreferencesPage::Appearance);
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
    fn compact_preferences_keep_title_and_done_visible() {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        let size = egui::vec2(720.0, 480.0);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        for preset in Preset::ALL {
            app.set_theme(&ctx, preset.theme());
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
                for label in ["Preferences", "Done"] {
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
        app.wrap = true;
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
        assert!(position(&output, "2").top() - position(&output, "1").top() > app.font_size * 2.0);
        app.wrap = false;
        frame(&ctx, &mut app, vec![]);
        let unscrolled = frame(&ctx, &mut app, vec![]);
        app.workspace.open[0].view.scroll_left = 120.0;
        app.workspace.open[0].restore_cursor = true;
        let scrolled = frame(&ctx, &mut app, vec![]);
        assert!((position(&unscrolled, "1").left() - position(&scrolled, "1").left()).abs() < 1.0);
    }

    #[test]
    #[ignore = "writes folder SVG previews to HEDIT_PREVIEW_DIR"]
    fn export_folder_previews() {
        let output =
            PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").expect("set HEDIT_PREVIEW_DIR"));
        std::fs::create_dir_all(&output).unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for (slug, preset) in [("ink", Preset::Ink), ("paper", Preset::Paper)] {
            let directory = tempfile::tempdir().unwrap();
            let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
            let ctx = egui::Context::default();
            let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
            app.set_theme(&ctx, preset.theme());
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
        click_label(&ctx, &mut app, "CUDA ▾");
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
        for preset in Preset::ALL {
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
            app.set_theme(&ctx, preset.theme());
            let slug = preset.name().to_lowercase().replace(' ', "-");
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
                    "preferences-editor",
                    [1280.0, 820.0],
                    Some(PreferencesPage::Editor),
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
                ("cuda", [1280.0, 820.0], None),
            ] {
                frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
                app.preferences_open = page.is_some();
                app.preferences_page = page.unwrap_or(PreferencesPage::Editor);
                app.find_open = name == "search";
                app.replace_open = name == "search";
                app.gpu_open = name == "cuda";
                if name.ends_with("syntax") {
                    click_label_sized(&ctx, &mut app, "CUDA ▾", egui::vec2(size[0], size[1]));
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
                            app.editor(ctx);
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
