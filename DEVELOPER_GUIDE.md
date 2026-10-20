# hedit developer guide

Build, maintain, and contribute to hedit. For using the application, see the
[user guide](USER_GUIDE.md).

- [Build and install](#build-and-install)
- [Checks and visual review](#checks-and-visual-review)
- [Architecture](#architecture)
- [Recovery and compatibility](#recovery-and-compatibility)
- [CUDA transport](#cuda-transport)
- [Themes and branding](#themes-and-branding)
- [Packaging and CI](#packaging-and-ci)
- [Contribution guidelines](#contribution-guidelines)
- [Roadmap](#roadmap)
- [Provenance and license](#provenance-and-license)

## Build and install

Requires Rust 1.92 or newer. The application uses eframe/egui with OpenGL on Linux
and builds through Cargo. Keep the build free of a web frontend, Node.js, and npm.

On Debian/Ubuntu:

```sh
sudo apt-get install build-essential pkg-config libx11-dev libxkbcommon-dev libwayland-dev libegl1-mesa-dev
cargo build --locked
cargo run --locked -- --session-dir /tmp/hedit-development
```

X11 and Wayland are enabled. Native file dialogs use the desktop's XDG portal.
CUDA and NVIDIA drivers are optional external tools, not editor build dependencies.
The current validated build target is Linux; macOS and Windows remain unverified.

```sh
cargo build --release --locked
make install
make package
```

`make install` defaults to `~/.local`; override `PREFIX` with an absolute path.
Use a staging root to inspect installation without changing your home:

```sh
make install DESTDIR=/tmp/hedit-stage PREFIX=/usr
/tmp/hedit-stage/usr/bin/hedit --version
```

Packaging and check scripts use Python 3.11 or newer and the standard library;
the installed application does not require Python.

## Checks and visual review

```sh
make check
python3 scripts/smoke-package.py
```

`make check` validates the native-only policy, Markdown links, Rust formatting,
clippy warnings, and tests. The package smoke test requires a built release
binary; it checks the checksum, extracts the archive, installs to a temporary
prefix, and runs the installed executable.

Tests cover unsaved/closed-buffer recovery, custom names, bookmarks, old session
metadata, process locks, corrupt manifests, failed restores/checkpoints, external
writes, atomic saves, Unicode, search boundaries, themes, and debugger transport.
A sparse 3 GiB file exercises bounded opening. A child process is killed after a
checkpoint to verify persistence and lock release.

### Headless layout review

```sh
HEDIT_PREVIEW_DIR=/tmp/hedit-previews cargo test --locked --bin hedit export_theme_previews -- --ignored
rsvg-convert /tmp/hedit-previews/hedit-paper-appearance.svg -o /tmp/hedit-paper.png
```

These SVG exports preserve egui layout and colors but substitute system fonts
and omit some GPU meshes/effects. They complement desktop testing. X11/Wayland
visual checks, accessibility review, and NVIDIA hardware validation remain open.

## Architecture

| Module | Responsibility |
| --- | --- |
| `src/app.rs` | Native UI, shortcuts, background jobs, checkpoint status, exit gating |
| `src/workspace.rs` | Open/closed buffers, names, bookmarks, UTF-8-safe edit splices |
| `src/document.rs` | Disk-backed piece table, bounded pages, source guards, atomic saves, undo |
| `src/session.rs` | Process lock, snapshots, manifests, fallback and preservation |
| `src/debugger.rs` | CUDA-GDB/MI and bounded NVIDIA tool subprocess output |
| `src/syntax.rs` | C/CUDA lexical highlighting with active theme colors |
| `src/theme.rs` | Validated RGB palettes, native styling, JSON theme files |
| `src/brand.rs` | Native mark and antialiased launcher icon |
| `src/cli.rs` | Arguments and file locations |
| `src/folder.rs` | Retained bounded folder utilities, not yet connected to the UI |
| `src/preview.rs` | Test-only headless SVG export |

Documents live behind shared mutexes. The UI applies minimal UTF-8-safe splices;
opening, saving, searching, and paging run on workers with conflicting edits
disabled. One foreground I/O job and one checkpoint can run at a time. Search
cancellation is checked between bounded batches.

Checkpoint workers capture immutable piece references and stream data outside
the editing lock. A generation counter prevents an older checkpoint from being
reported as current after newer edits. Window close is cancelled until current
work and a final checkpoint complete; failures keep the window open. The exit
hook makes another recovery attempt during framework teardown.

Files above 2 MiB use approximately 256 KiB views. Unchanged bytes stay on disk;
dirty paged snapshots copy source pieces into private recovery storage. Completed
copies can protect live pieces and history from later source changes. Before a
copy completes, unseen externally changed bytes cannot be reconstructed; a failed
visible edit is rescued into an independent scratchpad.

The renderer is a bounded standard text widget. Continuous document layout,
multiple selections, and folding require further editor work.

## Recovery and compatibility

| Session entry | Purpose |
| --- | --- |
| `session.lock` | Process-held storage lock |
| `session.json` | Latest committed manifest |
| `session.previous.json` | Previous valid manifest |
| `buffers-*/` | Recovery text or piece sources referenced by manifests |
| `window.ron` | Window geometry and appearance preferences |

A checkpoint syncs buffer data and its directory, writes and syncs the manifest,
retains the previous manifest, then atomically replaces the current one. Unix
directory entries are synced. Generations referenced by either manifest survive
reclamation; unchanged documents reuse snapshots.

The previous committed manifest stays authoritative until replacement. Failed
restores keep their references, and failed checkpoints preserve earlier recovery
data. Dirty and unnamed buffers are snapshotted. Clean named files reopen from
disk; missing clean files can be dropped. Externally growing clean logs must not
prevent unrelated scratchpads from checkpointing.

When the newest manifest fails, try the previous one. Archive the corrupt
manifest under `preserved-manifests/` and disable generation cleanup while that
marker exists. If both fail, preserve the old data under `preserved-*` before
starting fresh. Resume interrupted preservation on the next launch. Never delete
these archives automatically.

The shelf has 50 entries: only clean file-backed entries may be evicted. Undo is
bounded to 256 steps in the current process. Custom names and bookmarks use
serde defaults for compatibility with older sessions. Bookmark positions adjust
for ordinary splices but do not yet follow undo or external replacements.

Recovery is periodic, not a durable per-keystroke journal. Preserve source-change
guards and named-new-file no-clobber behavior. Add compatibility and failure tests
whenever changing persisted metadata or save/recovery behavior. Manual rescue
instructions belong in the [user guide](USER_GUIDE.md#recovery).

## CUDA transport

NVIDIA tools run as native subprocesses. CUDA-GDB uses bidirectional MI pipes;
stdout/stderr readers have bounded records and a bounded output queue. The
transcript retains 512 KiB. Run/stop notifications update session state.

Pass executable arguments through `Command::arg`, and quote console text as a
single MI C string. Do not introduce shell interpolation. Startup uses `--nx` to
avoid loading project or personal GDB init files. A pending `main` breakpoint is
added before Run. Stop requests debugger exit, then terminates an unresponsive
child; teardown reaps the process.

Compute Sanitizer and `nvidia-smi` use the same bounded-output model. The panel is
a textual workbench, without an interactive target terminal or source/variable
trees. Automated tests exercise transport with a subprocess fixture; actual
NVIDIA behavior still requires hardware testing.

## Themes and branding

Theme files contain a version, name, dark-controls flag, and opaque RGB roles.
Missing roles inherit Hedit Ink. Validate names, color strings, unknown fields,
versions, and the 64 KiB input limit before applying imports. Export through a
synced temporary file and atomic replacement. See the [user guide](USER_GUIDE.md#appearance)
for the format and controls, and `themes/` for complete presets.

### Identity assets

Use lowercase **hedit** and the tagline **Keep your train of thought.** The mark
combines a geometric h with stacked buffer lines and a cut shoulder. The wordmark
uses custom outlines; primary SVG assets have no font or bitmap dependency.

| Asset in `assets/` | Intended use |
| --- | --- |
| `hedit.svg` | Rounded application icon |
| `hedit-mark.svg` | Transparent symbol on dark backgrounds |
| `hedit-mark-mono.svg` | Single-color symbol using `currentColor` |
| `hedit-wordmark-dark.svg`, `hedit-wordmark-light.svg` | Standalone name |
| `hedit-lockup-dark.svg`, `hedit-lockup-light.svg` | Symbol and name |
| `hedit-logo-mono.svg` | Single-color lockup |
| `brand-board.svg` | Identity overview and palette |

Dark/light suffixes describe the background. Keep one stem-width of clear space.
Use the icon at 16 px or above, transparent mark at 24 px or above, and lockup at
160 px or above. Preserve proportions; use monochrome for single-ink work.

| Color | Value |
| --- | --- |
| Ink / Surface | `#10161F` / `#161E29` |
| Sea glass / Frost | `#64D8CB` / `#9AEBDF` |
| Slate | `#8DA0B4` |
| Paper / Deep teal | `#FAF8F3` / `#147568` |

Run `python3 scripts/generate-brand.py` to regenerate SVGs. Keep geometry aligned
with `src/brand.rs`. The brand board uses system text for labels; wordmarks are
paths. The launcher keeps fixed brand colors; the in-app mark uses theme colors.

Give the document most of the space. Use quiet borders, consistent controls,
monospace editing/tool output, and proportional UI text. Active tabs and buffer
rows need clear markers. Recovery must always have a written status as well as a
color cue.

## Packaging and CI

`make package` writes a versioned Linux tarball and SHA-256 file under `dist/`.
It includes the binary, three documentation entry points, license/notice, SVG
assets, themes, CUDA example, desktop entry, and shell installer. The installer
defaults to `~/.local` and accepts an absolute prefix. Architecture follows the
build host; newer Linux builds may require newer destination libraries.

`Makefile`, `scripts/package-linux.py`, and `packaging/linux/install.sh` must keep
installed paths consistent. Update `scripts/smoke-package.py` when changing the
archive or documentation layout. `scripts/check-docs.py` validates document links
and local section anchors.

Linux CI runs policy and documentation checks, formatting, clippy, tests, a
release build, and the package smoke test. The manual packaging workflow uploads
an archive artifact; it does not publish a release automatically.

## Contribution guidelines

Keep documentation in three entry points: `README.md` is the user landing page,
`USER_GUIDE.md` explains product use, and this file holds development, architecture,
branding, and roadmap details. Avoid creating separate topic guides that scatter
these subjects again.

Test changes that can lose buffers, overwrite external edits, or invalidate
recovery references. Keep tests isolated in temporary directories. Preserve
upstream license notices and state actual limits rather than presenting planned
features as available.

## Roadmap

### Delivered

Native editor and Cargo build; periodic recovery with fallback/preservation;
named scratchpads and a searchable shelf; guarded atomic saves; bounded large-file
pages, log following, literal search, and bookmarks; process-local undo; C/CUDA
highlighting; customizable themes and SVG identity; CUDA tools; Linux packaging
and consolidated user/developer guides.

### Reliability and daily use

- Durable edit journal, recovery checksums, filesystem fault injection, and soak tests.
- Persistent undo and bookmark anchors that follow undo/external changes.
- Explicit line-ending conversion and broader encoding support.
- Single-instance CLI forwarding, folder tree, and fuzzy file finder.
- Continuous large-file scrolling, global line indexes, and log filtering.
- X11/Wayland visual validation, accessibility, DEB/RPM packages, and release checks.

### Editor and GPU workbench

- Regex/case-insensitive search, replace-all controls, multiple cursors,
  rectangular selections, folding, and split views.
- More language lexers and incremental multiline state across pages.
- NVIDIA hardware/driver testing, source synchronization, gutter breakpoints,
  structured variables/watches, GPU execution views, and remote setup workflows.

## Provenance and license

The file engine, session foundation, and folder utilities derive from
[rsedit](https://github.com/goodappsstore/rsedit), based on commit
`63a9ee4966d3fe1cfcd7d4af70e963e10aa297ff`. hedit's native UI, workspace layer,
branding, and development direction are independent work. Preserve the original
MIT notices and attribution in [LICENSE](LICENSE) and [NOTICE](NOTICE).
