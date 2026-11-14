# hedit developer guide

Build, test, and release hedit. For using it, see the [user guide](USER_GUIDE.md).

- [Build and install](#build-and-install)
- [Checks](#checks)
- [Editor quality audit](#editor-quality-audit)
- [macOS builds](#macos-builds)
- [Windows builds](#windows-builds)
- [Code map](#code-map)
- [Terminal development](#terminal-development)
- [Rules to keep](#rules-to-keep)
- [Themes and branding](#themes-and-branding)
- [Packaging and releases](#packaging-and-releases)
- [Provenance and license](#provenance-and-license)

## Build and install

You need Rust 1.99 or newer, GNU Make, a C compiler (for the bundled Oniguruma
regex engine), and Python 3.11 or newer for the development scripts. CI uses
Rust 1.99.1 and Python 3.12. X11, Wayland, and OpenGL libraries load at run time,
so their development packages are not needed. Keep the project free of Node.js,
npm, and web frontends.

Default builds include the integrated terminal, which remains disabled in user
settings until enabled. Its pinned Ghostty VT source requires Zig 0.15.2 and Git.
Cargo and Make builds select the matching compiler automatically. If it is
absent or the system compiler has a different version, Python with pip installs
an isolated copy under `target/ghostty-toolchain`; system packages stay unchanged.
CI prepares the same toolchain with `python scripts/setup-zig.py`.
On GitHub Actions the compiler lives under `RUNNER_TEMP`, outside Cargo's target
cache, so restoring that cache cannot prune the Zig executable.
On macOS, the build also handles SDKs that expose the system library through
ARM64e entries that Zig 0.15.2 cannot resolve. `scripts/run-zig.py` creates an SDK
view in Cargo's build directory with Zig's bundled system-library definitions;
headers and frameworks still come from the selected Apple SDK. This applies only
to the Zig subprocess and leaves the installed SDK unchanged. The same view
supplies fallback math macros when the SDK expects a newer Clang header protocol.
Set `ZIG` to an existing compiler executable of the required version for offline
builds, or `PYTHON` to select a Python interpreter for automatic setup. An
explicit `ZIG` with the wrong version fails with an actionable error. Cargo builds
Ghostty's VT library as a static archive, so users do not need Ghostty or Zig
installed. The first build downloads the pinned Ghostty source and Zig packages.
For offline packaging, provide `GHOSTTY_SOURCE_DIR` and
`GHOSTTY_ZIG_SYSTEM_DIR` as described in the
[bindings build instructions](https://github.com/Uzaaft/libghostty-rs#building).
Use `cargo build --locked --no-default-features` for a build without the
terminal or the Zig dependency. Its settings page explains that terminal support
is unavailable. Run `cargo test --locked --no-default-features` to check that build.

```sh
sudo apt-get install build-essential   # Debian/Ubuntu
make dev                               # debug build
make run ARGS='--session-dir /tmp/hedit-dev'
make build                             # release build
make install                           # into ~/.local; set PREFIX to change it
make install DESTDIR=/tmp/stage PREFIX=/usr   # staged, to inspect the layout
```

Every launch, including `make run`, re-executes itself detached from the
terminal and returns once the window is up; later errors go to `hedit.log` in the
session directory. CI defines Linux, macOS, and Windows build/test jobs.
Targeted native macOS and SSH interaction checks supplement headless tests;
see the audit below for the platform and feature validation boundaries.

## Checks

Run `make ci` before committing. It checks formatting, Clippy with warnings
denied, the native-only policy, documentation links, script syntax, Rust tests,
and the offline release, installer, and package tests, then builds and
smoke-tests a Linux release package.

| Command | Purpose |
| --- | --- |
| `make static` | Formatting, policy, docs, and script checks; no compiling |
| `make lint` | `make static`, then Clippy |
| `make test` | Rust tests plus Zig setup, release metadata, installer, macOS, and Windows package tests |
| `make test-macos` | The tests that apply on macOS (the CI macOS job) |
| `make test-remote` | Headless SSH workspace and host recovery tests |
| `make test-remote-live` | Disposable loopback SSH server, real client/worker crash tests, and latency sampling; requires `sshd` |
| `make check` | Lint and tests, without a release build |
| `make package` / `make smoke` | Build a release package under `dist/` / install it in a temporary prefix and run it |
| `make release-check RELEASE_TAG=vX.Y.Z` | Check the tag, package version, and dated changelog entry |
| `make help` | Every target and variable (`CARGO`, `PYTHON`, `PREFIX`, `DESTDIR`, `ARGS`) |

Tests run in temporary directories and never touch your session. Some are
ignored by default because they write files or measure time:

```sh
# Layout previews as SVG (system fonts stand in for egui's)
HEDIT_PREVIEW_DIR=/tmp/previews cargo test --locked --bin hedit export_theme_previews -- --ignored

# The README screenshots, rendered in software at 2x
HEDIT_PREVIEW_DIR=/tmp/shots cargo test --locked --release --bin hedit export_readme_screenshots -- --ignored
for theme in dark light; do
    python3 scripts/ppm-to-png.py /tmp/shots/screenshot-$theme.ppm docs/images/screenshot-$theme.png
done

# CPU timings for typing, navigation, the folder sidebar and filename search
cargo test --locked --release --bin hedit profile_ -- --ignored --nocapture
```

### Editor quality audit

The audit covers the 220-command catalogue and the workflows documented in the
user guide. It verifies named scenarios; passing tests does not establish that
every feature is bug-free or every native platform has been exercised.

| Area | Evidence and scope |
| --- | --- |
| Startup, files, projects and many tabs | Launcher, opening, recovery and project tests; `app/audit_tests.rs` restores 100 named tabs with unsaved Unicode, reveals keyboard-selected tabs, and checks renamed/pinned/reordered tabs at several window sizes. |
| Editing, selections, Undo and split groups | Editing, cursor, folding, snippet and clipboard suites; audit tests compare visible selection rectangles against egui's original mesh rectangles, preserve shared glyph layouts during Select All, and replace/Undo Unicode/CRLF text across split groups. |
| Find, folders and code navigation | Find, folder-search, filename-index, counterpart and file-reference tests; existing profiles cover large folders, current-file Find and asynchronous opens. |
| Completion, languages and keyboard bindings | Completion, spelling, RTL/IME and settings suites; all commands are listed for key-binding configuration, including the palette, with conflict resolution. The palette itself omits its recursive entry from its own result list. |
| Git, builds, logs and terminal | Source-control, build, terminal and protocol suites cover these domains. Their presence in the catalogue is not a claim of live validation for every external tool or server. CUDA tools remain Linux-specific and require optional hardware/tools. |
| Markdown and media | Reader, layout, split-scroll and resource-loader tests cover source/preview behavior, images, math and diagrams. |
| SSH privacy and recovery | Headless helper and real loopback SSH/crash suites; targeted native editing/Undo and recovery checks. Derived UI caches remain in RAM and remote recovery remains on the SSH host. |
| Accessibility | Native AccessKit labels/text, focus reachability through split/busy/hidden-editor transitions, tab role/full-name/selected-state tests, descriptive close controls, valid word-length metadata for long ASCII and Unicode identifiers, and right-to-left direction/positions for pure RTL runs. Native screen-reader speech and mixed-script navigation still need dedicated assistive-technology testing. |
| Packaging and platforms | Local macOS Rust/Clippy/package checks and offline package fixtures. Fresh Linux/Windows CI and live GUI validation of this unpublished stack remain pending; no alternate test host or running Linux container daemon was available. |

The [SSH feature CI run at 78dcf65](https://github.com/MohamedElashri/hedit/actions/runs/43488093934)
passed Linux, macOS and remote SSH jobs, but Windows Clippy rejected the size of
`Action::Add`. The document payload is now boxed, removing the platform-dependent
inline size difference. This is a source fix; a fresh Windows run is still needed.
Those historical green jobs do not validate later local UI commits.

A live native split-group Undo check exposed a collision between a split editor's
widget ID and the primary editor's accessibility row ID. Pane IDs now have a
separate namespace; completion, custom shortcuts and Markdown focus handling use
the same IDs. The vendored egui bridge also exports window focus when a logical
focus request targets a widget absent from the current frame. Regression tests
check text-row roles, unique parentage, focus reachability and successive updates
through the same AccessKit consumer used by the native macOS adapter. The consumer
is a macOS test dependency already present in the locked native dependency graph.

The final optimized macOS app was exercised over private loopback SSH with 100
fixture files, a 40,000-line source file and Unicode/CRLF text. Native checks
covered keyboard tab reveal, full-file selection/deletion/Undo, the formerly
crashing split-group replacement/Undo, and pure RTL selection collapse. After
the host reported durability, the actual client was killed with SIGKILL and
restarted: its unsaved Unicode draft and CRLF format recovered from the host,
saved source files were unchanged, and the client session contained logs only.
This is targeted native coverage, not end-to-end latency or screen-reader speech
validation.

Confirmed fixes in this pass: split editor identity and accessibility focus;
visible tab rendering with active-tab reveal; linear MRU membership/pruning;
primary selection backgrounds painted below shared text meshes; visible-row bounds
for RTL and secondary selections; full accessible tab identity/state and close
labels; pure RTL accessibility direction/positions; RTL selection-collapse arrow
behavior; long accessibility word lengths represented without u8 overflow; palette
shortcut configuration/conflict handling; and the Windows action-enum lint failure.
AccessKit's u8 word-length representation splits an oversized identifier into
representable navigation segments. Accessibility-specific additions retain the
existing macOS feature guards.

Sampled optimized CPU frames on the development Mac (not native input/display
latency, GPU presentation, or SSH durability):

| Fixture | Before | After |
| --- | ---: | ---: |
| 1,000 open tabs, unchanged frame | 1.79 ms | 0.11 ms |
| 40,000-line full-file selection | 38.23 ms | 2.30 ms |
| 165,000-character single-line selection | 4.02 ms | 1.82 ms |

The four-pane 40,000-line fixture measured about 8.2 ms per warm CPU frame.
Typing in the 165,000-character paragraph sampled p50/p95 of 7.6/9.0 ms at its
start and 10.8/13.3 ms at its end. These are fixtures, not worst-case guarantees:
a single larger paragraph still reshapes as a whole after an edit. Keep long-line
layout, native screen-reader behavior, fresh platform CI and live Linux/Windows
checks open until separately verified. Parallel macOS recovery tests have also
occasionally returned `Busy` immediately after a journal is dropped/reopened;
serial runs corroborate recovery behavior without weakening the production lock.

Reproduce the focused profiles serially on an otherwise idle machine:

```sh
cargo test --locked --release --bin hedit profile_quality_audit -- --ignored --nocapture --test-threads=1
cargo test --locked --release --bin hedit profile_long_line_typing_audit -- --ignored --nocapture --test-threads=1
```

For layout jobs of at least 4 KiB, the vendored renderer counts Unicode scalars
in the actual formatted sections before shaping, then reserves glyph storage once
per paragraph and sizes the paragraph collection. It handles formatting splits,
CRLF, empty sections, gaps/overlaps and newline-disabled jobs. Small labels and
zero-row jobs skip this planning pass. The counts are temporary; they retain no
text, add no persistent cache and stay local to layout, including over SSH.
Glyph placement, font fallback, pair kerning, wrapping and bidi shaping retain
their existing behavior. Full paragraph shaping still scales with text length.

`cargo test --locked -p epaint --lib` includes differential checks against the
original geometric-growth path across fonts, formatting, Unicode, wrapping,
alignment, justification, elision and fractional display scales. The ignored
`profile_paragraph_allocation` compares reservation and the original growth path
in alternating order in one process with warm fonts, measuring full layout CPU
time and retained glyph-vector capacity after inline edits.
It excludes job construction, IO, GPU submission and native input delivery;
capacity bytes are not total process memory or measured resident memory.
A paired sample on the development Mac measured:

| Fixture | Original/planned p50 layout | Original/planned retained glyph capacity |
| --- | ---: | ---: |
| 165,000 characters, unwrapped | 7.72 / 6.93 ms | 16,777,216 / 10,560,512 bytes |
| 1,000,000 characters, unwrapped | 53.86 / 47.40 ms | 67,108,864 / 64,000,512 bytes |
| 165,000 characters, wrapped | 8.79 / 8.28 ms | 10,560,512 / 10,560,512 bytes |

These samples are not native latency measurements or hard timing thresholds.

The optimized macOS app was tested over pinned loopback SSH: a Unicode edit near
the millionth character laid out and returned to the clean original with Undo.
A long tab-expanded Unicode/CRLF file accepted an unsaved draft; after host
durability and SIGKILL of the actual client, restarting recovered its text and
CRLF format. Saved fixtures stayed unchanged, the local session contained only
its log, and no native panic was recorded.

```sh
cargo test --locked --release -p epaint --lib profile_paragraph_allocation -- --ignored --nocapture --test-threads=1
```

Each sized font also resolves cached ASCII glyphs through a direct 128-entry
table. The table is allocated lazily on the heap, keeping the font-family map
small; non-ASCII characters retain the sparse hash cache. Entries include the
resolved fallback font and original glyph metrics/UVs, so kerning and font
selection still use the original font implementation. The table belongs to that
font and atlas, stays in RAM and holds no workspace source text. Recreating the
font system drops it along with the other glyph caches.

Renderer tests compare cold/warm lookups for every ASCII code point, missing
glyphs and empty font families. Full-galley comparisons cover mixed Unicode,
fallback-order changes, kerning, decorations, elision, wrapping and fractional
scales. The test-only hash path and the direct path use independent atlases with
identical allocation order; the original lookup switch is absent from normal
builds. The ignored paired profile alternates both paths with warm fonts and
identical paragraph reservations:

```sh
cargo test --locked --release -p epaint --lib profile_glyph_lookup -- --ignored --nocapture --test-threads=1
```

Three consecutive idle runs on the development Mac, each with 20 alternating
samples per path, produced these medians of the per-run p50 layout CPU times:

| Fixture | Hash lookup | Direct ASCII lookup |
| --- | ---: | ---: |
| Short proportional-font label | 0.0016 ms | 0.0016 ms |
| 165,000 characters, unwrapped | 6.94 ms | 5.92 ms |
| 1,000,000 characters, unwrapped | 44.52 ms | 39.64 ms |
| 165,000 characters, wrapped | 8.03 ms | 6.99 ms |
| Mixed Unicode | 4.78 ms | 4.47 ms |
| Unicode-only source plus inline ASCII edits | 9.52 ms | 9.32 ms |

These measurements exclude job construction, IO, native event delivery and GPU
presentation. Unicode-only samples did not show a consistent regression across
these idle runs; short-label differences were below a microsecond. They are
fixture measurements, not hard latency thresholds.
The optimized native app also passed a Unicode edit/Undo near the millionth
character and font-size change/reset commands. A separate Unicode/CRLF draft
recovered after host durability and SIGKILL of the actual SSH client. Saved
fixtures stayed unchanged, the local session contained only its log, and no
native panic was recorded.

The font cache also keeps one weak candidate for appends to unwrapped paragraphs
of at least 64 KiB. With identical wrapping/alignment, contiguous formatting
sections and unchanged styles, it copies the existing glyph/mesh prefix and
shapes only the appended part of the final section, carrying the exact rounded
pen position and preceding glyph ID for kerning. Existing vertical placement and
mesh bounds are reused; the new cursor and mesh indexes start fresh. Single-block
editor pages keep the raw galley alive, so idle-frame cache eviction does not
prevent the next append from reusing it. The weak candidate adds no source-text
retention and disappears when the font system/atlas is reset.

Insertions elsewhere, deletions, new sections or changed styles, line breaks,
wrapping, alignment/justification and decorated runs use full layout. Existing
prefix buffers are still copied, so appends retain a cost proportional to their
size; this optimization removes repeated shaping and tessellation of that prefix,
not every linear operation. Differential tests cover full geometry/UV/color
equality, mixed fonts, fallback glyphs, combining marks, invisible characters,
kerning, fractional scales, live previous frames and idle/atlas-reset lifetimes.

```sh
cargo test --locked --release -p epaint --lib profile_long_paragraph_appends -- --ignored --nocapture --test-threads=1
```

Two idle paired runs on the development Mac (20 alternating samples per path)
measured these p50 full-layout/append-layout CPU times:

| Fixture | First run | Second run |
| --- | ---: | ---: |
| 165,000 ASCII characters | 5.55 / 0.85 ms | 5.55 / 0.80 ms |
| 1,000,000 ASCII characters | 35.82 / 5.99 ms | 35.62 / 6.26 ms |
| Unicode paragraph | 3.47 / 0.36 ms | 3.49 / 0.41 ms |

Job construction, IO and native input/display latency are excluded. The native
optimized SSH app separately accepted a Unicode append after an idle pause at
the end of a million-character paragraph, restored the clean original with
Undo, edited after a font-size change, and exercised the wrapping fallback.
Saved fixtures remained unchanged and no native panic was recorded.

Large editable pages use paragraph-aligned blocks (about 4 KiB each). An inline
edit reshapes the affected blocks and shares unchanged rows with the previous
frame. A single long paragraph can still require a large layout; adding or
removing rows and changing syntax colors can still require copying glyph metadata
for later rows; unchanged mesh vertex/index buffers remain shared.
The source coordinate index updates from UTF-8-safe splices. Its balanced chunks
cover at most 256 Unicode scalars; interior chunks stay at least half full after
edits. Byte lookup and supported tab-column lookup use a binary search followed
by at most one chunk's scalar walk. Chunk summaries carry column transforms for
the editor's tab widths 1–8, including CRLF and newline resets. Other widths retain
the scalar reference path. Edits rebuild the affected chunks and adjacent
boundaries, then adjust suffix offsets and column summaries; this metadata update
still scales with the number of chunks. Large deletions release excess capacity.
Only offsets and column metadata enter these RAM caches, including in SSH windows.
Initial indexing, minimal-splice discovery and paragraph shaping still scan text.
An unchanged tab can reuse its layout. Each pane keeps at most three inactive layouts within a
64 MiB accounting budget, in addition to its active layout. This is a cache
budget, not a limit on total editor memory.

`vendor/egui` and `vendor/epaint` pin the upstream version recorded in
`Cargo.lock` under its MIT license. The
renderer patch changes `Galley.rows` to `Vec<Arc<Row>>`, uses copy-on-write for
selection/transform mutations, and retains unrounded `Row.layout_rect` for
paragraph placement at fractional display scales. Hedit recomputes vertical
glyph rounding when moving rows. Keep these patches small when updating egui;
compare incremental results with the full renderer, including mesh vertices,
colors, wrapping, RTL, and 125%/150% scaling. The licenses remain in the vendor
directories and the shipped [MIT notice](assets/licenses/egui-epaint-MIT.txt).
The Markdown integration also uses copy-on-write when adding line padding.

`RowVisuals.mesh` is an `Arc<Mesh>`. A row can keep its mesh and triangle index
while `mesh_offset` translates it; `mesh_bounds` already includes that offset.
The tessellator applies it before rotation and passes it to spatial-index clipping.
Use `translate_mesh` for movement and `mesh_mut` before changing geometry: the
latter copies shared data, bakes the translation, and invalidates the local
triangle index. Color-only changes can keep the translation and index. Empty
visuals share an immutable empty mesh, avoiding an allocation for each placeholder.

Cached block placement still recomputes glyph baselines. If physical-pixel
rounding requires individual quad corrections, only that row's mesh is
materialized; otherwise its buffers and index stay shared with the live previous
frame. Syntax-section renumbering does not copy unchanged meshes. Glyph storage
is still copied when row metadata changes, and genuinely changed colors/styles
still need their normal update work. The inactive-layout accounting budget
includes the shared mesh allocation/header conservatively.

Tests compare row movement with the previous eager implementation and compare
translated rendering with explicitly materialized geometry across clipping,
rotation, scaling, selection, fractional scales and repeated movements.

```sh
cargo test --locked --release --bin hedit profile_row_mesh_movement -- --ignored --nocapture --test-threads=1
```

An idle paired sample on the development Mac (12 alternating samples) measured:

| Placement fixture | Eager p50 / p95 | Shared p50 / p95 | Used mesh geometry reused |
| --- | ---: | ---: | ---: |
| 40,000 short rows | 23.07 / 33.57 ms | 8.18 / 10.33 ms | 95,680,000 bytes |
| 100 long rows | 1.83 / 3.26 ms | 0.36 / 0.67 ms | 10,400,000 bytes |

This isolates row movement while a previous frame remains live. It excludes
initial layout, source indexing, syntax analysis, whole-galley assembly, IO and
native input/display latency. Geometry bytes count used vertices/indices, not
total allocation capacity or resident memory.
The optimized native SSH app separately passed header insertion/Undo in a
40,000-line CRLF file, replacement/Undo across two split groups, and host-only
Unicode/CRLF draft recovery after actual-client SIGKILL. Saved fixtures remained
unchanged, the local session contained logs only, and no native panic occurred.

The renderer also keeps a lazy, thread-safe cursor index on each galley.
Character/paragraph lookups use binary search; row and end lookups use cached
offsets. Pointer lookup uses binary search for finite rows with non-overlapping
interiors, preserving shared-boundary and equal-distance choices. Overlapping
or irregular geometry retains the original nearest-row walk. Within rows of at
least 128 glyphs, the first horizontal lookup lazily checks whether glyph centers
are finite and nondecreasing. Eligible rows use binary search with the original
strict midpoint tie rule; irregular, reordered RTL and nonfinite geometry retain
the linear walk. First validation still visits the row's glyphs. Pointer placement
and vertical arrow navigation share this lookup.

`CursorIndexCache` shares immutable metadata across galley clones. Its contents
are omitted from serialization and galley equality. The editor's viewport row
cache shares the same offsets without retaining glyphs or meshes. Initialized
index storage is counted in the inactive-layout budget. Every layout constructor,
including incremental block assembly and empty layouts, starts with a fresh
cache. Call `invalidate_cursor_index` after changing glyph counts, newline flags,
row order, or glyph/row geometry. Color-only changes can share the index;
RTL shaping and Markdown line-padding changes explicitly invalidate it.

`app/cursor_tests.rs` retains the pre-index linear routines as independent
references. Checks compare cursor fields and wrap preferences, including goal
columns beyond paragraph ends, empty soft rows, elision, RTL, mixed fonts,
zero-height rows, geometry gaps, incremental edits and fractional scales.
`profile_cursor_navigation` measures warm conversions near the end of a
40,000-line fixture. On the development Mac, pointer lookup changed from about
2.20 ms to 0.1 µs and character lookup from about 55 µs to 0.02 µs. These are
isolated CPU calls; first use builds the index, and native event/display latency
is excluded. Run profiles on an otherwise idle machine.

`app/long_line_tests.rs` adds real TextBuffer edit/paste/Undo checks in split
groups for identity and tab-expanded Unicode/CRLF text. Differential fixtures
exercise checkpoint boundaries, repeated edits, tab-width changes, snapshot
isolation, chunk density, midpoint ties, irregular glyph geometry, and warmed
indexes followed by RTL reshaping.

The ignored `profile_long_line_coordinates` isolates warm coordinate queries near
the end of 165,000-character, one-million-character and Unicode/tab fixtures. On
the development Mac, the million-character ASCII line measured approximately:

| Warm query | Before | After |
| --- | ---: | ---: |
| Character to UTF-8 byte | 895 µs | 0.2 µs |
| Logical line/column | 1,386 µs | 1.0 µs |
| Pointer to cursor | 1,207 µs | 0.1 µs |

The optimized macOS app was also tested over pinned loopback SSH with a
million-character first line. Native checks reached column 1,000,010, selected
and replaced its end marker with the mouse, restored it with Undo, and preserved
the preferred column while moving down to a shorter row and back up. A separate
long Unicode/tab file accepted an unsaved CRLF draft; after host durability and
SIGKILL of the actual client, restarting recovered that draft. Saved files stayed
unchanged and the client session contained only its log.

These are individual CPU queries, excluding cold initialization, rendering,
native event delivery and SSH durability. Whole-paragraph reshaping and long-row
rendering remain separate costs; these results are not end-to-end typing latency
or worst-case guarantees. Reproduce on an otherwise idle machine:

```sh
cargo test --locked --release --bin hedit profile_long_line_coordinates -- --ignored --nocapture --test-threads=1
```


#### Viewport text mesh submission

Large, partly clipped, unrotated text rows use a lazy triangle-bounds hierarchy
in `vendor/epaint/src/text/mesh_index.rs`. Each leaf covers 256 consecutive
triangles, normally 128 glyph quads. Traversal skips disjoint bounds and preserves
original triangle order, texture coordinates and color handling. Bounds use the
same rounded origin addition as emitted vertices, avoiding inverse-clip rounding
at large horizontal offsets. A physical-pixel guard preserves backend scissor
rounding at block edges. Scattered selection/background indices duplicate
only their referenced triangle vertices instead of copying a large unused span.
Small rows, rotated text, nonfinite geometry and unsupported index topology keep
the original whole-row path. Disabling coarse culling also keeps that path.
Overlapping geometry can still require broad blocks; this is conservative
culling, not a hard vertex-count limit.

The tessellator reserves only submitted geometry, rather than the entire galley.
Pure TextShape translation retains shared glyph meshes. Scaling, selection-mesh
insertion, RTL reshaping and individual quad corrections invalidate affected bounds;
row-mesh translations and color/UV changes preserve them. The derived cache contains only bounds and index
ranges, stays in RAM, is omitted from serialization/equality, and is counted in
the editor's inactive-layout accounting budget. It does not retain text or create
an SSH client recovery cache. Source layout meshes remain allocated separately.

`app/render_tests.rs` compares potentially visible triangle positions, UVs, colors
and paint order against unculled output, including clipping edges, multiple
scales, italics, backgrounds, underline/strikethrough, opacity, color overrides,
RTL, selections, batching offsets, sparse indices and geometry mutations. It also
checks bounded submission for ordinary long rows, shared initialization, pure
translation and serialization behavior.

The optimized CPU fixture `profile_viewport_text_rendering` measured a
million-character ASCII row at the start, middle and end of a 1,200-point viewport.
Warm mesh preparation changed from about 5.2–5.4 ms to 0.002–0.004 ms; submitted
mesh data changed from 104,000,000 bytes to roughly 20,000–27,000 bytes. The
40,000-line fixture still submitted about 91 KB, while reserved output capacity
changed from about 96 MB to 153 KB. These are CPU mesh samples, not measured GPU
transfer times, native input latency, total process memory or hard CI thresholds.
First clipped use builds the hierarchy by scanning triangles (about 9 ms in the
million-character fixture); full paragraph shaping and source-mesh construction
remain separate costs.

The final optimized macOS app was exercised over pinned loopback SSH with a
million-character unwrapped row. Native checks inspected its clipped end,
replaced selected text with Unicode in a split editor, and restored it with Undo.
A long Arabic/Hebrew/English line also rendered in a clipped, unwrapped viewport.
After an unsaved Unicode/CRLF draft became durable on the host, SIGKILL of the
actual client and restart recovered it; saved fixtures stayed unchanged, the
client session contained only its log, and no native panic was recorded. These
checks do not establish screen-reader speech or end-to-end input/display latency.

Reproduce the CPU fixture serially on an otherwise idle machine:

```sh
cargo test --locked --release --bin hedit profile_viewport_text_rendering -- --ignored --nocapture --test-threads=1
```

The editor preserves TextEdit paragraph cursors while the displayed text and
public selection agree, keeping preferred columns and soft-wrap affinity across
repaints. Explicit cursor navigation resets the goal even when its numeric
position is unchanged. Weak layout-job and projection identities cache the text
comparison without retaining another document copy or rescanning unchanged
frames; same-length external text changes cannot reuse stale paragraph
coordinates.

Native input can deliver a full drag in one frame. `app/editor_interaction.rs`
recovers its press anchor for a single primary-button gesture, using RTL-aware
hit testing and the configured click-distance threshold. Disabled editors,
other layers, word/line clicks and Alt/Control/Command gestures retain their
existing handling. Selection painting is restricted to visible rows.

The minimap caches at most 4,096 two-byte samples per pane, keyed by a weak
projection reference and sample count. It samples projected logical lines,
including tabs and folds, and bounds work within a sampled long line. New text,
folds, tab width or sample count invalidate it; scroll and color changes reuse
it. These derived caches remain in RAM, including in SSH workspaces.
`profile_minimap_interaction` compares warm CPU frames with the minimap enabled
and disabled on a 1.62 MB, 40,000-line fixture. On the development Mac, a
baseline sample measured 3.03 ms disabled and 3.46 ms enabled; three final runs
had medians of 3.02 ms disabled and 2.98 ms enabled. The remaining difference
is within measurement noise. These are CPU frame times, excluding native
input, GPU presentation and SSH durability.

The ignored `profile_typing_positions` benchmark reports p50/p95/p99 CPU frame
times for 64 inline edits at the start, middle, and end of a 1.46 MB buffer.
Run profiles serially on an otherwise idle machine. These headless frames do
not include native event delivery, GPU presentation, or SSH durability latency;
use a separate temporary session for native interaction and crash recovery tests.
Do not treat CPU timings as input-to-screen measurements or hard CI thresholds.

Completion indexing snapshots at most 32 open, fully loaded, non-followed buffers,
active first, within 8 MiB total and 4 MiB per buffer. UTF-8 boundaries are
preserved. A merged index replaces the former unbounded per-file caches, keeping
at most 100,000 words and 8 MiB of accounted word/string storage; allocator and
transient build capacity add overhead. These caches and snapshots stay in RAM.
Scope changes discard the old merged index, including when suggestions are
disabled. No files are read or execution services contacted to build it.

At most one index job and one ranking job run per window. Cancellation retains
the job slot until completion is received, so new input cannot create a backlog
of readers or snapshots. Indexing checks cancellation every 2,048 characters,
including within a long token; ranking checks every 256 words and retains only
the best 12 candidates. Snapshots up to 64 KiB and indexes up to 4,096 words may
run inline. Edits debounce rebuilds for 750 ms; manual completion forces a request
without putting large scans on the UI thread. Worker results must match the
current snapshot/query identity before publication.

Completion prefixes reuse the source line index and cache unchanged
view/revision/cursor lookups. Reverse scanning stops after 65 characters, since
indexed words have at most 64. Sparse source checkpoints bound the final character-to-byte walk, including
within long logical lines. Acceptance checks view, pane, revision,
head, start and exact typed prefix. Concurrent text/paste/navigation and accept
keys cannot apply a choice from the previous editor frame. Escape cancels delayed
requests too; keyboard-highlighted choices remain stable during index refreshes.
`profile_completion_prefix` compares the old linear prefix lookup with indexed
lookup. `profile_completion_typing` measures full CPU typing frames with and
without a populated completion index; both exclude native input/display latency.

File opens use two background readers with a shared limit of sixteen pending
requests. They clone existing documents under a short lock before reading pages;
closed-file reopening uses those snapshots too. Results wait while a mutation is
active and check document revisions and paths before replacing a view. Duplicate
opens reuse the live document. Only the latest request can take focus, and typing,
scrolling or switching away keeps the result in a background tab. Closing or
cancelling invalidates pending results; a blocked OS read may finish later. A
cancelled SSH read may still have created a baseline in host recovery. When a
saved workspace view exists, unlisted clean baselines recover into Closed Files;
unlisted unsaved drafts always reopen. No recovery history is deleted, and the
read never writes document recovery to the client. IPC acknowledgements and terminal waits
include the read queue. SSH flush barriers capture the edits submitted before
that call under the queue lock; later edits cannot invalidate an already queued
barrier. Saves still wait for all edits submitted before saving.

Go to Anything caches current-file matches by document, revision, page and query.
Symbol indexes survive query changes. Scans over 64 KiB run on one worker, and
stale results cannot populate a newer query. The picker lays out visible result
rows only. The ignored `profile_goto_navigation` profile measures warm picker
frames for symbol and text searches over an approximately 800 KiB source buffer.

Current-file Find and selected-word highlights cache matches by document ID,
revision, page, path, query, options and byte scope. Scans over 64 KiB, or with
queries longer than 256 bytes, use one cancellable worker per editor group
(at most four). Repeated queries share a RAM-only document snapshot; a group
retains one outstanding task rather than queueing source copies. Cancellation
is checked between regex matches, so an individual regex call can finish later.
No search snapshot or match cache is written to disk, including for SSH buffers.

Find navigation over fully loaded files reuses complete cached ranges and never
enters the document mutation lane. It counts all matches while retaining only
20,000 highlight ranges and a requested navigation answer beyond that prefix.
Repeated next/previous requests retain their order in a bounded queue. Incoming
editor input invalidates pending cursor movement before results are consumed;
revision, path, scope, selection and editor-group checks protect later results.
Paged-file navigation retains the existing whole-file backend search path.

Editor gutters, indentation guides, whitespace markers and Find highlights
visit visible rows. A weak galley identity keys cached row/line/character offsets,
so the cache does not retain layouts outside the existing layout budget.
Whitespace iteration restarts at indexed line boundaries to skip folded source
without materializing a full-document character vector. Highlight geometry is
compared with a full traversal for wrapping, tabs, folds, RTL and fractional
scales, including zero-width matches on empty lines and at EOF.
The ignored `profile_live_find` measures unchanged frames with highlights and
whitespace enabled over 1.70 MB, and the UI cost of submitting changed queries;
that submission timing excludes background completion. The development Mac
measured about 5.42/7.44 ms at the start/end before this change and 3.51/5.76 ms
afterward. These are headless CPU frame timings, not input-to-screen latency.

The folder sidebar caches a flattened expanded hierarchy and creates widgets
only for visible rows. Row identities include the root and path, including for
overlapping roots. Reveal scrolls to the cached row before rendering it. The tree
has accounting limits of 50,000 rows and 8 MiB; directory listings have a separate
256-directory / 16 MiB budget. Collapsed subtrees release unused listings. At
most four listing readers run, including stale reads from before a refresh;
an OS or SSH call already running can finish after cancellation.

Filename search has one coalescing worker and a single response mailbox. Its
metadata catalogue survives query changes, so warm queries perform no directory
reads. Roots, refreshes and exclusion changes invalidate it; request tickets
reject older query results. Reopening a completed picker after two seconds
refreshes the catalogue once. The picker also offers **Refresh folders**.
Automatic traversal never follows directory symlinks and uses backend-returned
paths verbatim, including encoded SSH URIs. Only filenames and metadata enter
the RAM cache; nothing is persisted on the client. Traversal stops at 50,000
entries, an 8 MiB accounting budget, or five seconds between directory reads.
One directory read may exceed that time; the existing backend timeout still
applies. Each listing includes at most 2,000 entries. Partial scans and unreadable
directories remain visible in the picker. These are accounting budgets, not
hard process-memory limits.

The ignored `profile_large_folder_sidebar` measures warm frames with 10,000
expanded files. `profile_filename_scan_and_index` compares repeated directory
walks with warm filtering over the same 10,000-file fixture;
`profile_warm_filename_index` isolates the filter. On the development Mac, the
sidebar changed from 5.16 ms to 0.06–0.08 ms per CPU frame, and filename queries
from about 13.5–13.9 ms per walk to 0.9 ms from the cache. These measurements exclude
native input delivery and display latency and are not CI timing thresholds.

Folder searches own immutable open-file snapshots and a separate cancellation
token; they never acquire the editor's mutation lock. Completion cannot clear a
save's busy state. Replacement previews require unchanged open-file revisions,
workspace roots, and search settings; disk hashes remain checked at application.
SSH receipts and connection changes request repaints through a callback invoked
outside the transport state lock. Pending receipts retain a timer for the
backpressure deadline; unchanged idle host polls do not repaint the window.

The offline User Guide renders the repository's Markdown with the same native
renderer as file previews. Edit the source documents directly; do not keep a
second copy of their text in UI code. Rebuild the executable after documentation
changes. Add linked local documents to the bundled allowlist in
`app/user_guide.rs`; the reader never resolves links against the user's disk.
Internal links and Contents stay in the modal, and only explicit external-link
clicks open a browser. Keep Help, About, F1, the command palette, and key bindings
routed through `Command::UserGuide`.

Markdown preparation retains an owned `PreparedMarkdown` event stream and a
bounded document tree per revision, using the renderer's re-exported parser types.
The native layout follows [VS Code's Markdown preview stylesheet](https://github.com/microsoft/vscode/blob/main/extensions/markdown-language-features/media/markdown.css):
14px body text at the default zoom, 1.6 line spacing, 26px horizontal padding,
explicit block margins, intrinsic table columns, and simple quote rules.
`parsers/document.rs` owns block geometry; `parsers/inline.rs` owns continuous
native text layout, selection, links, and baseline-aware media. Keep these
responsibilities separate so nested blocks cannot leak state into later content.
Split-divider ratios are scoped to document, editor group, and orientation in
native UI session data. Clamp both panes when the window changes size, keep the
divider keyboard focusable, and preserve source/preview clipping and scroll sync.
Code fences grow vertically with the document and scroll only horizontally;
their Copy control overlays the frame on hover or keyboard focus. Index original source
spans for headings, Contents, and block-level split scrolling; do not insert
marker HTML or synthetic paragraphs into the source. Large revisions prepare
on a worker, and stale replies cannot replace a newer revision. Inline math
must reserve its ascent and descent before drawing text in the same flow.
Keep the TeX baseline separate from the SVG bounds and the reading font size
separate from line height. Wide figures use tiled GPU textures at their original
logical size, since the pixel budget alone does not bound a texture's longest
side. Review light, dark, and compact output when changing reader layout:

```sh
HEDIT_PREVIEW_DIR=/tmp/hedit-reader cargo test --locked --bin hedit export_markdown_reader_previews -- --ignored
# Inline code, wrapping tables, nested styles, and compact split panes
HEDIT_PREVIEW_DIR=/tmp/hedit-layout cargo test --locked --bin hedit export_markdown_layout_previews -- --ignored
# Document typography, tables, alerts, code, and source/preview layouts
HEDIT_PREVIEW_DIR=/tmp/hedit-document cargo test --locked --bin hedit export_markdown_document_previews -- --ignored
```

## macOS builds

Releases ship one `hedit-X.Y.Z-macos-universal.zip` with an Apple Silicon and
Intel `hedit.app` (macOS 11 or newer). On a Mac with Xcode's command-line tools:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
make macos
```

`scripts/package-macos.py` joins the two builds with `lipo`, writes Info.plist
and the icon, copies themes and guides, signs, verifies, and zips the app. The
signature is ad hoc; pass `--identity 'Developer ID Application: …'` to sign
with a Developer ID. Notarization is not automated.

On macOS, run `make test-macos` instead of `make ci`: Linux packaging checks need
GNU tools. Native updater tests exercise signed app ZIPs, bundle swaps, and rollback.
Local CUDA tools are hidden on macOS. Before calling Mac support stable, check
Open/Save As, the clipboard, Retina rendering, Cmd+Q with recovery, Finder
document events, external changes, and saves on APFS on a clean Mac.

## Windows builds

Use the x86-64 MSVC Rust toolchain and Visual Studio Build Tools with **Desktop
development with C++** and a Windows SDK. Python 3.11 or newer runs the packaging
scripts; GNU Make is optional. From PowerShell:

```powershell
rustup default stable-x86_64-pc-windows-msvc
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --no-fail-fast
python scripts/test-release.py
python scripts/test-windows-package.py
$env:RUSTFLAGS = '-C target-feature=+crt-static'
cargo build --release --locked
python scripts/package-windows.py --smoke
```

The ZIP and SHA-256 file appear in `dist/`. The package includes `hedit.exe`,
guides, license notices, themes, and examples. Static CRT linking keeps the C
runtime inside the executable. The smoke check extracts to a path containing
spaces, verifies the checksum and resources, and runs `--version` and `--help`.
CI uses the same commands on `windows-2025` and publishes a Windows artifact on
each push. Tagged releases include it alongside the Linux and macOS packages.

Build systems use `cmd.exe /D /S /C` on Windows, with Cancel stopping the process
tree through `taskkill`; Unix platforms use `sh`. Write custom `shell_cmd` values
using the platform's shell syntax. The Python build uses `python` on Windows.
CUDA source editing works, while the local CUDA tool panel remains Linux-only.
Windows upgrades replace the portable folder; in-app updates support Linux and macOS.

Before calling Windows support stable, validate file dialogs, clipboard,
high-DPI rendering, window close/recovery, non-ASCII paths, file replacement on
NTFS, and opening files from Explorer on a clean Windows desktop.

## Code map

The library (`src/*.rs`) holds editing logic with no UI; the app (`src/app/`)
draws the window and turns input into library calls. Logic that can be tested
without a window belongs in the library, with tests under `tests/`.

| Area | Files |
| --- | --- |
| Documents, sessions, recovery | `document.rs` (piece table, pages, saves, undo), `workspace.rs`, `session.rs`, `text_encoding.rs` |
| Editing | `advanced.rs` (selections, folding, display projection), `editing.rs`, `text_commands.rs`, `completion.rs`, `intelligence.rs` (symbols, snippets, definitions, Git), `spelling.rs`, `bidi.rs` |
| Source Control | `git.rs` (bounded Git processes, status/diff/history parsing, index/worktree operations); `app/source_control.rs` (native sidebar, review workspace, background state) |
| Search and folders | `search.rs` (search, guarded replacement), `folder.rs` (listings, fuzzy matching), `projects.rs` (projects, EditorConfig) |
| Header/implementation switching | `counterpart.rs` (same-stem matches, sibling priority, project search); `app/counterpart.rs` (background request, stale-reply guard, picker); `tests/counterpart.rs` and `app/counterpart_tests.rs` |
| File references | `file_reference.rs` (quoted/bare paths and existing-file resolution); `app/file_reference.rs` (background resolution and picker); `tests/file_reference.rs` and `app/file_reference_tests.rs` |
| HTML/XML selection | `markup.rs` (balanced elements and Unicode selection ranges); `app/markup_selection.rs` (worker request); `tests/markup.rs` and `app/markup_selection_tests.rs`; results require the same view, revision, and selections |
| Syntax and look | `syntax.rs`, `syntax/` (grammars, lexer, keywords), `theme.rs`, `fonts.rs`, `brand.rs` |
| Process and updates | `cli.rs`, `launcher.rs`, `ipc.rs`, `windows.rs`, `update.rs`, `update/`, `build.rs`, `debugger.rs` |
| Window | `app.rs` (frame loop, shortcuts, editor drawing), `app/chrome.rs` (menus, tabs, sidebar, status bar), `app/commands.rs` (every command, the palette) |
| Launch activation | `activation.rs` (one-shot presentation, Wayland activation tokens and a nonblocking native protocol binding); `main.rs`, `ipc.rs`, `app/quality.rs` (launch and existing-session handoff) |
| App features | One file each in `src/app/`, named after the feature: `auto_complete.rs`, `definitions.rs`, `spell_check.rs`, `macros.rs`, `snippet_fields.rs`, `find_tools.rs`, `changes.rs`, `fold_gutter.rs`, `font_picker.rs`, and so on |
| Integrated terminal | `terminal.rs` (Ghostty VT and bounded PTY IO); `app/terminal_panel.rs` (tab lifecycle, onboarding, input focus); `app/terminal_tabs.rs` (compact tab strip, reordering, menus); `app/terminal_text.rs` (bidirectional cells and cached Rustybuzz/ab_glyph output); `app/terminal_tests.rs` (UI regression tests) |
| Markdown | `app/markdown.rs` (source/preview layout, background preparation, contents, links, local images), `app/markdown_scroll.rs` (bidirectional split scrolling), `app/markdown_style.rs` (shared document padding, fonts, contrast, and cached native code blocks), `app/markdown_media.rs` (equation baselines and cached native math/Mermaid rendering), `app/markdown_tests.rs`, `app/markdown_reader_tests.rs`, `app/markdown_document_tests.rs`, `app/markdown_pane_tests.rs`; per-tab modes are stored in `session.rs` |
| Offline help | `app/user_guide.rs` embeds `USER_GUIDE.md`, related documentation, and notices with `include_str!`; `app/user_guide/tests.rs` covers links, modal input, and layout |
| Settings | `settings.rs` (the `settings.json` schema), `app/settings_store.rs`, `app/preferences.rs`, `app/settings_pages.rs` |

Every menu item, shortcut, and palette entry goes through `Command` in
`app/commands.rs`, so availability and behavior stay the same everywhere. A new
command needs a variant in `Command::ALL`, a label, an `execute` arm, and a
`command_enabled` arm when availability depends on state. Add default shortcuts
to both the displayed binding and input dispatch. New dialogs also need
`opens_window` and `Overlay` entries so focus, Escape, and transient-window
replacement behave consistently.

## Terminal development

The Cargo `terminal` feature is enabled in default builds; the user setting
`terminal_enabled` defaults to `false`. Onboarding and settings only record the
choice. Spawn a shell only when the user opens the panel or creates a tab.
Older settings files skip onboarding and remain opted out. Disabling the feature
or quitting drops every session; hiding the panel keeps sessions alive. Terminal
tabs and scrollback are not part of editor session recovery.

Each tab owns a Ghostty VT instance on the UI thread and a `portable-pty` shell.
Reader and writer workers use bounded queues; poll inactive and hidden tabs too,
so output cannot stall their shells. Keep process termination and reaping off the
UI thread. Stable tab IDs preserve the active shell when tabs are reordered.

Route focused terminal input before editor shortcuts. Escape, Tab, arrows, and
interrupt keys belong to the shell; terminal tab shortcuts apply only while it
has focus. Keep command-palette and settings shortcuts available. IME preedit is
displayed locally and reaches the PTY only on commit. RTL cell maps and Arabic
shaping change display, cursor placement, and pointer selection; PTY bytes and
copied Unicode stay in logical order. Respect the `terminal_bidi` opt-out.

For terminal changes, run the focused checks as well as the usual project checks:

```sh
cargo test --locked terminal
cargo test --locked --no-default-features
python3 scripts/test-zig-setup.py
python3 scripts/test-run-zig.py

# Render onboarding, tabs, settings, narrow-window, and light-theme previews.
cargo test --locked --bin hedit export_terminal_previews -- --ignored
python3 scripts/ppm-to-png.py /tmp/hedit-terminal-panel.ppm /tmp/hedit-terminal-panel.png
```

The preview exporter writes `/tmp/hedit-terminal-{onboarding,panel,settings,compact,light}.ppm`.
Review Arabic joining and diacritics, mixed-script selection, the active tab's
close control in narrow windows, and both themes. The UI tests cover tab
isolation, focus, drag reordering, middle-click closing, IME, and logical Unicode
copying; backend tests cover PTY IO, VT replies, resizing, and process cleanup.

The local `vendor/libghostty-vt-sys` patch selects the pinned compiler for raw
Cargo builds as well as Make; its `PATCH.md` records the scope. Keep it aligned
with `scripts/setup-zig.py` when upgrading Ghostty. Run
`python3 scripts/setup-zig.py --print-path` to check compiler selection. If setup
reports an incomplete cache, remove only `target/ghostty-toolchain` and retry,
or set `ZIG` to a matching compiler. Automatic installation requires Python with
pip and network access on the first build.

## SSH architecture and testing

See [SSH remote workspaces](USER_GUIDE.md#ssh-remote-workspaces) for installation,
connection options, privacy boundaries, supported scope and limits.

`crates/hedit-core` shares the document, encoding, filesystem, folder search, Git
and build engine between the GUI and `crates/hedit-remote`. The helper does not
link the GUI. The editor opens each SSH workspace in a separate process/window;
local sessions and remote content do not share persistence. Release builds and
`cargo run` still select the editor by default. Workspace tests and Clippy cover
the shared engine and helper as well as the editor.

### Isolation and trust invariants

Remote documents use memory-backed pieces and an immutable remote marker. The
editor records that marker without locking a document during checkpoint capture,
and both local session capture and document snapshots reject remote documents.
Remote windows disable egui memory and window persistence, recent-file/project
writes, and Git draft persistence. Changing a document's name or save target must
not remove its remote identity.

The backend registry routes remote URIs explicitly and fails closed when a
provider disappears. Validate every host-provided filesystem path against the
opened host root before returning it to the UI. A host response must never
become a local file, folder, shell directory or process capability. Relative Git
paths preserve Unix bytes over the wire; URI encoding must preserve literal
percent signs, spaces and filename boundaries. Build working directories and
terminal fallback directories must also remain remote.

Source filesystem operations walk pinned directory descriptors, reject symlink
components, traversal and special files, and use exclusive staging and
no-replace renames where appropriate. Recovery files require account ownership,
private permissions, no hard links or symlinks, and supported durable storage.
On macOS, reject ACL grants that could bypass Unix permission bits, including
on recovery ancestors. Source file saves preserve their existing access rules.

Git, builds and shells require connection-scoped execution trust. They have the
SSH account's normal permissions; this is not a tool sandbox. Trust is reset on
reconnect or ownership loss. Do not infer execution trust from merely opening a
folder. SSH configuration and installed helper paths are explicitly trusted
inputs, never supplied automatically by repository content. OpenSSH arguments
must continue to override forwarding, unrelated connection reuse, local commands
and proxy execution. See the [OpenSSH configuration manual](https://man.openbsd.org/ssh_config).

### Recovery and concurrency invariants

The host journal uses SQLite WAL with full synchronization and macOS fullfsync.
Only construct a durable receipt after its transaction commits. Preserve the
original baseline, reconstructable current text and bounded host undo/redo history.
A recovery session identity, writer epoch, document sequence, payload digest and
base revision prevent stale writes and duplicate replay. Separate state directories
also contend on a canonical workspace inode lock. Each client view carries its
own expected host revision for editing, saving, history and reloading.

The client queues edits without network I/O on the UI thread. Bound pending work
and pause editing when it exceeds its age, count or byte budget. Reconnect must
check the original session, replay the same sequences and payloads, and compare
host revisions before permitting more edits. Retain uncertain text in memory;
never make a local recovery copy or silently discard it.

Save intents commit before source publication. Reconcile an interrupted save
against its destination digest rather than blindly executing it again. Rename
intents record inode identity and reconcile document paths transactionally. The
source check and filesystem rename cannot provide compare-and-swap against an
uncooperative external writer; preserve and report conflicts.

Compaction atomically checkpoints text and bounded history, retains recent
receipts, and rejects compacted old sequences as conflicts instead of new edits.
Corrupt, future-schema, full or unsupported storage must remain intact. SQLite's
[WAL documentation](https://sqlite.org/wal.html) describes its filesystem and
synchronization assumptions; process-kill tests do not certify power-loss safety.

Run tool jobs outside the edit actor. Separate authenticated service connections
and bounded output queues keep builds and terminal streams from sitting ahead
of edit receipts. Cancellation, dropped service connections and owner loss must
reclaim jobs and stop tracked processes. Do not grant a new owner while an old
job is still running. Drain terminal output before reporting exit. Bulk transfers
are bounded, use four chunks in flight, and reserve space for the outer reply
envelope as well as the payload.

### Verification

```sh
make test-remote
make test-remote-live
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
```

The live harness requires OpenSSH server/client and keygen. It starts an isolated
IPv4 loopback server with disposable keys, pinned host keys, private homes and
state, and no real user credentials or agent. It covers unknown/changed host keys,
disabled forwarding/local commands, quoted install paths, service authorization,
writer fencing, peer limits and abandoned-job cleanup. Deterministic process-kill
cases cover both sides of edit commit, source publication and compaction, plus
reconnecting the production client after losing a committed edit's receipt.
The editor backend probe exercises files, search, Git, builds, PTYs, scratchpads,
stale views, large reads/uploads, host-only view metadata and reload previews.

The fault-enabled helper is built separately and cannot be built in release
mode. Avoid logging document contents, patches, input, terminal output, tokens or
SSH diagnostics from production sessions. Test diagnostics may reference only
the harness's disposable resources.

A native macOS GUI run also typed an unsaved marker over SSH, killed the actual
editor, reopened host recovery, saved it, and scanned the local session directory
for the marker. No local marker was found. The CI Linux job runs the live harness;
Linux/Windows CI results and WAN testing were not locally verified. These tests
are not an independent security audit or hardware power-loss validation.

## Rules to keep

**Never lose text.** Changes to saving, recovery, replacement, or session data
need tests for the failure paths.

- A checkpoint writes buffer data, then a new manifest, keeping the previous one
  until the new one is synced. If the newest manifest fails to load, the previous
  one is used; if both fail, the old data is moved to `preserved-*` before starting
  fresh. Never delete preserved data automatically.
- Saves are atomic and refuse to overwrite a file that changed on disk since it
  was read. Replace in folders writes a file only when its hash still matches the
  preview, and edits open files as undoable changes instead.
- New session or settings fields need serde defaults, so older files still load.
  Retired `settings.json` keys go in `settings::RETIRED` so they load silently.

**Keep the UI thread fast.** Reading files, folder walks, Git, spell checking,
word indexing, font listing, and highlighting more than 64 KiB of text run on
worker threads; their results carry a view and revision, and stale ones are
dropped. Schedule repaints for input, worker results, or pending timed work such
as cursor blinking, recovery, and log following. An idle window with no such
work must wait for input instead of continually redrawing.

**Positions are characters.** Editing code uses Unicode scalar (character)
indices; convert to bytes with `advanced::byte` only at slicing. Test with CRLF
text, text without a final newline, and non-Latin text.

**Pass arguments explicitly.** Use `Command::arg` for files and tool arguments.
Build systems deliberately run `shell_cmd` through the platform shell; expand
file variables with the build runner's quoting rules. Other operations must not
construct shell commands from file names. Debugger, Git, build, and NVIDIA tool
output is bounded.

**Git changes saved repository state.** Source Control requires the installed
Git executable (2.30+); no shell or embedded web UI is involved. Keep commands
on background workers and serialize repository actions. Clear inherited repository
location variables, including `GIT_COMMON_DIR`, object directories, namespaces,
and index/worktree overrides. Preserve the user's normal Git configuration.
Parse porcelain v2 with
NUL records so paths containing spaces, newlines, and non-UTF-8 bytes stay intact
on Unix. Reject embedded NUL bytes before constructing NUL-delimited path input,
so a single supplied path cannot inject additional stage/unstage selections.
Pass literal pathspecs only to path-taking commands: applying
`--literal-pathspecs` globally breaks `stash --include-untracked` cleanup on Git
2.43. Preserve first-parent semantics for merge-commit file diffs. The history
graph follows object parents, not display order alone.

Block stage actions for unsaved selected files and worktree mutations for unsaved
repository buffers; allow index-only unstaging. Never replace dirty buffers with
Git output. Bind snapshots and operations to an exact canonical repository root,
so losing a nested repository cannot redirect an operation to its parent.
Reconcile file comparisons against refreshed status, and guard worker replies
with the requested selection. A cancelled picker is a separate result that
preserves the previous view. Partial staging and discard validate the reviewed
diff; conflict replacement validates working text and all reviewed index sides.
Include stdout and stderr in failed Git operation messages, since merge conflict
details may be printed to stdout. Skip symbolic branch aliases such as remote HEAD.
Derive branch names from their full namespace rather than `refname:short`, which
adds disambiguating prefixes when names collide. Use full remote refs for tracking,
reuse existing local upstreams, and disable implicit remote guessing for switches.
Surface commit detail errors only while that same commit is still selected.
Clone requests must expose their activity/cancel state. A failed or cancelled
clone reports its outcome while preserving the previous review, snapshot, and
commit draft; it must not apply its failed snapshot to the selected repository.
Cancelled repository workers also preserve the last review, then immediately
reconcile the repository with a fresh cancellation signal. Commit-history rows
must expose their selection state to accessibility clients.
Back up regular files before discard/conflict replacement, then report the
backup path. Keep existing Git gutter comparison independent of index operations.
Tests in `tests/git.rs` use disposable repositories and local bare remotes;
`app/source_control_tests.rs` exercises the native UI and unsaved-buffer guards.

The Git review components stay native: `app/source_control/file_tree.rs` caches
path-based trees and virtualizes visible rows, and `diff_view.rs` caches bounded
per-line syntax layouts. Character-change ranges are computed while parsing diffs
on background workers with a size limit and a shared time budget. Selected-line
patches are built against the index side, with recomputed hunk ranges and preserved
line endings; they never write working files. Selection indices refer to the
reviewed diff and must be cleared when that diff or file selection changes.
Pierre Diffs/Trees inform the interaction design; their DOM-based packages are
not dependencies of the native application.

Advanced queries live in `git/inspect.rs` and `app/source_control/inspect.rs`.
Branch/revision comparisons resolve both tips to immutable commit IDs before
listing files or generating patches. Merge-base mode changes the diff base, while
ahead/behind counts still describe the two tips. Comparison diffs are read-only;
never offer staging controls for them. Drop a selected comparison if a refreshed
tip moves. Scope and query identities guard background results, and changing
repositories clears the extra inspection state. File paths in history and blame
remain literal, including pathspec-like names and non-UTF-8 paths on Unix.

Line history uses committed text, bounds Git output, virtualizes visible rows,
and caches filtered row indices. Source previews bound individual line layouts
and share editor font/tab settings. Reflog recovery creates a branch without
switching the worktree. Undo uses an atomic reference transaction with soft-reset
semantics and preserves the index, working files, and unsaved buffers; rebase uses
the normal worktree mutation guard. Tag deletion uses compare-and-delete with the reviewed tag object ID.

Git UX helpers in `app/source_control/ux.rs` keep repository controls compact,
remember per-view locations, and build a filtered file-review queue. Cache queue
rows by snapshot/query identity and filters so idle frames do not re-sort large
repositories. Contextual links offer a return path; root changes clear all cached
locations. Route commit and previous/next review commands through the catalog so
palette, buttons, default shortcuts, and custom bindings share availability.
Background reads keep navigation enabled while mutations remain serialized;
completion notices describe actions without exposing raw command output.
Git dialogs focus/select the first input once, advance through URL forms with
Enter, and reserve explicit action buttons for destructive confirmations. Blame
validates UTF-8 source lines separately from raw filename metadata. The real-Git
suite also covers SHA-256 repositories, running-hook cancellation, lock failures,
remote divergence, and an opt-in test of the actual local-operation timeout.

Editor and Git code views share `editor_layout::Font` and `code_job` for font
metrics, spacing, gutters, and syntax layout. Git statuses use the active theme's
success/error/warning roles, and review selections use its selection roles.
Keep these surfaces tied to editor settings when adding display preferences.
Syntax highlighting supplies colors only: `code_job` enforces the editor font
on every section, including unhighlighted fallback text. Typography tests must
compare resolved glyph metrics after font-family changes, not only FontIds.

**Large files are paged.** Files over 2 MiB load in about 256 KiB pages;
commands that need the whole text must check `page.info.paged`.

## Themes and branding

`src/theme.rs` declares every color role once in the `roles!` table and holds
the built-in themes, one row per theme. To add a role, add a row and a color to
every built-in (plus a `DERIVED` fallback if older theme files need one). To add a
built-in theme, add its row and regenerate the files in `themes/`:

```sh
HEDIT_WRITE_THEMES=1 cargo test --locked --test themes
```

The theme tests require readable contrast and a `themes/` folder that matches
the catalog. The [palette credits](assets/licenses/theme-palettes.md) record
upstream sources and licenses; bundle their notices with any new adaptations.

The name is lowercase **hedit** and the tagline is **Keep your train of thought.**
Brand SVGs in `assets/` are generated by `python3 scripts/generate-brand.py`; keep
their geometry in step with `src/brand.rs`. Dark/light in a file name describes
the background it is for.

## Packaging and releases

`make package` builds a Linux tarball and its SHA-256 file with the binary, the
three documents, the license, `assets/`, themes, the CUDA example, a desktop file,
and an installer script. `Makefile`, `scripts/package-linux.py`, and
`packaging/linux/install.sh` must install the same paths; update
`scripts/smoke-package.py` when the layout changes.

The CI workflow runs Linux, macOS, and Windows checks on pushes and pull requests. Linux and
macOS use Makefile targets; Windows uses Cargo and Python directly, without
requiring GNU Make. The release workflow first runs the shared Linux, macOS,
and Windows validation jobs; all package builds wait for that lint/test gate.
Pushing a version tag runs the release workflow, which builds
Linux x86-64 and ARM64 tarballs, the universal macOS app, and a Windows x86-64 ZIP, tests them, and
publishes them with checksums. Tags with a hyphen (`vX.Y.Z-rc.1`) publish as
prereleases.

To release:

1. Set the version in `Cargo.toml`, run `cargo check` to update `Cargo.lock`, and
   date the version's section in `CHANGELOG.md`.
2. Run `make ci` and `make release-check RELEASE_TAG=vX.Y.Z`. The release check
   also requires a dated, nonempty changelog entry. Preview the GitHub release
   description with `RELEASE_TAG=vX.Y.Z python3 scripts/check-release.py --notes-file /tmp/hedit-release-notes.md`.
3. Commit, push, then tag:

   ```sh
   git tag -a vX.Y.Z -m "hedit X.Y.Z"
   git push origin vX.Y.Z
   ```

The publishing workflow uses the matching changelog entry as the GitHub release
body, including when retrying an existing draft. Keep that entry ready for readers.
The changelog lists only changes users of the previous release will notice: a
bug fixed during development of a feature that has not shipped is not a fix.

The in-app updater (`Help → Check for updates…`) never downloads or installs
without the user asking. It checks the SHA-256 file from the same release, runs
the new binary's `--version` before installing, rejects unsafe archive entries
before extracting, and swaps the Linux binary in last, rolling back on failure.
On macOS it selects the universal ZIP, validates bundle metadata and the code
signature, and atomically exchanges the complete app bundle with `RENAME_SWAP`.
Staging happens beside the app; read-only and translocated apps use the manual
download fallback. The current process stays open until the user chooses Restart
or quits. Restart follows the normal successful-checkpoint shutdown path, then
relaunches the executable path captured before the update with only the saved
session directory. The old UI and its recovery lock must be dropped before the
new process starts; startup file, stdin, and wait arguments must not be replayed.
macOS release signatures are currently ad hoc, so this checks bundle integrity rather
than a Developer ID identity or notarization.
`install.sh` is served from `main`, not from a release, so keep it compatible with
the asset names of published releases.

## Provenance and license

The file engine, session foundation, and folder utilities derive from
[rsedit](https://github.com/goodappsstore/rsedit) (commit
`63a9ee4966d3fe1cfcd7d4af70e963e10aa297ff`); keep its MIT notice in
[LICENSE](LICENSE). Syntax definitions come from the `two-face` crate, each under
its own permissive license.

The spell checker's en_US dictionary (`assets/dictionaries/en_US.aff` and
`en_US.dic`) is from SCOWL and is built into the binary. Its notices are in
[assets/dictionaries/en_US-LICENSE.txt](assets/dictionaries/en_US-LICENSE.txt),
which ships with the packages; keep the two together.

Math rendering uses `latex-rust` and its embedded STIX Two Math font. Keep the
[font license](assets/licenses/STIX-Two-Math-OFL.txt),
[renderer notice](assets/licenses/latex-rust-NOTICE.txt),
[MIT license](assets/licenses/latex-rust-MIT.txt),
[modified color table](assets/licenses/latex-rust-dvipsnames.tsv), and its
[LPPL license](assets/licenses/LPPL-1.3c.txt) in distributed
packages. Mermaid uses `mermaid-rs-renderer`; `resvg` rasterizes both kinds of SVG
on worker threads without a browser or network fetches. Keep the
[Mermaid MIT notice](assets/licenses/mermaid-rs-renderer-MIT.txt) and
[resvg MIT notice](assets/licenses/resvg-MIT.txt) with the packages too.

The native Markdown renderer includes a local dependency patch for document
layout; see [the patch notes](vendor/egui_commonmark/PATCH.md). Keep its
[MIT notice](assets/licenses/egui_commonmark-MIT.txt) with distributed packages.

Document views use bundled Noto Sans regular, semibold, italic, and semibold
italic fonts, plus Noto Sans Symbols for symbols. Editor and application fonts keep their existing settings.
Keep the [Noto font license](assets/licenses/Noto-Sans-OFL.txt) and font files
under `assets/fonts/` with distributed packages. The source fonts come from
[the Noto project](https://github.com/notofonts/noto-fonts).

Terminal RTL runs use Rustybuzz for OpenType shaping, with each glyph cluster
fitted to its original Ghostty cells. Unicode bidi layout supplies reversible
cell maps for the cursor and selection. The original text and VT stream are
unchanged. Keep the bundled Noto Sans Arabic font and its
[OFL notice](assets/licenses/Noto-Sans-Arabic-OFL.txt) in distributed packages;
the source is [Google Fonts](https://github.com/google/fonts/tree/main/ofl/notosansarabic).
