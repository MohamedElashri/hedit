# Local egui_commonmark patch

Based on egui_commonmark 0.20.0 from crates.io, upstream:
https://github.com/lampsitter/egui_commonmark

The original MIT and Apache-2.0 licenses are retained. This patch replaces the
upstream block layout with a native document renderer modeled on
[VS Code's Markdown preview](https://github.com/microsoft/vscode/blob/main/extensions/markdown-language-features/media/markdown.css).
It does not execute HTML or require a webview.

`PreparedMarkdown` owns the original parser events and a bounded document tree,
prepared once per source revision. `parsers/document.rs` lays out vertical blocks
with explicit margins: paragraphs, heading rules, nested lists, task markers,
quotes, alerts, tables, code fences, and equations. Tree preparation limits
recursion; excessively nested containers fall back to readable inline content.
Comments have no layout footprint. `<br>` inserts a break; other HTML stays
literal. The optional scrollable API wraps the same renderer in a scroll area.

Tables measure intrinsic content widths and distribute available space between
minimum and preferred column widths. Each row shares the same top-aligned cell
geometry and reserves its tallest cell's height. Empty cells retain width;
images stay bounded by their cells. Narrow tables keep natural width, long cells
wrap, and wide tables own a local horizontal scroll area. Horizontal separators
replace striped rows and outer panels. Text and single-line media honor column
alignment.

`parsers/inline.rs` retains one continuous native galley for text, emphasis, code,
links, and punctuation. Nested formatting restores its enclosing style. Real
semibold and italic font families are supplied by hedit. Inline code inherits
the surrounding type size, compensates for the monospace ascent, and pads
code-only wrapped rows to retain the paragraph line height. Glyph positions,
meshes, link hit targets, and selection rows share the adjusted geometry.
Native Labels own text selection and copying across document blocks.

Links retain per-row hit targets, keyboard activation, accessibility information,
a hover/focus underline, and a Copy link address context action. Application
link hooks still handle section and local-file navigation. Linked images are
clickable. Native math metrics provide width, height, and baseline reservation
before laying out a mixed inline flow; wide figures retain local overflow.

The application supplies code-block rendering, fonts, theme colors, and media
callbacks. Code blocks use native selectable text with syntax colors, natural
vertical height, horizontal scrolling, and a hover/focus Copy overlay.
Navigation callbacks still receive original block source spans and heading
levels. Parser types are re-exported for shared Contents, links, background
preparation, and split-scroll indexing. No synthetic markers enter source text.
The mutable viewer still applies task-checkbox edits to original source spans.

Regression geometry, interactions, media lifecycle, and visual audit fixtures
live in hedit's `src/app/markdown_tests.rs`, `markdown_reader_tests.rs`,
`markdown_layout_tests.rs`, and `markdown_document_tests.rs`. Review light, dark,
compact, and split output when changing layout. Keep this patch until an
upstream renderer compatible with our egui version meets these contracts.
