//! Native Markdown reading and a live source/preview split.
use super::markdown_media::{Figure, Kind, Media};
use super::*;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use hedit::session::MarkdownView;
use pulldown_cmark::{Event, Parser, Tag};
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

struct Prepared {
    targets: Vec<String>,
    rendered: String,
    headings: HashMap<String, usize>,
    figures: Vec<Figure>,
    heading_chars: Vec<usize>,
    source_len: usize,
}

/// Add invisible scroll targets while keeping the complete Markdown document
/// together, so reference links and footnotes still resolve across sections.
fn prepare(text: &str) -> Prepared {
    let mut targets = vec![];
    let mut headings = HashMap::new();
    let mut counts = HashMap::<String, usize>::new();
    let mut rendered = String::with_capacity(text.len());
    let mut copied = 0;
    let mut heading = None;
    let mut list_depth = 0usize;
    let mut figure: Option<(usize, String, Figure)> = None;
    let mut figures = vec![];
    let mut heading_chars = Vec::new();
    let (mut counted_byte, mut counted_chars) = (0, 0);
    // Match the renderer, including dollar-delimited inline/display math.
    let options = pulldown_cmark::Options::ENABLE_TABLES
        | pulldown_cmark::Options::ENABLE_TASKLISTS
        | pulldown_cmark::Options::ENABLE_STRIKETHROUGH
        | pulldown_cmark::Options::ENABLE_FOOTNOTES
        | pulldown_cmark::Options::ENABLE_DEFINITION_LIST
        | pulldown_cmark::Options::ENABLE_MATH;
    for (event, span) in Parser::new_ext(text, options).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                match tag {
                    Tag::List(_) => list_depth += 1,
                    Tag::CodeBlock(pulldown_cmark::CodeBlockKind::Fenced(info)) => {
                        let language = info.split_whitespace().next().unwrap_or("");
                        let kind = if language.eq_ignore_ascii_case("mermaid") {
                            Some(Kind::Mermaid)
                        } else if language.eq_ignore_ascii_case("math") {
                            Some(Kind::DisplayMath)
                        } else {
                            None
                        };
                        if let Some(kind) = kind {
                            let line = text[..span.start].rfind('\n').map_or(0, |at| at + 1);
                            let continuation: String = text[line..span.start]
                                .chars()
                                .map(|c| {
                                    if c == '>' || c.is_whitespace() {
                                        c
                                    } else {
                                        ' '
                                    }
                                })
                                .collect();
                            figure = Some((
                                span.start,
                                continuation,
                                Figure {
                                    kind,
                                    source: String::new(),
                                },
                            ));
                        }
                    }
                    Tag::Link { dest_url, .. } => targets.push(dest_url.into_string()),
                    Tag::Heading { .. } => {
                        let index = headings.len();
                        counted_chars += text[counted_byte..span.start].chars().count();
                        counted_byte = span.start;
                        heading_chars.push(counted_chars);
                        rendered.push_str(&text[copied..span.start]);
                        // Keep a nested heading inside its quote/list. List
                        // markers become continuation indentation on the new
                        // lines, while blockquote markers remain in place.
                        let line = text[..span.start].rfind('\n').map_or(0, |at| at + 1);
                        let continuation: String = text[line..span.start]
                            .chars()
                            .map(|c| {
                                if c == '>' || c.is_whitespace() {
                                    c
                                } else {
                                    ' '
                                }
                            })
                            .collect();
                        rendered.push_str(&format!(
                            "<!--hedit-markdown-anchor:{index}-->\n{continuation}\n{continuation}"
                        ));
                        copied = span.start;
                        heading = Some((
                            index,
                            String::new(),
                            (list_depth > 0).then_some(continuation),
                        ));
                    }
                    _ => {}
                }
            }
            Event::Text(text) if figure.is_some() => {
                figure.as_mut().unwrap().2.source.push_str(&text)
            }
            Event::Text(text)
            | Event::Code(text)
            | Event::InlineMath(text)
            | Event::DisplayMath(text)
                if heading.is_some() =>
            {
                heading.as_mut().unwrap().1.push_str(&text);
            }
            Event::End(end) => {
                if end == pulldown_cmark::TagEnd::CodeBlock {
                    if let Some((start, continuation, value)) = figure.take() {
                        rendered.push_str(&text[copied..start]);
                        rendered.push_str(&format!(
                            "<!--hedit-markdown-figure:{}-->\n{continuation}\n",
                            figures.len()
                        ));
                        copied = span.end;
                        figures.push(value);
                    }
                }
                if matches!(end, pulldown_cmark::TagEnd::List(_)) {
                    list_depth = list_depth.saturating_sub(1);
                }
                if matches!(end, pulldown_cmark::TagEnd::Heading(_)) {
                    if let Some((index, label, continuation)) = heading.take() {
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
                        headings.insert(unique, index);
                        if let Some(continuation) = continuation {
                            rendered.push_str(&text[copied..span.end]);
                            // The native renderer suppresses heading line
                            // breaks inside lists; keep the next paragraph
                            // from running into the heading on the same row.
                            rendered.push_str(&format!(
                                "\n{continuation}<!--hedit-markdown-break-->\n{continuation}\n"
                            ));
                            copied = span.end;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    rendered.push_str(&text[copied..]);
    Prepared {
        rendered,
        targets,
        headings,
        figures,
        heading_chars,
        source_len: counted_chars + text[counted_byte..].chars().count(),
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
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
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
        let path = local_path(&root, target).map_err(LoadError::Loading)?;
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
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new("Markdown")
                    .strong()
                    .color(self.theme.accent.color()),
            );
            for (label, view, action) in [
                ("Source", MarkdownView::Source, Command::MarkdownSource),
                ("Split", MarkdownView::Split, Command::MarkdownSplit),
                ("Preview", MarkdownView::Preview, Command::MarkdownPreview),
            ] {
                if ui
                    .add_enabled(
                        self.command_enabled(action),
                        egui::Button::new(label).selected(mode == view),
                    )
                    .clicked()
                {
                    command = Some(action);
                }
            }
            if available && mode != MarkdownView::Source {
                ui.weak("Live preview");
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
                let mut source = area;
                let mut preview = area;
                // Stack the views in narrow windows instead of crushing them.
                if area.width() >= 680.0 {
                    source.max.x = area.center().x - 6.0;
                    preview.min.x = area.center().x + 6.0;
                } else {
                    source.max.y = area.center().y - 6.0;
                    preview.min.y = area.center().y + 6.0;
                }
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
            self.markdown.media.forget(id);
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
                    Ok(prepared) => links.prepared = Some(Arc::new(prepared)),
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
                    Ok(prepared) => links.prepared = Some(Arc::new(prepared)),
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
        let media = self.markdown.media.clone();
        let figures = prepared.clone();
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
        let html = move |ui: &mut egui::Ui, html: &str| {
            if let Some(marker) = html
                .trim()
                .strip_prefix("<!--hedit-markdown-anchor:")
                .and_then(|s| s.strip_suffix("-->"))
            {
                if let Ok(index) = marker.parse::<usize>() {
                    if let Some(&source) = figures.heading_chars.get(index) {
                        positions
                            .borrow_mut()
                            .push((source as f32, ui.next_widget_position().y));
                    }
                }
                if marker.parse::<usize>().ok() == anchor && anchor.is_some() {
                    ui.scroll_to_rect(
                        egui::Rect::from_min_size(ui.next_widget_position(), egui::vec2(1.0, 1.0)),
                        Some(egui::Align::Min),
                    );
                    seen.store(true, Ordering::Relaxed);
                }
            } else if html.trim() == "<!--hedit-markdown-break-->" {
                ui.label("\n");
            } else if let Some(index) = html
                .trim()
                .strip_prefix("<!--hedit-markdown-figure:")
                .and_then(|s| s.strip_suffix("-->"))
                .and_then(|s| s.parse::<usize>().ok())
            {
                if let Some(figure) = figures.figures.get(index) {
                    media.show(ui, id, &figure.source, figure.kind);
                }
            } else {
                ui.label(html);
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
        let surface = self.theme.surface.color();
        let mut content_top = 0.0;
        let output = ui.push_id(("markdown", id, pane), |ui| {
            let mut scroll = egui::ScrollArea::both()
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
                egui::Frame::new()
                    .fill(surface)
                    .inner_margin(20.0)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.style_mut()
                            .text_styles
                            .insert(egui::TextStyle::Body, FontId::proportional(font_size + 2.0));
                        ui.style_mut().text_styles.insert(
                            egui::TextStyle::Heading,
                            FontId::proportional(font_size + 14.0),
                        );
                        ui.style_mut()
                            .text_styles
                            .insert(egui::TextStyle::Monospace, FontId::monospace(font_size));
                        ui.style_mut().url_in_tooltip = true;
                        CommonMarkViewer::new()
                            .default_implicit_uri_scheme(format!("{IMAGE_SCHEME}{id}/"))
                            .max_image_width(Some(ui.available_width().max(0.0) as usize))
                            .render_html_fn(Some(&html))
                            .render_math_fn(Some(&math))
                            .show(ui, cache, &prepared.rendered);
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
