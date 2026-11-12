//! A document is a vertical sequence of blocks, not a single wrapping UI row.
//!
//! The tree is prepared alongside the original events, once per revision. Each
//! block owns its geometry, margins and overflow; source spans remain intact.
use std::ops::Range;

use egui::{Align, Color32, Layout, Rect, Sense, Stroke, TextStyle, Ui};
use egui_commonmark_backend::{misc::CommonMarkOptions, CommonMarkCache};
use pulldown_cmark::{Event, Tag, TagEnd};

use super::inline::{CheckboxClickEvent, InlineRenderer};

pub(crate) type Events = [(Event<'static>, Range<usize>)];

#[derive(Default)]
pub(crate) struct Document {
    nodes: Vec<Node>,
}

struct Node {
    events: Range<usize>,
    children: Vec<Node>,
    opaque: bool,
}

impl Document {
    pub(crate) fn new(events: &Events) -> Self {
        let mut cursor = 0;
        Self {
            nodes: parse(events, &mut cursor, None, 0),
        }
    }
}

fn parse(events: &Events, cursor: &mut usize, end: Option<TagEnd>, depth: usize) -> Vec<Node> {
    let mut nodes = Vec::new();
    while let Some((event, _)) = events.get(*cursor) {
        let start = *cursor;
        *cursor += 1;
        if let Event::End(tag) = event {
            if Some(*tag) == end {
                break;
            }
            continue;
        }
        let opaque = depth >= 64;
        let children = if let Event::Start(tag) = event {
            if opaque {
                let mut nesting = 1;
                while let Some((event, _)) = events.get(*cursor) {
                    *cursor += 1;
                    match event {
                        Event::Start(_) => nesting += 1,
                        Event::End(_) => nesting -= 1,
                        _ => {}
                    }
                    if nesting == 0 {
                        break;
                    }
                }
                Vec::new()
            } else {
                parse(events, cursor, Some(tag.to_end()), depth + 1)
            }
        } else {
            Vec::new()
        };
        nodes.push(Node {
            events: start..*cursor,
            children,
            opaque,
        });
    }
    nodes
}

pub(crate) struct Renderer<'a, 'f> {
    events: &'a Events,
    cache: &'a mut CommonMarkCache,
    options: &'a CommonMarkOptions<'f>,
    inline: InlineRenderer<'f>,
    code: Option<&'f crate::RenderCodeBlockFn>,
    block: Option<&'f crate::RenderBlockFn>,
}

impl<'a, 'f> Renderer<'a, 'f> {
    pub(crate) fn new(
        events: &'a Events,
        cache: &'a mut CommonMarkCache,
        options: &'a CommonMarkOptions<'f>,
        code: Option<&'f crate::RenderCodeBlockFn>,
        block: Option<&'f crate::RenderBlockFn>,
        measure: Option<&'f crate::MeasureMathFn>,
    ) -> Self {
        Self {
            events,
            cache,
            options,
            inline: InlineRenderer::new(measure),
            code,
            block,
        }
    }

    pub(crate) fn show(
        mut self,
        ui: &mut Ui,
        document: &Document,
    ) -> (egui::InnerResponse<()>, Vec<CheckboxClickEvent>) {
        let response = ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            ui.set_width(ui.available_width().max(1.0));
            self.blocks(ui, &document.nodes, false, 0);
        });
        (response, self.inline.take_checkbox_events())
    }

    fn first(&self, node: &Node) -> &'a Event<'static> {
        &self.events[node.events.start].0
    }
    fn scale(&self, ui: &Ui) -> f32 {
        TextStyle::Body.resolve(ui.style()).size / 14.0
    }
    fn mark(&self, ui: &mut Ui, node: &Node, heading: Option<u8>) {
        if let Some(block) = self.block {
            block(ui, self.events[node.events.start].1.clone(), heading);
        }
    }

    fn blocks(&mut self, ui: &mut Ui, nodes: &[Node], item: bool, depth: usize) {
        let scale = self.scale(ui);
        let mut bottom = 0.0_f32;
        let mut at = 0;
        while at < nodes.len() {
            let node = &nodes[at];
            if matches!(self.first(node), Event::Start(Tag::MetadataBlock(_)))
                || self.hidden_comment(node)
            {
                at += 1;
                continue;
            }
            if !is_block(self.first(node)) {
                // Tight lists contain bare inline events. Keep an entire run
                // together so punctuation and styles share one wrap decision.
                let start = node.events.start;
                let mut end = node.events.end;
                at += 1;
                while at < nodes.len() && !is_block(self.first(&nodes[at])) {
                    end = nodes[at].events.end;
                    at += 1;
                }
                ui.add_space(bottom);
                self.inline.render_inline(
                    ui,
                    self.cache,
                    self.options,
                    &self.events[start..end],
                    None,
                    false,
                    None,
                );
                bottom = if item { 0.0 } else { 16.0 * scale };
                continue;
            }
            let top = match self.first(node) {
                Event::Start(Tag::Heading { level, .. })
                    if *level != pulldown_cmark::HeadingLevel::H1 =>
                {
                    24.0 * scale
                }
                _ => 0.0,
            };
            ui.add_space(bottom.max(top));
            ui.push_id(node.events.start, |ui| self.block(ui, node, depth));
            bottom = match self.first(node) {
                Event::Start(Tag::List(_) | Tag::Table(_)) => 9.8 * scale,
                Event::Start(Tag::Paragraph) if item => 9.8 * scale,
                Event::Start(Tag::MetadataBlock(_)) => 0.0,
                _ => 16.0 * scale,
            };
            at += 1;
        }
    }

    fn block(&mut self, ui: &mut Ui, node: &Node, depth: usize) {
        let scale = self.scale(ui);
        let heading = match self.first(node) {
            Event::Start(Tag::Heading { level, .. }) => Some(*level as u8),
            _ => None,
        };
        self.mark(ui, node, heading);
        if node.opaque
            || (ui.available_width() < TextStyle::Body.resolve(ui.style()).size * 12.0
                && matches!(
                    self.first(node),
                    Event::Start(Tag::BlockQuote(_) | Tag::List(_))
                ))
        {
            // Extremely deep input stays readable without recursive layout or
            // an unbounded Rust call stack. Ordinary nesting keeps its tree.
            let events: Vec<_> = inner(self.events, node)
                .iter()
                .filter(|(e, _)| !matches!(e, Event::Start(_) | Event::End(_)))
                .cloned()
                .collect();
            self.inline
                .render_inline(ui, self.cache, self.options, &events, None, false, None);
            return;
        }
        match self.first(node) {
            Event::Start(Tag::Paragraph) => {
                self.inline.render_inline(
                    ui,
                    self.cache,
                    self.options,
                    inner(self.events, node),
                    None,
                    false,
                    None,
                );
            }
            Event::Start(Tag::Heading { level, .. }) => {
                let level = *level as u8 - 1;
                self.inline.render_inline(
                    ui,
                    self.cache,
                    self.options,
                    inner(self.events, node),
                    Some(level),
                    false,
                    None,
                );
                if level < 2 {
                    let size = TextStyle::Body.resolve(ui.style()).size
                        * if level == 0 { 2.0 } else { 1.5 };
                    ui.add_space(size * 0.3);
                    rule(ui, ui.available_width(), border(ui, false));
                }
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let source: String = inner(self.events, node)
                    .iter()
                    .filter_map(|(e, _)| {
                        if let Event::Text(t) = e {
                            Some(t.as_ref())
                        } else {
                            None
                        }
                    })
                    .collect();
                let language = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(lang) => Some(lang.as_ref()),
                    _ => None,
                };
                if let Some(code) = self.code {
                    code(ui, &source, language);
                } else {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(source.trim_end_matches('\n')).monospace(),
                        )
                        .wrap(),
                    );
                }
            }
            Event::Start(Tag::BlockQuote(_)) => {
                let marker = ui.painter().add(egui::Shape::Noop);
                let width = ui.available_width();
                let alert_marker: String = node
                    .children
                    .first()
                    .map(|first| {
                        inner(self.events, first)
                            .iter()
                            .take_while(|(e, _)| !matches!(e, Event::SoftBreak | Event::HardBreak))
                            .filter_map(|(e, _)| {
                                if let Event::Text(text) = e {
                                    Some(text.as_ref())
                                } else {
                                    None
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let alert = egui_commonmark_backend::alerts::try_get_alert(
                    &self.options.alerts,
                    alert_marker.trim(),
                )
                .cloned()
                .map(|mut alert| {
                    alert.accent_color = readable_color(ui, alert.accent_color);
                    alert
                });
                let response = egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: (15.0 * scale) as i8,
                        right: (16.0 * scale) as i8,
                        top: 0,
                        bottom: 0,
                    })
                    .show(ui, |ui| {
                        ui.set_width((width - 31.0 * scale).max(1.0));
                        if let Some(alert) = &alert {
                            let mut title = egui::RichText::new(&alert.identifier_rendered)
                                .strong()
                                .color(alert.accent_color);
                            if let Some(font) = ui
                                .style()
                                .text_styles
                                .get(&TextStyle::Name("MarkdownStrong".into()))
                            {
                                title = title.family(font.family.clone());
                            }
                            ui.label(title);
                            if let Some(first) = node.children.first() {
                                let events = inner(self.events, first);
                                let skip = events
                                    .iter()
                                    .position(|(e, _)| {
                                        matches!(e, Event::SoftBreak | Event::HardBreak)
                                    })
                                    .map_or(events.len(), |at| at + 1);
                                if events.len() > skip {
                                    ui.add_space(8.0 * scale);
                                    self.inline.render_inline(
                                        ui,
                                        self.cache,
                                        self.options,
                                        &events[skip..],
                                        None,
                                        false,
                                        None,
                                    );
                                }
                            }
                            if node.children.len() > 1 {
                                ui.add_space(16.0 * scale);
                                self.blocks(ui, &node.children[1..], false, depth);
                            }
                        } else {
                            self.blocks(ui, &node.children, false, depth);
                        }
                    })
                    .response;
                let rect = Rect::from_min_size(
                    response.rect.min,
                    egui::vec2(5.0 * scale, response.rect.height()),
                );
                ui.painter().set(
                    marker,
                    egui::Shape::rect_filled(
                        rect,
                        1,
                        alert
                            .as_ref()
                            .map_or(ui.visuals().hyperlink_color.gamma_multiply(0.45), |alert| {
                                alert.accent_color
                            }),
                    ),
                );
            }
            Event::Start(Tag::List(start)) => self.list(ui, node, *start, depth),
            Event::Start(Tag::Table(alignments)) => self.table(ui, node, alignments),
            Event::Start(Tag::FootnoteDefinition(label)) => {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0 * scale;
                    ui.label(egui::RichText::new(format!("{label}.")).small().strong());
                    ui.vertical(|ui| self.blocks(ui, &node.children, false, depth));
                });
            }
            Event::Start(Tag::MetadataBlock(_)) => {}
            Event::Start(Tag::HtmlBlock) => {
                self.inline.render_inline(
                    ui,
                    self.cache,
                    self.options,
                    inner(self.events, node),
                    None,
                    false,
                    None,
                );
            }
            Event::Rule => rule(ui, ui.available_width(), border(ui, false)),
            Event::DisplayMath(source) => {
                if let Some(math) = self.options.math_fn {
                    math(ui, source, false);
                }
            }
            // Definition lists retain their structural indentation, even when
            // the backend enables the extension independently of the app.
            Event::Start(Tag::DefinitionList | Tag::Item) => {
                self.blocks(ui, &node.children, false, depth)
            }
            Event::Start(Tag::DefinitionListDefinition) => {
                let width = ui.available_width();
                ui.horizontal_top(|ui| {
                    ui.add_space(TextStyle::Body.resolve(ui.style()).size * 2.0);
                    ui.vertical(|ui| {
                        ui.set_width((width - 28.0 * scale).max(1.0));
                        self.blocks(ui, &node.children, false, depth);
                    });
                });
            }
            Event::Start(Tag::DefinitionListTitle) => {
                self.inline.render_inline(
                    ui,
                    self.cache,
                    self.options,
                    inner(self.events, node),
                    None,
                    true,
                    None,
                );
            }
            _ => {}
        }
    }

    fn list(&mut self, ui: &mut Ui, node: &Node, start: Option<u64>, depth: usize) {
        let body = TextStyle::Body.resolve(ui.style()).size;
        let indent = (body * 2.0).max(start.map_or(0.0, |start| {
            ui.fonts(|f| {
                f.layout_no_wrap(
                    format!("{}.", start.saturating_add(node.children.len() as u64)),
                    TextStyle::Body.resolve(ui.style()),
                    Color32::WHITE,
                )
                .size()
                .x + body * 0.5
            })
        }));
        for (index, item) in node.children.iter().enumerate() {
            let task = inner(self.events, item)
                .iter()
                .take_while(|(event, _)| {
                    !matches!(
                        event,
                        Event::Text(_) | Event::Code(_) | Event::Start(Tag::List(_))
                    )
                })
                .find_map(|(event, span)| {
                    if let Event::TaskListMarker(checked) = event {
                        Some((*checked, span.clone()))
                    } else {
                        None
                    }
                });
            if index > 0
                && item
                    .children
                    .iter()
                    .any(|n| matches!(self.first(n), Event::Start(Tag::Paragraph)))
            {
                ui.add_space(body * 0.7);
            }
            self.mark(ui, item, None);
            let width = ui.available_width();
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(indent, body * 1.6), Sense::hover());
                if let Some((checked, span)) = task {
                    let task_rect = Rect::from_center_size(
                        egui::pos2(rect.right() - body * 0.7, rect.center().y),
                        egui::vec2(body * 1.6, body * 1.6),
                    );
                    ui.scope_builder(egui::UiBuilder::new().max_rect(task_rect), |ui| {
                        ui.spacing_mut().icon_width = body;
                        self.inline.task_marker(ui, checked, span, self.options);
                    });
                } else if let Some(start) = start {
                    ui.painter().text(
                        egui::pos2(rect.right() - body * 0.5, rect.top()),
                        egui::Align2::RIGHT_TOP,
                        format!("{}.", start.saturating_add(index as u64)),
                        TextStyle::Body.resolve(ui.style()),
                        ui.visuals().text_color(),
                    );
                } else {
                    let center = egui::pos2(rect.right() - body * 0.5, rect.top() + body * 0.7);
                    if depth % 2 == 0 {
                        ui.painter()
                            .circle_filled(center, body * 0.11, ui.visuals().text_color());
                    } else {
                        ui.painter().circle_stroke(
                            center,
                            body * 0.11,
                            Stroke::new(1.0_f32, ui.visuals().text_color()),
                        );
                    }
                }
                ui.vertical(|ui| {
                    ui.set_width((width - indent).max(1.0));
                    self.blocks(ui, &item.children, true, depth + 1);
                });
            });
        }
    }

    fn hidden_comment(&self, node: &Node) -> bool {
        if !matches!(self.first(node), Event::Start(Tag::HtmlBlock)) {
            return false;
        }
        let mut comment = false;
        inner(self.events, node)
            .iter()
            .all(|(event, _)| match event {
                Event::Html(text) => super::inline::visible_html(text, &mut comment)
                    .trim()
                    .is_empty(),
                _ => true,
            })
    }

    fn table(&mut self, ui: &mut Ui, node: &Node, alignments: &[pulldown_cmark::Alignment]) {
        let rows: Vec<_> = node
            .children
            .iter()
            .filter(|n| matches!(self.first(n), Event::Start(Tag::TableHead | Tag::TableRow)))
            .collect();
        let columns = alignments.len();
        if columns == 0 {
            return;
        }
        let scale = self.scale(ui);
        let padding = 20.0 * scale;
        let mut preferred = vec![40.0 * scale; columns];
        let mut minimum = preferred.clone();
        for (row_index, row) in rows.iter().enumerate() {
            for (column, cell) in row.children.iter().take(columns).enumerate() {
                let (natural, word) = self.inline.intrinsic_width(
                    ui,
                    self.options,
                    inner(self.events, cell),
                    row_index == 0,
                );
                preferred[column] = preferred[column].max((natural + padding).min(2400.0 * scale));
                minimum[column] = minimum[column].max(word.min(140.0 * scale) + padding);
            }
        }
        for (preferred, minimum) in preferred.iter_mut().zip(&minimum) {
            *preferred = preferred.max(*minimum);
        }
        let available = ui.available_width().max(1.0);
        let natural: f32 = preferred.iter().sum();
        let min: f32 = minimum.iter().sum();
        let ratio = ((available - min) / (natural - min).max(1.0)).clamp(0.0, 1.0);
        let widths: Vec<_> = preferred
            .iter()
            .zip(&minimum)
            .map(|(natural, min)| min + (natural - min) * ratio)
            .collect();
        let table_width: f32 = widths.iter().sum();
        egui::ScrollArea::horizontal()
            .id_salt("table-overflow")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.set_width(table_width);
                for (row_index, row) in rows.iter().enumerate() {
                    let response = ui
                        .horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            for (column, width) in widths.iter().copied().enumerate() {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(width, 0.0),
                                    Layout::top_down(Align::LEFT),
                                    |ui| {
                                        ui.set_width(width);
                                        egui::Frame::new()
                                            .inner_margin(egui::Margin {
                                                left: (10.0 * scale) as i8,
                                                right: (10.0 * scale) as i8,
                                                top: (5.0 * scale) as i8,
                                                bottom: (5.0 * scale) as i8,
                                            })
                                            .show(ui, |ui| {
                                                let inner_width = (width - padding).max(1.0);
                                                ui.set_width(inner_width);
                                                ui.set_min_height(
                                                    TextStyle::Body.resolve(ui.style()).size * 1.6,
                                                );
                                                if let Some(cell) = row.children.get(column) {
                                                    let align = match alignments[column] {
                                                        pulldown_cmark::Alignment::Center => {
                                                            Align::Center
                                                        }
                                                        pulldown_cmark::Alignment::Right => {
                                                            Align::RIGHT
                                                        }
                                                        _ => Align::LEFT,
                                                    };
                                                    self.inline.render_inline(
                                                        ui,
                                                        self.cache,
                                                        self.options,
                                                        inner(self.events, cell),
                                                        None,
                                                        row_index == 0,
                                                        Some(align),
                                                    );
                                                }
                                            });
                                    },
                                );
                            }
                        })
                        .response;
                    if row_index < rows.len().saturating_sub(1) {
                        ui.painter().line_segment(
                            [
                                response.rect.left_bottom(),
                                egui::pos2(
                                    response.rect.left() + table_width,
                                    response.rect.bottom(),
                                ),
                            ],
                            Stroke::new(1.0_f32, border(ui, row_index == 0)),
                        );
                        ui.add_space(1.0);
                    }
                }
            });
    }
}

fn is_block(event: &Event<'_>) -> bool {
    matches!(
        event,
        Event::Rule
            | Event::DisplayMath(_)
            | Event::Start(
                Tag::Paragraph
                    | Tag::Heading { .. }
                    | Tag::CodeBlock(_)
                    | Tag::BlockQuote(_)
                    | Tag::List(_)
                    | Tag::Table(_)
                    | Tag::FootnoteDefinition(_)
                    | Tag::HtmlBlock
                    | Tag::MetadataBlock(_)
                    | Tag::DefinitionList
                    | Tag::DefinitionListTitle
                    | Tag::DefinitionListDefinition
            )
    )
}

fn border(ui: &Ui, strong: bool) -> Color32 {
    let alpha = if strong { 176 } else { 46 };
    if ui.visuals().dark_mode {
        Color32::from_white_alpha(alpha)
    } else {
        Color32::from_black_alpha(alpha)
    }
}
fn rule(ui: &mut Ui, width: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width.max(1.0), 1.0), Sense::hover());
    ui.painter().line_segment(
        [rect.left_center(), rect.right_center()],
        Stroke::new(1.0_f32, color),
    );
}

fn inner<'a>(events: &'a Events, node: &Node) -> &'a Events {
    if matches!(events[node.events.start].0, Event::Start(_)) {
        &events[node.events.start + 1..node.events.end.saturating_sub(1)]
    } else {
        &events[node.events.clone()]
    }
}

pub(crate) fn parser_options_math(math: bool) -> pulldown_cmark::Options {
    let options = egui_commonmark_backend::pulldown::parser_options();
    if math {
        options | pulldown_cmark::Options::ENABLE_MATH
    } else {
        options
    }
}

fn readable_color(ui: &Ui, mut color: Color32) -> Color32 {
    fn luminance(color: Color32) -> f32 {
        [color.r(), color.g(), color.b()]
            .iter()
            .zip([0.2126, 0.7152, 0.0722])
            .map(|(value, weight)| {
                let value = f32::from(*value) / 255.0;
                (if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }) * weight
            })
            .sum()
    }
    let background = luminance(ui.visuals().panel_fill);
    let text = ui.visuals().text_color();
    for _ in 0..32 {
        let foreground = luminance(color);
        if (foreground.max(background) + 0.05) / (foreground.min(background) + 0.05) >= 4.5 {
            return color;
        }
        color = Color32::from_rgb(
            (f32::from(color.r()) * 0.85 + f32::from(text.r()) * 0.15).round() as u8,
            (f32::from(color.g()) * 0.85 + f32::from(text.g()) * 0.15).round() as u8,
            (f32::from(color.b()) * 0.85 + f32::from(text.b()) * 0.15).round() as u8,
        );
    }
    text
}
