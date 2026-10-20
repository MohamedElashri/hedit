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
    find_open: bool,
    find_focus: bool,
    query: String,
    replacement: String,
    palette: bool,
    palette_query: String,
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
    appearance_open: bool,
    appearance_original: Theme,
    theme_hex: Vec<String>,
    rename_target: Option<u64>,
    scratchpad_name: String,
    scratchpad_name_focus: bool,
    shelf_open: bool,
    shelf_query: String,
    bookmarks_open: bool,
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
            .and_then(|s| eframe::get_value(s, "font_size"))
            .unwrap_or(14.0);
        Self {
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
            notice: "Ready. Untitled buffers stay with you.".into(),
            wrap,
            font_size,
            sidebar: true,
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
            debugger: "cuda-gdb".into(),
            breakpoint: String::new(),
            console: String::new(),
            sanitizer: "memcheck".into(),
            poll_at: Instant::now(),
            focus_editor: true,
            theme,
            appearance_open: false,
            appearance_original,
            theme_hex,
            rename_target: None,
            scratchpad_name: String::new(),
            scratchpad_name_focus: false,
            shelf_open: false,
            shelf_query: String::new(),
            bookmarks_open: false,
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
            let (path, line, col) = hedit::cli::location(&path);
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
            self.error =
                Some("This buffer has edits. Save As first to keep them before reloading.".into());
            return;
        }
        self.job(ctx, move || {
            let shared = editor.get(id)?;
            let mut doc = shared.lock().map_err(|e| e.to_string())?;
            if reload {
                let path = path.ok_or("Untitled buffers have no file to reload.")?;
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
        self.job(ctx, move || {
            let target = if choose {
                match rfd::FileDialog::new()
                    .set_title("Save buffer — hedit")
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
                    self.error = Some(format!("RECOVERY CHECKPOINT FAILED\n{error}\nKeep hedit open and save your buffers to files."));
                    self.closing = false;
                }
            }
        }
        if !self.busy && !self.closing {
            if let Some(path) = self.pending_paths.pop_front() {
                self.open(ctx, Some(path));
            }
        }
        if self.closing && !self.busy && !self.checkpoint_pending {
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
        if take(shift, Key::P) {
            self.palette = true;
            self.palette_query.clear();
        }
        if take(cmd, Key::P) {
            self.palette = true;
            self.palette_query.clear();
        }
        if take(cmd, Key::N) {
            let r = self.workspace.new_buffer();
            self.report(r);
        }
        if take(cmd, Key::O) {
            self.open(ctx, None);
        }
        if take(shift, Key::S) {
            self.save_file(ctx, true);
        } else if take(cmd, Key::S) {
            self.save_file(ctx, false);
        }
        if take(shift, Key::T) {
            let r = self.workspace.reopen();
            self.report(r);
        }
        if take(cmd, Key::W) {
            let r = self.workspace.close(self.workspace.active);
            self.report(r);
        }
        if take(cmd, Key::F2) {
            self.toggle_bookmark();
        }
        if take(Modifiers::SHIFT, Key::F2) {
            self.adjacent_bookmark(ctx, false);
        } else if take(Modifiers::NONE, Key::F2) {
            self.adjacent_bookmark(ctx, true);
        }
        if take(shift, Key::R) {
            self.open_scratchpad_name();
        }
        if take(cmd, Key::Comma) {
            self.open_appearance();
        }
        if take(cmd, Key::F) {
            self.find_open = true;
            self.find_focus = true;
        }
        if take(cmd, Key::G) {
            self.find_open = true;
        }
        if take(shift, Key::W) {
            self.wrap = !self.wrap;
        }
        // Let single-line search and debugger inputs keep their own undo history.
        let editor_id = egui::Id::new((
            "editor",
            self.active().page.info.id,
            self.active().page.start,
        ));
        if ctx.memory(|m| m.has_focus(editor_id)) {
            if take(shift, Key::Z) || take(cmd, Key::Y) {
                self.history(ctx, true);
            } else if take(cmd, Key::Z) {
                self.history(ctx, false);
            }
        }
        if take(cmd, Key::Equals) || take(cmd, Key::Plus) {
            self.font_size = (self.font_size + 1.0).min(28.0);
        }
        if take(cmd, Key::Minus) {
            self.font_size = (self.font_size - 1.0).max(10.0);
        }
        if take(Modifiers::NONE, Key::Escape) {
            self.palette = false;
            self.find_open = false;
            self.rename_target = None;
            self.shelf_open = false;
            self.bookmarks_open = false;
            self.focus_editor = true;
        }
    }
    fn chrome(&mut self, ctx: &egui::Context) {
        let theme = self.theme.clone();
        egui::TopBottomPanel::top("top")
            .frame(
                egui::Frame::new()
                    .fill(theme.panel.color())
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    brand::paint(ui, 30.0, theme.accent.color(), theme.muted.color());
                    ui.label(RichText::new("hedit").size(18.0).strong());
                    ui.separator();
                    ui.add_enabled_ui(!self.busy && !self.closing, |ui| {
                        if ui
                            .button("+ New")
                            .on_hover_text("New scratch buffer · Ctrl+N")
                            .clicked()
                        {
                            let r = self.workspace.new_buffer();
                            self.report(r);
                        }
                        if ui
                            .button("Open…")
                            .on_hover_text("Open file · Ctrl+O")
                            .clicked()
                        {
                            self.open(ctx, None);
                        }
                        if ui
                            .button("Save")
                            .on_hover_text("Save file · Ctrl+S")
                            .clicked()
                        {
                            self.save_file(ctx, false);
                        }
                        ui.menu_button("More", |ui| {
                            if ui
                                .button("Save as…                  Ctrl+Shift+S")
                                .clicked()
                            {
                                self.save_file(ctx, true);
                                ui.close_menu();
                            }
                            if ui.button("Reopen closed buffer    Ctrl+Shift+T").clicked() {
                                let r = self.workspace.reopen();
                                self.report(r);
                                ui.close_menu();
                            }
                            if ui.button("Command palette         Ctrl+Shift+P").clicked() {
                                self.palette = true;
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui
                                .add_enabled(
                                    self.active().page.info.path.is_none(),
                                    egui::Button::new("Name scratchpad…         Ctrl+Shift+R"),
                                )
                                .clicked()
                            {
                                self.open_scratchpad_name();
                                ui.close_menu();
                            }
                            if ui.button("Browse closed buffers…").clicked() {
                                self.shelf_open = true;
                                ui.close_menu();
                            }
                            if ui.button("Bookmarks…").clicked() {
                                self.bookmarks_open = true;
                                ui.close_menu();
                            }
                            ui.checkbox(&mut self.wrap, "Word wrap");
                            ui.checkbox(&mut self.sidebar, "Show buffer sidebar");
                        });
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button("Appearance")
                            .on_hover_text("Themes and custom colors · Ctrl+,")
                            .clicked()
                        {
                            self.open_appearance();
                        }
                        ui.toggle_value(&mut self.gpu_open, "CUDA");
                        if ui
                            .button("Find")
                            .on_hover_text("Find in file · Ctrl+F")
                            .clicked()
                        {
                            self.find_open = true;
                            self.find_focus = true;
                        }
                    });
                });
                ui.add_space(6.0);
                let mut select = None;
                let mut close = None;
                egui::ScrollArea::horizontal()
                    .id_salt("tabs")
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 5.0;
                        ui.horizontal(|ui| {
                            for (index, buffer) in self.workspace.open.iter().enumerate() {
                                let active = index == self.workspace.active;
                                let title: String = buffer.view.name.chars().take(28).collect();
                                let title = if buffer.view.name.chars().count() > 28 {
                                    format!("{title}…")
                                } else {
                                    title
                                };
                                let frame = egui::Frame::new()
                                    .fill(if active {
                                        theme.background.color()
                                    } else {
                                        theme.surface.color()
                                    })
                                    .corner_radius(egui::CornerRadius::same(6))
                                    .inner_margin(egui::Margin::symmetric(10, 5));
                                let output = frame.show(ui, |ui| {
                                    ui.push_id(buffer.page.info.id, |ui| {
                                        ui.horizontal(|ui| {
                                            if buffer.page.info.dirty {
                                                ui.label(
                                                    RichText::new("•").color(theme.accent.color()),
                                                );
                                            }
                                            if ui
                                                .add(
                                                    egui::Button::new(RichText::new(title).color(
                                                        if active {
                                                            theme.text.color()
                                                        } else {
                                                            theme.muted.color()
                                                        },
                                                    ))
                                                    .frame(false),
                                                )
                                                .on_hover_text(
                                                    buffer.page.info.path.as_deref().unwrap_or(
                                                        "Unsaved buffer · included in recovery",
                                                    ),
                                                )
                                                .clicked()
                                            {
                                                select = Some(index);
                                            }
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new("×")
                                                            .color(theme.muted.color()),
                                                    )
                                                    .frame(false),
                                                )
                                                .on_hover_text("Close to buffer shelf")
                                                .clicked()
                                            {
                                                close = Some(index);
                                            }
                                        })
                                    });
                                });
                                if active {
                                    let rect = output.response.rect;
                                    ui.painter().line_segment(
                                        [
                                            egui::pos2(rect.left() + 7.0, rect.bottom()),
                                            egui::pos2(rect.right() - 7.0, rect.bottom()),
                                        ],
                                        egui::Stroke::new(2.0, theme.accent.color()),
                                    );
                                }
                            }
                        });
                    });
                if !self.busy && !self.closing {
                    if let Some(index) = select {
                        self.workspace.active = index;
                        self.changed();
                        self.focus_editor = true;
                    }
                    if let Some(index) = close {
                        let r = self.workspace.close(index);
                        self.report(r);
                    }
                }
            });
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(theme.panel.color())
                    .inner_margin(egui::Margin::symmetric(14, 5)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let label = if self.closing {
                        "Finishing recovery…"
                    } else if self.checkpoint_pending {
                        "Checkpointing…"
                    } else if self.persisted == self.generation {
                        "● Recovery up to date"
                    } else {
                        "● Recovery pending"
                    };
                    let color = if self.persisted == self.generation {
                        theme.success.color()
                    } else {
                        theme.warning.color()
                    };
                    let response = ui.label(RichText::new(label).size(11.0).color(color));
                    if let Some(saved) = self.last_saved {
                        response.on_hover_text(format!(
                            "Last completed checkpoint: {} seconds ago",
                            saved.elapsed().as_secs()
                        ));
                    }
                    if self.busy {
                        ui.spinner();
                        if ui.small_button("Cancel search").clicked() {
                            self.cancel.store(true, Ordering::Relaxed);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let buffer = self.active();
                        let info = &buffer.page.info;
                        let prefix: String =
                            buffer.page.text.chars().take(buffer.view.head).collect();
                        let line = prefix.bytes().filter(|b| *b == b'\n').count() + 1;
                        let col = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
                        ui.label(
                            RichText::new(format!(
                                "{}Ln {line}, Col {col}",
                                if info.paged { "Page " } else { "" }
                            ))
                            .size(11.0)
                            .color(theme.muted.color()),
                        );
                        ui.separator();
                        ui.label(
                            RichText::new(format!(
                                "UTF-8{}  ·  {}",
                                if info.bom { " BOM" } else { "" },
                                info.eol
                            ))
                            .size(11.0)
                            .color(theme.muted.color()),
                        );
                        ui.separator();
                        ui.label(
                            RichText::new(format_bytes(info.size))
                                .size(11.0)
                                .color(theme.muted.color()),
                        );
                    });
                })
            });
        if self.sidebar {
            egui::SidePanel::left("buffers")
                .default_width(220.0)
                .min_width(170.0)
                .max_width(420.0)
                .resizable(true)
                .frame(
                    egui::Frame::new()
                        .fill(theme.panel.color())
                        .inner_margin(egui::Margin::symmetric(12, 16)),
                )
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("WORKSPACE")
                                .size(10.0)
                                .strong()
                                .color(theme.muted.color()),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(self.workspace.open.len().to_string())
                                    .size(10.0)
                                    .color(theme.muted.color()),
                            );
                        });
                    });
                    ui.add_space(10.0);
                    let mut select = None;
                    egui::ScrollArea::vertical()
                        .id_salt("buffer-list")
                        .max_height((ui.available_height() * 0.6).max(90.0))
                        .show(ui, |ui| {
                            for (index, buffer) in self.workspace.open.iter().enumerate() {
                                let active = index == self.workspace.active;
                                let kind = if syntax::is_cuda(&buffer.page.info.name) {
                                    "cu"
                                } else if buffer.page.info.path.is_none() {
                                    "•"
                                } else {
                                    "txt"
                                };
                                let label = format!("{kind}   {}", buffer.view.name);
                                let response = ui
                                    .allocate_ui_with_layout(
                                        egui::vec2(ui.available_width(), 32.0),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.add(
                                                egui::Button::new(
                                                    RichText::new(label).size(12.0).color(
                                                        if active {
                                                            theme.text.color()
                                                        } else {
                                                            theme.muted.color()
                                                        },
                                                    ),
                                                )
                                                .min_size(egui::vec2(ui.available_width(), 32.0))
                                                .fill(if active {
                                                    theme.elevated.color()
                                                } else {
                                                    theme.panel.color()
                                                })
                                                .stroke(egui::Stroke::NONE)
                                                .truncate(),
                                            )
                                        },
                                    )
                                    .inner;
                                if active {
                                    let rect = response.rect;
                                    ui.painter().line_segment(
                                        [
                                            rect.left_top() + egui::vec2(0.0, 6.0),
                                            rect.left_bottom() - egui::vec2(0.0, 6.0),
                                        ],
                                        egui::Stroke::new(2.0, theme.accent.color()),
                                    );
                                }
                                if response.on_hover_text(&buffer.view.name).clicked() {
                                    select = Some(index);
                                }
                            }
                        });
                    if !self.busy && !self.closing {
                        if let Some(index) = select {
                            self.workspace.active = index;
                            self.changed();
                            self.focus_editor = true;
                        }
                    }
                    ui.add_space(22.0);
                    egui::Frame::new()
                        .fill(theme.surface.color())
                        .corner_radius(6)
                        .inner_margin(12.0)
                        .show(ui, |ui| {
                            ui.set_min_width((ui.available_width() - 4.0).max(0.0));
                            ui.label(
                                RichText::new("BUFFER SHELF")
                                    .size(10.0)
                                    .strong()
                                    .color(theme.muted.color()),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(format!(
                                    "{} closed buffers kept",
                                    self.workspace.closed.len()
                                ))
                                .size(12.0),
                            );
                            if ui
                                .add_enabled(
                                    !self.busy && !self.closing,
                                    egui::Button::new("Reopen latest")
                                        .min_size(egui::vec2(ui.available_width(), 26.0)),
                                )
                                .clicked()
                            {
                                let r = self.workspace.reopen();
                                self.report(r);
                            }
                            if ui.button("Browse shelf…").clicked() {
                                self.shelf_open = true;
                            }
                        });
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(
                            "No filename needed. Your scratch buffers come back with you.",
                        )
                        .size(11.0)
                        .color(theme.muted.color()),
                    );
                    ui.add_space(20.0);
                    ui.label(
                        RichText::new(
                            "Ctrl+P   switch buffers\nCtrl+Shift+T   reopen\nCtrl+,   appearance",
                        )
                        .size(10.0)
                        .color(theme.muted.color()),
                    );
                });
        }
    }
    fn editor(&mut self, ctx: &egui::Context) {
        let theme = self.theme.clone();
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme.background.color())
                    .inner_margin(egui::Margin::symmetric(18, 12)),
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
                let display_path = self.active().page.info.path.clone();
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(display_path.as_deref().unwrap_or("UNTITLED / SCRATCHPAD"))
                            .small()
                            .color(theme.muted.color()),
                    );
                    if ui
                        .button(format!(
                            "Bookmarks ({})",
                            self.active().view.bookmarks.len()
                        ))
                        .clicked()
                    {
                        self.bookmarks_open = true;
                    }
                });
                if self.active().external {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(
                            theme.warning.color(),
                            "File changed outside hedit. Your buffer has been kept.",
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
                            .add_enabled(!self.busy, egui::Button::new("Save buffer as…"))
                            .clicked()
                        {
                            self.save_file(ctx, true);
                        }
                    });
                }
                if self.find_open {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Find");
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.query)
                                .desired_width(210.0)
                                .hint_text("Literal, case-sensitive"),
                        );
                        if self.find_focus {
                            response.request_focus();
                            self.find_focus = false;
                        }
                        let enter =
                            response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        if ui
                            .add_enabled(!self.busy, egui::Button::new("Next / wrap"))
                            .clicked()
                            || enter
                        {
                            self.find(ctx);
                        }
                        ui.add(
                            egui::TextEdit::singleline(&mut self.replacement)
                                .desired_width(150.0)
                                .hint_text("Replacement"),
                        );
                        if ui
                            .add_enabled(
                                !self.busy && !self.active().follow,
                                egui::Button::new("Replace selected"),
                            )
                            .clicked()
                        {
                            self.replace_selection();
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Go to line");
                        ui.add(egui::TextEdit::singleline(&mut self.goto).desired_width(90.0));
                        if ui
                            .add_enabled(!self.busy, egui::Button::new("Go"))
                            .clicked()
                        {
                            self.goto_line(ctx);
                        }
                        ui.label(
                            RichText::new(&self.notice)
                                .small()
                                .color(theme.muted.color()),
                        );
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
                ui.horizontal(|ui| {
                    let follow_allowed = !self.busy
                        && !self.closing
                        && !self.active().page.info.dirty
                        && self.active().page.info.path.is_some();
                    let index = self.workspace.active;
                    if ui
                        .add_enabled(
                            follow_allowed,
                            egui::Checkbox::new(
                                &mut self.workspace.open[index].follow,
                                "Follow log (read only)",
                            ),
                        )
                        .changed()
                        && self.active().follow
                    {
                        let offset = self.active().page.info.size.saturating_sub(PAGE_SIZE);
                        self.load_page(ctx, offset, false);
                    }
                    if syntax::is_c_family(&self.active().page.info.name) {
                        ui.label(
                            RichText::new(syntax::language_label(&self.active().page.info.name))
                                .small()
                                .color(theme.accent.color()),
                        );
                    }
                });
                ui.separator();
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
                let cuda = syntax::is_c_family(&buffer.page.info.name);
                let font_size = self.font_size;
                let wrap = self.wrap;
                let mut layouter = move |ui: &egui::Ui, text: &str, wrap_width: f32| {
                    ui.fonts(|fonts| {
                        fonts.layout_job(syntax::highlight_with_theme(
                            text,
                            font_size,
                            if wrap { wrap_width } else { f32::INFINITY },
                            cuda,
                            &theme,
                        ))
                    })
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
                let scroll_output = scroll.show(ui, |ui| {
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
                                && !self.appearance_open
                                && !self.palette
                                && self.rename_target.is_none()
                                && !self.shelf_open
                                && !self.bookmarks_open,
                        );
                    if !self.wrap {
                        edit = edit.desired_width(f32::INFINITY);
                    }
                    let mut output = edit.show(ui);
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
                            // the user's edited view in a new independent scratchpad.
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
                                "{error}\nThe edited page has been kept in a new scratch buffer."
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
                    ui.label("Debugger");
                    ui.add(egui::TextEdit::singleline(&mut self.debugger).desired_width(150.0));
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
        if !self.appearance_open {
            self.appearance_original = self.theme.clone();
        }
        self.appearance_open = true;
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
    fn appearance(&mut self, ctx: &egui::Context) {
        if !self.appearance_open {
            return;
        }
        let before = self.theme.clone();
        let mut open = true;
        egui::Window::new("Appearance").id(egui::Id::new("appearance"))
            .open(&mut open).default_width(500.0).default_height(580.0)
            .min_width(360.0).resizable(true).collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER,[0.0,0.0]).show(ctx,|ui| {
                ui.label(RichText::new("Make room for your kind of focus.").size(19.0).strong());
                ui.label(RichText::new("Pick a palette, then make it yours. Colors preview as you edit.").small().color(self.theme.muted.color()));
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    for preset in Preset::ALL {
                        let theme=preset.theme();
                        let selected=self.theme==theme;
                        let button=egui::Button::new(RichText::new(preset.name()).color(theme.text.color()))
                            .fill(theme.background.color()).stroke(egui::Stroke::new(if selected {2.0}else{1.0},if selected {self.theme.accent.color()}else{self.theme.border.color()}));
                        if ui.add(button).clicked() {self.set_theme(ctx,theme);}
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label("Theme name");
                    ui.add(egui::TextEdit::singleline(&mut self.theme.name).char_limit(80).desired_width(235.0));
                    ui.checkbox(&mut self.theme.dark,"Dark controls");
                });
                ui.separator();
                egui::ScrollArea::vertical().id_salt("theme-scroll").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible).max_height((ctx.screen_rect().height()-350.0).clamp(110.0,330.0)).show(ui,|ui| {
                    egui::Grid::new("theme-colors").num_columns(3).spacing([22.0,8.0]).striped(true).show(ui,|ui| {
                        for (index,role) in Role::ALL.into_iter().enumerate() {
                            ui.label(role.label());
                            let color=self.theme.color_mut(role);
                            if ui.color_edit_button_srgb(&mut color.0).changed() {
                                self.theme_hex[index]=color.hex();
                            }
                            let valid=Rgb::try_from(self.theme_hex[index].clone()).is_ok();
                            let text_color=if valid {self.theme.text.color()}else{self.theme.error.color()};
                            let response=ui.add(egui::TextEdit::singleline(&mut self.theme_hex[index]).id_salt(("hex",index)).desired_width(90.0).font(egui::TextStyle::Monospace).char_limit(7).text_color(text_color));
                            if response.changed() {
                                if let Ok(color)=Rgb::try_from(self.theme_hex[index].clone()) {*self.theme.color_mut(role)=color;}
                            }
                            if !valid {response.on_hover_text("Use #RRGGBB. The last valid color remains active.");}
                            ui.end_row();
                        }
                    });
                });
                ui.separator();
                let contrast=Theme::contrast(self.theme.text,self.theme.background);
                let selection_contrast=Theme::contrast(self.theme.selection_text,self.theme.selection);
                let readable=contrast>=4.5 && selection_contrast>=4.5;
                ui.label(RichText::new(format!("Text contrast {contrast:.1}:1 · Selection {selection_contrast:.1}:1{}",if readable {""}else{" · Low contrast"}))
                    .small().color(if readable {self.theme.muted.color()}else{self.theme.warning.color()}));
                ui.add_space(4.0);
                egui::Frame::new().fill(self.theme.background.color()).stroke(egui::Stroke::new(1.0,self.theme.border.color())).inner_margin(12.0).show(ui,|ui| {
                    ui.label(syntax::highlight_with_theme("__global__ void kernel() { // a little room to think\n    const int value = 42;\n}",12.0,ui.available_width(),true,&self.theme));
                });
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(!self.busy,egui::Button::new("Import JSON…")).clicked() {self.import_theme(ctx);}
                    if ui.add_enabled(!self.busy,egui::Button::new("Export JSON…")).clicked() {self.export_theme(ctx);}
                    if ui.button("Revert changes").clicked() {self.set_theme(ctx,self.appearance_original.clone());}
                    if ui.button("Done").clicked() {self.appearance_open=false;self.focus_editor=true;}
                });
                ui.label(RichText::new("Your appearance is saved automatically on this device.").small().color(self.theme.muted.color()));
            });
        self.appearance_open &= open;
        if !open {
            self.focus_editor = true;
        }
        if self.theme != before {
            if (self.theme.dark != before.dark
                || Role::ALL
                    .into_iter()
                    .any(|role| self.theme.color(role) != before.color(role)))
                && Preset::ALL.into_iter().any(|p| self.theme.name == p.name())
                && !Preset::ALL.into_iter().any(|p| self.theme == p.theme())
            {
                self.theme.name = "My theme".into();
            }
            self.theme.apply(ctx);
        }
    }

    fn open_scratchpad_name(&mut self) {
        if self.active().page.info.path.is_some() {
            self.error =
                Some("Only unsaved scratchpads can be named. Use Save As for files.".into());
            return;
        }
        self.rename_target = Some(self.active().page.info.id);
        self.scratchpad_name = self.active().view.custom_name.clone().unwrap_or_default();
        self.scratchpad_name_focus = true;
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
            self.notice = "No bookmarks in this buffer.".into();
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
            egui::Window::new("Name scratchpad")
                .id(egui::Id::new("scratchpad-name"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(360.0)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Give this buffer a name without creating a file.");
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.scratchpad_name)
                            .char_limit(80)
                            .desired_width(f32::INFINITY)
                            .hint_text("Investigation notes"),
                    );
                    if self.scratchpad_name_focus {
                        response.request_focus();
                        self.scratchpad_name_focus = false;
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
                    let result = self.workspace.name_scratchpad(index, &self.scratchpad_name);
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
            egui::Window::new("Buffer shelf")
                .open(&mut open)
                .default_width(480.0)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label("Closed buffers keep their text, names, and bookmarks.");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.shelf_query)
                            .desired_width(f32::INFINITY)
                            .hint_text("Filter closed buffers…"),
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
                        ui.label("No closed buffers match that name.");
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

    fn command_palette(&mut self, ctx: &egui::Context) {
        if !self.palette {
            return;
        }
        let mut selected = None;
        egui::Window::new("Switch buffer / Commands")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 100.0])
            .default_width(500.0)
            .show(ctx, |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.palette_query)
                        .desired_width(f32::INFINITY)
                        .hint_text("Type a buffer name or command…"),
                );
                response.request_focus();
                let filter = self.palette_query.to_lowercase();
                let mut first = None;
                for (index, buffer) in self.workspace.open.iter().enumerate() {
                    if !buffer.view.name.to_lowercase().contains(&filter) {
                        continue;
                    }
                    if first.is_none() {
                        first = Some(index);
                    }
                    if ui
                        .selectable_label(index == self.workspace.active, &buffer.view.name)
                        .clicked()
                    {
                        selected = Some(index);
                    }
                }
                if ui.input(|i| i.key_pressed(Key::Enter)) {
                    selected = first;
                }
                ui.separator();
                for (label, action) in [
                    ("New buffer", 0),
                    ("Open file…", 1),
                    ("Reopen closed buffer", 2),
                    ("Toggle CUDA panel", 3),
                    ("Toggle word wrap", 4),
                    ("Checkpoint now", 5),
                    ("Appearance / Custom theme", 6),
                    ("Name scratchpad", 7),
                    ("Browse closed buffers", 8),
                    ("Show bookmarks", 9),
                    ("Toggle bookmark", 10),
                ] {
                    if label.to_lowercase().contains(&filter) && ui.button(label).clicked() {
                        match action {
                            0 => {
                                let r = self.workspace.new_buffer();
                                self.report(r);
                            }
                            1 => self.open(ctx, None),
                            2 => {
                                let r = self.workspace.reopen();
                                self.report(r);
                            }
                            3 => self.gpu_open = !self.gpu_open,
                            4 => self.wrap = !self.wrap,
                            5 => self.checkpoint(ctx),
                            6 => self.open_appearance(),
                            7 => self.open_scratchpad_name(),
                            8 => self.shelf_open = true,
                            9 => self.bookmarks_open = true,
                            _ => self.toggle_bookmark(),
                        }
                        self.palette = false;
                    }
                }
            });
        if let Some(index) = selected {
            self.workspace.active = index;
            self.palette = false;
            self.changed();
            self.focus_editor = true;
        }
    }
}
impl eframe::App for HeditApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive(ctx);
        if let Some(debug) = &mut self.debug {
            debug.poll();
        }
        self.poll_external(ctx);
        if ctx.input(|i| i.viewport().close_requested()) && !self.exit_ready {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.closing = true;
        }
        self.shortcuts(ctx);
        self.chrome(ctx);
        self.gpu(ctx);
        self.editor(ctx);
        if !self.busy && !self.closing {
            self.command_palette(ctx);
        }
        self.appearance(ctx);
        self.buffer_tools(ctx);
        let paths: Vec<_> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        self.pending_paths.extend(paths);
        ctx.request_repaint_after(Duration::from_millis(
            if self.debug.is_some() || self.busy {
                50
            } else {
                200
            },
        ));
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "wrap", &self.wrap);
        eframe::set_value(storage, "font_size", &self.font_size);
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

    fn frame(ctx: &egui::Context, app: &mut HeditApp, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 820.0),
            )),
            focused: true,
            events,
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            app.shortcuts(ctx);
            app.chrome(ctx);
            app.gpu(ctx);
            app.editor(ctx);
            app.command_palette(ctx);
            app.appearance(ctx);
            app.buffer_tools(ctx);
        });
        assert!(!output.shapes.is_empty());
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
            for (name, size, appearance) in [
                ("editor", [1280.0, 820.0], false),
                ("appearance", [1280.0, 820.0], true),
                ("compact", [720.0, 480.0], false),
                ("compact-appearance", [720.0, 480.0], true),
            ] {
                app.appearance_open = appearance;
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
                            app.editor(ctx);
                            app.appearance(ctx);
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
        app.open_scratchpad_name();
        frame(&ctx, &mut app, vec![]);
        assert_eq!(app.scratchpad_name, "Named notes");
        assert_eq!(app.active().view.bookmarks.len(), 1);
    }
}
