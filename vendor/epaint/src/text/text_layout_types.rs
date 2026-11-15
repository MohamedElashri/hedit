#![allow(clippy::derived_hash_with_manual_eq)] // We need to impl Hash for f32, but we don't implement Eq, which is fine
#![allow(clippy::wrong_self_convention)] // We use `from_` to indicate conversion direction. It's non-diomatic, but makes sense in this context.

use std::ops::Range;
use std::sync::Arc;

use super::{
    cursor::{CCursor, Cursor, PCursor, RCursor},
    font::UvRect,
};
use crate::{Color32, FontId, Mesh, Stroke};
use emath::{pos2, vec2, Align, NumExt, OrderedFloat, Pos2, Rect, Vec2};

/// Describes the task of laying out text.
///
/// This supports mixing different fonts, color and formats (underline etc).
///
/// Pass this to [`crate::Fonts::layout_job`] or [`crate::text::layout`].
///
/// ## Example:
/// ```
/// use epaint::{Color32, text::{LayoutJob, TextFormat}, FontFamily, FontId};
///
/// let mut job = LayoutJob::default();
/// job.append(
///     "Hello ",
///     0.0,
///     TextFormat {
///         font_id: FontId::new(14.0, FontFamily::Proportional),
///         color: Color32::WHITE,
///         ..Default::default()
///     },
/// );
/// job.append(
///     "World!",
///     0.0,
///     TextFormat {
///         font_id: FontId::new(14.0, FontFamily::Monospace),
///         color: Color32::BLACK,
///         ..Default::default()
///     },
/// );
/// ```
///
/// As you can see, constructing a [`LayoutJob`] is currently a lot of work.
/// It would be nice to have a helper macro for it!
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct LayoutJob {
    /// The complete text of this job, referenced by [`LayoutSection`].
    pub text: String,

    /// The different section, which can have different fonts, colors, etc.
    pub sections: Vec<LayoutSection>,

    /// Controls the text wrapping and elision.
    pub wrap: TextWrapping,

    /// The first row must be at least this high.
    /// This is in case we lay out text that is the continuation
    /// of some earlier text (sharing the same row),
    /// in which case this will be the height of the earlier text.
    /// In other cases, set this to `0.0`.
    pub first_row_min_height: f32,

    /// If `true`, all `\n` characters will result in a new _paragraph_,
    /// starting on a new row.
    ///
    /// If `false`, all `\n` characters will be ignored
    /// and show up as the replacement character.
    ///
    /// Default: `true`.
    pub break_on_newline: bool,

    /// How to horizontally align the text (`Align::LEFT`, `Align::Center`, `Align::RIGHT`).
    pub halign: Align,

    /// Justify text so that word-wrapped rows fill the whole [`TextWrapping::max_width`].
    pub justify: bool,

    /// Round output sizes using [`emath::GuiRounding`], to avoid rounding errors in layout code.
    pub round_output_to_gui: bool,
}

impl Default for LayoutJob {
    #[inline]
    fn default() -> Self {
        Self {
            text: Default::default(),
            sections: Default::default(),
            wrap: Default::default(),
            first_row_min_height: 0.0,
            break_on_newline: true,
            halign: Align::LEFT,
            justify: false,
            round_output_to_gui: true,
        }
    }
}

impl LayoutJob {
    /// Break on `\n` and at the given wrap width.
    #[inline]
    pub fn simple(text: String, font_id: FontId, color: Color32, wrap_width: f32) -> Self {
        Self {
            sections: vec![LayoutSection {
                leading_space: 0.0,
                byte_range: 0..text.len(),
                format: TextFormat::simple(font_id, color),
            }],
            text,
            wrap: TextWrapping {
                max_width: wrap_width,
                ..Default::default()
            },
            break_on_newline: true,
            ..Default::default()
        }
    }

    /// Does not break on `\n`, but shows the replacement character instead.
    #[inline]
    pub fn simple_singleline(text: String, font_id: FontId, color: Color32) -> Self {
        Self {
            sections: vec![LayoutSection {
                leading_space: 0.0,
                byte_range: 0..text.len(),
                format: TextFormat::simple(font_id, color),
            }],
            text,
            wrap: Default::default(),
            break_on_newline: false,
            ..Default::default()
        }
    }

    #[inline]
    pub fn single_section(text: String, format: TextFormat) -> Self {
        Self {
            sections: vec![LayoutSection {
                leading_space: 0.0,
                byte_range: 0..text.len(),
                format,
            }],
            text,
            wrap: Default::default(),
            break_on_newline: true,
            ..Default::default()
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }

    /// Helper for adding a new section when building a [`LayoutJob`].
    pub fn append(&mut self, text: &str, leading_space: f32, format: TextFormat) {
        let start = self.text.len();
        self.text += text;
        let byte_range = start..self.text.len();
        self.sections.push(LayoutSection {
            leading_space,
            byte_range,
            format,
        });
    }

    /// The height of the tallest font used in the job.
    ///
    /// Returns a value rounded to [`emath::GUI_ROUNDING`].
    pub fn font_height(&self, fonts: &crate::Fonts) -> f32 {
        let mut max_height = 0.0_f32;
        for section in &self.sections {
            max_height = max_height.max(fonts.row_height(&section.format.font_id));
        }
        max_height
    }

    /// The wrap with, with a small margin in some cases.
    pub fn effective_wrap_width(&self) -> f32 {
        if self.round_output_to_gui {
            // On a previous pass we may have rounded down by at most 0.5 and reported that as a width.
            // egui may then set that width as the max width for subsequent frames, and it is important
            // that we then don't wrap earlier.
            self.wrap.max_width + 0.5
        } else {
            self.wrap.max_width
        }
    }
}

impl std::hash::Hash for LayoutJob {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let Self {
            text,
            sections,
            wrap,
            first_row_min_height,
            break_on_newline,
            halign,
            justify,
            round_output_to_gui,
        } = self;

        text.hash(state);
        sections.hash(state);
        wrap.hash(state);
        emath::OrderedFloat(*first_row_min_height).hash(state);
        break_on_newline.hash(state);
        halign.hash(state);
        justify.hash(state);
        round_output_to_gui.hash(state);
    }
}

// ----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct LayoutSection {
    /// Can be used for first row indentation.
    pub leading_space: f32,

    /// Range into the galley text
    pub byte_range: Range<usize>,

    pub format: TextFormat,
}

impl std::hash::Hash for LayoutSection {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let Self {
            leading_space,
            byte_range,
            format,
        } = self;
        OrderedFloat(*leading_space).hash(state);
        byte_range.hash(state);
        format.hash(state);
    }
}

// ----------------------------------------------------------------------------

/// Formatting option for a section of text.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct TextFormat {
    pub font_id: FontId,

    /// Extra spacing between letters, in points.
    ///
    /// Default: 0.0.
    ///
    /// For even text it is recommended you round this to an even number of _pixels_.
    pub extra_letter_spacing: f32,

    /// Explicit line height of the text in points.
    ///
    /// This is the distance between the bottom row of two subsequent lines of text.
    ///
    /// If `None` (the default), the line height is determined by the font.
    ///
    /// For even text it is recommended you round this to an even number of _pixels_.
    pub line_height: Option<f32>,

    /// Text color
    pub color: Color32,

    pub background: Color32,

    pub italics: bool,

    pub underline: Stroke,

    pub strikethrough: Stroke,

    /// If you use a small font and [`Align::TOP`] you
    /// can get the effect of raised text.
    ///
    /// If you use a small font and [`Align::BOTTOM`]
    /// you get the effect of a subscript.
    ///
    /// If you use [`Align::Center`], you get text that is centered
    /// around a common center-line, which is nice when mixining emojis
    /// and normal text in e.g. a button.
    pub valign: Align,
}

impl Default for TextFormat {
    #[inline]
    fn default() -> Self {
        Self {
            font_id: FontId::default(),
            extra_letter_spacing: 0.0,
            line_height: None,
            color: Color32::GRAY,
            background: Color32::TRANSPARENT,
            italics: false,
            underline: Stroke::NONE,
            strikethrough: Stroke::NONE,
            valign: Align::BOTTOM,
        }
    }
}

impl std::hash::Hash for TextFormat {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let Self {
            font_id,
            extra_letter_spacing,
            line_height,
            color,
            background,
            italics,
            underline,
            strikethrough,
            valign,
        } = self;
        font_id.hash(state);
        emath::OrderedFloat(*extra_letter_spacing).hash(state);
        if let Some(line_height) = *line_height {
            emath::OrderedFloat(line_height).hash(state);
        }
        color.hash(state);
        background.hash(state);
        italics.hash(state);
        underline.hash(state);
        strikethrough.hash(state);
        valign.hash(state);
    }
}

impl TextFormat {
    #[inline]
    pub fn simple(font_id: FontId, color: Color32) -> Self {
        Self {
            font_id,
            color,
            ..Default::default()
        }
    }
}

// ----------------------------------------------------------------------------

/// How to wrap and elide text.
///
/// This enum is used in high-level APIs where providing a [`TextWrapping`] is too verbose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum TextWrapMode {
    /// The text should expand the `Ui` size when reaching its boundary.
    Extend,

    /// The text should wrap to the next line when reaching the `Ui` boundary.
    Wrap,

    /// The text should be elided using "…" when reaching the `Ui` boundary.
    ///
    /// Note that using [`TextWrapping`] and [`LayoutJob`] offers more control over the elision.
    Truncate,
}

/// Controls the text wrapping and elision of a [`LayoutJob`].
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct TextWrapping {
    /// Wrap text so that no row is wider than this.
    ///
    /// If you would rather truncate text that doesn't fit, set [`Self::max_rows`] to `1`.
    ///
    /// Set `max_width` to [`f32::INFINITY`] to turn off wrapping and elision.
    ///
    /// Note that `\n` always produces a new row
    /// if [`LayoutJob::break_on_newline`] is `true`.
    pub max_width: f32,

    /// Maximum amount of rows the text galley should have.
    ///
    /// If this limit is reached, text will be truncated
    /// and [`Self::overflow_character`] appended to the final row.
    /// You can detect this by checking [`Galley::elided`].
    ///
    /// If set to `0`, no text will be outputted.
    ///
    /// If set to `1`, a single row will be outputted,
    /// eliding the text after [`Self::max_width`] is reached.
    /// When you set `max_rows = 1`, it is recommended you also set [`Self::break_anywhere`] to `true`.
    ///
    /// Default value: `usize::MAX`.
    pub max_rows: usize,

    /// If `true`: Allow breaking between any characters.
    /// If `false` (default): prefer breaking between words, etc.
    ///
    /// NOTE: Due to limitations in the current implementation,
    /// when truncating text using [`Self::max_rows`] the text may be truncated
    /// in the middle of a word even if [`Self::break_anywhere`] is `false`.
    /// Therefore it is recommended to set [`Self::break_anywhere`] to `true`
    /// whenever [`Self::max_rows`] is set to `1`.
    pub break_anywhere: bool,

    /// Character to use to represent elided text.
    ///
    /// The default is `…`.
    ///
    /// If not set, no character will be used (but the text will still be elided).
    pub overflow_character: Option<char>,
}

impl std::hash::Hash for TextWrapping {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let Self {
            max_width,
            max_rows,
            break_anywhere,
            overflow_character,
        } = self;
        emath::OrderedFloat(*max_width).hash(state);
        max_rows.hash(state);
        break_anywhere.hash(state);
        overflow_character.hash(state);
    }
}

impl Default for TextWrapping {
    fn default() -> Self {
        Self {
            max_width: f32::INFINITY,
            max_rows: usize::MAX,
            break_anywhere: false,
            overflow_character: Some('…'),
        }
    }
}

impl TextWrapping {
    /// Create a [`TextWrapping`] from a [`TextWrapMode`] and an available width.
    pub fn from_wrap_mode_and_width(mode: TextWrapMode, max_width: f32) -> Self {
        match mode {
            TextWrapMode::Extend => Self::no_max_width(),
            TextWrapMode::Wrap => Self::wrap_at_width(max_width),
            TextWrapMode::Truncate => Self::truncate_at_width(max_width),
        }
    }

    /// A row can be as long as it need to be.
    pub fn no_max_width() -> Self {
        Self {
            max_width: f32::INFINITY,
            ..Default::default()
        }
    }

    /// A row can be at most `max_width` wide but can wrap in any number of lines.
    pub fn wrap_at_width(max_width: f32) -> Self {
        Self {
            max_width,
            ..Default::default()
        }
    }

    /// Elide text that doesn't fit within the given width, replaced with `…`.
    pub fn truncate_at_width(max_width: f32) -> Self {
        Self {
            max_width,
            max_rows: 1,
            break_anywhere: true,
            ..Default::default()
        }
    }
}

// ----------------------------------------------------------------------------

/// Text that has been laid out, ready for painting.
///
/// You can create a [`Galley`] using [`crate::Fonts::layout_job`];
///
/// Needs to be recreated if the underlying font atlas texture changes, which
/// happens under the following conditions:
/// - `pixels_per_point` or `max_texture_size` change. These parameters are set
///   in [`crate::text::Fonts::begin_pass`]. When using `egui` they are set
///   from `egui::InputState` and can change at any time.
/// - The atlas has become full. This can happen any time a new glyph is added
///   to the atlas, which in turn can happen any time new text is laid out.
///
/// The name comes from typography, where a "galley" is a metal tray
/// containing a column of set type, usually the size of a page of text.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Galley {
    /// The job that this galley is the result of.
    /// Contains the original string and style sections.
    pub job: Arc<LayoutJob>,

    /// Rows of text, from top to bottom.
    ///
    /// The number of characters in all rows sum up to `job.text.chars().count()`
    /// unless [`Self::elided`] is `true`.
    ///
    /// Note that a paragraph (a piece of text separated with `\n`)
    /// can be split up into multiple rows.
    pub rows: Vec<Arc<Row>>,

    /// Derived cursor metadata. Reset with `invalidate_cursor_index` after
    /// changing row topology or glyph/row geometry; color-only changes preserve it.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub cursor_cache: super::CursorIndexCache,

    /// Set to true the text was truncated due to [`TextWrapping::max_rows`].
    pub elided: bool,

    /// Bounding rect.
    ///
    /// `rect.top()` is always 0.0.
    ///
    /// With [`LayoutJob::halign`]:
    /// * [`Align::LEFT`]: `rect.left() == 0.0`
    /// * [`Align::Center`]: `rect.center() == 0.0`
    /// * [`Align::RIGHT`]: `rect.right() == 0.0`
    pub rect: Rect,

    /// Tight bounding box around all the meshes in all the rows.
    /// Can be used for culling.
    pub mesh_bounds: Rect,

    /// Total number of vertices in all the row meshes.
    pub num_vertices: usize,

    /// Total number of indices in all the row meshes.
    pub num_indices: usize,

    /// The number of physical pixels for each logical point.
    /// Since this affects the layout, we keep track of it
    /// so that we can warn if this has changed once we get to
    /// tessellation.
    pub pixels_per_point: f32,
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Row {
    /// Unrounded layout bounds, retained for incremental paragraph placement.
    pub layout_rect: Rect,
    /// This is included in case there are no glyphs
    pub section_index_at_start: u32,

    /// One for each `char`.
    pub glyphs: Vec<Glyph>,

    /// Logical bounding rectangle based on font heights etc.
    /// Use this when drawing a selection or similar!
    /// Includes leading and trailing whitespace.
    pub rect: Rect,

    /// The mesh, ready to be rendered.
    pub visuals: RowVisuals,

    /// If true, this [`Row`] came from a paragraph ending with a `\n`.
    /// The `\n` itself is omitted from [`Self::glyphs`].
    /// A `\n` in the input text always creates a new [`Row`] below it,
    /// so that text that ends with `\n` has an empty [`Row`] last.
    /// This also implies that the last [`Row`] in a [`Galley`] always has `ends_with_newline == false`.
    pub ends_with_newline: bool,
}

/// The tessellated output of a row.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct RowVisuals {
    /// Derived triangle bounds. Invalidate after mesh geometry/index changes.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub mesh_cache: super::TextMeshCache,
    /// The tessellated text, using non-normalized (texel) UV coordinates.
    /// That is, you need to divide the uv coordinates by the texture size.
    pub mesh: Arc<Mesh>,

    /// Translation applied when painting the shared mesh. Glyph and row
    /// coordinates retain their normal galley coordinates.
    #[cfg_attr(feature = "serde", serde(default))]
    pub mesh_offset: Vec2,

    /// Bounds in galley coordinates, including `mesh_offset`, for culling.
    /// Does NOT include leading or trailing whitespace glyphs!!
    pub mesh_bounds: Rect,

    /// The number of triangle indices added before the first glyph triangle.
    ///
    /// This can be used to insert more triangles after the background but before the glyphs,
    /// i.e. for text selection visualization.
    pub glyph_index_start: usize,

    /// The range of vertices in the mesh that contain glyphs (as opposed to background, underlines, strikethorugh, etc).
    ///
    /// The glyph vertices comes after backgrounds (if any), but before any underlines and strikethrough.
    pub glyph_vertex_range: Range<usize>,
}

impl Default for RowVisuals {
    fn default() -> Self {
        Self {
            mesh_cache: Default::default(),
            mesh: {
                // Default visuals are placeholders while rows are laid out.
                // Share an immutable empty mesh instead of allocating one Arc
                // for every empty row or short-lived placeholder.
                static EMPTY: std::sync::OnceLock<Arc<Mesh>> = std::sync::OnceLock::new();
                Arc::clone(EMPTY.get_or_init(|| Arc::new(Mesh::default())))
            },
            mesh_offset: Vec2::ZERO,
            mesh_bounds: Rect::NOTHING,
            glyph_index_start: 0,
            glyph_vertex_range: 0..0,
        }
    }
}

impl RowVisuals {
    /// Move geometry without copying its immutable mesh or spatial index.
    pub fn translate_mesh(&mut self, delta: Vec2) {
        self.mesh_offset += delta;
        self.mesh_bounds = self.mesh_bounds.translate(delta);
    }

    /// Materialize any translation before changing vertices or indices.
    /// The caller updates `mesh_bounds` after its mutation.
    pub fn mesh_mut(&mut self) -> &mut Mesh {
        self.mesh_cache.invalidate();
        let mesh = Arc::make_mut(&mut self.mesh);
        if self.mesh_offset != Vec2::ZERO {
            mesh.translate(self.mesh_offset);
            self.mesh_offset = Vec2::ZERO;
        }
        mesh
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Glyph {
    /// The character this glyph represents.
    pub chr: char,

    /// Baseline position, relative to the galley.
    /// Logical position: pos.y is the same for all chars of the same [`TextFormat`].
    pub pos: Pos2,

    /// Logical width of the glyph.
    pub advance_width: f32,

    /// Height of this row of text.
    ///
    /// Usually same as [`Self::font_height`],
    /// unless explicitly overridden by [`TextFormat::line_height`].
    pub line_height: f32,

    /// The ascent of this font.
    pub font_ascent: f32,

    /// The row/line height of this font.
    pub font_height: f32,

    /// The ascent of the sub-font within the font (`FontImpl`).
    pub font_impl_ascent: f32,

    /// The row/line height of the sub-font within the font (`FontImpl`).
    pub font_impl_height: f32,

    /// Position and size of the glyph in the font texture, in texels.
    pub uv_rect: UvRect,

    /// Index into [`LayoutJob::sections`]. Decides color etc.
    pub section_index: u32,
}

impl Glyph {
    #[inline]
    pub fn size(&self) -> Vec2 {
        Vec2::new(self.advance_width, self.line_height)
    }

    #[inline]
    pub fn max_x(&self) -> f32 {
        self.pos.x + self.advance_width
    }

    /// Same y range for all characters with the same [`TextFormat`].
    #[inline]
    pub fn logical_rect(&self) -> Rect {
        Rect::from_min_size(self.pos - vec2(0.0, self.font_ascent), self.size())
    }
}

// ----------------------------------------------------------------------------

impl Row {
    /// The text on this row, excluding the implicit `\n` if any.
    pub fn text(&self) -> String {
        self.glyphs.iter().map(|g| g.chr).collect()
    }

    /// Excludes the implicit `\n` after the [`Row`], if any.
    #[inline]
    pub fn char_count_excluding_newline(&self) -> usize {
        self.glyphs.len()
    }

    /// Includes the implicit `\n` after the [`Row`], if any.
    #[inline]
    pub fn char_count_including_newline(&self) -> usize {
        self.glyphs.len() + (self.ends_with_newline as usize)
    }

    #[inline]
    pub fn min_y(&self) -> f32 {
        self.rect.top()
    }

    #[inline]
    pub fn max_y(&self) -> f32 {
        self.rect.bottom()
    }

    #[inline]
    pub fn height(&self) -> f32 {
        self.rect.height()
    }

    /// Closest char at the desired x coordinate.
    /// Returns something in the range `[0, char_count_excluding_newline()]`.
    pub fn char_at(&self, desired_x: f32) -> usize {
        for (i, glyph) in self.glyphs.iter().enumerate() {
            if desired_x < glyph.logical_rect().center().x {
                return i;
            }
        }
        self.char_count_excluding_newline()
    }

    pub fn x_offset(&self, column: usize) -> f32 {
        if let Some(glyph) = self.glyphs.get(column) {
            glyph.pos.x
        } else {
            self.rect.right()
        }
    }
}

impl Galley {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.job.is_empty()
    }

    /// The full, non-elided text of the input job.
    #[inline]
    pub fn text(&self) -> &str {
        &self.job.text
    }

    #[inline]
    pub fn size(&self) -> Vec2 {
        self.rect.size()
    }
}

impl AsRef<str> for Galley {
    #[inline]
    fn as_ref(&self) -> &str {
        self.text()
    }
}

impl std::borrow::Borrow<str> for Galley {
    #[inline]
    fn borrow(&self) -> &str {
        self.text()
    }
}

impl std::ops::Deref for Galley {
    type Target = str;
    #[inline]
    fn deref(&self) -> &str {
        self.text()
    }
}

// ----------------------------------------------------------------------------

/// ## Physical positions
impl Galley {
    /// Zero-width rect past the last character.
    fn end_pos(&self) -> Rect {
        if let Some(row) = self.rows.last() {
            let x = row.rect.right();
            Rect::from_min_max(pos2(x, row.min_y()), pos2(x, row.max_y()))
        } else {
            // Empty galley
            Rect::from_min_max(pos2(0.0, 0.0), pos2(0.0, 0.0))
        }
    }

    /// Returns a 0-width Rect.
    pub fn pos_from_cursor(&self, cursor: &Cursor) -> Rect {
        self.pos_from_pcursor(cursor.pcursor) // pcursor is what TextEdit stores
    }

    /// Returns a 0-width Rect.
    pub fn pos_from_pcursor(&self, pcursor: PCursor) -> Rect {
        let cursor = self.from_pcursor(pcursor);
        if let Some(row) = self.rows.get(cursor.rcursor.row) {
            let x = row.x_offset(cursor.rcursor.column);
            Rect::from_min_max(pos2(x, row.min_y()), pos2(x, row.max_y()))
        } else {
            self.end_pos()
        }
    }

    /// Returns a 0-width Rect.
    pub fn pos_from_ccursor(&self, ccursor: CCursor) -> Rect {
        self.pos_from_cursor(&self.from_ccursor(ccursor))
    }

    /// Returns a 0-width Rect.
    pub fn pos_from_rcursor(&self, rcursor: RCursor) -> Rect {
        self.pos_from_cursor(&self.from_rcursor(rcursor))
    }

    /// Cursor at the given position within the galley.
    ///
    /// A cursor above the galley is considered
    /// same as a cursor at the start,
    /// and a cursor below the galley is considered
    /// same as a cursor at the end.
    /// This allows implementing text-selection by dragging above/below the galley.
    pub fn cursor_from_pos(&self, pos: Vec2) -> Cursor {
        if let Some(first_row) = self.rows.first() {
            if pos.y < first_row.min_y() {
                return self.begin();
            }
        }
        if let Some(last_row) = self.rows.last() {
            if last_row.max_y() < pos.y {
                return self.end();
            }
        }

        if self.cursor_index().disjoint && pos.y.is_finite() {
            let mut row_nr = self.rows.partition_point(|row| row.max_y() < pos.y);
            row_nr = row_nr.min(self.rows.len().saturating_sub(1));
            if let Some(row) = self.rows.get(row_nr) {
                if row_nr > 0 && pos.y < row.min_y() {
                    let previous = &self.rows[row_nr - 1];
                    if (pos.y - previous.max_y()).abs() <= (row.min_y() - pos.y).abs() {
                        // Equal endpoints can belong to several zero-height rows.
                        // The original walk keeps the earliest equal-distance row.
                        let bottom = previous.max_y();
                        row_nr = self.rows.partition_point(|row| row.max_y() < bottom);
                    }
                }
                let row = &self.rows[row_nr];
                let column = self.column_at(row_nr, pos.x);
                let at = self.cursor_index().rows[row_nr];
                let prefer_next_row = column < row.char_count_excluding_newline();
                return Cursor {
                    ccursor: CCursor {
                        index: at.start + column,
                        prefer_next_row,
                    },
                    rcursor: RCursor {
                        row: row_nr,
                        column,
                    },
                    pcursor: PCursor {
                        paragraph: at.paragraph,
                        offset: at.offset + column,
                        prefer_next_row,
                    },
                };
            }
        }

        let mut best_y_dist = f32::INFINITY;
        let mut cursor = Cursor::default();

        let mut ccursor_index = 0;
        let mut pcursor_it = PCursor::default();

        for (row_nr, row) in self.rows.iter().enumerate() {
            let is_pos_within_row = row.min_y() <= pos.y && pos.y <= row.max_y();
            let y_dist = (row.min_y() - pos.y).abs().min((row.max_y() - pos.y).abs());
            if is_pos_within_row || y_dist < best_y_dist {
                best_y_dist = y_dist;
                let column = self.column_at(row_nr, pos.x);
                let prefer_next_row = column < row.char_count_excluding_newline();
                cursor = Cursor {
                    ccursor: CCursor {
                        index: ccursor_index + column,
                        prefer_next_row,
                    },
                    rcursor: RCursor {
                        row: row_nr,
                        column,
                    },
                    pcursor: PCursor {
                        paragraph: pcursor_it.paragraph,
                        offset: pcursor_it.offset + column,
                        prefer_next_row,
                    },
                };

                if is_pos_within_row {
                    return cursor;
                }
            }
            ccursor_index += row.char_count_including_newline();
            if row.ends_with_newline {
                pcursor_it.paragraph += 1;
                pcursor_it.offset = 0;
            } else {
                pcursor_it.offset += row.char_count_including_newline();
            }
        }

        cursor
    }
}

/// ## Cursor positions
impl Galley {
    /// Cursor to the first character.
    ///
    /// This is the same as [`Cursor::default`].
    #[inline]
    #[allow(clippy::unused_self)]
    pub fn begin(&self) -> Cursor {
        Cursor::default()
    }

    /// Cursor to one-past last character.
    pub fn end(&self) -> Cursor {
        if self.rows.is_empty() {
            return Cursor::default();
        }
        let end = self.cursor_index().end;
        Cursor {
            ccursor: CCursor {
                index: end.start,
                prefer_next_row: true,
            },
            rcursor: self.end_rcursor(),
            pcursor: PCursor {
                paragraph: end.paragraph,
                offset: end.offset,
                prefer_next_row: true,
            },
        }
    }

    pub fn end_rcursor(&self) -> RCursor {
        if let Some(last_row) = self.rows.last() {
            RCursor {
                row: self.rows.len() - 1,
                column: last_row.char_count_including_newline(),
            }
        } else {
            Default::default()
        }
    }
}

/// ## Cursor conversions
impl Galley {
    // The returned cursor is clamped.
    pub fn from_ccursor(&self, ccursor: CCursor) -> Cursor {
        let index = self.cursor_index();
        let mut low = 0;
        let mut high = self.rows.len();
        while low < high {
            let row = low + (high - low) / 2;
            let end = index.rows[row].start + self.rows[row].char_count_excluding_newline();
            let skip = end < ccursor.index
                || end == ccursor.index
                    && ccursor.prefer_next_row
                    && !self.rows[row].ends_with_newline;
            if skip {
                low = row + 1;
            } else {
                high = row;
            }
        }
        let row = low;
        if let Some(at) = index.rows.get(row) {
            let column = ccursor.index - at.start;
            return Cursor {
                ccursor,
                rcursor: RCursor { row, column },
                pcursor: PCursor {
                    paragraph: at.paragraph,
                    offset: at.offset + column,
                    prefer_next_row: ccursor.prefer_next_row,
                },
            };
        }
        Cursor {
            ccursor: CCursor {
                index: index.end.start,
                prefer_next_row: ccursor.prefer_next_row,
            },
            rcursor: self.end_rcursor(),
            pcursor: PCursor {
                paragraph: index.end.paragraph,
                offset: index.end.offset,
                prefer_next_row: ccursor.prefer_next_row,
            },
        }
    }

    pub fn from_rcursor(&self, rcursor: RCursor) -> Cursor {
        let Some(row) = self.rows.get(rcursor.row) else {
            return self.end();
        };
        let at = self.cursor_index().rows[rcursor.row];
        let column = rcursor.column.at_most(row.char_count_excluding_newline());
        let prefer_next_row = rcursor.column < row.char_count_excluding_newline();
        Cursor {
            ccursor: CCursor {
                index: at.start + column,
                prefer_next_row,
            },
            rcursor,
            pcursor: PCursor {
                paragraph: at.paragraph,
                offset: at.offset
                    + if row.ends_with_newline {
                        rcursor.column
                    } else {
                        column
                    },
                prefer_next_row,
            },
        }
    }

    // TODO(emilk): return identical cursor, or clamp?
    pub fn from_pcursor(&self, pcursor: PCursor) -> Cursor {
        let index = self.cursor_index();
        let mut low = index
            .rows
            .partition_point(|at| at.paragraph < pcursor.paragraph);
        let mut high = index
            .rows
            .partition_point(|at| at.paragraph <= pcursor.paragraph);
        let end = high;
        while low < high {
            let r = low + (high - low) / 2;
            let row = &self.rows[r];
            let offset_end = index.rows[r].offset + row.char_count_excluding_newline();
            let skip = !row.ends_with_newline
                && (offset_end < pcursor.offset
                    || offset_end == pcursor.offset && pcursor.prefer_next_row);
            if skip {
                low = r + 1;
            } else {
                high = r;
            }
        }
        if low < end {
            let row = &self.rows[low];
            let at = index.rows[low];
            let column = pcursor.offset - at.offset;
            return Cursor {
                ccursor: CCursor {
                    index: at.start + column.at_most(row.char_count_excluding_newline()),
                    prefer_next_row: pcursor.prefer_next_row,
                },
                rcursor: RCursor { row: low, column },
                pcursor,
            };
        }
        Cursor {
            ccursor: CCursor {
                index: index.end.start,
                prefer_next_row: pcursor.prefer_next_row,
            },
            rcursor: self.end_rcursor(),
            pcursor,
        }
    }
}

/// ## Cursor positions
impl Galley {
    pub fn cursor_left_one_character(&self, cursor: &Cursor) -> Cursor {
        if cursor.ccursor.index == 0 {
            Default::default()
        } else {
            let ccursor = CCursor {
                index: cursor.ccursor.index,
                prefer_next_row: true, // default to this when navigating. It is more often useful to put cursor at the begging of a row than at the end.
            };
            self.from_ccursor(ccursor - 1)
        }
    }

    pub fn cursor_right_one_character(&self, cursor: &Cursor) -> Cursor {
        let ccursor = CCursor {
            index: cursor.ccursor.index,
            prefer_next_row: true, // default to this when navigating. It is more often useful to put cursor at the begging of a row than at the end.
        };
        self.from_ccursor(ccursor + 1)
    }

    pub fn cursor_up_one_row(&self, cursor: &Cursor) -> Cursor {
        if cursor.rcursor.row == 0 {
            Cursor::default()
        } else {
            let new_row = cursor.rcursor.row - 1;

            let cursor_is_beyond_end_of_current_row = cursor.rcursor.column
                >= self.rows[cursor.rcursor.row].char_count_excluding_newline();

            let new_rcursor = if cursor_is_beyond_end_of_current_row {
                // keep same column
                RCursor {
                    row: new_row,
                    column: cursor.rcursor.column,
                }
            } else {
                // keep same X coord
                let x = self.pos_from_cursor(cursor).center().x;
                let column = if x > self.rows[new_row].rect.right() {
                    // beyond the end of this row - keep same column
                    cursor.rcursor.column
                } else {
                    self.column_at(new_row, x)
                };
                RCursor {
                    row: new_row,
                    column,
                }
            };
            self.from_rcursor(new_rcursor)
        }
    }

    pub fn cursor_down_one_row(&self, cursor: &Cursor) -> Cursor {
        if cursor.rcursor.row + 1 < self.rows.len() {
            let new_row = cursor.rcursor.row + 1;

            let cursor_is_beyond_end_of_current_row = cursor.rcursor.column
                >= self.rows[cursor.rcursor.row].char_count_excluding_newline();

            let new_rcursor = if cursor_is_beyond_end_of_current_row {
                // keep same column
                RCursor {
                    row: new_row,
                    column: cursor.rcursor.column,
                }
            } else {
                // keep same X coord
                let x = self.pos_from_cursor(cursor).center().x;
                let column = if x > self.rows[new_row].rect.right() {
                    // beyond the end of the next row - keep same column
                    cursor.rcursor.column
                } else {
                    self.column_at(new_row, x)
                };
                RCursor {
                    row: new_row,
                    column,
                }
            };

            self.from_rcursor(new_rcursor)
        } else {
            self.end()
        }
    }

    pub fn cursor_begin_of_row(&self, cursor: &Cursor) -> Cursor {
        self.from_rcursor(RCursor {
            row: cursor.rcursor.row,
            column: 0,
        })
    }

    pub fn cursor_end_of_row(&self, cursor: &Cursor) -> Cursor {
        self.from_rcursor(RCursor {
            row: cursor.rcursor.row,
            column: self.rows[cursor.rcursor.row].char_count_excluding_newline(),
        })
    }
}
