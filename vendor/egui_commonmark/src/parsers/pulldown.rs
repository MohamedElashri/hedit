use std::iter::Peekable;
use std::ops::Range;

use crate::{CommonMarkCache, CommonMarkOptions};

#[cfg(feature = "pulldown_cmark")]
use egui::Pos2;
use egui::{self, Id, TextStyle, Ui};

use crate::List;
use egui_commonmark_backend::elements::*;
use egui_commonmark_backend::misc::*;
use egui_commonmark_backend::pulldown::*;
use pulldown_cmark::{CowStr, HeadingLevel};

/// Finish the current wrapped row once, instead of inserting a two-line label.
/// Reset its height so a large heading, table, or quote cannot enlarge later rows.
pub(crate) fn newline(ui: &mut Ui) {
    if ui.next_widget_position().x > ui.max_rect().left() + 0.5 {
        ui.end_row();
    }
    ui.set_row_height(egui::TextStyle::Body.resolve(ui.style()).size * 1.6);
}

fn rich_text(style: &Style, ui: &Ui, text: &str) -> egui::RichText {
    let mut inline = style.clone();
    inline.heading = None;
    inline.quote = false;
    let mut text = inline.to_richtext(ui, text);
    let body = egui::TextStyle::Body.resolve(ui.style()).size;
    if (style.strong || style.heading.is_some()) && !style.code {
        if let Some(font) = ui
            .style()
            .text_styles
            .get(&TextStyle::Name("MarkdownStrong".into()))
        {
            text = text.family(font.family.clone());
        }
    }
    let size = style.heading.map_or(body, |level| {
        body * [2.0, 1.55, 1.28, 1.12, 1.0, 1.0][usize::from(level).min(5)]
    });
    if style.heading.is_some() {
        text = text.size(size).strong();
    }
    text.line_height(Some(size * if style.heading.is_some() { 1.3 } else { 1.6 }))
}

fn with_math_font<R>(ui: &mut Ui, heading: Option<u8>, contents: impl FnOnce(&mut Ui) -> R) -> R {
    if let Some(level) = heading {
        let mut font = TextStyle::Body.resolve(ui.style());
        font.size *= [2.0, 1.55, 1.28, 1.12, 1.0, 1.0][usize::from(level).min(5)];
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

fn quote(ui: &mut Ui, contents: impl FnOnce(&mut Ui)) {
    let mark = ui.painter().add(egui::Shape::Noop);
    let color = ui.visuals().hyperlink_color;
    let rect = egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(3)
        .inner_margin(egui::Margin {
            left: 14,
            right: 12,
            top: 8,
            bottom: 8,
        })
        .show(ui, contents)
        .response
        .rect;
    ui.painter().set(
        mark,
        egui::Shape::line_segment(
            [rect.left_top(), rect.left_bottom()],
            egui::Stroke::new(2.0_f32, color),
        ),
    );
}

/// Newline logic is constructed by the following:
/// All elements try to insert a newline before them (if they are allowed)
/// and end their own line.
struct Newline {
    /// Whether an element should insert a newline before it
    should_start_newline: bool,
    /// Whether an element should end it's own line using a newline
    /// This will have to be set to false in cases such as when blocks are within
    /// a list.
    should_end_newline: bool,
    /// only false when the widget is the last one.
    should_end_newline_forced: bool,
}

impl Default for Newline {
    fn default() -> Self {
        Self {
            // Default as false as the first line should not have a newline above it
            should_start_newline: false,
            should_end_newline: true,
            should_end_newline_forced: true,
        }
    }
}

impl Newline {
    pub fn can_insert_end(&self) -> bool {
        self.should_end_newline && self.should_end_newline_forced
    }

    pub fn can_insert_start(&self) -> bool {
        self.should_start_newline
    }

    pub fn try_insert_start(&self, ui: &mut Ui) {
        if self.should_start_newline {
            newline(ui);
        }
    }

    pub fn try_insert_end(&self, ui: &mut Ui) {
        if self.can_insert_end() {
            newline(ui);
        }
    }
}

#[derive(Default)]
struct DefinitionList {
    is_first_item: bool,
    is_def_list_def: bool,
}

#[derive(Default)]
struct InlineFlow {
    job: egui::text::LayoutJob,
    links: Vec<(Range<usize>, String)>,
    link_start: Option<usize>,
}

pub struct CommonMarkViewerInternal<'f> {
    code_block_fn: Option<&'f crate::RenderCodeBlockFn>,
    block_fn: Option<&'f crate::RenderBlockFn>,
    measure_math_fn: Option<&'f crate::MeasureMathFn>,
    inline_end: Option<pulldown_cmark::TagEnd>,
    paragraph_height: Option<f32>,
    flow: Option<InlineFlow>,
    flow_serial: usize,
    curr_table: usize,
    text_style: Style,
    list: List,
    link: Option<Link>,
    image: Option<Image>,
    line: Newline,
    code_block: Option<CodeBlock>,

    /// Only populated if the html_fn option has been set
    html_block: String,
    html_comment: bool,
    is_list_item: bool,
    def_list: DefinitionList,
    is_table: bool,
    is_blockquote: bool,
    checkbox_events: Vec<CheckboxClickEvent>,
}

pub(crate) struct CheckboxClickEvent {
    pub(crate) checked: bool,
    pub(crate) span: Range<usize>,
}

impl<'f> CommonMarkViewerInternal<'f> {
    pub fn new(
        code_block_fn: Option<&'f crate::RenderCodeBlockFn>,
        block_fn: Option<&'f crate::RenderBlockFn>,
        measure_math_fn: Option<&'f crate::MeasureMathFn>,
    ) -> Self {
        Self {
            code_block_fn,
            block_fn,
            measure_math_fn,
            inline_end: None,
            paragraph_height: None,
            flow: None,
            flow_serial: 0,
            curr_table: 0,
            text_style: Style::default(),
            list: List::default(),
            link: None,
            image: None,
            line: Newline::default(),
            is_list_item: false,
            def_list: Default::default(),
            code_block: None,
            html_block: String::new(),
            html_comment: false,
            is_table: false,
            is_blockquote: false,
            checkbox_events: Vec::new(),
        }
    }
}

pub(crate) fn parser_options_math(is_math_enabled: bool) -> pulldown_cmark::Options {
    if is_math_enabled {
        parser_options() | pulldown_cmark::Options::ENABLE_MATH
    } else {
        parser_options()
    }
}

impl<'f> CommonMarkViewerInternal<'f> {
    /// Be aware that this acquires egui::Context internally.
    /// If split Id is provided then split points will be populated
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        text: &str,
        split_points_id: Option<Id>,
    ) -> (egui::InnerResponse<()>, Vec<CheckboxClickEvent>) {
        self.show_events(
            ui,
            cache,
            options,
            pulldown_cmark::Parser::new_ext(text, parser_options_math(options.math_fn.is_some()))
                .into_offset_iter(),
            split_points_id,
        )
    }

    pub(crate) fn show_events<'e>(
        &mut self,
        ui: &mut egui::Ui,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        events: impl Iterator<Item = (pulldown_cmark::Event<'e>, Range<usize>)>,
        split_points_id: Option<Id>,
    ) -> (egui::InnerResponse<()>, Vec<CheckboxClickEvent>) {
        let max_width = options.max_width(ui);
        let layout = egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true);

        let re = ui.allocate_ui_with_layout(egui::vec2(max_width, 0.0), layout, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let height = TextStyle::Body.resolve(ui.style()).size * 1.6;
            ui.set_row_height(height);

            let mut events = events.enumerate().peekable();

            while let Some((index, (e, src_span))) = events.next() {
                let start_position = ui.next_widget_position();
                let is_element_end = matches!(e, pulldown_cmark::Event::End(_));
                let should_add_split_point = self.list.is_inside_a_list() && is_element_end;

                if events.peek().is_none() {
                    self.line.should_end_newline_forced = false;
                }

                self.process_event(ui, &mut events, e, src_span, cache, options, max_width);

                if let Some(source_id) = split_points_id {
                    if should_add_split_point {
                        let scroll_cache = scroll_cache(cache, &source_id);
                        let end_position = ui.next_widget_position();

                        let split_point_exists = scroll_cache
                            .split_points
                            .iter()
                            .any(|(i, _, _)| *i == index);

                        if !split_point_exists {
                            scroll_cache
                                .split_points
                                .push((index, start_position, end_position));
                        }
                    }
                }

                if index == 0 {
                    self.line.should_start_newline = true;
                }
            }

            if let Some(source_id) = split_points_id {
                scroll_cache(cache, &source_id).page_size =
                    Some(ui.next_widget_position().to_vec2());
            }
        });

        (re, std::mem::take(&mut self.checkbox_events))
    }

    #[cfg(feature = "pulldown_cmark")]
    pub(crate) fn show_scrollable(
        &mut self,
        source_id: Id,
        ui: &mut egui::Ui,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        text: &str,
    ) {
        let available_size = ui.available_size();
        let scroll_id = source_id.with("_scroll_area");

        let Some(page_size) = scroll_cache(cache, &source_id).page_size else {
            egui::ScrollArea::vertical()
                .id_salt(scroll_id)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    self.show(ui, cache, options, text, Some(source_id));
                });
            // Prevent repopulating points twice at startup
            scroll_cache(cache, &source_id).available_size = available_size;
            return;
        };

        let events =
            pulldown_cmark::Parser::new_ext(text, parser_options_math(options.math_fn.is_some()))
                .into_offset_iter()
                .collect::<Vec<_>>();

        let num_rows = events.len();

        egui::ScrollArea::vertical()
            .id_salt(scroll_id)
            // Elements have different widths, so the scroll area cannot try to shrink to the
            // content, as that will mean that the scroll bar will move when loading elements
            // with different widths.
            .auto_shrink([false, true])
            .show_viewport(ui, |ui, viewport| {
                ui.set_height(page_size.y);
                let layout = egui::Layout::left_to_right(egui::Align::BOTTOM).with_main_wrap(true);

                let max_width = options.max_width(ui);
                ui.allocate_ui_with_layout(egui::vec2(max_width, 0.0), layout, |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let scroll_cache = scroll_cache(cache, &source_id);

                    // finding the first element that's not in the viewport anymore
                    let (first_event_index, _, first_end_position) = scroll_cache
                        .split_points
                        .iter()
                        .filter(|(_, _, end_position)| end_position.y < viewport.min.y)
                        .nth_back(1)
                        .copied()
                        .unwrap_or((0, Pos2::ZERO, Pos2::ZERO));

                    // finding the last element that's just outside the viewport
                    let last_event_index = scroll_cache
                        .split_points
                        .iter()
                        .filter(|(_, start_position, _)| start_position.y > viewport.max.y)
                        .nth(1)
                        .map(|(index, _, _)| *index)
                        .unwrap_or(num_rows);

                    ui.allocate_space(first_end_position.to_vec2());

                    // only rendering the elements that are inside the viewport
                    let mut events = events
                        .into_iter()
                        .enumerate()
                        .skip(first_event_index)
                        .take(last_event_index - first_event_index)
                        .peekable();

                    while let Some((i, (e, src_span))) = events.next() {
                        if events.peek().is_none() {
                            self.line.should_end_newline_forced = false;
                        }

                        self.process_event(ui, &mut events, e, src_span, cache, options, max_width);

                        if i == 0 {
                            self.line.should_start_newline = true;
                        }
                    }
                });
            });

        // Forcing full re-render to repopulate split points for the new size
        let scroll_cache = scroll_cache(cache, &source_id);
        if available_size != scroll_cache.available_size {
            scroll_cache.available_size = available_size;
            scroll_cache.page_size = None;
            scroll_cache.split_points.clear();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn process_event<'e>(
        &mut self,
        ui: &mut Ui,
        events: &mut Peekable<impl Iterator<Item = EventIteratorItem<'e>>>,
        event: pulldown_cmark::Event,
        src_span: Range<usize>,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        max_width: f32,
    ) {
        let block = match &event {
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::Heading { level, .. }) => {
                Some(Some(*level as u8))
            }
            pulldown_cmark::Event::Start(
                pulldown_cmark::Tag::Paragraph
                | pulldown_cmark::Tag::CodeBlock(_)
                | pulldown_cmark::Tag::BlockQuote(_)
                | pulldown_cmark::Tag::List(_)
                | pulldown_cmark::Tag::Table(_)
                | pulldown_cmark::Tag::FootnoteDefinition(_),
            )
            | pulldown_cmark::Event::Rule => Some(None),
            _ => None,
        };
        if let Some(heading) = block {
            self.flush_inline(ui, cache);
            // A source position is attached to the actual block, without
            // inserting HTML or extra paragraphs into the source document.
            if !self.list.is_inside_a_list() || heading.is_some() {
                newline(ui);
            }
            if let Some(block_fn) = self.block_fn {
                block_fn(ui, src_span.clone(), heading);
            }
        }
        self.event(ui, event, src_span, cache, options, max_width);

        if let Some(end) = self.inline_end.take() {
            let paragraph = delayed_events(events, |tag| tag == end);
            let previous = self.paragraph_height;
            let previous_flow = self.flow.replace(InlineFlow::default());
            let measurement = self.inline_measure(ui, &paragraph);
            self.paragraph_height = measurement.map(|m| m.0);
            Self::reserve_inline(ui, max_width, measurement);
            let mut paragraph = paragraph.into_iter().enumerate().peekable();
            while let Some((_, (event, span))) = paragraph.next() {
                self.process_event(ui, &mut paragraph, event, span, cache, options, max_width);
            }
            self.flush_inline(ui, cache);
            self.flow = previous_flow;
            self.paragraph_height = previous;
        }
        self.def_list_def_wrapping(events, max_width, cache, options, ui);
        self.item_list_wrapping(events, max_width, cache, options, ui);
        self.table(events, cache, options, ui, max_width);
        self.blockquote(events, max_width, cache, options, ui);
    }

    fn flush_inline(&mut self, ui: &mut Ui, cache: &mut CommonMarkCache) {
        let Some(slot) = &mut self.flow else {
            return;
        };
        if slot.job.text.is_empty() {
            return;
        }
        let mut flow = std::mem::take(slot);
        if let (Some(start), Some(link)) = (flow.link_start.take(), self.link.as_ref()) {
            flow.links
                .push((start..flow.job.text.len(), link.destination.clone()));
            slot.link_start = Some(0);
        }
        let width = ui.available_width();
        flow.job.wrap.max_width = width;
        flow.job.first_row_min_height = ui.cursor().height();
        flow.job.halign = egui::Align::LEFT;
        flow.job.justify = false;
        if let Some(first) = flow.job.sections.first_mut() {
            first.leading_space = width - ui.available_size_before_wrap().x;
        }
        let pos = egui::pos2(ui.max_rect().left(), ui.cursor().top());
        let galley = ui.fonts(|fonts| fonts.layout_job(flow.job.clone()));
        // The native Label owns selection and copying. Text, emphasis, code,
        // links, and their punctuation share one Unicode wrapping decision.
        ui.add(egui::Label::new(flow.job));
        let serial = self.flow_serial;
        self.flow_serial += 1;
        for (link, (range, target)) in flow.links.into_iter().enumerate() {
            let start = galley.text()[..range.start].chars().count();
            let label = &galley.text()[range];
            let end = start + label.chars().count();
            let mut character = 0;
            for (row_index, row) in galley.rows.iter().enumerate() {
                let mut rect = egui::Rect::NOTHING;
                for glyph in &row.glyphs {
                    if (start..end).contains(&character) {
                        rect = rect.union(glyph.logical_rect().translate(pos.to_vec2()));
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
        self.measure_math_fn.map(|measure| {
            with_math_font(ui, self.text_style.heading, |ui| {
                events
                    .iter()
                    .filter_map(|(event, _)| match event {
                        pulldown_cmark::Event::InlineMath(source) => Some(measure(ui, source)),
                        _ => None,
                    })
                    .fold(
                        (TextStyle::Body.resolve(ui.style()).size * 1.6, 0.0_f32),
                        |a, b| (a.0.max(b.0), a.1.max(b.1)),
                    )
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

    fn def_list_def_wrapping<'e>(
        &mut self,
        events: &mut Peekable<impl Iterator<Item = EventIteratorItem<'e>>>,
        max_width: f32,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        ui: &mut Ui,
    ) {
        if self.def_list.is_def_list_def {
            self.def_list.is_def_list_def = false;

            let item_events = delayed_events(events, |tag| {
                matches!(tag, pulldown_cmark::TagEnd::DefinitionListDefinition)
            });

            let mut events_iter = item_events.into_iter().enumerate().peekable();

            self.line.try_insert_start(ui);

            // Proccess a single event separately so that we do not insert spaces where we do not
            // want them
            self.line.should_start_newline = false;
            if let Some((_, (e, src_span))) = events_iter.next() {
                self.process_event(ui, &mut events_iter, e, src_span, cache, options, max_width);
            }

            ui.label(" ".repeat(options.indentation_spaces));
            self.line.should_start_newline = true;
            self.line.should_end_newline = false;
            // Required to ensure that the content is aligned with the identation
            ui.horizontal_wrapped(|ui| {
                while let Some((_, (e, src_span))) = events_iter.next() {
                    self.process_event(
                        ui,
                        &mut events_iter,
                        e,
                        src_span,
                        cache,
                        options,
                        max_width,
                    );
                }
            });
            self.line.should_end_newline = true;

            // Only end the definition items line if it is not the last element in the list
            if !matches!(
                events.peek(),
                Some((
                    _,
                    (
                        pulldown_cmark::Event::End(pulldown_cmark::TagEnd::DefinitionList),
                        _
                    )
                ))
            ) {
                self.line.try_insert_end(ui);
            }
        }
    }

    fn item_list_wrapping<'e>(
        &mut self,
        events: &mut impl Iterator<Item = EventIteratorItem<'e>>,
        max_width: f32,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        ui: &mut Ui,
    ) {
        if self.is_list_item {
            self.is_list_item = false;

            let item_events = delayed_events_list_item(events);
            let previous = self.paragraph_height;
            let previous_flow = self.flow.replace(InlineFlow::default());
            let measurement = self.inline_measure(ui, &item_events);
            self.paragraph_height = measurement.map(|m| m.0);
            let mut events_iter = item_events.into_iter().enumerate().peekable();

            // Required to ensure that the content of the list item is aligned with
            // the * or - when wrapping
            ui.horizontal_wrapped(|ui| {
                Self::reserve_inline(ui, ui.available_width(), measurement);
                while let Some((_, (e, src_span))) = events_iter.next() {
                    self.process_event(
                        ui,
                        &mut events_iter,
                        e,
                        src_span,
                        cache,
                        options,
                        max_width,
                    );
                }
                self.flush_inline(ui, cache);
            });
            self.flow = previous_flow;
            self.paragraph_height = previous;
        }
    }

    fn blockquote<'e>(
        &mut self,
        events: &mut Peekable<impl Iterator<Item = EventIteratorItem<'e>>>,
        max_width: f32,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        ui: &mut Ui,
    ) {
        if self.is_blockquote {
            // Claim this quote before recursively processing its children.
            self.is_blockquote = false;
            let mut collected_events = Vec::new();
            let mut depth = 1;
            for (_, (event, span)) in events.by_ref() {
                match &event {
                    pulldown_cmark::Event::Start(pulldown_cmark::Tag::BlockQuote(_)) => depth += 1,
                    pulldown_cmark::Event::End(pulldown_cmark::TagEnd::BlockQuote(_)) => depth -= 1,
                    _ => {}
                }
                collected_events.push((event, span));
                if depth == 0 {
                    break;
                }
            }
            self.line.try_insert_start(ui);

            // Currently the blockquotes are made in such a way that they need a newline at the end
            // and the start so when this is the first element in the markdown the newline must be
            // manually enabled
            self.line.should_start_newline = true;
            if let Some(alert) = parse_alerts(&options.alerts, &mut collected_events) {
                egui_commonmark_backend::alert_ui(alert, ui, |ui| {
                    let mut events = collected_events.into_iter().enumerate().peekable();
                    while let Some((_, (event, src_span))) = events.next() {
                        self.process_event(
                            ui,
                            &mut events,
                            event,
                            src_span,
                            cache,
                            options,
                            max_width,
                        );
                    }
                })
            } else {
                quote(ui, |ui| {
                    let previous_quote = self.text_style.quote;
                    self.text_style.quote = true;
                    let mut events = collected_events.into_iter().enumerate().peekable();
                    while let Some((_, (event, src_span))) = events.next() {
                        self.process_event(
                            ui,
                            &mut events,
                            event,
                            src_span,
                            cache,
                            options,
                            max_width,
                        );
                    }
                    self.text_style.quote = previous_quote;
                });
            }

            if events.peek().is_none() {
                self.line.should_end_newline_forced = false;
            }

            self.line.try_insert_end(ui);
            self.is_blockquote = false;
        }
    }

    fn table<'e>(
        &mut self,
        events: &mut Peekable<impl Iterator<Item = EventIteratorItem<'e>>>,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        ui: &mut Ui,
        max_width: f32,
    ) {
        if self.is_table {
            self.line.try_insert_start(ui);

            let id = ui.id().with("_table").with(self.curr_table);
            self.curr_table += 1;

            let width = ui.available_width().min(max_width).max(1.0);
            egui::ScrollArea::horizontal()
                .id_salt((id, "overflow"))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        let Table { header, rows } = parse_table(events);

                        let columns = header.len().max(1);
                        let cell_width = ((width - 24.0 - 16.0 * (columns - 1) as f32)
                            / columns as f32)
                            .max(80.0);
                        egui::Grid::new(id)
                            .striped(true)
                            .spacing(egui::vec2(16.0, 10.0))
                            .max_col_width(cell_width)
                            .show(ui, |ui| {
                                for col in header {
                                    let previous_height = self.paragraph_height;
                                    let previous_flow = self.flow.replace(InlineFlow::default());
                                    let measurement = self.inline_measure(ui, &col);
                                    self.paragraph_height = measurement.map(|m| m.0);
                                    ui.horizontal_wrapped(|ui| {
                                        ui.set_width(cell_width);
                                        Self::reserve_inline(ui, cell_width, measurement);
                                        let strong = self.text_style.strong;
                                        self.text_style.strong = true;
                                        for (e, src_span) in col {
                                            let tmp_start = std::mem::replace(
                                                &mut self.line.should_start_newline,
                                                false,
                                            );
                                            let tmp_end = std::mem::replace(
                                                &mut self.line.should_end_newline,
                                                false,
                                            );
                                            self.event(ui, e, src_span, cache, options, max_width);
                                            self.line.should_start_newline = tmp_start;
                                            self.line.should_end_newline = tmp_end;
                                        }
                                        self.flush_inline(ui, cache);
                                        self.text_style.strong = strong;
                                    });
                                    self.flow = previous_flow;
                                    self.paragraph_height = previous_height;
                                }

                                ui.end_row();

                                for row in rows {
                                    for col in row {
                                        let previous_height = self.paragraph_height;
                                        let previous_flow =
                                            self.flow.replace(InlineFlow::default());
                                        let measurement = self.inline_measure(ui, &col);
                                        self.paragraph_height = measurement.map(|m| m.0);
                                        ui.horizontal_wrapped(|ui| {
                                            ui.set_width(cell_width);
                                            Self::reserve_inline(ui, cell_width, measurement);
                                            for (e, src_span) in col {
                                                let tmp_start = std::mem::replace(
                                                    &mut self.line.should_start_newline,
                                                    false,
                                                );
                                                let tmp_end = std::mem::replace(
                                                    &mut self.line.should_end_newline,
                                                    false,
                                                );
                                                self.event(
                                                    ui, e, src_span, cache, options, max_width,
                                                );
                                                self.line.should_start_newline = tmp_start;
                                                self.line.should_end_newline = tmp_end;
                                            }
                                            self.flush_inline(ui, cache);
                                        });
                                        self.flow = previous_flow;
                                        self.paragraph_height = previous_height;
                                    }

                                    ui.end_row();
                                }
                            });
                    });
                });

            self.is_table = false;
            if events.peek().is_none() {
                self.line.should_end_newline_forced = false;
            }

            self.line.try_insert_end(ui);
        }
    }

    fn event(
        &mut self,
        ui: &mut Ui,
        event: pulldown_cmark::Event,
        src_span: Range<usize>,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        max_width: f32,
    ) {
        match event {
            pulldown_cmark::Event::Start(tag) => {
                if matches!(&tag, pulldown_cmark::Tag::Image { .. }) {
                    self.flush_inline(ui, cache);
                }
                self.start_tag(ui, tag, options);
            }
            pulldown_cmark::Event::End(tag) => self.end_tag(ui, tag, cache, options, max_width),
            pulldown_cmark::Event::Text(text) => {
                self.event_text(text, ui);
            }
            pulldown_cmark::Event::Code(text) => {
                self.text_style.code = true;
                self.event_text(text, ui);
                self.text_style.code = false;
            }
            pulldown_cmark::Event::InlineHtml(text) => {
                if self.html_comment || text.trim_start().starts_with("<!--") {
                    self.html_comment = !text.contains("-->");
                    return;
                }
                match text.trim().to_ascii_lowercase().as_str() {
                    "<br>" | "<br/>" | "<br />" => {
                        if self.flow.is_some() {
                            self.event_text("\n".into(), ui);
                        } else {
                            ui.end_row();
                            newline(ui);
                        }
                    }
                    _ if text.trim().starts_with("<!--") => {}
                    _ => self.event_text(text, ui),
                }
            }

            pulldown_cmark::Event::Html(text) => {
                if self.html_comment || text.trim_start().starts_with("<!--") {
                    self.html_comment = !text.contains("-->");
                    return;
                }
                if options.html_fn.is_some() {
                    self.html_block.push_str(&text);
                } else {
                    self.event_text(text, ui);
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
                    soft_break(ui);
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
                self.line.try_insert_start(ui);
                rule(ui, self.line.can_insert_end());
            }
            pulldown_cmark::Event::TaskListMarker(mut checkbox) => {
                self.flush_inline(ui, cache);
                if options.mutable {
                    if ui
                        .add(egui::Checkbox::without_text(&mut checkbox))
                        .clicked()
                    {
                        self.checkbox_events.push(CheckboxClickEvent {
                            checked: checkbox,
                            span: src_span,
                        });
                    }
                } else {
                    ui.add(ImmutableCheckbox::without_text(&mut checkbox));
                }
            }
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
        let mut rich_text = rich_text(&self.text_style, ui, &text);
        if let Some(height) = self.paragraph_height {
            rich_text = rich_text.line_height(Some(height));
        }
        if let Some(image) = &mut self.image {
            image.alt_text.push(rich_text);
        } else if let Some(block) = &mut self.code_block {
            block.content.push_str(&text);
        } else if let Some(flow) = &mut self.flow {
            if self.link.is_some() {
                rich_text = rich_text.color(ui.visuals().hyperlink_color).underline();
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

    fn start_tag(&mut self, ui: &mut Ui, tag: pulldown_cmark::Tag, options: &CommonMarkOptions) {
        match tag {
            pulldown_cmark::Tag::Paragraph => {
                self.line.try_insert_start(ui);
                self.inline_end = Some(pulldown_cmark::TagEnd::Paragraph);
            }
            pulldown_cmark::Tag::Heading { level, .. } => {
                self.inline_end = Some(pulldown_cmark::TagEnd::Heading(level));
                // Headings should always insert a newline even if it is at the start.
                // Whether this is okay in all scenarios is a different question.
                newline(ui);
                self.text_style.heading = Some(match level {
                    HeadingLevel::H1 => 0,
                    HeadingLevel::H2 => 1,
                    HeadingLevel::H3 => 2,
                    HeadingLevel::H4 => 3,
                    HeadingLevel::H5 => 4,
                    HeadingLevel::H6 => 5,
                });
            }

            // deliberately not using the built in alerts from pulldown-cmark as
            // the markdown itself cannot be localized :( e.g: [!TIP]
            pulldown_cmark::Tag::BlockQuote(_) => {
                self.is_blockquote = true;
            }
            pulldown_cmark::Tag::CodeBlock(c) => {
                match c {
                    pulldown_cmark::CodeBlockKind::Fenced(lang) => {
                        self.code_block = Some(crate::CodeBlock {
                            lang: Some(lang.to_string()),
                            content: "".to_string(),
                        });
                    }
                    pulldown_cmark::CodeBlockKind::Indented => {
                        self.code_block = Some(crate::CodeBlock {
                            lang: None,
                            content: "".to_string(),
                        });
                    }
                }
                self.line.try_insert_start(ui);
            }

            pulldown_cmark::Tag::List(point) => {
                if !self.list.is_inside_a_list() && self.line.can_insert_start() {
                    newline(ui);
                }

                if let Some(number) = point {
                    self.list.start_level_with_number(number);
                } else {
                    self.list.start_level_without_number();
                }
                self.line.should_start_newline = false;
                self.line.should_end_newline = false;
            }

            pulldown_cmark::Tag::Item => {
                self.is_list_item = true;
                self.list.start_item(ui, options);
            }

            pulldown_cmark::Tag::FootnoteDefinition(note) => {
                self.line.try_insert_start(ui);

                self.line.should_start_newline = false;
                self.line.should_end_newline = false;
                footnote(ui, &note);
            }
            pulldown_cmark::Tag::Table(_) => {
                self.is_table = true;
            }
            pulldown_cmark::Tag::TableHead => {}
            pulldown_cmark::Tag::TableRow => {}
            pulldown_cmark::Tag::TableCell => {}
            pulldown_cmark::Tag::Emphasis => {
                self.text_style.emphasis = true;
            }
            pulldown_cmark::Tag::Strong => {
                self.text_style.strong = true;
            }
            pulldown_cmark::Tag::Strikethrough => {
                self.text_style.strikethrough = true;
            }
            pulldown_cmark::Tag::Link { dest_url, .. } => {
                if let Some(flow) = &mut self.flow {
                    flow.link_start = Some(flow.job.text.len());
                }
                self.link = Some(crate::Link {
                    destination: dest_url.to_string(),
                    text: Vec::new(),
                });
            }
            pulldown_cmark::Tag::Image { dest_url, .. } => {
                self.image = Some(crate::Image::new(&dest_url, options));
            }
            pulldown_cmark::Tag::HtmlBlock => {
                self.line.try_insert_start(ui);
            }
            pulldown_cmark::Tag::MetadataBlock(_) => {}

            pulldown_cmark::Tag::DefinitionList => {
                self.line.try_insert_start(ui);
                self.def_list.is_first_item = true;
            }
            pulldown_cmark::Tag::DefinitionListTitle => {
                // we disable newline as the first title should not insert a newline
                // as we have already done that upon the DefinitionList Tag
                if !self.def_list.is_first_item {
                    self.line.try_insert_start(ui)
                } else {
                    self.def_list.is_first_item = false;
                }
            }
            pulldown_cmark::Tag::DefinitionListDefinition => {
                self.def_list.is_def_list_def = true;
            }
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
            pulldown_cmark::TagEnd::Paragraph => {
                self.flush_inline(ui, cache);
                newline(ui);
            }
            pulldown_cmark::TagEnd::Heading { .. } => {
                self.flush_inline(ui, cache);
                newline(ui);
                self.text_style.heading = None;
            }
            pulldown_cmark::TagEnd::BlockQuote(_) => {}
            pulldown_cmark::TagEnd::CodeBlock => {
                self.end_code_block(ui, cache, options, max_width);
            }

            pulldown_cmark::TagEnd::List(_) => {
                self.line.should_start_newline = true;
                self.line.should_end_newline = true;

                self.list.end_level(ui, self.line.can_insert_end());

                if !self.list.is_inside_a_list() {
                    // Reset all the state and make it ready for the next list that occurs
                    self.list = List::default();
                }
            }
            pulldown_cmark::TagEnd::Item => {}
            pulldown_cmark::TagEnd::FootnoteDefinition => {
                self.line.should_start_newline = true;
                self.line.should_end_newline = true;
                self.line.try_insert_end(ui);
            }
            pulldown_cmark::TagEnd::Table => {}
            pulldown_cmark::TagEnd::TableHead => {}
            pulldown_cmark::TagEnd::TableRow => {}
            pulldown_cmark::TagEnd::TableCell => {
                // Ensure space between cells
                ui.label("  ");
            }
            pulldown_cmark::TagEnd::Emphasis => {
                self.text_style.emphasis = false;
            }
            pulldown_cmark::TagEnd::Strong => {
                self.text_style.strong = false;
            }
            pulldown_cmark::TagEnd::Strikethrough => {
                self.text_style.strikethrough = false;
            }
            pulldown_cmark::TagEnd::Link { .. } => {
                if let Some(link) = self.link.take() {
                    if let Some(flow) = &mut self.flow {
                        if let Some(start) = flow.link_start.take() {
                            flow.links
                                .push((start..flow.job.text.len(), link.destination));
                        }
                        return;
                    }
                    let mut job = egui::text::LayoutJob::default();
                    for text in link.text {
                        text.color(ui.visuals().hyperlink_color)
                            .underline()
                            .append_to(
                                &mut job,
                                ui.style(),
                                egui::FontSelection::Default,
                                egui::Align::LEFT,
                            );
                    }
                    if cache.link_hooks().contains_key(&link.destination) {
                        let response = ui.link(job).on_hover_text(&link.destination);
                        if response.clicked() || response.middle_clicked() {
                            cache.link_hooks_mut().insert(link.destination, true);
                        }
                    } else {
                        ui.hyperlink_to(job, link.destination);
                    }
                }
            }
            pulldown_cmark::TagEnd::Image { .. } => {
                if let Some(image) = self.image.take() {
                    image.end(ui, options);
                }
            }
            pulldown_cmark::TagEnd::HtmlBlock => {
                if let Some(html_fn) = options.html_fn {
                    html_fn(ui, &self.html_block);
                    self.html_block.clear();
                }
            }

            pulldown_cmark::TagEnd::MetadataBlock(_) => {}

            pulldown_cmark::TagEnd::DefinitionList => self.line.try_insert_end(ui),
            pulldown_cmark::TagEnd::DefinitionListTitle
            | pulldown_cmark::TagEnd::DefinitionListDefinition => {}
        }
    }

    fn end_code_block(
        &mut self,
        ui: &mut Ui,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        max_width: f32,
    ) {
        if let Some(block) = self.code_block.take() {
            if let Some(render) = self.code_block_fn {
                render(ui, &block.content, block.lang.as_deref());
            } else {
                block.end(ui, cache, options, max_width);
            }
            self.line.try_insert_end(ui);
        }
    }
}
