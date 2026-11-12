//! Shared document typography and native code controls for previews and help.
use super::*;
use std::hash::{Hash, Hasher};

pub(super) fn apply(ui: &mut egui::Ui, theme: &Theme, font_size: f32) {
    let body = body_size(font_size);
    let style = ui.style_mut();
    let regular = egui::FontFamily::Name("hedit-reader".into());
    let strong = egui::FontFamily::Name("hedit-reader-strong".into());
    for (name, size, family) in [
        (egui::TextStyle::Body, body, regular.clone()),
        (egui::TextStyle::Small, (body - 2.0).max(12.0), regular),
        (egui::TextStyle::Heading, body * 1.8, strong.clone()),
        (egui::TextStyle::Name("MarkdownStrong".into()), body, strong),
        (
            egui::TextStyle::Name("MarkdownEmphasis".into()),
            body,
            egui::FontFamily::Name("hedit-reader-emphasis".into()),
        ),
        (
            egui::TextStyle::Name("MarkdownStrongEmphasis".into()),
            body,
            egui::FontFamily::Name("hedit-reader-strong-emphasis".into()),
        ),
        (
            egui::TextStyle::Monospace,
            body,
            egui::FontFamily::Monospace,
        ),
    ] {
        style.text_styles.insert(name, FontId::new(size, family));
    }
    style.spacing.item_spacing.y = 0.0;
    style.visuals.override_text_color = None;
    style.visuals.widgets.noninteractive.fg_stroke.color = theme.text.color();
    style.visuals.widgets.active.fg_stroke.color = theme.text.color();
    style.visuals.hyperlink_color = link_color(theme).color();
    style.visuals.code_bg_color = theme.elevated.color();
    style.visuals.faint_bg_color = theme.panel.color();
    style.visuals.panel_fill = theme.surface.color();
    style.visuals.striped = false;
    style.url_in_tooltip = true;
}

fn body_size(font_size: f32) -> f32 {
    font_size.clamp(10.0, 32.0)
}

/// Match the editor preview's document padding instead of a magazine column.
/// Code, tables, and equations own their horizontal scrolling inside this page.
pub(super) fn reader<R>(
    ui: &mut egui::Ui,
    theme: &Theme,
    font_size: f32,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.scope(|ui| {
        apply(ui, theme, font_size);
        let available = ui.available_width();
        let padding = 26.0_f32.min(((available - 1.0) / 2.0).max(0.0));
        let width = (available - 2.0 * padding).max(1.0);
        ui.horizontal_top(|ui| {
            ui.add_space(((available - width) / 2.0).max(0.0));
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.add_space(body_size(font_size));
                let result = contents(ui);
                ui.add_space(48.0);
                result
            })
            .inner
        })
        .inner
    })
    .inner
}

fn link_color(theme: &Theme) -> hedit::theme::Rgb {
    let mut link = theme.accent;
    let target = if Theme::contrast(theme.text, theme.surface) >= 4.5 {
        theme.text
    } else {
        let black = hedit::theme::Rgb::new(0, 0, 0);
        let white = hedit::theme::Rgb::new(255, 255, 255);
        if Theme::contrast(black, theme.surface) > Theme::contrast(white, theme.surface) {
            black
        } else {
            white
        }
    };
    for _ in 0..32 {
        if Theme::contrast(link, theme.surface) >= 4.5 {
            break;
        }
        link = hedit::theme::Rgb(std::array::from_fn(|i| {
            (f32::from(link.0[i]) * 0.85 + f32::from(target.0[i]) * 0.15).round() as u8
        }));
    }
    if Theme::contrast(link, theme.surface) >= 4.5 {
        link
    } else {
        target
    }
}

#[derive(Clone, Copy)]
struct CodeKey<'a> {
    text: &'a str,
    language: &'a str,
    size: u32,
    theme: &'a Theme,
}
impl Hash for CodeKey<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text.hash(state);
        self.language.hash(state);
        self.size.hash(state);
        self.theme.dark.hash(state);
        for role in hedit::theme::Role::ALL {
            self.theme.color(role).0.hash(state);
        }
    }
}
#[derive(Default)]
struct Highlighter;
impl egui::cache::ComputerMut<CodeKey<'_>, Arc<egui::text::LayoutJob>> for Highlighter {
    fn compute(&mut self, key: CodeKey<'_>) -> Arc<egui::text::LayoutJob> {
        let language = if key.text.len() > 64 * 1024 {
            syntax::Language::PlainText
        } else {
            syntax::Language::from_id(key.language)
                .unwrap_or_else(|| syntax::Language::detect(&format!("code.{}", key.language)))
        };
        let mut job = syntax::highlight_language(
            key.text,
            f32::from_bits(key.size),
            f32::INFINITY,
            language,
            key.theme,
        );
        for section in &mut job.sections {
            section.format.line_height = Some(f32::from_bits(key.size) * 1.357);
        }
        Arc::new(job)
    }
}

pub(super) fn code_block(ui: &mut egui::Ui, theme: &Theme, source: &str, language: Option<&str>) {
    let text = source.strip_suffix('\n').unwrap_or(source);
    let language = language
        .unwrap_or("")
        .split_whitespace()
        .next()
        .unwrap_or("");
    let size = egui::TextStyle::Monospace.resolve(ui.style()).size;
    let job = ui.ctx().memory_mut(|memory| {
        memory
            .caches
            .cache::<egui::cache::FrameCache<Arc<egui::text::LayoutJob>, Highlighter>>()
            .get(CodeKey {
                text,
                language,
                size: size.to_bits(),
                theme,
            })
    });
    let width = ui.available_width().max(1.0);
    let scale = size / 14.0;
    let padding = 16.0 * scale;
    let content_width = (width - 2.0 * padding - 2.0).max(1.0);
    let frame = egui::Frame::new()
        .fill(theme.panel.color())
        .stroke(egui::Stroke::new(1.0_f32, theme.border.color()))
        .corner_radius(3)
        .inner_margin(padding)
        .show(ui, |ui| {
            ui.set_width(content_width);
            // A code sample participates in the document's normal vertical
            // flow and selection. Only long lines own a horizontal scrollbar.
            egui::ScrollArea::horizontal()
                .id_salt("code-lines")
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.set_min_width(content_width);
                    let galley = ui.fonts(|fonts| fonts.layout_job((*job).clone()));
                    ui.add(egui::Label::new(galley).selectable(true));
                });
        });
    let id = ui.make_persistent_id("copy-code");
    let copied_id = id.with("copied-at");
    let time = ui.input(|input| input.time);
    let copied = ui
        .data(|data| data.get_temp::<f64>(copied_id))
        .is_some_and(|at| time - at < 2.0);
    let size = egui::vec2(54.0 * scale, 24.0 * scale);
    let rect = egui::Rect::from_min_size(
        egui::pos2(
            frame.response.rect.right() - size.x - 8.0 * scale,
            frame.response.rect.top() + 8.0 * scale,
        ),
        size,
    );
    let response = ui
        .interact(rect, id, egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Copy code");
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Copy code")
    });
    let hovered = ui
        .input(|input| input.pointer.hover_pos())
        .is_some_and(|pos| frame.response.rect.contains(pos));
    if hovered || response.has_focus() || copied {
        ui.painter().rect_filled(
            rect,
            3,
            if response.hovered() {
                theme.elevated.color()
            } else {
                theme.panel.color()
            },
        );
        ui.painter().rect_stroke(
            rect,
            3,
            egui::Stroke::new(
                1.0_f32,
                if response.has_focus() {
                    theme.accent.color()
                } else {
                    theme.border.color()
                },
            ),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            if copied { "Copied" } else { "Copy" },
            egui::TextStyle::Small.resolve(ui.style()),
            theme.text.color(),
        );
    }
    let keyboard = response.has_focus()
        && ui.input_mut(|input| {
            input.consume_key(Modifiers::NONE, Key::Enter)
                || input.consume_key(Modifiers::NONE, Key::Space)
        });
    if response.clicked() || keyboard {
        ui.ctx().copy_text(text.to_owned());
        ui.data_mut(|data| data.insert_temp(copied_id, time));
        ui.ctx().request_repaint_after(Duration::from_secs(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reader_links_remain_readable_across_the_theme_catalog() {
        for theme in hedit::theme::builtins() {
            assert!(
                Theme::contrast(link_color(theme), theme.surface) >= 4.5,
                "{} links",
                theme.name
            );
        }
    }
}
