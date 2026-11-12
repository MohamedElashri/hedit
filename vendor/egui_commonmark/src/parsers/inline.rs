//! Continuous inline text, native selection and media inside a document block.
use crate::{CommonMarkCache, CommonMarkOptions};
use egui::{TextStyle, Ui};
use egui_commonmark_backend::elements::{footnote_start, ImmutableCheckbox};
use egui_commonmark_backend::misc::{Image, Link, Style};
use pulldown_cmark::CowStr;
use std::ops::Range;
const HEADING_SCALE: [f32; 6] = [2.0, 1.5, 1.25, 1.0, 0.875, 0.85];

pub(crate) fn newline(ui: &mut Ui) {
    if ui.next_widget_position().x > ui.max_rect().left() + 0.5 {
        ui.end_row();
    }
    ui.set_row_height(egui::TextStyle::Body.resolve(ui.style()).size * 1.6);
}

fn rich_text(style: &Style, ui: &Ui, text: &str, height: Option<f32>) -> egui::RichText {
    let mut inline = style.clone();
    inline.heading = None;
    inline.quote = false;
    let font = reader_font(style, ui);
    if style.emphasis && !style.code && font.is_some() {
        inline.emphasis = false;
    }
    let mut text = inline.to_richtext(ui, text);
    let body = egui::TextStyle::Body.resolve(ui.style()).size;
    if !style.code {
        if let Some(font) = &font {
            text = text.family(font.family.clone());
        }
    }
    let size = style.heading.map_or(body, |level| {
        body * HEADING_SCALE[usize::from(level).min(5)]
    });
    if style.heading.is_some() {
        text = text.strong();
    }
    text = text.size(size);
    let height = height.unwrap_or(size * if style.heading.is_some() { 1.25 } else { 1.6 });
    let height = if style.code {
        // Fixed line heights align the tops of different fonts in egui. Shorten
        // the code's line box by twice the ascent difference so center alignment
        // gives it the prose baseline and a vertically symmetric background.
        let mut prose = TextStyle::Body.resolve(ui.style());
        if let Some(font) = &font {
            prose.family = font.family.clone();
        }
        prose.size = size;
        let mut code = TextStyle::Monospace.resolve(ui.style());
        code.size = size;
        let ascent = |font| {
            ui.fonts(|fonts| {
                let galley = fonts.layout_no_wrap("M".into(), font, egui::Color32::WHITE);
                let glyph = &galley.rows[0].glyphs[0];
                glyph.font_impl_ascent + 0.5 * (glyph.font_height - glyph.font_impl_height)
            })
        };
        (height - 2.0 * (ascent(prose) - ascent(code))).max(1.0)
    } else {
        height
    };
    text.line_height(Some(height))
}

fn reader_font<'a>(style: &Style, ui: &'a Ui) -> Option<&'a egui::FontId> {
    let name = match (style.strong || style.heading.is_some(), style.emphasis) {
        (true, true) => "MarkdownStrongEmphasis",
        (false, true) => "MarkdownEmphasis",
        (true, false) => "MarkdownStrong",
        _ => return None,
    };
    ui.style().text_styles.get(&TextStyle::Name(name.into()))
}

/// Code's shorter, baseline-adjusted box must not tighten code-only wrapped
/// lines. Pad the laid-out rows without changing source text or font metrics.
fn pad_inline_rows(galley: &mut std::sync::Arc<egui::Galley>, height: f32) -> bool {
    let pixels_per_point = galley.pixels_per_point;
    let snap = |value: f32| (value * pixels_per_point).round() / pixels_per_point;
    let height = snap(height);
    if !galley
        .rows
        .iter()
        .any(|row| row.rect.height() < height - 0.5)
    {
        return false;
    }
    let galley = std::sync::Arc::make_mut(galley);
    let mut offset = 0.0;
    let mut mesh_bounds = egui::Rect::NOTHING;
    for row in &mut galley.rows {
        let padding = (height - row.rect.height()).max(0.0);
        let shift = egui::vec2(0.0, offset + snap(padding / 2.0));
        for glyph in &mut row.glyphs {
            glyph.pos += shift;
        }
        for vertex in &mut row.visuals.mesh.vertices {
            vertex.pos += shift;
        }
        row.visuals.mesh_bounds = row.visuals.mesh_bounds.translate(shift);
        mesh_bounds = mesh_bounds.union(row.visuals.mesh_bounds);
        row.rect = row.rect.translate(egui::vec2(0.0, offset));
        row.rect.max.y += padding;
        offset += padding;
    }
    galley.rect.max.y += offset;
    galley.mesh_bounds = mesh_bounds;
    true
}

fn with_math_font<R>(ui: &mut Ui, heading: Option<u8>, contents: impl FnOnce(&mut Ui) -> R) -> R {
    if let Some(level) = heading {
        let mut font = TextStyle::Body.resolve(ui.style());
        font.size *= HEADING_SCALE[usize::from(level).min(5)];
        if let Some(strong) = ui
            .style()
            .text_styles
            .get(&TextStyle::Name("MarkdownStrong".into()))
        {
            font.family = strong.family.clone();
        }
        ui.scope(|ui| {
            ui.style_mut().text_styles.insert(TextStyle::Body, font);
            contents(ui)
        })
        .inner
    } else {
        contents(ui)
    }
}

#[derive(Default)]
struct InlineFlow {
    job: egui::text::LayoutJob,
    links: Vec<(Range<usize>, String)>,
    link_start: Option<usize>,
    alignment: Option<egui::Align>,
}

pub(crate) struct CheckboxClickEvent {
    pub(crate) checked: bool,
    pub(crate) span: Range<usize>,
}

pub(crate) struct InlineRenderer<'f> {
    measure_math_fn: Option<&'f crate::MeasureMathFn>,
    paragraph_height: Option<f32>,
    flow: Option<InlineFlow>,
    flow_serial: usize,
    text_style: Style,
    inline_styles: Vec<Style>,
    link: Option<Link>,
    image: Option<Image>,
    html_comment: bool,
    checkbox_events: Vec<CheckboxClickEvent>,
}

impl<'f> InlineRenderer<'f> {
    pub(crate) fn new(measure_math_fn: Option<&'f crate::MeasureMathFn>) -> Self {
        Self {
            measure_math_fn,
            paragraph_height: None,
            flow: None,
            flow_serial: 0,
            text_style: Style::default(),
            inline_styles: Vec::new(),
            link: None,
            image: None,
            html_comment: false,
            checkbox_events: Vec::new(),
        }
    }
    pub(crate) fn task_marker(
        &mut self,
        ui: &mut Ui,
        mut checked: bool,
        span: Range<usize>,
        options: &CommonMarkOptions,
    ) {
        if options.mutable {
            if ui.add(egui::Checkbox::without_text(&mut checked)).clicked() {
                self.checkbox_events
                    .push(CheckboxClickEvent { checked, span });
            }
        } else {
            ui.add(ImmutableCheckbox::without_text(&mut checked));
        }
    }

    pub(crate) fn take_checkbox_events(&mut self) -> Vec<CheckboxClickEvent> {
        std::mem::take(&mut self.checkbox_events)
    }

    /// Inline layout is scoped to one document block. No list, quote, table or
    /// paragraph lifecycle is allowed to escape into its neighbouring blocks.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_inline(
        &mut self,
        ui: &mut Ui,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        events: &[(pulldown_cmark::Event<'_>, Range<usize>)],
        heading: Option<u8>,
        strong: bool,
        alignment: Option<egui::Align>,
    ) -> egui::Rect {
        self.text_style = Style {
            heading,
            strong,
            ..Default::default()
        };
        self.inline_styles.clear();
        self.link = None;
        self.image = None;
        self.html_comment = false;
        let text_only = !events.iter().any(|(e, _)| {
            matches!(
                e,
                pulldown_cmark::Event::InlineMath(_)
                    | pulldown_cmark::Event::FootnoteReference(_)
                    | pulldown_cmark::Event::Start(pulldown_cmark::Tag::Image { .. })
            )
        });
        self.flow = Some(InlineFlow {
            alignment: alignment.filter(|_| text_only),
            ..Default::default()
        });
        let body = TextStyle::Body.resolve(ui.style()).size;
        let nominal = heading.map_or(body * 1.6, |level| {
            body * HEADING_SCALE[usize::from(level).min(5)] * 1.25
        });
        let measurement = self.inline_measure(ui, events);
        self.paragraph_height = Some(measurement.map_or(nominal, |m| m.0));
        let width = ui.available_width().max(1.0);
        let natural = if !text_only && alignment.is_some() {
            self.intrinsic_width(ui, options, events, strong).0
        } else {
            width
        };
        let indent = if natural < width {
            (width - natural) * alignment.map_or(0.0, |align| align.to_factor())
        } else {
            0.0
        };
        let response = ui.allocate_ui_with_layout(
            egui::vec2(width, 0.0),
            egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
            |ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                ui.set_row_height(self.paragraph_height.unwrap());
                Self::reserve_inline(ui, width, measurement);
                if indent > 0.0 {
                    ui.add_space(indent);
                }
                for (event, span) in events {
                    self.event(ui, event.clone(), span.clone(), cache, options, width);
                }
                self.flush_inline(ui, cache);
            },
        );
        self.flow = None;
        self.paragraph_height = None;
        response.response.rect
    }

    /// CSS table layout starts with intrinsic column widths, then distributes
    /// available width between each column's minimum and preferred measures.
    pub(crate) fn intrinsic_width(
        &self,
        ui: &Ui,
        options: &CommonMarkOptions,
        events: &[(pulldown_cmark::Event<'_>, Range<usize>)],
        strong: bool,
    ) -> (f32, f32) {
        let mut style = Style {
            strong,
            ..Default::default()
        };
        let mut stack = Vec::new();
        let mut job = egui::text::LayoutJob::default();
        let mut word = 0.0_f32;
        let mut line = 0.0_f32;
        let mut widest = 0.0_f32;
        let mut image_depth = 0;
        for (event, _) in events {
            if image_depth > 0 {
                match event {
                    pulldown_cmark::Event::Start(pulldown_cmark::Tag::Image { .. }) => {
                        image_depth += 1
                    }
                    pulldown_cmark::Event::End(pulldown_cmark::TagEnd::Image) => image_depth -= 1,
                    _ => {}
                }
                continue;
            }
            let value = match event {
                pulldown_cmark::Event::Start(
                    tag @ (pulldown_cmark::Tag::Strong
                    | pulldown_cmark::Tag::Emphasis
                    | pulldown_cmark::Tag::Strikethrough),
                ) => {
                    stack.push(style.clone());
                    match tag {
                        pulldown_cmark::Tag::Strong => style.strong = true,
                        pulldown_cmark::Tag::Emphasis => style.emphasis = true,
                        _ => style.strikethrough = true,
                    }
                    continue;
                }
                pulldown_cmark::Event::End(
                    pulldown_cmark::TagEnd::Strong
                    | pulldown_cmark::TagEnd::Emphasis
                    | pulldown_cmark::TagEnd::Strikethrough,
                ) => {
                    style = stack.pop().unwrap_or_default();
                    continue;
                }
                pulldown_cmark::Event::InlineMath(source) if self.measure_math_fn.is_some() => {
                    line += measured_run(ui, &mut job);
                    let width = self.measure_math_fn.unwrap()(ui, source).width;
                    line += width;
                    word = word.max(width);
                    continue;
                }
                pulldown_cmark::Event::Start(pulldown_cmark::Tag::Image { dest_url, .. }) => {
                    image_depth = 1;
                    line += measured_run(ui, &mut job);
                    let image = Image::new(dest_url, options);
                    let width = match ui
                        .ctx()
                        .try_load_image(&image.uri, egui::load::SizeHint::default())
                    {
                        Ok(egui::load::ImagePoll::Ready { image }) => image.width() as f32,
                        _ => 80.0 * TextStyle::Body.resolve(ui.style()).size / 14.0,
                    }
                    .min(ui.available_width());
                    line += width;
                    word = word.max(width.min(80.0));
                    continue;
                }
                pulldown_cmark::Event::HardBreak => {
                    line += measured_run(ui, &mut job);
                    widest = widest.max(line);
                    line = 0.0;
                    continue;
                }
                pulldown_cmark::Event::InlineHtml(html)
                    if matches!(html.as_ref(), "<br>" | "<br/>" | "<br />") =>
                {
                    line += measured_run(ui, &mut job);
                    widest = widest.max(line);
                    line = 0.0;
                    continue;
                }
                pulldown_cmark::Event::Text(text)
                | pulldown_cmark::Event::Code(text)
                | pulldown_cmark::Event::InlineMath(text) => text.as_ref(),
                pulldown_cmark::Event::SoftBreak => " ",
                _ => continue,
            };
            style.code = matches!(
                event,
                pulldown_cmark::Event::Code(_) | pulldown_cmark::Event::InlineMath(_)
            );
            rich_text(&style, ui, value, None).append_to(
                &mut job,
                ui.style(),
                egui::FontSelection::Default,
                egui::Align::Center,
            );
            for value in value.split_whitespace() {
                let galley = egui::WidgetText::from(rich_text(&style, ui, value, None))
                    .into_galley(
                        ui,
                        Some(egui::TextWrapMode::Extend),
                        f32::INFINITY,
                        egui::FontSelection::Default,
                    );
                word = word.max(galley.size().x);
            }
            style.code = false;
        }
        line += measured_run(ui, &mut job);
        (widest.max(line), word)
    }

    fn flush_inline(&mut self, ui: &mut Ui, cache: &mut CommonMarkCache) {
        let Some(slot) = &mut self.flow else {
            return;
        };
        if slot.job.text.is_empty() {
            return;
        }
        let mut flow = std::mem::take(slot);
        slot.alignment = flow.alignment;
        if let (Some(start), Some(link)) = (flow.link_start.take(), self.link.as_ref()) {
            flow.links
                .push((start..flow.job.text.len(), link.destination.clone()));
            slot.link_start = Some(0);
        }
        let width = ui.available_width();
        flow.job.wrap.max_width = width;
        flow.job.first_row_min_height = ui.cursor().height();
        flow.job.halign = flow.alignment.unwrap_or(egui::Align::LEFT);
        flow.job.justify = false;
        if let Some(first) = flow.job.sections.first_mut() {
            first.leading_space = width - ui.available_size_before_wrap().x;
        }
        let mut pos = egui::pos2(ui.max_rect().left(), ui.cursor().top());
        let mut galley = ui.fonts(|fonts| fonts.layout_job(flow.job.clone()));
        let height = self.paragraph_height.unwrap_or_else(|| {
            let body = TextStyle::Body.resolve(ui.style()).size;
            self.text_style.heading.map_or(body * 1.6, |level| {
                body * HEADING_SCALE[usize::from(level).min(5)] * 1.25
            })
        });
        let padded = pad_inline_rows(&mut galley, height);
        // The native Label owns selection and copying. Text, emphasis, code,
        // links, and their punctuation share one Unicode wrapping decision.
        if let Some(align) = flow.alignment {
            let response = ui
                .with_layout(egui::Layout::top_down(align), |ui| {
                    ui.add(egui::Label::new(galley.clone()))
                })
                .inner;
            pos = match align {
                egui::Align::Min => response.rect.left_top(),
                egui::Align::Center => response.rect.center_top(),
                egui::Align::Max => response.rect.right_top(),
            };
        } else if padded {
            // Match Label's wrapped-row allocation, then let a native Label
            // paint/select the adjusted galley at exactly the same position.
            for row in &galley.rows {
                ui.allocate_rect(row.rect.translate(pos.to_vec2()), egui::Sense::hover());
            }
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(("inline-flow", self.flow_serial))
                    .max_rect(galley.rect.translate(pos.to_vec2()))
                    .layout(egui::Layout::top_down(egui::Align::LEFT)),
            );
            child.add(egui::Label::new(galley.clone()));
        } else {
            ui.add(egui::Label::new(flow.job));
        }
        let serial = self.flow_serial;
        self.flow_serial += 1;
        for (link, (range, target)) in flow.links.into_iter().enumerate() {
            let start = galley.text()[..range.start].chars().count();
            let label = &galley.text()[range];
            let end = start + label.chars().count();
            let mut character = 0;
            for (row_index, row) in galley.rows.iter().enumerate() {
                let mut rect = egui::Rect::NOTHING;
                let mut baseline = 0.0_f32;
                let mut size = 0.0_f32;
                for glyph in &row.glyphs {
                    if (start..end).contains(&character) {
                        rect = rect.union(glyph.logical_rect().translate(pos.to_vec2()));
                        baseline = baseline.max(pos.y + glyph.pos.y);
                        size = size.max(
                            galley.job.sections[glyph.section_index as usize]
                                .format
                                .font_id
                                .size,
                        );
                    }
                    character += 1;
                }
                if row.ends_with_newline {
                    character += 1;
                }
                if !rect.is_positive() {
                    continue;
                }
                let response = ui
                    .interact(
                        rect,
                        ui.id().with(("reader-link", serial, link, row_index)),
                        egui::Sense::click(),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(&target);
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Link, ui.is_enabled(), label)
                });
                if response.hovered() || response.has_focus() {
                    let y = baseline + (size * 0.1).max(1.0);
                    ui.painter().line_segment(
                        [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                        egui::Stroke::new(1.0_f32, ui.visuals().hyperlink_color),
                    );
                }
                if response.has_focus() {
                    ui.painter().rect_stroke(
                        rect.expand(1.0),
                        1,
                        egui::Stroke::new(1.0_f32, ui.visuals().hyperlink_color),
                        egui::StrokeKind::Outside,
                    );
                }
                response.context_menu(|ui| {
                    if ui.button("Copy link address").clicked() {
                        ui.ctx().copy_text(target.clone());
                        ui.close_menu();
                    }
                });
                let keyboard =
                    response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if response.clicked() || response.middle_clicked() || keyboard {
                    if cache.link_hooks().contains_key(&target) {
                        cache.link_hooks_mut().insert(target.clone(), true);
                    } else {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(&target));
                    }
                }
            }
        }
    }

    fn inline_measure(
        &self,
        ui: &mut Ui,
        events: &[(pulldown_cmark::Event<'_>, Range<usize>)],
    ) -> Option<(f32, f32)> {
        let body = TextStyle::Body.resolve(ui.style()).size;
        let nominal = self.text_style.heading.map_or(body * 1.6, |level| {
            body * HEADING_SCALE[usize::from(level).min(5)] * 1.25
        });
        self.measure_math_fn.map(|measure| {
            with_math_font(ui, self.text_style.heading, |ui| {
                events
                    .iter()
                    .filter_map(|(event, _)| match event {
                        pulldown_cmark::Event::InlineMath(source) => Some({
                            let metrics = measure(ui, source);
                            (metrics.height, metrics.above)
                        }),
                        _ => None,
                    })
                    .fold((nominal, 0.0_f32), |a, b| (a.0.max(b.0), a.1.max(b.1)))
            })
        })
    }

    fn reserve_inline(ui: &mut Ui, width: f32, measurement: Option<(f32, f32)>) {
        if let Some((height, ascent)) = measurement {
            if ascent > 0.0 {
                newline(ui);
                ui.set_row_height(ascent);
                ui.allocate_exact_size(egui::vec2(width, ascent), egui::Sense::hover());
                ui.end_row();
            }
            ui.set_row_height(height);
        }
    }

    fn event(
        &mut self,
        ui: &mut Ui,
        event: pulldown_cmark::Event,
        _src_span: Range<usize>,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        max_width: f32,
    ) {
        match event {
            pulldown_cmark::Event::Start(tag) => self.start_tag(ui, tag, cache, options),
            pulldown_cmark::Event::End(tag) => self.end_tag(ui, tag, cache, options, max_width),
            pulldown_cmark::Event::Text(text) => {
                if !self.html_comment {
                    self.event_text(text, ui);
                }
            }
            pulldown_cmark::Event::Code(text) => {
                self.text_style.code = true;
                self.event_text(text, ui);
                self.text_style.code = false;
            }
            pulldown_cmark::Event::InlineHtml(text) => {
                let text = visible_html(&text, &mut self.html_comment);
                if text.is_empty() {
                    return;
                }
                match text.trim().to_ascii_lowercase().as_str() {
                    "<br>" | "<br/>" | "<br />" => self.event_text("\n".into(), ui),
                    _ => self.event_text(text.into(), ui),
                }
            }
            pulldown_cmark::Event::Html(text) => {
                let text = visible_html(&text, &mut self.html_comment);
                if text.is_empty() {
                    return;
                }
                if let Some(html) = options.html_fn {
                    html(ui, &text);
                } else {
                    self.event_text(text.into(), ui);
                }
            }
            pulldown_cmark::Event::FootnoteReference(footnote) => {
                self.flush_inline(ui, cache);
                footnote_start(ui, &footnote);
            }
            pulldown_cmark::Event::SoftBreak => {
                if self.flow.is_some() {
                    self.event_text(" ".into(), ui);
                } else {
                    ui.label(" ");
                }
            }
            pulldown_cmark::Event::HardBreak => {
                if self.flow.is_some() {
                    self.event_text("\n".into(), ui);
                } else {
                    ui.end_row();
                    newline(ui);
                }
            }
            pulldown_cmark::Event::Rule => {
                newline(ui);
            }
            // The document list owns the marker gutter, including task boxes.
            pulldown_cmark::Event::TaskListMarker(_) => {}
            pulldown_cmark::Event::InlineMath(tex) => {
                self.flush_inline(ui, cache);
                if let Some(math_fn) = options.math_fn {
                    with_math_font(ui, self.text_style.heading, |ui| math_fn(ui, &tex, true));
                }
            }
            pulldown_cmark::Event::DisplayMath(tex) => {
                self.flush_inline(ui, cache);
                if let Some(math_fn) = options.math_fn {
                    newline(ui);
                    math_fn(ui, &tex, false);
                    newline(ui);
                }
            }
        }
    }

    fn event_text(&mut self, text: CowStr, ui: &mut Ui) {
        let mut rich_text = rich_text(&self.text_style, ui, &text, self.paragraph_height);
        if let Some(image) = &mut self.image {
            image.alt_text.push(rich_text);
        } else if let Some(flow) = &mut self.flow {
            if self.link.is_some() {
                rich_text = rich_text.color(ui.visuals().hyperlink_color);
            }
            rich_text.append_to(
                &mut flow.job,
                ui.style(),
                egui::FontSelection::Default,
                ui.text_valign(),
            );
        } else if let Some(link) = &mut self.link {
            link.text.push(rich_text);
        } else {
            ui.label(rich_text);
        }
    }

    fn start_tag(
        &mut self,
        ui: &mut Ui,
        tag: pulldown_cmark::Tag,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
    ) {
        match tag {
            tag @ (pulldown_cmark::Tag::Strong
            | pulldown_cmark::Tag::Emphasis
            | pulldown_cmark::Tag::Strikethrough) => {
                self.inline_styles.push(self.text_style.clone());
                match tag {
                    pulldown_cmark::Tag::Strong => self.text_style.strong = true,
                    pulldown_cmark::Tag::Emphasis => self.text_style.emphasis = true,
                    _ => self.text_style.strikethrough = true,
                }
            }
            pulldown_cmark::Tag::Link { dest_url, .. } => {
                if let Some(flow) = &mut self.flow {
                    flow.link_start = Some(flow.job.text.len());
                }
                self.link = Some(Link {
                    destination: dest_url.to_string(),
                    text: Vec::new(),
                });
            }
            pulldown_cmark::Tag::Image { dest_url, .. } => {
                self.flush_inline(ui, cache);
                self.image = Some(Image::new(&dest_url, options));
            }
            _ => {}
        }
    }

    fn end_tag(
        &mut self,
        ui: &mut Ui,
        tag: pulldown_cmark::TagEnd,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        max_width: f32,
    ) {
        match tag {
            pulldown_cmark::TagEnd::Strong
            | pulldown_cmark::TagEnd::Emphasis
            | pulldown_cmark::TagEnd::Strikethrough => {
                if let Some(style) = self.inline_styles.pop() {
                    self.text_style = style;
                }
            }
            pulldown_cmark::TagEnd::Link => {
                if let Some(link) = self.link.take() {
                    if let Some(flow) = &mut self.flow {
                        if let Some(start) = flow.link_start.take() {
                            flow.links
                                .push((start..flow.job.text.len(), link.destination));
                        }
                    }
                }
            }
            pulldown_cmark::TagEnd::Image => {
                if let Some(image) = self.image.take() {
                    let mut response = ui.add(
                        egui::Image::from_uri(&image.uri)
                            .fit_to_original_size(1.0)
                            .max_width(max_width.min(ui.available_width())),
                    );
                    if let Some(link) = &self.link {
                        response = response
                            .interact(egui::Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if response.clicked() {
                            activate(ui, cache, &link.destination);
                        }
                    }
                    if options.show_alt_text_on_hover && !image.alt_text.is_empty() {
                        response.on_hover_ui_at_pointer(|ui| {
                            for alt in image.alt_text {
                                ui.label(alt);
                            }
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

fn activate(ui: &Ui, cache: &mut CommonMarkCache, target: &str) {
    if cache.link_hooks().contains_key(target) {
        cache.link_hooks_mut().insert(target.to_owned(), true);
    } else {
        ui.ctx().open_url(egui::OpenUrl::new_tab(target));
    }
}

/// Comments are zero-width content, including comments split across events.
pub(super) fn visible_html(text: &str, comment: &mut bool) -> String {
    let mut remaining = text;
    let mut visible = String::new();
    while !remaining.is_empty() {
        if *comment {
            let Some(end) = remaining.find("-->") else {
                break;
            };
            remaining = &remaining[end + 3..];
            *comment = false;
        } else if let Some(start) = remaining.find("<!--") {
            visible.push_str(&remaining[..start]);
            remaining = &remaining[start + 4..];
            *comment = true;
        } else {
            visible.push_str(remaining);
            break;
        }
    }
    visible
}

fn measured_run(ui: &Ui, job: &mut egui::text::LayoutJob) -> f32 {
    if job.text.is_empty() {
        return 0.0;
    }
    ui.fonts(|fonts| fonts.layout_job(std::mem::take(job)))
        .size()
        .x
}
