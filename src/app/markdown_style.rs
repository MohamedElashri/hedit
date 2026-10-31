//! Shared document typography and native code controls for previews and help.
use super::*;
use std::hash::{Hash, Hasher};

pub(super) fn apply(ui: &mut egui::Ui, theme: &Theme, font_size: f32) {
    let body = (font_size + 1.0).clamp(14.0, 30.0);
    let style = ui.style_mut();
    let regular = egui::FontFamily::Name("hedit-reader".into());
    let strong = egui::FontFamily::Name("hedit-reader-strong".into());
    for (name, size, family) in [
        (egui::TextStyle::Body, body, regular.clone()),
        (egui::TextStyle::Small, (body - 2.0).max(12.0), regular),
        (egui::TextStyle::Heading, body * 1.8, strong.clone()),
        (egui::TextStyle::Name("MarkdownStrong".into()), body, strong),
        (
            egui::TextStyle::Monospace,
            (body - 1.0).max(12.0),
            egui::FontFamily::Monospace,
        ),
    ] {
        style.text_styles.insert(name, FontId::new(size, family));
    }
    style.spacing.item_spacing.y = 7.0;
    style.visuals.override_text_color = None;
    style.visuals.widgets.noninteractive.fg_stroke.color = theme.text.color();
    style.visuals.widgets.active.fg_stroke.color = theme.text.color();
    style.visuals.hyperlink_color = link_color(theme).color();
    style.visuals.code_bg_color = theme.elevated.color();
    style.visuals.faint_bg_color = theme.panel.color();
    style.visuals.striped = true;
    style.url_in_tooltip = true;
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
            section.format.line_height = Some(f32::from_bits(key.size) * 1.45);
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
    let width = ui
        .available_width()
        .min(ui.ctx().screen_rect().width())
        .max(80.0);
    egui::Frame::new()
        .fill(theme.background.color())
        .stroke(egui::Stroke::new(1.0_f32, theme.border.color()))
        .corner_radius(4)
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width((width - 24.0).max(56.0));
                let id = ui.make_persistent_id("code-copied");
                let copied = ui.data(|d| d.get_temp::<bool>(id).unwrap_or(false));
                let mut copy = false;
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(if language.is_empty() {
                            "Plain text"
                        } else {
                            language
                        })
                        .small()
                        .color(theme.muted.color()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let response = ui
                            .small_button(if copied { "Copied" } else { "Copy" })
                            .on_hover_text("Copy code");
                        copy = response.clicked();
                        if copied && !response.hovered() {
                            ui.data_mut(|d| d.insert_temp(id, false));
                        }
                    });
                });
                ui.add_space(4.0);
                let code_width = ui.available_width();
                let output = egui::ScrollArea::horizontal()
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let mut text = text;
                        let mut layouter = |ui: &egui::Ui, _: &str, _: f32| {
                            ui.fonts(|f| f.layout_job((*job).clone()))
                        };
                        egui::TextEdit::multiline(&mut text)
                            .code_editor()
                            .font(egui::TextStyle::Monospace)
                            .frame(false)
                            .desired_width(code_width)
                            .desired_rows(1)
                            .layouter(&mut layouter)
                            .show(ui)
                    })
                    .inner;
                if copy {
                    use egui::TextBuffer;
                    let selected = output
                        .cursor_range
                        .map(|cursor| text.char_range(cursor.as_sorted_char_range()))
                        .filter(|s| !s.is_empty());
                    ui.ctx().copy_text(selected.unwrap_or(text).to_owned());
                    ui.data_mut(|d| d.insert_temp(id, true));
                }
            });
        });
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
