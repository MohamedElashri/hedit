//! A commonmark viewer for egui
//!
//! # Example
//!
//! ```
//! # use egui_commonmark::*;
//! # use egui::__run_test_ui;
//! let markdown =
//! r"# Hello world
//!
//! * A list
//! * [ ] Checkbox
//! ";
//!
//! # __run_test_ui(|ui| {
//! let mut cache = CommonMarkCache::default();
//! CommonMarkViewer::new().show(ui, &mut cache, markdown);
//! # });
//!
//! ```
//!
//! Remember to opt into the image formats you want to use!
//!
//! ```toml
//! image = { version = "0.25", default-features = false, features = ["png"] }
//! ```
//! # FAQ
//!
//! ## URL is not displayed when hovering over a link
//!
//! By default egui does not show urls when you hover hyperlinks. To enable it,
//! you can do the following before calling any ui related functions:
//!
//! ```
//! # use egui::__run_test_ui;
//! # __run_test_ui(|ui| {
//! ui.style_mut().url_in_tooltip = true;
//! # });
//! ```
//!
//!
//! # Compile time evaluation of markdown
//!
//! If you want to embed markdown directly the binary then you can enable the `macros` feature.
//! This will do the parsing of the markdown at compile time and output egui widgets.
//!
//! ## Example
//!
//! ```
//! use egui_commonmark::{CommonMarkCache, commonmark};
//! # egui::__run_test_ui(|ui| {
//! let mut cache = CommonMarkCache::default();
//! let _response = commonmark!(ui, &mut cache, "# ATX Heading Level 1");
//! # });
//! ```
//!
//! Alternatively you can embed a file
//!
//!
//! ## Example
//!
//! ```rust,ignore
//! use egui_commonmark::{CommonMarkCache, commonmark_str};
//! # egui::__run_test_ui(|ui| {
//! let mut cache = CommonMarkCache::default();
//! commonmark_str!(ui, &mut cache, "content.md");
//! # });
//! ```
//!
//! For more information check out the documentation for
//! [egui_commonmark_macros](https://docs.rs/crate/egui_commonmark_macros/latest)
#![cfg_attr(feature = "document-features", doc = "# Features")]
#![cfg_attr(feature = "document-features", doc = document_features::document_features!())]

#[cfg(feature = "pulldown_cmark")]
use egui::Id;

mod parsers;

pub use egui_commonmark_backend::alerts::{Alert, AlertBundle};
pub use egui_commonmark_backend::misc::CommonMarkCache;
pub use egui_commonmark_backend::RenderMathFn;
pub use pulldown_cmark;

/// Parsed once, with source offsets retained for navigation and split scrolling.
/// Applications can construct this on a worker instead of parsing on every frame.
pub struct PreparedMarkdown {
    events: Vec<(pulldown_cmark::Event<'static>, std::ops::Range<usize>)>,
    document: parsers::document::Document,
}

impl PreparedMarkdown {
    pub fn new(source: &str, math: bool) -> Self {
        let events: Vec<_> =
            pulldown_cmark::Parser::new_ext(source, parsers::document::parser_options_math(math))
                .into_offset_iter()
                .map(|(event, span)| (event.into_static(), span))
                .collect();
        Self {
            document: parsers::document::Document::new(&events),
            events,
        }
    }

    pub fn events(
        &self,
    ) -> impl Iterator<Item = &(pulldown_cmark::Event<'static>, std::ops::Range<usize>)> {
        self.events.iter()
    }
}

/// Called at the start of a source block. Heading levels are one through six.
pub type RenderBlockFn = dyn Fn(&mut egui::Ui, std::ops::Range<usize>, Option<u8>);
/// Cached geometry needed before an inline equation is painted.
#[derive(Clone, Copy, Debug)]
pub struct InlineMathMetrics {
    pub height: f32,
    pub above: f32,
    pub width: f32,
}
pub type MeasureMathFn = dyn Fn(&egui::Ui, &str) -> InlineMathMetrics;

/// Application-provided native code block, with an optional language identifier.
pub type RenderCodeBlockFn = dyn Fn(&mut egui::Ui, &str, Option<&str>);

#[cfg(feature = "macros")]
pub use egui_commonmark_macros::*;

#[cfg(feature = "macros")]
// Do not rely on this directly!
#[doc(hidden)]
pub use egui_commonmark_backend;

use egui_commonmark_backend::*;

#[derive(Default)]
pub struct CommonMarkViewer<'f> {
    options: CommonMarkOptions<'f>,
    code_block_fn: Option<&'f RenderCodeBlockFn>,
    block_fn: Option<&'f RenderBlockFn>,
    measure_math_fn: Option<&'f MeasureMathFn>,
}

impl<'f> CommonMarkViewer<'f> {
    pub fn new() -> Self {
        Self::default()
    }

    /// The amount of spaces a bullet point is indented. By default this is 4
    /// spaces.
    pub fn indentation_spaces(mut self, spaces: usize) -> Self {
        self.options.indentation_spaces = spaces;
        self
    }

    /// The maximum size images are allowed to be. They will be scaled down if
    /// they are larger
    pub fn max_image_width(mut self, width: Option<usize>) -> Self {
        self.options.max_image_width = width;
        self
    }

    /// The default width of the ui. This is only respected if this is larger than
    /// the [`max_image_width`](Self::max_image_width)
    pub fn default_width(mut self, width: Option<usize>) -> Self {
        self.options.default_width = width;
        self
    }

    /// Show alt text when hovering over images. By default this is enabled.
    pub fn show_alt_text_on_hover(mut self, show: bool) -> Self {
        self.options.show_alt_text_on_hover = show;
        self
    }

    /// Allows changing the default implicit `file://` uri scheme.
    /// This does nothing if [`explicit_image_uri_scheme`](`Self::explicit_image_uri_scheme`) is enabled
    ///
    /// # Example
    /// ```
    /// # use egui_commonmark::CommonMarkViewer;
    /// CommonMarkViewer::new().default_implicit_uri_scheme("https://example.org/");
    /// ```
    pub fn default_implicit_uri_scheme<S: Into<String>>(mut self, scheme: S) -> Self {
        self.options.default_implicit_uri_scheme = scheme.into();
        self
    }

    /// By default any image without a uri scheme such as `foo://` is assumed to
    /// be of the type `file://`. This assumption can sometimes be wrong or be done
    /// incorrectly, so if you want to always be explicit with the scheme then set
    /// this to `true`
    pub fn explicit_image_uri_scheme(mut self, use_explicit: bool) -> Self {
        self.options.use_explicit_uri_scheme = use_explicit;
        self
    }

    #[cfg(feature = "better_syntax_highlighting")]
    /// Set the syntax theme to be used inside code blocks in light mode
    pub fn syntax_theme_light<S: Into<String>>(mut self, theme: S) -> Self {
        self.options.theme_light = theme.into();
        self
    }

    #[cfg(feature = "better_syntax_highlighting")]
    /// Set the syntax theme to be used inside code blocks in dark mode
    pub fn syntax_theme_dark<S: Into<String>>(mut self, theme: S) -> Self {
        self.options.theme_dark = theme.into();
        self
    }

    /// Specify what kind of alerts are supported. This can also be used to localize alerts.
    ///
    /// By default [github flavoured markdown style alerts](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax#alerts)
    /// are used
    pub fn alerts(mut self, alerts: AlertBundle) -> Self {
        self.options.alerts = alerts;
        self
    }

    /// Allows rendering math. This has to be done manually as you might want a different
    /// implementation for the web and native.
    ///
    /// The example is template code for rendering a svg image. Make sure to enable the
    /// `egui_extras/svg` feature for the result to show up.
    ///
    /// ## Example
    ///
    /// ```
    /// # use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};
    /// # use egui_commonmark::CommonMarkViewer;
    /// let mut math_images = Rc::new(RefCell::new(HashMap::new()));
    /// CommonMarkViewer::new()
    ///     .render_math_fn(Some(&move |ui, math, inline| {
    ///         let mut map = math_images.borrow_mut();
    ///         let svg = map
    ///             .entry(math.to_string())
    ///             .or_insert_with(|| {
    ///                 if inline {
    ///                     // render as inline
    ///                     // dummy data for the example
    ///                     Arc::new([0])
    ///                 } else {
    ///                     Arc::new([0])
    ///                 }
    ///             });
    ///
    ///     let uri = format!("{}.svg", egui::Id::from(math.to_string()).value());
    ///     ui.add(
    ///          egui::Image::new(egui::ImageSource::Bytes {
    ///             uri: uri.into(),
    ///             bytes: egui::load::Bytes::Shared(svg.clone()),
    ///          })
    ///          .fit_to_original_size(1.0),
    ///     );
    ///     }));
    /// ```
    pub fn render_math_fn(mut self, func: Option<&'f RenderMathFn>) -> Self {
        self.options.math_fn = func;
        self
    }

    /// Allows custom handling of html. Enabling this will disable plain text rendering
    /// of html blocks. Nodes are included in the provided text
    pub fn render_html_fn(mut self, func: Option<&'f RenderHtmlFn>) -> Self {
        self.options.html_fn = func;
        self
    }

    /// Use the application's own selectable code block and syntax colors.
    pub fn render_code_block_fn(mut self, func: Option<&'f RenderCodeBlockFn>) -> Self {
        self.code_block_fn = func;
        self
    }

    pub fn render_block_fn(mut self, func: Option<&'f RenderBlockFn>) -> Self {
        self.block_fn = func;
        self
    }

    pub fn measure_math_fn(mut self, func: Option<&'f MeasureMathFn>) -> Self {
        self.measure_math_fn = func;
        self
    }

    pub fn show_prepared(
        self,
        ui: &mut egui::Ui,
        cache: &mut CommonMarkCache,
        prepared: &PreparedMarkdown,
    ) -> egui::InnerResponse<()> {
        egui_commonmark_backend::prepare_show(cache, ui.ctx());
        parsers::document::Renderer::new(
            &prepared.events,
            cache,
            &self.options,
            self.code_block_fn,
            self.block_fn,
            self.measure_math_fn,
        )
        .show(ui, &prepared.document)
        .0
    }

    /// Shows rendered markdown
    pub fn show(
        self,
        ui: &mut egui::Ui,
        cache: &mut CommonMarkCache,
        text: &str,
    ) -> egui::InnerResponse<()> {
        let prepared = PreparedMarkdown::new(text, self.options.math_fn.is_some());
        self.show_prepared(ui, cache, &prepared)
    }

    /// Shows rendered markdown, and allows the rendered ui to mutate the source text.
    ///
    /// The only currently implemented mutation is allowing checkboxes to be toggled through the ui.
    pub fn show_mut(
        mut self,
        ui: &mut egui::Ui,
        cache: &mut CommonMarkCache,
        text: &mut String,
    ) -> egui::InnerResponse<()> {
        self.options.mutable = true;
        egui_commonmark_backend::prepare_show(cache, ui.ctx());

        let prepared = PreparedMarkdown::new(text, self.options.math_fn.is_some());
        let (response, checkmark_events) = parsers::document::Renderer::new(
            &prepared.events,
            cache,
            &self.options,
            self.code_block_fn,
            self.block_fn,
            self.measure_math_fn,
        )
        .show(ui, &prepared.document);

        // Update source text for checkmarks that were clicked
        for ev in checkmark_events {
            if ev.checked {
                text.replace_range(ev.span, "[x]")
            } else {
                text.replace_range(ev.span, "[ ]")
            }
        }

        response
    }

    /// Convenience wrapper for a vertically scrolling document. Reuse a
    /// `PreparedMarkdown` with `show_prepared` when the source is unchanged.
    #[cfg(feature = "pulldown_cmark")]
    pub fn show_scrollable(
        self,
        source_id: impl std::hash::Hash,
        ui: &mut egui::Ui,
        cache: &mut CommonMarkCache,
        text: &str,
    ) {
        egui::ScrollArea::vertical()
            .id_salt(Id::new(source_id))
            .show(ui, |ui| self.show(ui, cache, text));
    }
}
