//! Native Markdown reading and a live source/preview split.
use super::markdown_media::{Figure, Kind, Media};
use super::*;
use egui_commonmark::pulldown_cmark::{self, Event, Tag};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer, PreparedMarkdown};
use hedit::session::MarkdownView;
use std::collections::HashMap;
use std::io::Read;

// Rendering is deliberately bounded; large and paged files remain editable.
const PREVIEW_LIMIT: usize = hedit::document::SMALL_LIMIT as usize;
const IMAGE_SCHEME: &str = "hedit-markdown://";

#[derive(Default)]
pub(super) struct Markdown {
    cache: Option<CommonMarkCache>,
    images: Arc<Images>,
    installed: bool,
    links: HashMap<u64, Links>,
    anchor: Option<(u64, String)>,
    pending_link: Option<String>,
    pub(super) media: Arc<Media>,
    pub(super) scroll: markdown_scroll::Sync,
}

struct Links {
    revision: u64,
    path: Option<String>,
    prepared: Option<Arc<Prepared>>,
    job: Option<PrepareJob>,
    error: Option<String>,
}

struct PrepareJob {
    revision: u64,
    path: Option<String>,
    receiver: Receiver<Result<Prepared, String>>,
}

pub(super) struct Prepared {
    pub(super) targets: Vec<String>,
    pub(super) parsed: PreparedMarkdown,
    pub(super) headings: HashMap<String, usize>,
    pub(super) heading_bytes: Vec<usize>,
    pub(super) topics: Vec<(String, u8)>,
    figures: Vec<Figure>,
    blocks: HashMap<usize, usize>,
    source_len: usize,
}

/// Keep one parsed event stream for the lifetime of a document revision.
/// Source offsets identify real blocks; rendering never inserts marker HTML,
/// blank paragraphs, or synthetic line breaks into the document.
pub(super) fn prepare(text: &str) -> Prepared {
    let parsed = PreparedMarkdown::new(text, true);
    let mut targets = vec![];
    let mut headings = HashMap::new();
    let mut heading_bytes = vec![];
    let mut topics = vec![];
    let mut counts = HashMap::<String, usize>::new();
    let mut heading = None;
    let mut figure: Option<Figure> = None;
    let mut figures = vec![];
    let mut blocks = HashMap::new();
    let (mut counted_byte, mut counted_chars) = (0, 0);
    for (event, span) in parsed.events() {
        if matches!(event, Event::Start(_) | Event::Rule) {
            counted_chars += text[counted_byte..span.start].chars().count();
            counted_byte = span.start;
            blocks.insert(span.start, counted_chars);
        }
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                heading_bytes.push(span.start);
                heading = Some((String::new(), *level as u8));
            }
            Event::Start(Tag::Link { dest_url, .. }) => targets.push(dest_url.to_string()),
            Event::Start(Tag::CodeBlock(pulldown_cmark::CodeBlockKind::Fenced(info))) => {
                figure = figure_kind(info).map(|kind| Figure {
                    kind,
                    source: String::new(),
                });
            }
            Event::Text(value) if figure.is_some() => {
                figure.as_mut().unwrap().source.push_str(value);
            }
            Event::InlineMath(value) | Event::DisplayMath(value) => {
                figures.push(Figure {
                    kind: if matches!(event, Event::InlineMath(_)) {
                        Kind::InlineMath
                    } else {
                        Kind::DisplayMath
                    },
                    source: value.to_string(),
                });
                if let Some((label, _)) = &mut heading {
                    label.push_str(value);
                }
            }
            Event::Text(value) | Event::Code(value) if heading.is_some() => {
                heading.as_mut().unwrap().0.push_str(value);
            }
            Event::SoftBreak | Event::HardBreak if heading.is_some() => {
                heading.as_mut().unwrap().0.push(' ');
            }
            Event::End(pulldown_cmark::TagEnd::Heading(_)) => {
                if let Some((label, level)) = heading.take() {
                    let slug: String = label
                        .to_lowercase()
                        .chars()
                        .filter(|c| {
                            c.is_alphanumeric() || c.is_whitespace() || matches!(c, '-' | '_')
                        })
                        .map(|c| if c.is_whitespace() { '-' } else { c })
                        .collect();
                    let count = counts.entry(slug.clone()).or_default();
                    let mut unique = if *count == 0 {
                        slug.clone()
                    } else {
                        format!("{slug}-{count}")
                    };
                    while headings.contains_key(&unique) {
                        *count += 1;
                        unique = format!("{slug}-{count}");
                    }
                    *count += 1;
                    headings.insert(unique, topics.len());
                    topics.push((label, level));
                }
            }
            Event::End(pulldown_cmark::TagEnd::CodeBlock) => {
                if let Some(value) = figure.take() {
                    figures.push(value);
                }
            }
            _ => {}
        }
    }
    Prepared {
        parsed,
        targets,
        headings,
        heading_bytes,
        topics,
        figures,
        blocks,
        source_len: counted_chars + text[counted_byte..].chars().count(),
    }
}

fn figure_kind(info: &str) -> Option<Kind> {
    match info
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "mermaid" => Some(Kind::Mermaid),
        "math" => Some(Kind::DisplayMath),
        _ => None,
    }
}

fn prepare_checked(text: &str) -> Result<Prepared, String> {
    std::panic::catch_unwind(|| prepare(text))
        .map_err(|_| "The Markdown preview could not be prepared.".into())
}

/// Read local images off the UI thread after resolving their paths against
/// the document, including percent-encoded filenames.
#[derive(Default)]
struct Images {
    roots: std::sync::Mutex<HashMap<u64, PathBuf>>,
    files: Arc<std::sync::Mutex<HashMap<String, ImageEntry>>>,
    serial: std::sync::atomic::AtomicU64,
    in_flight: Arc<std::sync::atomic::AtomicUsize>,
}

type ImageBytes = std::task::Poll<Result<Arc<[u8]>, String>>;
struct ImageEntry {
    request: u64,
    state: ImageBytes,
}

fn read_image(path: &std::path::Path) -> Result<Arc<[u8]>, String> {
    const LIMIT: u64 = 32 * 1024 * 1024;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A FIFO must not block open() before we can reject its file type.
        options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
    }
    let file = options.open(path).map_err(|e| e.to_string())?;
    let info = file.metadata().map_err(|e| e.to_string())?;
    if !info.is_file() {
        return Err("Local images must be regular files.".into());
    }
    if info.len() > LIMIT {
        return Err("The image exceeds 32 MiB.".into());
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > LIMIT {
        return Err("The image exceeds 32 MiB.".into());
    }
    Ok(bytes.into())
}

impl egui::load::BytesLoader for Images {
    fn id(&self) -> &str {
        IMAGE_SCHEME
    }
    fn load(&self, ctx: &egui::Context, uri: &str) -> egui::load::BytesLoadResult {
        use egui::load::LoadError;
        let Some(relative) = uri.strip_prefix(IMAGE_SCHEME) else {
            return Err(LoadError::NotSupported);
        };
        let (id, target) = relative
            .split_once('/')
            .ok_or_else(|| LoadError::Loading("Invalid image path".into()))?;
        let root = self
            .roots
            .lock()
            .unwrap()
            .get(&id.parse::<u64>().unwrap_or(0))
            .cloned()
            .ok_or_else(|| LoadError::Loading("The Markdown file is no longer open".into()))?;
        let path = local_image_path(&root, target).map_err(LoadError::Loading)?;
        let mut files = self.files.lock().unwrap();
        if let Some(entry) = files.get(uri) {
            return match &entry.state {
                std::task::Poll::Ready(Ok(bytes)) => Ok(egui::load::BytesPoll::Ready {
                    size: None,
                    bytes: egui::load::Bytes::Shared(bytes.clone()),
                    mime: None,
                }),
                std::task::Poll::Ready(Err(error)) => Err(LoadError::Loading(error.clone())),
                std::task::Poll::Pending => Ok(egui::load::BytesPoll::Pending { size: None }),
            };
        }
        // A document containing many images must not spawn unbounded threads.
        // Finishing reads repaint the UI so the remaining images can start.
        if self.in_flight.load(Ordering::Relaxed) >= 8 {
            return Ok(egui::load::BytesPoll::Pending { size: None });
        }
        self.in_flight.fetch_add(1, Ordering::Relaxed);
        let request = self.serial.fetch_add(1, Ordering::Relaxed);
        files.insert(
            uri.to_owned(),
            ImageEntry {
                request,
                state: std::task::Poll::Pending,
            },
        );
        drop(files);
        let files = self.files.clone();
        let uri = uri.to_owned();
        let ctx = ctx.clone();
        let in_flight = self.in_flight.clone();
        thread::spawn(move || {
            let result = read_image(&path);
            if let Some(entry) = files
                .lock()
                .unwrap()
                .get_mut(&uri)
                .filter(|entry| entry.request == request)
            {
                // Refresh or Save As may have replaced this request already.
                entry.state = std::task::Poll::Ready(result);
            }
            in_flight.fetch_sub(1, Ordering::Relaxed);
            ctx.request_repaint();
        });
        Ok(egui::load::BytesPoll::Pending { size: None })
    }
    fn forget(&self, uri: &str) {
        self.files.lock().unwrap().remove(uri);
    }
    fn forget_all(&self) {
        self.files.lock().unwrap().clear();
    }
    fn byte_size(&self) -> usize {
        self.files
            .lock()
            .unwrap()
            .values()
            .map(|entry| match &entry.state {
                std::task::Poll::Ready(Ok(bytes)) => bytes.len(),
                _ => 0,
            })
            .sum()
    }
}

impl Markdown {
    fn forget_images(&mut self, ctx: &egui::Context, id: u64) {
        let prefix = format!("{IMAGE_SCHEME}{id}/");
        let uris: Vec<_> = self
            .images
            .files
            .lock()
            .unwrap()
            .keys()
            .filter(|uri| uri.starts_with(&prefix))
            .cloned()
            .collect();
        // Do not hold the cache lock while egui calls the loader's forget().
        for uri in uris {
            ctx.forget_image(&uri);
        }
    }

    pub(super) fn retain_documents(&mut self, ctx: &egui::Context, open: &[Buffer]) {
        self.scroll.retain(open);
        let closed: Vec<_> = self
            .images
            .roots
            .lock()
            .unwrap()
            .keys()
            .filter(|id| !open.iter().any(|b| b.view.id == **id))
            .copied()
            .collect();
        for id in closed {
            self.forget_images(ctx, id);
            self.images.roots.lock().unwrap().remove(&id);
            self.links.remove(&id);
            self.media.forget(id);
        }
        if self
            .anchor
            .as_ref()
            .is_some_and(|(id, _)| !open.iter().any(|b| b.view.id == *id))
        {
            self.anchor = None;
        }
    }
}

fn local_path(root: &std::path::Path, target: &str) -> Result<PathBuf, String> {
    let decoded = percent_encoding::percent_decode_str(target)
        .decode_utf8()
        .map_err(|e| e.to_string())?;
    Ok(root.join(decoded.as_ref()))
}

fn local_image_path(root: &std::path::Path, target: &str) -> Result<PathBuf, String> {
    let path = local_path(root, target)?;
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        // Opening a UNC path can authenticate to an image author's SMB server.
        // Check before any filesystem access, including metadata/canonicalize.
        // Device namespaces are not image files either. Keep ordinary drive
        // paths, including extended-length local paths, available.
        if matches!(path.components().next(), Some(Component::Prefix(prefix))
            if !matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
        {
            return Err(
                "Network shares and device paths are not loaded as Markdown images.".into(),
            );
        }
    }
    Ok(path)
}

fn document_root(path: Option<&str>) -> PathBuf {
    path.and_then(|path| {
        std::path::Path::new(path)
            .parent()
            .map(std::path::Path::to_path_buf)
    })
    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

impl HeditApp {
    pub(super) fn reveal_markdown_source(&mut self, ctx: &egui::Context) {
        if self.markdown_preview_only() {
            self.set_markdown_view(ctx, MarkdownView::Split);
        }
    }

    pub(super) fn refresh_markdown(&mut self, ctx: &egui::Context) {
        let id = self.active().view.id;
        self.markdown.forget_images(ctx, id);
        self.markdown.media.forget(id);
        if let Some(links) = self.markdown.links.get_mut(&id) {
            links.error = None;
        }
        ctx.request_repaint();
    }

    pub(super) fn finish_markdown_link(&mut self, ctx: &egui::Context, opened: bool) {
        let Some(fragment) = self.markdown.pending_link.take() else {
            return;
        };
        if opened && !self.closing && self.markdown_available() {
            self.set_markdown_view(ctx, MarkdownView::Preview);
            self.markdown.anchor = Some((self.active().view.id, fragment));
            ctx.request_repaint();
        }
    }
    pub(super) fn markdown_preview_only(&self) -> bool {
        self.markdown_available() && self.active().view.markdown_view == MarkdownView::Preview
    }
    pub(super) fn markdown_available(&self) -> bool {
        let b = self.active();
        b.language() == syntax::Language::Markdown
            && !b.page.info.paged
            && b.page.text.len() <= PREVIEW_LIMIT
    }

    pub(super) fn set_markdown_view(&mut self, ctx: &egui::Context, view: MarkdownView) {
        if self.active().view.markdown_view != view && view != MarkdownView::Source {
            self.refresh_markdown(ctx);
        }
        let b = &mut self.workspace.open[self.workspace.active];
        if b.view.markdown_view == view {
            return;
        }
        self.markdown.scroll.reset(b.view.id);
        b.view.markdown_view = view;
        // A hidden TextEdit must not keep handling typing or clipboard events.
        if view == MarkdownView::Preview {
            let base = egui::Id::new(("editor", b.page.info.id, b.page.start));
            ctx.memory_mut(|memory| {
                memory.surrender_focus(base);
                for pane in 1..panes::MAX_GROUPS {
                    memory.surrender_focus(base.with(pane));
                }
            });
        }
        self.focus_editor = view != MarkdownView::Preview;
        self.changed();
    }

    pub(super) fn markdown_pane(&mut self, ui: &mut egui::Ui, pane: usize) {
        if self.active().language() != syntax::Language::Markdown {
            self.editor_source_pane(ui, pane);
            return;
        }
        let available = self.markdown_available();
        let mode = self.active().view.markdown_view;
        let mut command = None;
        let contents = self
            .markdown
            .links
            .get(&self.active().view.id)
            .and_then(|links| links.prepared.clone());
        let mut jump = None;
        ui.horizontal_wrapped(|ui| {
            for (label, view, action) in [
                ("Source", MarkdownView::Source, Command::MarkdownSource),
                ("Split", MarkdownView::Split, Command::MarkdownSplit),
                ("Preview", MarkdownView::Preview, Command::MarkdownPreview),
            ] {
                if ui
                    .add_enabled(
                        self.command_enabled(action),
                        egui::SelectableLabel::new(mode == view, label),
                    )
                    .clicked()
                {
                    command = Some(action);
                }
            }
            if available && mode != MarkdownView::Source {
                ui.add_enabled_ui(
                    contents.as_ref().is_some_and(|p| !p.topics.is_empty()),
                    |ui| {
                        ui.menu_button("Contents", |ui| {
                            if ui.button("Top of document").clicked() {
                                jump = Some(String::new());
                                ui.close_menu();
                            }
                            ui.separator();
                            if let Some(prepared) = &contents {
                                egui::ScrollArea::vertical()
                                    .max_height(320.0)
                                    .show(ui, |ui| {
                                        for (index, (title, level)) in
                                            prepared.topics.iter().enumerate()
                                        {
                                            ui.horizontal(|ui| {
                                                ui.add_space(
                                                    f32::from(level.saturating_sub(1)) * 12.0,
                                                );
                                                if ui.selectable_label(false, title).clicked() {
                                                    jump = prepared
                                                        .headings
                                                        .iter()
                                                        .find(|(_, value)| **value == index)
                                                        .map(|(slug, _)| slug.clone());
                                                    ui.close_menu();
                                                }
                                            });
                                        }
                                    });
                            }
                        });
                    },
                );
            }
            if ui
                .add_enabled(
                    self.command_enabled(Command::MarkdownRefresh),
                    egui::Button::new("Refresh"),
                )
                .on_hover_text("Reload the preview and local images")
                .clicked()
            {
                command = Some(Command::MarkdownRefresh);
            }
        });
        if let Some(command) = command {
            self.execute(ui.ctx(), command);
        }
        if let Some(fragment) = jump {
            self.markdown.anchor = Some((self.active().view.id, fragment));
            ui.ctx().request_repaint();
        }
        ui.separator();
        if !available {
            ui.weak("Preview is available for fully loaded Markdown documents up to 2 MiB.");
            self.editor_source_pane(ui, pane);
            return;
        }
        match self.active().view.markdown_view {
            MarkdownView::Source => self.editor_source_pane(ui, pane),
            MarkdownView::Preview => {
                self.focus_editor = false;
                self.markdown_preview(ui, pane);
            }
            MarkdownView::Split => {
                let area = ui.available_rect_before_wrap();
                let stacked = area.width() < 680.0;
                let id = ui.make_persistent_id((
                    "markdown-divider",
                    self.active().view.id,
                    pane,
                    stacked,
                ));
                let mut ratio = ui.data(|data| data.get_temp::<f32>(id).unwrap_or(0.5));
                let length = if stacked { area.height() } else { area.width() };
                let start = if stacked { area.top() } else { area.left() };
                let minimum =
                    ((if stacked { 100.0 } else { 240.0 }) / length.max(1.0)).clamp(0.1, 0.45);
                ratio = ratio.clamp(minimum, 1.0 - minimum);
                let position = start + length * ratio;
                let mut divider = area;
                if stacked {
                    divider.min.y = position - 4.0;
                    divider.max.y = position + 4.0;
                } else {
                    divider.min.x = position - 4.0;
                    divider.max.x = position + 4.0;
                }
                let response = ui
                    .interact(divider, id, egui::Sense::click_and_drag())
                    .on_hover_cursor(if stacked {
                        egui::CursorIcon::ResizeVertical
                    } else {
                        egui::CursorIcon::ResizeHorizontal
                    })
                    .on_hover_text("Drag or use arrow keys to resize; double-click to reset");
                if response.clicked() {
                    response.request_focus();
                }
                if response.has_focus() {
                    let change = ui.input_mut(|input| {
                        let modifiers = input.modifiers;
                        if modifiers != Modifiers::NONE && modifiers != Modifiers::SHIFT {
                            return 0.0;
                        }
                        let (backward, forward) = if stacked {
                            (Key::ArrowUp, Key::ArrowDown)
                        } else {
                            (Key::ArrowLeft, Key::ArrowRight)
                        };
                        let step = if modifiers.shift { 0.05 } else { 0.01 };
                        if input.consume_key(modifiers, backward) {
                            -step
                        } else if input.consume_key(modifiers, forward) {
                            step
                        } else {
                            0.0
                        }
                    });
                    ratio = (ratio + change).clamp(minimum, 1.0 - minimum);
                }
                if response.double_clicked() {
                    ratio = 0.5;
                } else if response.dragged() {
                    if let Some(pointer) = response.interact_pointer_pos() {
                        let pointer = if stacked { pointer.y } else { pointer.x };
                        ratio = ((pointer - start) / length.max(1.0)).clamp(minimum, 1.0 - minimum);
                    }
                }
                response.widget_info(|| {
                    egui::WidgetInfo::slider(
                        ui.is_enabled(),
                        f64::from(ratio * 100.0),
                        "Source pane percentage",
                    )
                });
                ui.data_mut(|data| data.insert_temp(id, ratio));
                let position = start + length * ratio;
                let mut source = area;
                let mut preview = area;
                if stacked {
                    source.max.y = position - 4.0;
                    preview.min.y = position + 4.0;
                } else {
                    source.max.x = position - 4.0;
                    preview.min.x = position + 4.0;
                }
                let color = if response.hovered() || response.dragged() || response.has_focus() {
                    self.theme.accent.color()
                } else {
                    self.theme.border.color()
                };
                let line = if stacked {
                    [
                        egui::pos2(area.left(), position),
                        egui::pos2(area.right(), position),
                    ]
                } else {
                    [
                        egui::pos2(position, area.top()),
                        egui::pos2(position, area.bottom()),
                    ]
                };
                ui.painter()
                    .line_segment(line, egui::Stroke::new(1.0_f32, color));
                ui.scope_builder(egui::UiBuilder::new().max_rect(source), |ui| {
                    ui.set_clip_rect(ui.clip_rect().intersect(source));
                    self.editor_source_pane(ui, pane);
                });
                ui.scope_builder(egui::UiBuilder::new().max_rect(preview), |ui| {
                    ui.set_clip_rect(ui.clip_rect().intersect(preview));
                    self.markdown_preview(ui, pane);
                });
                let id = self.active().view.id;
                if let Some(top) = self.markdown.scroll.synchronize(ui.ctx(), id, pane) {
                    self.workspace.open[self.workspace.active].view.scroll_top = f64::from(top);
                    self.generation = self.generation.wrapping_add(1);
                }
                ui.allocate_rect(area, egui::Sense::hover());
            }
        }
    }

    fn markdown_preview(&mut self, ui: &mut egui::Ui, pane: usize) {
        // The preview is a full editor surface, even for short documents.
        let viewport = ui.available_rect_before_wrap();
        ui.painter()
            .rect_filled(viewport, 0.0, self.theme.surface.color());
        let b = self.active();
        let (id, revision, path) = (b.view.id, b.page.info.revision, b.page.info.path.clone());
        self.markdown.media.set_font_size(self.settings.font_size);
        let root = document_root(path.as_deref());
        let previous_root = self
            .markdown
            .images
            .roots
            .lock()
            .unwrap()
            .insert(id, root.clone());
        if previous_root
            .as_ref()
            .is_some_and(|previous| previous != &root)
        {
            // Save As can move a document to another folder. Identically
            // named relative images must then be reloaded from the new root.
            self.markdown.forget_images(ui.ctx(), id);
        }
        if !self.markdown.installed {
            egui_extras::install_image_loaders(ui.ctx());
            ui.ctx().add_bytes_loader(self.markdown.images.clone());
            self.markdown.installed = true;
        }
        self.markdown
            .links
            .retain(|id, _| self.workspace.open.iter().any(|b| b.view.id == *id));
        let text = &self.workspace.open[self.workspace.active].page.text;
        let links = self.markdown.links.entry(id).or_insert_with(|| Links {
            revision: u64::MAX,
            path: None,
            prepared: None,
            job: None,
            error: None,
        });
        if links.revision != revision || links.path != path {
            links.revision = revision;
            links.path = path.clone();
            links.prepared = None;
            links.error = None;
        }
        let completed = links
            .job
            .as_ref()
            .and_then(|job| match job.receiver.try_recv() {
                Ok(result) => Some((job.revision, job.path.clone(), result)),
                Err(mpsc::TryRecvError::Disconnected) => Some((
                    job.revision,
                    job.path.clone(),
                    Err("Preview preparation stopped.".into()),
                )),
                Err(mpsc::TryRecvError::Empty) => None,
            });
        if let Some((completed_revision, completed_path, result)) = completed {
            links.job = None;
            if completed_revision == revision && completed_path == path {
                match result {
                    Ok(prepared) => {
                        self.markdown.media.retain(id, &prepared.figures);
                        links.prepared = Some(Arc::new(prepared));
                    }
                    Err(error) => links.error = Some(error),
                }
            }
        }
        if links.prepared.is_none() && links.job.is_none() && links.error.is_none() {
            if text.len() > 64 * 1024 {
                let text = text.clone();
                let ctx = ui.ctx().clone();
                let (sender, receiver) = mpsc::channel();
                links.job = Some(PrepareJob {
                    revision,
                    path: path.clone(),
                    receiver,
                });
                thread::spawn(move || {
                    let _ = sender.send(prepare_checked(&text));
                    ctx.request_repaint();
                });
            } else {
                match prepare_checked(text) {
                    Ok(prepared) => {
                        self.markdown.media.retain(id, &prepared.figures);
                        links.prepared = Some(Arc::new(prepared));
                    }
                    Err(error) => links.error = Some(error),
                }
            }
        }
        let Some(prepared) = links.prepared.clone() else {
            if let Some(error) = &links.error {
                ui.colored_label(self.theme.error.color(), error);
            } else {
                ui.spinner();
                ui.weak("Preparing preview…");
            }
            return;
        };
        let media = self.markdown.media.clone();
        let measure = move |ui: &egui::Ui, source: &str| media.inline_height(ui, id, source);
        let media = self.markdown.media.clone();
        let math = move |ui: &mut egui::Ui, source: &str, inline: bool| {
            media.show(
                ui,
                id,
                source,
                if inline {
                    Kind::InlineMath
                } else {
                    Kind::DisplayMath
                },
            );
        };
        let blocks = prepared.clone();
        let section = self
            .markdown
            .anchor
            .as_ref()
            .filter(|(view, _)| *view == id)
            .map(|(_, fragment)| {
                percent_encoding::percent_decode_str(fragment)
                    .decode_utf8_lossy()
                    .into_owned()
            });
        let top = section.as_ref().is_some_and(String::is_empty);
        let anchor = section
            .as_ref()
            .and_then(|section| prepared.headings.get(section))
            .copied();
        if section.is_some() && !top && anchor.is_none() {
            self.notice = "Section not found in this Markdown document.".into();
            self.markdown.anchor = None;
        }
        let found = Arc::new(AtomicBool::new(false));
        let seen = found.clone();
        let section_positions = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let positions = std::rc::Rc::clone(&section_positions);
        let block = move |ui: &mut egui::Ui, span: std::ops::Range<usize>, heading: Option<u8>| {
            if let Some(&source) = blocks.blocks.get(&span.start) {
                positions
                    .borrow_mut()
                    .push((source as f32, ui.next_widget_position().y));
            }
            if heading.is_some()
                && anchor.is_some()
                && blocks.heading_bytes.get(anchor.unwrap()) == Some(&span.start)
            {
                ui.scroll_to_rect(
                    egui::Rect::from_min_size(ui.next_widget_position(), egui::vec2(1.0, 1.0)),
                    Some(egui::Align::Min),
                );
                seen.store(true, Ordering::Relaxed);
            }
        };
        let split = self.active().view.markdown_view == MarkdownView::Split;
        let synchronized_offset = if split {
            self.markdown.scroll.preview_offset(id, pane)
        } else {
            None
        };
        let cache = self
            .markdown
            .cache
            .get_or_insert_with(CommonMarkCache::default);
        cache.link_hooks_clear();
        for target in &prepared.targets {
            cache.add_link_hook(target);
        }
        let font_size = self.settings.font_size;
        let mut content_top = 0.0;
        let output = ui.push_id(("markdown", id, pane), |ui| {
            let mut scroll = egui::ScrollArea::vertical()
                .id_salt("rendered")
                .auto_shrink([false, false]);
            if let Some(offset) = synchronized_offset {
                scroll = scroll.vertical_scroll_offset(offset);
            }
            scroll.show(ui, |ui| {
                content_top = ui.max_rect().top();
                if top {
                    ui.scroll_to_rect(
                        egui::Rect::from_min_size(ui.next_widget_position(), egui::vec2(1.0, 1.0)),
                        Some(egui::Align::Min),
                    );
                    found.store(true, Ordering::Relaxed);
                }
                markdown_style::reader(ui, &self.theme, font_size, |ui| {
                    let code_theme = self.theme.clone();
                    let media = self.markdown.media.clone();
                    let code = move |ui: &mut egui::Ui, text: &str, language: Option<&str>| {
                        if let Some(kind) = language.and_then(figure_kind) {
                            media.show(ui, id, text, kind);
                        } else {
                            markdown_style::code_block(ui, &code_theme, text, language);
                        }
                    };
                    CommonMarkViewer::new()
                        .default_implicit_uri_scheme(format!("{IMAGE_SCHEME}{id}/"))
                        .max_image_width(Some(ui.available_width().max(0.0) as usize))
                        .render_block_fn(Some(&block))
                        .render_math_fn(Some(&math))
                        .measure_math_fn(Some(&measure))
                        .render_code_block_fn(Some(&code))
                        .show_prepared(ui, cache, &prepared.parsed);
                });
            })
        });
        if split {
            let output = output.inner;
            let mut sections = vec![(0.0, 0.0)];
            for (source, y) in section_positions.take() {
                let y = (y - content_top).max(0.0);
                if source > sections.last().unwrap().0 && y > sections.last().unwrap().1 {
                    sections.push((source, y));
                }
            }
            sections.push((prepared.source_len as f32, output.content_size.y));
            self.markdown.scroll.preview(
                id,
                pane,
                markdown_scroll::Preview {
                    viewport: markdown_scroll::Viewport::new(&output, viewport),
                    sections,
                    revision,
                    navigation: found.load(Ordering::Relaxed),
                },
            );
        }
        let clicked = prepared
            .targets
            .iter()
            .find(|target| cache.get_link_hook(target) == Some(true))
            .cloned();
        if found.load(Ordering::Relaxed) {
            self.markdown.anchor = None;
        }
        if let Some(target) = clicked {
            self.markdown_link(ui.ctx(), &root, &target);
        }
    }

    fn markdown_link(&mut self, ctx: &egui::Context, root: &std::path::Path, target: &str) {
        if self.busy || self.closing {
            return;
        }
        if target.starts_with("https://")
            || target.starts_with("http://")
            || target.starts_with("mailto:")
        {
            ctx.open_url(egui::OpenUrl::new_tab(target));
        } else if let Some(fragment) = target.strip_prefix('#') {
            self.markdown.anchor = Some((self.active().view.id, fragment.to_owned()));
            ctx.request_repaint();
        } else if target.contains(':') && !std::path::Path::new(target).is_absolute() {
            self.notice = "This link type is not supported.".into();
        } else {
            let (path, fragment) = target
                .split_once('#')
                .map_or((target, None), |(path, fragment)| (path, Some(fragment)));
            let path = path.split('?').next().unwrap_or(path);
            match local_path(root, path) {
                Ok(path) => {
                    self.markdown.pending_link = Some(fragment.unwrap_or("").to_owned());
                    self.open(ctx, Some(path));
                }
                Err(error) => self.error = Some(error),
            }
        }
    }
}

#[cfg(test)]
mod image_path_tests {
    use super::*;
    #[cfg(windows)]
    use egui::load::BytesLoader;
    #[cfg(windows)]
    use std::path::Path;

    #[cfg(windows)]
    #[test]
    fn network_and_device_images_are_rejected_before_starting_io() {
        let images = Images::default();
        images.roots.lock().unwrap().insert(1, r"C:\notes".into());
        let ctx = egui::Context::default();
        for target in [
            "%5C%5Cserver.invalid%5Cshare%5Cimage.png",
            "//server.invalid/share/image.png",
            "%5C%5C%3F%5CUNC%5Cserver.invalid%5Cshare%5Cimage.png",
            "%5C%5C.%5Cpipe%5Cimage.png",
            "%5C%5C%3F%5CGLOBALROOT%5CDevice%5Cimage.png",
        ] {
            let uri = format!("{IMAGE_SCHEME}1/{target}");
            let error = images
                .load(&ctx, &uri)
                .err()
                .expect("network and device images must be rejected")
                .to_string();
            assert!(error.contains("Network shares and device paths"), "{error}");
            assert_eq!(images.in_flight.load(Ordering::Relaxed), 0);
            assert!(images.files.lock().unwrap().is_empty());
        }
        for target in [
            "image.png",
            "../image.png",
            "C:/images/a.png",
            r"\\?\C:\images\a.png",
        ] {
            assert!(local_image_path(Path::new(r"C:\notes"), target).is_ok());
        }
        assert!(local_image_path(Path::new(r"\\server.invalid\share"), "a.png").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn fifo_images_are_rejected_without_waiting_for_a_writer() {
        use std::{os::unix::fs::OpenOptionsExt, process::Command, sync::mpsc};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.png");
        assert!(Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success());
        let input = path.clone();
        let (tx, rx) = mpsc::channel();
        let worker = thread::spawn(move || tx.send(read_image(&input)).unwrap());
        let result = rx.recv_timeout(Duration::from_secs(2));
        // Release an unfixed reader before asserting, so a failing regression
        // test never leaves a blocked thread behind.
        let _ = std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
            .open(&path);
        worker.join().unwrap();
        assert!(result
            .expect("opening a FIFO blocked the image worker")
            .is_err());
    }
}
