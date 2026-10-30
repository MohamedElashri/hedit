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
