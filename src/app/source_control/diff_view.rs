//! Native, cached syntax coloring and line selection shared by both layouts.
use super::*;
use hedit::syntax;
use std::{
    hash::{Hash, Hasher},
    ops::Range,
};

#[derive(Clone, Copy)]
pub(super) struct TextStyle {
    pub font: editor_layout::Font,
    pub language: syntax::Language,
    pub tab_size: usize,
    pub selected: bool,
}

#[derive(Clone, Copy)]
struct CodeKey<'a> {
    text: &'a str,
    emphasis: &'a [Range<usize>],
    language: &'a str,
    font: editor_layout::Font,
    tab_size: usize,
    selected: bool,
    metadata: bool,
    theme: &'a Theme,
    addition: bool,
}
impl Hash for CodeKey<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text.hash(state);
        self.emphasis.hash(state);
        self.language.hash(state);
        self.font.size.to_bits().hash(state);
        self.font.line_height.map(f32::to_bits).hash(state);
        self.tab_size.hash(state);
        self.selected.hash(state);
        self.metadata.hash(state);
        self.addition.hash(state);
        for role in hedit::theme::Role::ALL {
            self.theme.color(role).0.hash(state);
        }
    }
}
#[derive(Default)]
struct Highlighter;
impl egui::cache::ComputerMut<CodeKey<'_>, Arc<egui::text::LayoutJob>> for Highlighter {
    fn compute(&mut self, key: CodeKey<'_>) -> Arc<egui::text::LayoutJob> {
        let text: String = key
            .text
            .chars()
            .take(64 * 1024 / key.tab_size.max(4))
            .collect();
        let projection = hedit::advanced::Projection::new(&text, key.tab_size, &[]);
        let source_bytes: Vec<_> = text
            .char_indices()
            .map(|(i, _)| i)
            .chain([text.len()])
            .collect();
        let display_bytes: Vec<_> = projection
            .text
            .char_indices()
            .map(|(i, _)| i)
            .chain([projection.text.len()])
            .collect();
        let emphasis: Vec<_> = key
            .emphasis
            .iter()
            .filter(|r| r.end <= text.len())
            .map(|r| {
                let start = source_bytes.partition_point(|i| *i < r.start);
                let end = source_bytes.partition_point(|i| *i < r.end);
                display_bytes[projection.display(start)]..display_bytes[projection.display(end)]
            })
            .collect();
        let language =
            syntax::Language::from_id(key.language).unwrap_or(syntax::Language::PlainText);
        let runs =
            syntax::grammar::Grammar::default().highlight(&projection.text, language, key.theme);
        let mut job = editor_layout::code_job(
            &projection.text,
            language,
            key.font,
            f32::INFINITY,
            key.theme,
            runs.as_deref().map(Vec::as_slice),
        );
        if key.selected {
            for section in &mut job.sections {
                section.format.color = key.theme.selection_text.color();
            }
            return Arc::new(job);
        }
        if key.metadata {
            for section in &mut job.sections {
                section.format.color = key.theme.muted.color();
            }
        }
        let background =
            status_color(if key.addition { 'A' } else { 'D' }, key.theme).gamma_multiply(0.23);
        let sections = std::mem::take(&mut job.sections);
        for section in sections {
            let mut points = vec![section.byte_range.start, section.byte_range.end];
            for range in &emphasis {
                if section.byte_range.start < range.start && range.start < section.byte_range.end {
                    points.push(range.start);
                }
                if section.byte_range.start < range.end && range.end < section.byte_range.end {
                    points.push(range.end);
                }
            }
            points.sort_unstable();
            points.dedup();
            for pair in points.windows(2) {
                let mut section = section.clone();
                section.byte_range = pair[0]..pair[1];
                if emphasis.iter().any(|r| r.contains(&pair[0])) {
                    section.format.background = background;
                }
                job.sections.push(section);
            }
        }
        Arc::new(job)
    }
}

pub(super) fn paint_text(
    ui: &egui::Ui,
    theme: &Theme,
    line: &git::DiffLine,
    style: TextStyle,
    rect: egui::Rect,
    origin_x: f32,
) {
    let galley = layout_line(ui, theme, line, style);
    let y = rect.center().y - galley.size().y / 2.0;
    ui.painter()
        .with_clip_rect(rect.intersect(ui.clip_rect()))
        .galley(egui::pos2(origin_x, y), galley, theme.text.color());
}

pub(super) fn layout_line(
    ui: &egui::Ui,
    theme: &Theme,
    line: &git::DiffLine,
    style: TextStyle,
) -> Arc<egui::Galley> {
    let language = if matches!(line.kind, '+' | '-' | ' ') {
        style.language
    } else {
        syntax::Language::PlainText
    };
    let text = if matches!(line.kind, '+' | '-' | ' ') {
        line.text.get(1..).unwrap_or("")
    } else {
        &line.text
    };
    let text = &text[..text
        .char_indices()
        .nth(16_384)
        .map_or(text.len(), |(i, _)| i)];
    let job = ui.ctx().memory_mut(|m| {
        m.caches
            .cache::<egui::cache::FrameCache<Arc<egui::text::LayoutJob>, Highlighter>>()
            .get(CodeKey {
                text,
                emphasis: &line.emphasis,
                language: language.id(),
                font: style.font,
                tab_size: style.tab_size,
                selected: style.selected,
                metadata: !matches!(line.kind, '+' | '-' | ' '),
                theme,
                addition: line.kind == '+',
            })
    });
    ui.fonts(|f| {
        let galley = f.layout_job((*job).clone());
        bidi_view::apply(f, galley, bidi_view::Base::for_language(language)).0
    })
}

pub(super) fn last_line(diff: &Diff) -> usize {
    let old = diff.lines.iter().rev().find_map(|l| l.old).unwrap_or(1);
    let new = diff.lines.iter().rev().find_map(|l| l.new).unwrap_or(1);
    old.max(new)
}

pub(super) fn line_background(kind: char, theme: &Theme) -> Option<egui::Color32> {
    match kind {
        '+' => Some(theme.success.color().gamma_multiply(0.10)),
        '-' => Some(theme.error.color().gamma_multiply(0.10)),
        '@' => Some(theme.surface.color()),
        _ => None,
    }
}

pub(super) fn layout_controls(ui: &mut egui::Ui, prefer_split: &mut bool, can_split: bool) {
    let split = *prefer_split && can_split;
    if ui.selectable_label(!split, "Unified").clicked() {
        *prefer_split = false;
    }
    if ui
        .add_enabled(can_split, egui::SelectableLabel::new(split, "Side by side"))
        .on_disabled_hover_text("Widen the review area to show side-by-side diffs.")
        .clicked()
    {
        *prefer_split = true;
    }
}

pub(super) fn context_menu(response: &egui::Response, line: &git::DiffLine, diff: &Diff) {
    response.context_menu(|ui| {
        if ui.button("Copy line").clicked() {
            let text = if matches!(line.kind, '+' | '-' | ' ') {
                line.text.get(1..).unwrap_or("")
            } else {
                &line.text
            };
            ui.ctx().copy_text(text.to_owned());
            ui.close_menu();
        }
        if ui.button("Copy diff").clicked() {
            ui.ctx().copy_text(diff.patch.clone());
            ui.close_menu();
        }
    });
}

impl SourceControl {
    pub(super) fn select_line(&mut self, index: usize, diff: &Diff, extend: bool) {
        if !diff
            .lines
            .get(index)
            .is_some_and(|l| matches!(l.kind, '+' | '-'))
        {
            return;
        }
        if let Some(anchor) = self.line_anchor.filter(|_| extend) {
            self.selected_lines.extend(
                (anchor.min(index)..=anchor.max(index))
                    .filter(|i| matches!(diff.lines[*i].kind, '+' | '-')),
            );
        } else {
            if !self.selected_lines.remove(&index) {
                self.selected_lines.insert(index);
            }
            self.line_anchor = Some(index);
        }
    }
}

impl HeditApp {
    pub(super) fn git_hunk_action(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        selection: &Selection,
        diff: &Diff,
        target: (usize, egui::Rect),
        pending: bool,
    ) {
        if !diff.supports_partial() || target.0 >= diff.hunks.len() {
            return;
        }
        let label = if selection.kind == DiffKind::Staged {
            "Unstage hunk"
        } else {
            "Stage hunk"
        };
        let clicked = ui
            .add_enabled_ui(!pending, |ui| {
                ui.put(target.1, egui::Button::new(label).small()).clicked()
            })
            .inner;
        if clicked {
            self.git_action(
                ctx,
                Operation::Hunk {
                    selection: selection.clone(),
                    expected: diff.patch.clone(),
                    hunk: target.0,
                },
            );
        }
    }
    pub(super) fn git_text_style(&self, ui: &egui::Ui, path: &Path) -> TextStyle {
        let absolute = self
            .source_control
            .root
            .as_ref()
            .map(|root| root.join(path));
        let buffer = absolute.as_deref().and_then(|path| {
            self.workspace
                .open
                .iter()
                .find(|buffer| buffer.page.info.path.as_deref().map(Path::new) == Some(path))
        });
        let language = buffer.map_or_else(
            || syntax::Language::detect(&path.to_string_lossy()),
            |b| b.language(),
        );
        let tab_size = buffer
            .and_then(|b| b.view.tab_size)
            .or_else(|| {
                self.settings
                    .syntax_specific
                    .get(language.id())
                    .and_then(|s| s.tab_size)
            })
            .or(self.settings.format.tab_size)
            .unwrap_or(4)
            .clamp(1, 8) as usize;
        TextStyle {
            font: editor_layout::Font::from_settings(ui, &self.settings),
            language,
            tab_size,
            selected: false,
        }
    }

    pub(super) fn git_line_actions(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        selection: &Selection,
        diff: &Diff,
        pending: bool,
    ) {
        if !diff.supports_partial() {
            return;
        }
        let count = self.source_control.selected_lines.len();
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(!pending && count > 0, |ui| {
                let action = if selection.kind == DiffKind::Staged {
                    "Unstage selected"
                } else {
                    "Stage selected"
                };
                if ui.small_button(format!("{action} ({count})")).clicked() {
                    self.git_action(
                        ctx,
                        Operation::Lines {
                            selection: selection.clone(),
                            expected: diff.patch.clone(),
                            lines: self.source_control.selected_lines.iter().copied().collect(),
                        },
                    );
                }
            });
            if count > 0 && ui.small_button("Clear selection").clicked() {
                self.source_control.clear_line_selection();
            }
            ui.label(
                RichText::new("Click changed lines to select · Shift-click for a range")
                    .small()
                    .color(self.theme.muted.color()),
            );
        });
    }

    pub(super) fn git_diff_line_selection(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        diff: &Diff,
        index: usize,
        pending: bool,
        content_left: f32,
    ) -> bool {
        if !diff.supports_partial() || !matches!(diff.lines[index].kind, '+' | '-') {
            return false;
        }
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                ui.is_enabled() && !pending,
                self.source_control.selected_lines.contains(&index),
                &diff.lines[index].text,
            )
        });
        if !pending && response.clicked() {
            self.source_control
                .select_line(index, diff, ui.input(|i| i.modifiers.shift));
        }
        if self.source_control.selected_lines.contains(&index) {
            let mut content = response.rect;
            content.min.x = content_left;
            ui.painter()
                .rect_filled(content, 0.0, self.theme.selection.color());
        }
        self.source_control.selected_lines.contains(&index)
    }
}
