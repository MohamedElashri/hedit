# Markdown rendering audit

Audit date: 2026-11-09.

This audit uses the actual native application code through egui frame and input
replays, real on-disk Markdown files, and software-rendered screenshots. Live
computer use was attempted before and after a session restart, but the tool
reported no apps or browsers and an unavailable browser access host. Live
desktop testing remains outstanding; these results must not be described as a
completed hands-on desktop audit.

## Reproduce

Open the files in [the stress fixture directory](../tests/fixtures/markdown/typography.md)
in an isolated hedit session. Each fixture links to related documents where
appropriate. The image fixture deliberately includes missing and blocked images.

```sh
HEDIT_PREVIEW_DIR=/tmp/hedit-markdown-audit cargo test --locked --bin hedit audit_markdown_real_files -- --ignored --nocapture
```

The runner opens seven fixture files plus README, USER_GUIDE, DEVELOPER_GUIDE,
and CHANGELOG: 11 files across four configurations, or 44 scenarios. Configurations
are dark Preview, light Preview with larger type, dark side-by-side Split, and
light compact stacked Split. Each scenario writes top and bottom screenshots,
for 88 captures, and records its duration in `audit.tsv`. System font fallbacks
and uploaded image textures are included in the software renderer.
Set `HEDIT_AUDIT_FILTER=diagrams` (or another filename substring) to recheck a
specific group after a targeted change.

The runner checks finite clipping, unchanged text and disk contents, clean
document state, successful media completion, separate source/preview panes, and
both panes reaching the document end together. Screenshots still require visual
review; passing these invariants is not evidence that every pixel is correct.

## Findings fixed

| Trigger | Previous behavior | Correction |
| --- | --- | --- |
| Wrapped table cells | Text overlapped following rows and table borders | Anchor the full cell layout at the row's top |
| Table alignment syntax | Centered and numeric columns stayed left-aligned | Retain alignment and apply it to text and mixed math content |
| Wrapped cells after the first column | The first line had an unexplained indent | Remove redundant cell-ending spaces |
| Nested strong/emphasis and styled headers | Closing an inner span removed the outer style | Restore the previous style from a stack |
| Definition lists with rich text | Styling split the sentence with extra spacing; the next block ran against it | Use a continuous inline flow and finish each definition block |
| Footnotes | Bare reference labels ran into prose without navigation | Bracket references and link them to definitions |
| Local screenshot wider than the texture limit | A 2560-pixel screenshot panicked with a 2048-pixel limit | Bound decoded image size and resize on the background worker before upload |
| Diagrams and invalid notation at larger text sizes | The viewport cropped ordinary diagrams and error source | Allocate space from the figure's natural height or error text, capped for tall content |
| Missing images | Only a warning icon identified the failure | Show the alternative text and expose the error on hover |
| Linked images | Clicking the image did not activate the surrounding link | Give image widgets the link's pointer and keyboard behavior |

Focused regressions cover these failures, including aligned link hit targets,
fractions/matrices, and resizing. Existing tests also cover live edits, reload,
per-tab modes, recovery, Contents and duplicate/Unicode headings, local and
external links, Copy controls, invalid math/diagrams, media cleanup, large-file
limits, and bidirectional scroll synchronization.

## Results

The final real-file matrix passed all 44 scenarios and produced 88 captures.
The four diagram configurations were rechecked after the final large-text
error-height correction; malformed fenced math also has a focused regression.
Visual review included normal and compact tables, large type, nested containers,
math, diagrams, image loading/fallback, multilingual text with system fonts, and
the repository README. Ten new focused regressions were added; the Markdown
suite passed 55 tests. Full local CI passed serially: 612 Rust tests, 43 Python
packaging tests, strict lint, an optimized build, and package checksum,
extraction, installation, and executable smoke checks.

A separate debug-build size test used repeated rich prose saved as real files.
It completed without crashes or source changes:

| File size | First prepared frame | Median of five subsequent frames |
| --- | --- | --- |
| 65,304 bytes | 29 ms | 20 ms |
| 262,105 bytes | 123 ms | 85 ms |
| 2,097,146 bytes | 906 ms | 702 ms |

These are local debug timings, not release performance claims or portable test
thresholds. Near-limit documents are expensive in this build. An optimized-build
profile and investigation of rendering work outside the viewport remain needed.
Reproduce with `profile_markdown_document_sizes -- --ignored --nocapture`, using
the same `HEDIT_PREVIEW_DIR` convention as the screenshot audit.

## Limits and outstanding checks

- The native Markdown text flow does not yet use the editor's full bidirectional
  shaping path. Arabic/Hebrew ordering and shaping need a dedicated follow-up;
  installed font fallback alone does not establish correctness.
- Embedded HTML remains literal text, except comments and `<br>`, by design.
  Consequently, the repository README's HTML picture/navigation header does not
  look like its GitHub rendering. This audit does not add a browser HTML engine.
- Native Mermaid and TeX support are bounded; unsupported input must stay
  readable as source. Web and `file://` images remain blocked by design.
- Local images are limited to 32 MiB and 16 megapixels. Very wide or tall images
  are resampled for the GPU; original files are not modified.
- Live pointer/keyboard behavior, OS clipboard integration, real window resizing,
  display scaling, and screen-reader behavior still need desktop verification.
  macOS and Windows need their own hands-on checks.
- The matrix samples the top and bottom of long guides. It does not constitute
  visual inspection of every page or every possible Markdown extension.
