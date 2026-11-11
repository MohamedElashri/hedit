# Local egui_commonmark patch

Based on egui_commonmark 0.20.0 from crates.io, upstream:
https://github.com/lampsitter/egui_commonmark

In `src/parsers/pulldown.rs`, blockquotes and alerts route their collected events
through `process_event` rather than calling `event` directly. This gives nested
list items, tables, and code blocks their normal rendering lifecycle. In
particular, list-item state must not escape a quote and collect unrelated later
paragraphs into one enormous row. Quote collection tracks nesting depth, and a
quote claims its processing flag before recursing so children are not accidentally
wrapped in another copy of their parent. The enclosing quote style is restored
after a nested quote finishes.

The optional scrollable API and its imports are gated with `pulldown_cmark`,
matching the public API feature gate and avoiding unused-code warnings when the
feature is disabled.

Regression coverage is in `src/app/markdown_tests.rs` in the hedit repository.
Keep this patch until an upstream version compatible with our egui version
includes the fix. The original MIT and Apache-2.0 licenses are retained.

Stroke widths use explicit `f32` literals for compatibility with newer Rust
compilers' float inference checks.

The reader also uses real semibold text, explicit link colors and underlines,
consistent line heights, wrapping table cells, and an opt-in code-block rendering
callback. Paragraph boundaries finish the current wrapped row once and reset its
height, instead of allocating blank two-line labels. List rows use the same break
logic. Quote text retains normal contrast inside a padded panel with an accent
rule. `app/markdown_style.rs` supplies hedit's shared typography and native code
blocks, including its own theme-aware, frame-cached highlighter and Copy button.

The reader now accepts `PreparedMarkdown`, an owned event stream that can be
parsed on a worker and reused across frames. Navigation callbacks receive the
original source span of each block and its heading level. hedit uses this single
parser for contents, links, rendering, and split scrolling; it no longer injects
HTML anchors, figure placeholders, or extra paragraphs into Markdown source.
The parser types are re-exported so applications do not need a second parser
version for indexing the same document.

Inline math is measured before paragraph, list-item, and table-cell layout.
Its ascent and descent reserve space before any preceding text is drawn, and
the application retains the actual TeX baseline when painting the equation.
Headings and loose list paragraphs always finish their row. Tables own a local
horizontal scroll area, HTML comments are hidden, and inline `<br>` elements
insert a line break. Regression coverage includes wide figures, nested math,
contents navigation, and media lifecycle tests in `app/markdown_reader_tests.rs`.

Text, emphasis, inline code, links, and punctuation share a continuous native
text layout within each inline run. Links retain per-row hit targets, native
selection/copying, keyboard activation, and accessibility information. A link
ending near the right margin cannot leave its following comma on a new line
solely because the link and punctuation were separate widgets.

Inline code inherits its surrounding paragraph or heading size. Its line box
compensates for the monospace font's ascent, aligning code with the prose
baseline while centering the background within the shared line. Regression
geometry and visual audit fixtures live in `app/markdown_layout_tests.rs`.

Tables allocate fixed-width, top-aligned cells inside rows that reserve the
full height of their tallest cell. Empty cells retain their width, parser cell
delimiters add no widgets, and the backend's trailing empty row is discarded.
Text cells honor Markdown's left/center/right alignment, including their link
hit targets; mixed media cells retain their baseline-aware inline flow. Tables
still scroll horizontally within the reading column when necessary.

Emphasis, strong, and strikethrough tags save and restore their enclosing style
instead of clearing a boolean on close. This preserves outer formatting after
nested spans and keeps table headings semibold after explicit strong markup.

Code-only wrapped rows retain the paragraph's minimum line height by padding
the prepared galley, including its glyph positions, meshes, and selection rows.
The native Label still owns text selection/copying, and link hit targets use the
same padded galley. Table images are bounded by their cell width so loading a
wide image cannot shift the remaining columns in only that row.
