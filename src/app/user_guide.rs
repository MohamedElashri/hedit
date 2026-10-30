//! Read the documentation shipped in this executable, without disk or network I/O.
use super::*;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

macro_rules! documents {
    ($($path:literal),* $(,)?) => {
        const DOCUMENTS: &[(&str, &str)] = &[$(($path, include_str!(concat!("../../", $path)))),*];
    };
}
documents! {
    "USER_GUIDE.md", "DEVELOPER_GUIDE.md", "CHANGELOG.md", "LICENSE",
    "assets/licenses/theme-palettes.md", "vendor/egui_commonmark/PATCH.md",
    "assets/dictionaries/en_US-LICENSE.txt",
    "assets/licenses/STIX-Two-Math-OFL.txt", "assets/licenses/latex-rust-NOTICE.txt",
    "assets/licenses/latex-rust-MIT.txt", "assets/licenses/latex-rust-dvipsnames.tsv",
    "assets/licenses/LPPL-1.3c.txt", "assets/licenses/mermaid-rs-renderer-MIT.txt",
    "assets/licenses/resvg-MIT.txt", "assets/licenses/egui_commonmark-MIT.txt",
    "assets/licenses/ayu-colors-MIT.txt", "assets/licenses/night-owl-MIT.txt",
    "assets/licenses/cobalt2-MIT.txt", "assets/licenses/everforest-MIT.txt",
    "assets/licenses/kanagawa-MIT.txt", "assets/licenses/flexoki-MIT.txt",
    "assets/licenses/vscode-MIT.txt", "assets/licenses/tomorrow-theme-MIT.txt",
    "assets/licenses/horizon-MIT.txt", "assets/licenses/iceberg-MIT.txt",
    "assets/licenses/papercolor-MIT.txt", "assets/licenses/synthwave-84-MIT.txt",
    "assets/licenses/alabaster-MIT.txt",
}

struct Reader {
    prepared: markdown::Prepared,
    topics: Vec<String>,
}
impl Reader {
    fn new(document: usize) -> Self {
        let (path, source) = DOCUMENTS[document];
        let text = if path.ends_with(".md") {
            source.into()
        } else {
            format!("```text\n{source}\n```\n")
        };
        let mut topics = vec![];
        let mut heading = None;
        for event in Parser::new(&text) {
            match event {
                Event::Start(Tag::Heading { .. }) => heading = Some(String::new()),
                Event::Text(text) | Event::Code(text) if heading.is_some() => {
                    heading.as_mut().unwrap().push_str(&text)
                }
                Event::End(TagEnd::Heading(_)) => topics.push(heading.take().unwrap()),
                _ => {}
            }
        }
        Self {
            prepared: markdown::prepare(&text),
            topics,
        }
    }
}

#[derive(Default)]
pub(super) struct Guide {
    pub open: bool,
    document: usize,
    reader: Option<Reader>,
    cache: CommonMarkCache,
    jump: Option<usize>,
    scroll: Option<f32>,
    position: f32,
    history: Vec<(usize, f32)>,
    notice: Option<String>,
}

impl Guide {
    fn load(&mut self, document: usize) {
        if self.reader.is_none() || self.document != document {
            self.reader = Some(Reader::new(document));
            self.document = document;
            self.notice = None;
        }
    }
    pub fn start(&mut self) {
        if self.document != 0 {
            self.scroll = Some(0.0);
            self.jump = None;
        }
        self.load(0);
        self.history.clear();
        self.open = true;
    }
    fn title(&self) -> &str {
        match self.document {
            0 => "User guide",
            1 => "Developer guide",
            2 => "Release notes",
            3 => "MIT License",
            _ => DOCUMENTS[self.document].0.rsplit('/').next().unwrap(),
        }
    }
    fn back(&mut self) {
        if let Some((document, offset)) = self.history.pop() {
            self.load(document);
            self.jump = None;
            self.scroll = Some(offset);
        }
    }
    fn jump_to(&mut self, heading: Option<usize>) {
        self.history.push((self.document, self.position));
        self.jump = heading;
        self.scroll = heading.is_none().then_some(0.0);
        self.notice = None;
    }
    /// Resolve only explicitly bundled paths, never paths on the user's disk.
    fn resolve(&self, target: &str) -> Option<(usize, String)> {
        let (path, fragment) = target.split_once('#').unwrap_or((target, ""));
        let path = percent_encoding::percent_decode_str(path)
            .decode_utf8()
            .ok()?;
        let document = if path.is_empty() {
            self.document
        } else {
            let mut parts: Vec<_> = DOCUMENTS[self.document].0.split('/').collect();
            parts.pop();
            for part in path.split('/') {
                match part {
                    "" | "." => {}
                    ".." => {
                        parts.pop()?;
                    }
                    _ => parts.push(part),
                }
            }
            DOCUMENTS.iter().position(|(p, _)| *p == parts.join("/"))?
        };
        Some((
            document,
            percent_encoding::percent_decode_str(fragment)
                .decode_utf8()
                .ok()?
                .into_owned(),
        ))
    }
    fn navigate(&mut self, ctx: &egui::Context, target: &str) {
        if target.starts_with("https://") || target.starts_with("http://") {
            ctx.open_url(egui::OpenUrl::new_tab(target));
            return;
        }
        if let Some((document, fragment)) = self.resolve(target) {
            self.history.push((self.document, self.position));
            self.load(document);
            self.notice = None;
            self.scroll = Some(0.0);
            self.jump = self
                .reader
                .as_ref()
                .unwrap()
                .prepared
                .headings
                .get(&fragment)
                .copied();
            if !fragment.is_empty() && self.jump.is_none() {
                self.notice = Some("That section is not in this bundled document.".into());
            }
        } else {
            self.notice = Some("This reference is not bundled with the application.".into());
        }
    }
}

impl HeditApp {
    pub(super) fn user_guide(&mut self, ctx: &egui::Context) {
        if !self.guide.open {
            return;
        }
        self.guide.load(self.guide.document);
        let width = (ctx.screen_rect().width() - 56.0).clamp(160.0, 940.0);
        let height = (ctx.screen_rect().height() - 150.0).clamp(60.0, 650.0);
        let mut close = false;
        let mut clicked = None;
        let response = egui::Modal::new(egui::Id::new("user-guide"))
            .frame(egui::Frame::popup(&ctx.style()).inner_margin(14.0))
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(self.guide.title()).size(20.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        close = controls::close_button(ui, "Close guide · Esc").clicked();
                    });
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!self.guide.history.is_empty(), egui::Button::new("Back"))
                        .clicked()
                    {
                        self.guide.back();
                    }
                    egui::ComboBox::from_id_salt("guide-contents")
                        .selected_text("Contents")
                        .width((ui.available_width() - 12.0).min(350.0))
                        .height((height - 30.0).max(80.0))
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(false, "Top").clicked() {
                                self.guide.jump_to(None);
                            }
                            let mut selected = None;
                            for (index, title) in self
                                .guide
                                .reader
                                .as_ref()
                                .unwrap()
                                .topics
                                .iter()
                                .enumerate()
                            {
                                if ui.selectable_label(false, title).clicked() {
                                    selected = Some(index);
                                }
                            }
                            if let Some(index) = selected {
                                self.guide.jump_to(Some(index));
                            }
                        });
                });
                ui.separator();
                if let Some(notice) = &self.guide.notice {
                    ui.weak(notice);
                }
                let reader = self.guide.reader.as_ref().unwrap();
                self.guide.cache.link_hooks_clear();
                for target in &reader.prepared.targets {
                    self.guide.cache.add_link_hook(target);
                }
                let jump = self.guide.jump;
                let found = std::rc::Rc::new(std::cell::Cell::new(false));
                let seen = std::rc::Rc::clone(&found);
                let html = move |ui: &mut egui::Ui, text: &str| {
                    if let Some(marker) = text
                        .trim()
                        .strip_prefix("<!--hedit-markdown-anchor:")
                        .and_then(|s| s.strip_suffix("-->"))
                    {
                        if jump.is_some() && marker.parse::<usize>().ok() == jump {
                            ui.scroll_to_rect(
                                egui::Rect::from_min_size(
                                    ui.next_widget_position(),
                                    egui::vec2(1.0, 1.0),
                                ),
                                Some(egui::Align::Min),
                            );
                            seen.set(true);
                        }
                    } else {
                        ui.label(text);
                    }
                };
                let mut scroll = egui::ScrollArea::both()
                    .id_salt(("guide-body", self.guide.document))
                    .auto_shrink([false, false])
                    .min_scrolled_height(height)
                    .max_height(height);
                if let Some(offset) = self.guide.scroll.take() {
                    scroll = scroll.vertical_scroll_offset(offset);
                }
                let output = scroll.show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.style_mut().url_in_tooltip = true;
                    CommonMarkViewer::new().render_html_fn(Some(&html)).show(
                        ui,
                        &mut self.guide.cache,
                        &reader.prepared.rendered,
                    );
                });
                self.guide.position = output.state.offset.y;
                if found.get() {
                    self.guide.jump = None;
                }
                for target in &reader.prepared.targets {
                    if self.guide.cache.get_link_hook(target) == Some(true) {
                        clicked = Some(target.clone());
                        break;
                    }
                }
            });
        if let Some(target) = clicked {
            self.guide.navigate(ctx, &target);
            ctx.request_repaint();
        }
        if close || response.should_close() {
            self.guide.open = false;
            self.focus_editor = true;
        }
    }
}

#[cfg(test)]
mod tests;
