//! Keep one layout for the displayed page, including its syntax colors.
use eframe::egui::{self, text::Galley};
use hedit::{
    syntax::{self, Language},
    theme::Theme,
};
use std::sync::{mpsc, Arc, Weak};

/// Re-highlighting more than this many bytes runs on a background thread,
/// showing the quick lexer's colors meanwhile.
const BACKGROUND: usize = 64 * 1024;

type Runs = Arc<Vec<(usize, egui::Color32)>>;

type FontAtlas = egui::epaint::mutex::Mutex<egui::epaint::TextureAtlas>;

/// The editor font size and, with extra line spacing, the row height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Font {
    pub size: f32,
    pub line_height: Option<f32>,
}

#[derive(Default)]
pub(super) struct LayoutCache {
    entry: Option<Entry>,
    grammar: syntax::grammar::Grammar,
    /// A grammar highlight running on a background thread.
    job: Option<mpsc::Receiver<syntax::grammar::Moved>>,
}

struct Entry {
    language: Language,
    font: Font,
    wrap_width: f32,
    pixels_per_point: f32,
    atlas: Weak<FontAtlas>,
    theme: Theme,
    /// The grammar colors used, or `None` for the lexer's.
    runs: Option<Runs>,
    galley: Arc<Galley>,
    /// Glyph directions of rows with right-to-left text.
    bidi: Arc<super::bidi_view::BidiRows>,
}

impl LayoutCache {
    /// The height of the first laid-out row.
    #[cfg(test)]
    pub(super) fn row_height(&self) -> Option<f32> {
        let entry = self.entry.as_ref()?;
        entry.galley.rows.first().map(|row| row.rect.height())
    }
    /// The latest layout and its row directions.
    pub(super) fn last(&self) -> Option<(Arc<Galley>, Arc<super::bidi_view::BidiRows>)> {
        let entry = self.entry.as_ref()?;
        Some((Arc::clone(&entry.galley), Arc::clone(&entry.bidi)))
    }
    pub(super) fn layout(
        &mut self,
        ui: &egui::Ui,
        text: &str,
        language: Language,
        font: Font,
        wrap_width: f32,
        theme: &Theme,
    ) -> Arc<Galley> {
        let Font {
            size: font_size,
            line_height,
        } = font;
        let runs = self.grammar_runs(ui.ctx(), text, language, theme);
        let pixels_per_point = ui.ctx().pixels_per_point();
        // egui can recreate the font atlas when fonts change or it fills up.
        // A galley's texture coordinates belong to that particular atlas.
        let atlas = ui.fonts(|fonts| fonts.texture_atlas());
        if let Some(entry) = &self.entry {
            if entry.language == language
                && entry.font == font
                && entry.wrap_width == wrap_width
                && entry.pixels_per_point == pixels_per_point
                && entry.atlas.ptr_eq(&Arc::downgrade(&atlas))
                && entry.theme == *theme
                && match (&entry.runs, &runs) {
                    (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                    (None, None) => true,
                    _ => false,
                }
                && entry.galley.job.text == text
            {
                return Arc::clone(&entry.galley);
            }
        }
        let mut job = match &runs {
            Some(runs) => syntax::layout_runs(text, runs, font_size, wrap_width),
            None => syntax::highlight_language(text, font_size, wrap_width, language, theme),
        };
        if line_height.is_some() {
            for section in &mut job.sections {
                section.format.line_height = line_height;
            }
        }
        let (galley, bidi) = ui.fonts(|fonts| {
            let galley = fonts.layout_job(job);
            super::bidi_view::apply(
                fonts,
                galley,
                super::bidi_view::Base::for_language(language),
            )
        });
        self.entry = Some(Entry {
            language,
            font,
            wrap_width,
            pixels_per_point,
            atlas: Arc::downgrade(&atlas),
            theme: theme.clone(),
            runs,
            galley: Arc::clone(&galley),
            bidi: Arc::new(bidi),
        });
        galley
    }
    /// Grammar colors for `text`, or `None` to use the lexer: for languages
    /// without a grammar, and while a large highlight runs in the background.
    fn grammar_runs(
        &mut self,
        ctx: &egui::Context,
        text: &str,
        language: Language,
        theme: &Theme,
    ) -> Option<Runs> {
        if !syntax::grammar::has_grammar(language) {
            return None;
        }
        if let Some(job) = &self.job {
            match job.try_recv() {
                Ok(syntax::grammar::Moved(grammar)) => {
                    self.grammar = grammar;
                    self.job = None;
                }
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(mpsc::TryRecvError::Disconnected) => self.job = None,
            }
        }
        if let Some(runs) = self.grammar.cached(text, language, theme) {
            return Some(runs);
        }
        if self.grammar.cost(text, language, theme) <= BACKGROUND {
            return self.grammar.highlight(text, language, theme);
        }
        // The thread updates the cache and hands it back; edits made meanwhile
        // are then highlighted incrementally from its result.
        let moved = syntax::grammar::Moved(std::mem::take(&mut self.grammar));
        let (tx, rx) = mpsc::channel();
        let (text, theme, ctx) = (text.to_owned(), theme.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut moved = moved;
            moved.0.highlight(&text, language, &theme);
            let _ = tx.send(moved);
            ctx.request_repaint();
        });
        self.job = Some(rx);
        None
    }
    /// Wait for a background highlight, for tests.
    #[cfg(test)]
    pub(super) fn finish_highlight(&mut self) {
        if let Some(job) = self.job.take() {
            if let Ok(syntax::grammar::Moved(grammar)) = job.recv() {
                self.grammar = grammar;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_pages_are_highlighted_in_the_background_then_incrementally() {
        let ctx = egui::Context::default();
        let theme = Theme::default();
        let font = Font {
            size: 14.0,
            line_height: None,
        };
        let large = "fn main() { let x = 1; } // note\n".repeat(4000);
        let mut cache = LayoutCache::default();
        let uses_grammar = |cache: &mut LayoutCache, text: &str| {
            let mut grammar = false;
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    cache.layout(ui, text, Language::Rust, font, 800.0, &theme);
                    grammar = cache.entry.as_ref().unwrap().runs.is_some();
                });
            });
            grammar
        };
        assert!(
            !uses_grammar(&mut cache, &large),
            "lexer while the grammar works"
        );
        assert!(cache.job.is_some());
        cache.finish_highlight();
        assert!(uses_grammar(&mut cache, &large));
        // A small edit is highlighted right away.
        let edited = large.replacen("let x", "let y", 1);
        assert!(uses_grammar(&mut cache, &edited));
        assert!(cache.job.is_none());
    }

    #[test]
    fn unchanged_pages_reuse_layout_and_all_visual_inputs_invalidate_it() {
        let ctx = egui::Context::default();
        let mut cache = LayoutCache::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let theme = Theme::default();
                let source = "let n = 42; // λ\n".repeat(4000);
                let first = cache.layout(
                    ui,
                    &source,
                    Language::Rust,
                    Font {
                        size: 14.0,
                        line_height: None,
                    },
                    800.0,
                    &theme,
                );
                for _ in 0..20 {
                    let reused = cache.layout(
                        ui,
                        &source,
                        Language::Rust,
                        Font {
                            size: 14.0,
                            line_height: None,
                        },
                        800.0,
                        &theme,
                    );
                    assert!(Arc::ptr_eq(&first, &reused));
                }
                let edited = source.replacen("42", "43", 1);
                let changed = cache.layout(
                    ui,
                    &edited,
                    Language::Rust,
                    Font {
                        size: 14.0,
                        line_height: None,
                    },
                    800.0,
                    &theme,
                );
                assert!(!Arc::ptr_eq(&first, &changed));
                assert_eq!(changed.job.text, edited);
                let plain = cache.layout(
                    ui,
                    &edited,
                    Language::PlainText,
                    Font {
                        size: 14.0,
                        line_height: None,
                    },
                    800.0,
                    &theme,
                );
                assert!(!Arc::ptr_eq(&changed, &plain));
                assert_eq!(plain.job.sections.len(), 1);
                let zoomed = cache.layout(
                    ui,
                    &edited,
                    Language::PlainText,
                    Font {
                        size: 16.0,
                        line_height: None,
                    },
                    800.0,
                    &theme,
                );
                assert!(!Arc::ptr_eq(&plain, &zoomed));
                let wrapped = cache.layout(
                    ui,
                    &edited,
                    Language::PlainText,
                    Font {
                        size: 16.0,
                        line_height: None,
                    },
                    120.0,
                    &theme,
                );
                assert!(!Arc::ptr_eq(&zoomed, &wrapped));
                let paper = Theme::paper();
                let recolored = cache.layout(
                    ui,
                    &edited,
                    Language::PlainText,
                    Font {
                        size: 16.0,
                        line_height: None,
                    },
                    120.0,
                    &paper,
                );
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
                        Font {
                            size: 14.0,
                            line_height: None,
                        },
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
                        Font {
                            size: 14.0,
                            line_height: None,
                        },
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

/// Cache the original-coordinate mapping independently of syntax galley layout.
#[derive(Default)]
pub(super) struct ProjectionCache {
    entry: Option<(
        String,
        u8,
        Vec<hedit::advanced::Fold>,
        Arc<hedit::advanced::Projection>,
    )>,
}
impl ProjectionCache {
    pub(super) fn get(
        &mut self,
        text: &str,
        width: u8,
        folds: &[hedit::advanced::Fold],
    ) -> Arc<hedit::advanced::Projection> {
        if let Some((source, previous_width, previous_folds, projection)) = &self.entry {
            if source == text && *previous_width == width && previous_folds == folds {
                return Arc::clone(projection);
            }
        }
        let projection = Arc::new(hedit::advanced::Projection::new(
            text,
            width as usize,
            folds,
        ));
        self.entry = Some((
            text.to_owned(),
            width,
            folds.to_vec(),
            Arc::clone(&projection),
        ));
        projection
    }
}
