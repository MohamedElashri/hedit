//! Keep one layout for the displayed page, including its syntax colors.
use eframe::egui::{self, text::Galley};
use hedit::{
    syntax::{self, Language},
    theme::Theme,
};
use std::sync::{Arc, Weak};

type FontAtlas = egui::epaint::mutex::Mutex<egui::epaint::TextureAtlas>;

#[derive(Default)]
pub(super) struct LayoutCache {
    entry: Option<Entry>,
}

struct Entry {
    language: Language,
    font_size: f32,
    wrap_width: f32,
    pixels_per_point: f32,
    atlas: Weak<FontAtlas>,
    theme: Theme,
    galley: Arc<Galley>,
}

impl LayoutCache {
    pub(super) fn layout(
        &mut self,
        ui: &egui::Ui,
        text: &str,
        language: Language,
        font_size: f32,
        wrap_width: f32,
        theme: &Theme,
    ) -> Arc<Galley> {
        let pixels_per_point = ui.ctx().pixels_per_point();
        // egui can recreate the font atlas when fonts change or it fills up.
        // A galley's texture coordinates belong to that particular atlas.
        let atlas = ui.fonts(|fonts| fonts.texture_atlas());
        if let Some(entry) = &self.entry {
            if entry.language == language
                && entry.font_size == font_size
                && entry.wrap_width == wrap_width
                && entry.pixels_per_point == pixels_per_point
                && entry.atlas.ptr_eq(&Arc::downgrade(&atlas))
                && entry.theme == *theme
                && entry.galley.job.text == text
            {
                return Arc::clone(&entry.galley);
            }
        }
        let galley = ui.fonts(|fonts| {
            fonts.layout_job(syntax::highlight_language(
                text, font_size, wrap_width, language, theme,
            ))
        });
        self.entry = Some(Entry {
            language,
            font_size,
            wrap_width,
            pixels_per_point,
            atlas: Arc::downgrade(&atlas),
            theme: theme.clone(),
            galley: Arc::clone(&galley),
        });
        galley
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_pages_reuse_layout_and_all_visual_inputs_invalidate_it() {
        let ctx = egui::Context::default();
        let mut cache = LayoutCache::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let theme = Theme::default();
                let source = "let n = 42; // λ\n".repeat(4000);
                let first = cache.layout(ui, &source, Language::Rust, 14.0, 800.0, &theme);
                for _ in 0..20 {
                    let reused = cache.layout(ui, &source, Language::Rust, 14.0, 800.0, &theme);
                    assert!(Arc::ptr_eq(&first, &reused));
                }
                let edited = source.replacen("42", "43", 1);
                let changed = cache.layout(ui, &edited, Language::Rust, 14.0, 800.0, &theme);
                assert!(!Arc::ptr_eq(&first, &changed));
                assert_eq!(changed.job.text, edited);
                let plain = cache.layout(ui, &edited, Language::PlainText, 14.0, 800.0, &theme);
                assert!(!Arc::ptr_eq(&changed, &plain));
                assert_eq!(plain.job.sections.len(), 1);
                let zoomed = cache.layout(ui, &edited, Language::PlainText, 16.0, 800.0, &theme);
                assert!(!Arc::ptr_eq(&plain, &zoomed));
                let wrapped = cache.layout(ui, &edited, Language::PlainText, 16.0, 120.0, &theme);
                assert!(!Arc::ptr_eq(&zoomed, &wrapped));
                let paper = hedit::theme::Preset::Paper.theme();
                let recolored = cache.layout(ui, &edited, Language::PlainText, 16.0, 120.0, &paper);
                assert!(!Arc::ptr_eq(&wrapped, &recolored));
                assert_eq!(recolored.job.sections[0].format.color, paper.text.color());
            });
        });
    }

    #[test]
    fn display_scale_changes_invalidate_the_layout() {
        let ctx = egui::Context::default();
        let mut cache = LayoutCache::default();
        for scale in [1.0, 2.0] {
            ctx.set_pixels_per_point(scale);
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    cache.layout(
                        ui,
                        "let n = 1;",
                        Language::Rust,
                        14.0,
                        800.0,
                        &Theme::default(),
                    );
                    assert_eq!(cache.entry.as_ref().unwrap().pixels_per_point, scale);
                });
            });
        }
    }

    #[test]
    fn replacing_fonts_invalidates_cached_texture_coordinates() {
        let ctx = egui::Context::default();
        let mut cache = LayoutCache::default();
        let mut previous = None;
        for replace in [false, true] {
            if replace {
                let mut fonts = egui::FontDefinitions::default();
                fonts
                    .families
                    .insert(egui::FontFamily::Monospace, vec!["Ubuntu-Light".into()]);
                ctx.set_fonts(fonts);
            }
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let galley = cache.layout(
                        ui,
                        "let n = 1;",
                        Language::Rust,
                        14.0,
                        800.0,
                        &Theme::default(),
                    );
                    if let Some(previous) = &previous {
                        assert!(!Arc::ptr_eq(previous, &galley));
                    }
                    previous = Some(galley);
                });
            });
        }
    }
}
