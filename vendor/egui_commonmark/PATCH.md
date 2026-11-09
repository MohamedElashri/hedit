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

Table cells are anchored at the grid row's top rather than centering an initial
single-line horizontal layout. Wrapped content and inline math can grow that
layout; centering it first lets painted content extend beyond the row height
and overlap later rows or the table border. Regression coverage checks headers,
wrapped text, line breaks, and math while resizing Preview and Split views.

The real-file audit also retains table column alignments, removes the backend's
redundant cell-ending spaces, and measures aligned mixed text/math cells before
painting. Measurement UIs are invisible and do not activate links. Inline style
state is restored from a stack so nested emphasis and strong spans retain their
outer formatting, including inside table headers. Definition lists use the same
continuous inline flow as paragraphs. Footnote references have bracketed labels
and link hooks that hedit resolves to the original definition source span.
Image widgets expose alternative text on errors and preserve their enclosing
link's activation behavior.
