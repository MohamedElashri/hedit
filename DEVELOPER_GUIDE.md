# hedit developer guide

Build, maintain, and contribute to hedit. For using the application, see the
[user guide](USER_GUIDE.md).

- [Build and install](#build-and-install)
- [Checks and visual review](#checks-and-visual-review)
- [macOS builds](#macos-builds)
- [Architecture](#architecture)
- [Recovery and compatibility](#recovery-and-compatibility)
- [CUDA transport](#cuda-transport)
- [Themes and branding](#themes-and-branding)
- [Packaging and CI](#packaging-and-ci)
- [Manual updater](#manual-updater)
- [Contribution guidelines](#contribution-guidelines)
- [Roadmap](#roadmap)
- [Provenance and license](#provenance-and-license)

## Build and install

Requires Rust 1.92 or newer, GNU Make, and Python 3.11 or newer for development
scripts. CI and release builds use Rust 1.92.0 and Python 3.12; use those versions
for matching local checks. The application uses eframe/egui with OpenGL on Linux
and builds through Cargo. Keep the build free of a web frontend, Node.js, and npm.

On Debian/Ubuntu:

```sh
sudo apt-get install build-essential pkg-config libx11-dev libxkbcommon-dev libwayland-dev libegl1-mesa-dev
make dev
make run ARGS='--session-dir /tmp/hedit-development'
```

All editor launches, including `make run`, re-exec in an independent OS session, disconnect terminal
streams, and return after native window initialization. The child alone acquires
the recovery lock; stdin is handed off through a private temporary spool and
checkpointed before window creation. Startup failures return to the launcher;
later stderr diagnostics append to `hedit.log` in the session directory.

X11 and Wayland are enabled. Native file dialogs use the desktop's XDG portal.
CUDA and NVIDIA drivers are optional external tools, not editor build dependencies.
Linux is runtime-validated. Universal macOS packaging is configured, with native
validation pending; Windows remains unverified.

```sh
make build
make install
make package
```

`make install` defaults to `~/.local`; override `PREFIX` with an absolute path.
Use a staging root to inspect installation without changing your home:

```sh
make install DESTDIR=/tmp/hedit-stage PREFIX=/usr
/tmp/hedit-stage/usr/bin/hedit --version
```

Development scripts use Python's standard library; the installed application
does not require Python. Run `make` or `make help` to list the available commands.

## Checks and visual review

```sh
make ci
```

Run `make ci` **before committing**. Both the pull-request workflow and each
release build run this exact target. It checks formatting, clippy warnings,
the native-only policy, documentation links, Python/shell syntax, Rust tests,
and offline installer tests. It then builds a release, creates the archive and
checksum, and verifies package extraction, installation to a temporary prefix,
and the installed executable. A failed command stops the check with a nonzero exit.

Use narrower targets while working:

| Command | Purpose |
| --- | --- |
| `make help` | List targets and configurable variables; also the default for `make` |
| `make dev` / `make build` | Debug / release build |
| `make run ARGS='--new'` | Run the editor with arguments |
| `make fmt` / `make fmt-check` | Apply / check Rust formatting |
| `make lint` | All static checks, including clippy with warnings denied |
| `make docs` | Check links, images, and local headings |
| `make test` | Rust tests and offline release-installer tests |
| `make check` | Lint and tests without a release build |
| `make package` | Build and package a release under `dist/` |
| `make smoke` | Build and test a package in a temporary installation |
| `make ci` | Full pre-commit and GitHub CI validation |
| `make release-check RELEASE_TAG=v0.1.3` | Validate a release tag against `Cargo.toml` |
| `make workflow-lint` | Optional workflow validation; requires `actionlint` |
| `make clean` | Remove Cargo build artifacts |

Override `CARGO` or `PYTHON` when needed. `PREFIX` and `DESTDIR` control installation;
`ARGS` passes editor arguments. `ACTIONLINT` can point to a specific actionlint
executable. `make clean` keeps installed files, release archives, and session data.

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

For a repeatable CPU timing check, run
`cargo test --locked --bin hedit profile_syntax_picker -- --ignored --nocapture`.
This opens the picker over a 231 KB CUDA document and reports the mean time for
20 hover frames. It measures headless rendering work, not desktop input latency.

## macOS builds

The release workflow produces one `hedit-0.1.3-macos-universal.zip` containing
`hedit.app`, with native Apple Silicon and Intel slices. The deployment target is
macOS 11.0. On a Mac with Xcode's command-line tools and Rust installed:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
make macos
```

`make macos` builds both release executables. `scripts/package-macos.py` verifies
the input architectures, combines them with `lipo`, creates Info.plist and an
ICNS icon, copies themes/examples/guides, signs and verifies the app, checks its
version, and archives it with `ditto`. A failed assembly leaves any previous ZIP
intact. A `.sha256` file accompanies the ZIP. Apple's
[universal binary documentation](https://developer.apple.com/documentation/apple-silicon/building-a-universal-macos-binary)
describes the architecture slices.

The default signature is ad-hoc. For an available Developer ID identity, package
already-built slices with:

```sh
python3 scripts/package-macos.py --identity 'Developer ID Application: Your Name (TEAMID)'
```

This enables hardened runtime and a timestamp. Notarization and ticket stapling
are separate steps and are not yet automated. Follow Apple's
[notarization documentation](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)
before treating downloaded bundles as ready for normal public distribution.
Do not disable system-wide Gatekeeper settings to test an app.

The [Universal macOS build](.github/workflows/macos-probe.yml) workflow can run
manually and is also called by release automation. One Apple Silicon `macos-15`
runner checks and lints both targets, runs native editor/recovery tests, builds
both slices, and uploads one universal app archive. It unpacks that archive,
verifies its signature and both architectures, and runs the CLI; when Rosetta is
available it also runs the Intel slice. Runner architectures are documented by
[GitHub](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
It does not create two Mac downloads.

The native test list excludes the library updater suite, whose Unix tests still
expect Linux installation layouts, GNU tar, and `scripts/package-linux.py`.
Use the explicit test command in the workflow on macOS rather than `make ci`.
Linux runs the full suite, offline installer tests, and mocked macOS packaging
contract tests. Folder tests compare resolved paths, including temporary
directories reached through symlinks.

Linux source checks and strict Clippy checks pass for both Mac targets. These
checks do not link Mach-O code or validate AppKit. The first native workflow run
and GUI review are still pending; Linux remains the runtime-validated platform.
Before calling Mac support stable, check Open/Save As, multiple folders, clipboard,
Retina rendering, Cmd+Q recovery, Dock reopen, Finder document events, external
changes, and atomic saves on APFS (including case-insensitive volumes), then
launch an actually downloaded archive on a clean Mac.

Mac shortcuts display Cmd/Option. The bundle identifier remains `hedit.hedit`,
with recovery under `~/Library/Application Support/hedit.hedit/session/`.
Updates currently support Linux only; Mac upgrades replace the app manually.
Local CUDA debugging, Compute Sanitizer, and NVIDIA GPU commands are hidden and
blocked at their backend entry points on macOS. CUDA source highlighting,
editing, and line-comment commands remain available. NVIDIA identifies CUDA
10.2 as the final Mac release in its
[release notes](https://docs.nvidia.com/cuda/archive/10.2/cuda-toolkit-release-notes/index.html).

## Architecture

| Module | Responsibility |
| --- | --- |
| `src/app.rs` | Editor rendering, shortcuts, background jobs, checkpoint status, exit gating |
| `src/app/chrome.rs` | Menus, tabs, sidebar, status bar, bottom search panel |
| `src/app/editor_layout.rs` | One-entry cache of the displayed page's highlighted text layout |
| `src/app/commands.rs` | Shared menu/shortcut commands and keyboard-driven palette |
| `src/app/quality.rs`, `quality_windows.rs` | Project/search/file workflows, settings, comparisons, navigation, completion and native dialogs |
| `src/search.rs` | Shared Unicode regex engine, ignore-aware folder traversal and guarded replacement plans |
| `src/projects.rs` | Project/workspace JSON, formatting precedence and EditorConfig |
| `src/intelligence.rs` | Source-line symbols, completion, snippet expansion and read-only Git transport |
| `src/text_encoding.rs` | Lossless UTF-8, UTF-16 and explicit legacy encoding conversion |
| `src/ipc.rs`, `src/launcher.rs` | Per-session request mailbox, startup acknowledgements and terminal detachment |
| `src/update.rs`, `src/update/install.rs` | Manual release checks, checksum verification, archive preflight, staged installation and rollback |
| `src/app/updates.rs` | Explicit check/install actions, worker progress, restart guidance and close gating |
| `src/app/preferences.rs` | Unified settings, theme controls, tool/recovery preferences, navigation dialogs |
| `src/workspace.rs` | Open/closed buffers, names, bookmarks, UTF-8-safe edit splices |
| `src/document.rs` | Disk-backed piece table, bounded pages, source guards, atomic saves, undo |
| `src/session.rs` | Process lock, snapshots, manifests, fallback and preservation |
| `src/advanced.rs` | Multi-selection edits, indentation heuristics, folding/tab projections |
| `src/app/panes.rs` | Shared-document split views and independent view positions |
| `src/app/advanced_edit.rs` | Occurrence commands, multi-cursor input, and folding |
| `src/editing.rs` | Unicode-safe line/text transformations used by Edit commands |
| `src/app/edit_actions.rs` | Apply transformations through backend undo/recovery |
| `src/debugger.rs` | CUDA-GDB/MI and bounded NVIDIA tool subprocess output |
| `src/syntax.rs`, `src/syntax/lexer.rs`, `src/syntax/keywords.rs` | Syntax catalog, filename detection, and bounded lexical highlighting with active theme colors |
| `src/theme.rs` | Validated RGB palettes, native styling, JSON theme files |
| `src/brand.rs` | Native mark and antialiased launcher icon |
| `src/cli.rs` | Arguments and file locations |
| `src/folder.rs` | Bounded directory listings, fuzzy path search, safe file/folder creation |
| `src/app/project.rs` | Multiple folder roots, lazy trees, background listings/search, creation dialog |
| `src/app/tree_row.rs` | Native tree rows with vector folder/document icons |
| `src/preview.rs` | Test-only headless SVG export |

Documents live behind shared mutexes. The UI applies minimal UTF-8-safe splices;
opening, saving, searching, and paging run on workers with conflicting edits
disabled. One foreground I/O job and one checkpoint can run at a time. Folder listings
and quick-open scans use separate workers so they leave editing available.
Generation tags discard stale results after adding/removing a root or refreshing. Directory
listing concurrency is capped at eight; quick open runs one scan at a time with
a 120 ms typing debounce. Quick open shares a 50,000-entry, one-second and
80-result budget across roots, and deduplicates overlapping paths.
`Workspace.folders` persists root paths, expanded paths, collapsed roots and
folder-section visibility. The legacy optional `folder` metadata migrates into
the new `folders` collection and remains in checkpoints for compatibility.
Whole-sidebar visibility is an independent window preference. Tree icons use
native vector shapes instead of font glyphs. Search
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
visible edit is rescued into an independent unsaved file.

The renderer uses a bounded standard text widget. Tab stops, selections, and
folding use the source/display projection in `src/advanced.rs`. Continuous
whole-file layout remains future work for paged buffers.

`Language` defines stable syntax IDs, display names, and filename associations
in one catalog. It includes separate C, C++, and CUDA modes. The status-bar menu
filters the catalog by name, extension, or special filename, and scales its
scrolling list to the available window height.
The picker is anchored above its status-bar button. Filtered results are cached
until the query changes, and a stable scroll area draws only visible rows.
`Buffer::language()` resolves a recognized `TabView.language_override` first,
then the current filename; unknown IDs fall back to detection. A syntax change
only updates view metadata and the checkpoint generation, preserving text,
selection, scroll position, and undo history. No session schema change is needed.
The status-bar menu and editor renderer use the same resolved language.

The editor retains one highlighted layout and checks the exact text, syntax,
font size, wrap width, theme, display scale, and font atlas before reusing it. This avoids
lexing and hashing a large layout again during menu hovers. The cache holds only
the displayed page and invalidates immediately on edits or visual changes.

Sorted keyword tables provide language-specific lookup, including case-insensitive
keywords where appropriate, without allocations. Lexers work on the displayed
page, merge adjacent equal-color tokens, and use the active theme's syntax roles (plus severity colors for logs). They preserve
source bytes and operate without plugins or runtime downloads. They are lexical
approximations, not full language grammars: embedded languages and multiline
state across page boundaries remain future work. Add syntax IDs, associations,
token-color tests, and recovery tests together when extending support.
Every registered extension and special filename is checked against detection;
format samples, Unicode fragments, manual syntax persistence, filtered selection,
and compact-menu geometry are covered by tests.

## Recovery and compatibility

| Session entry | Purpose |
| --- | --- |
| `session.lock` | Process-held storage lock |
| `session.json` | Latest committed manifest |
| `session.previous.json` | Previous valid manifest |
| `buffers-*/` | Recovery text or piece sources referenced by manifests |
| `window.ron` | Window geometry, theme, editor layout, font size, CUDA-GDB path |

A checkpoint syncs buffer data and its directory, writes and syncs the manifest,
retains the previous manifest, then atomically replaces the current one. Unix
directory entries are synced. Generations referenced by either manifest survive
reclamation; unchanged documents reuse snapshots.

The previous committed manifest stays authoritative until replacement. Failed
restores keep their references, and failed checkpoints preserve earlier recovery
data. Dirty and unnamed buffers are snapshotted. Clean named files reopen from
disk; missing clean files can be dropped. Externally growing clean logs must not
prevent unrelated unsaved files from checkpointing.

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
color cue. Keep preferences in the unified Editor, Appearance, CUDA Tools, and
Recovery pages. Menus, shortcuts, and the palette should route through the same
command dispatcher so availability and behavior stay consistent. The line-number
gutter follows laid-out rows, skips wrap continuations, and remains fixed during
horizontal scrolling; paged documents use page-relative line numbers.

## Packaging and CI

`make package` writes a versioned Linux tarball and SHA-256 file under `dist/`.
It includes the binary, three documentation entry points, license, SVG
assets, themes, CUDA example, desktop entry, and shell installer. The installer
defaults to `~/.local` and accepts an absolute prefix. Architecture follows the
build host; newer Linux builds may require newer destination libraries.

`Makefile`, `scripts/package-linux.py`, and `packaging/linux/install.sh` must keep
installed paths consistent. Update `scripts/smoke-package.py` when changing the
archive or documentation layout. `scripts/check-docs.py` validates document links
and local section anchors.

The Linux CI workflow and Linux release matrix both call `make ci`; keep checks
in the Makefile instead of duplicating their commands in workflow YAML. Workflow
steps set up dependencies and the pinned Rust/Python versions. Offline installer tests cover public
and authenticated downloads, version/architecture selection, checksum rejection,
failed downloads, and preservation of an existing installation on download failure.

### Publish a release

The `Release packages` workflow builds Linux x86-64 and ARM64 natively on Ubuntu
24.04 and one universal macOS app through its reusable Mac workflow. It publishes when a version tag is pushed; the tag must equal `v` followed
by the version in `Cargo.toml`. Builds run checks and package smoke tests before
publication. GitHub generates release notes, and each release includes two Linux
tarballs, one universal macOS ZIP, their SHA-256 files, and the Linux `install.sh`
downloader. The publish job waits for all three packages.

1. Set the version in `Cargo.toml` and run `cargo check` to update `Cargo.lock`.
2. Run `make ci` and `make release-check RELEASE_TAG=v0.1.3`, using the new tag.
3. Commit the version, lockfile, and release changes, then push the commit.
4. Create and push an annotated tag matching the version:

   ```sh
   git tag -a v0.1.3 -m "hedit 0.1.3"
   git push origin v0.1.3
   ```

The workflow uses GitHub's `GITHUB_TOKEN` with write permission only in the
publication job; no personal token secret is needed. Actions must be enabled and
repository/organization policy must permit that permission. A private repository
keeps its releases private; installation uses the authenticated steps in the
[user guide](USER_GUIDE.md#private-repository-access).

Assets are uploaded to a draft before it is published. If upload/publication
fails, rerun the failed workflow jobs to finish the draft. Published releases are
left intact; use a new version tag for subsequent changes. Tags containing a
hyphen, such as `v0.2.0-rc.1`, publish as prereleases and do not become Latest.
Stable tags become Latest. The default installer selects Latest; `--version`
selects a specific stable or prerelease tag.

Running the workflow manually on a branch produces downloadable Actions artifacts
without publishing a release. Running it on a version tag also publishes that tag.
See GitHub's [release command documentation](https://cli.github.com/manual/gh_release_create)
for draft and publication behavior.

## Manual updater

The updater is introduced in 0.1.1. Existing 0.1.0 binaries need one external
upgrade; adding code here does not retrofit the published release.

`Help → Check for updates…` starts a worker for GitHub's latest stable release
API. Constructors, frame polling, and timers do not initiate network work. A
separate **Install update** action is required after review. The UI never installs
from a successful check alone and never restarts the application. Private access
uses `gh api --hostname github.com`; public access uses curl with HTTPS-only
redirects and bounded downloads. Both paths pin the selected tag/asset IDs.

Use semantic version precedence, ignore build metadata for update ordering, and
reject drafts, prereleases, downgrades, duplicate assets, and oversized responses.
SHA-256 verification is against the checksum asset from the same GitHub release;
this is transport/repository trust, not an independent signing-key mechanism.

GNU tar supplies parsing and extraction. Preflight its resolved numeric-owner
listing in the C locale, retaining absolute names for validation. Reject links,
special files, duplicate paths, traversal, nonportable names, too many entries,
and excessive unpacked size before extraction into a private directory. Release
archive filenames must use ASCII letters/digits, `/`, `_`, `.`, `-`, or `+`.
Clear `TAR_OPTIONS`; pass arguments directly without shell interpolation. Never
execute a downloaded installer script. Run the verified new binary with
`--version` and check its exact version before installation.

Recognize prefix and extracted-package layouts from the executable location.
Reject source builds and unwritable or symlinked resource paths. An OS file lock
serializes installers; a captured executable identity detects outside replacement.
Stage new resources on the destination filesystem, preserving additional custom
files. Commit resource directories/files first, then atomically rename the new
binary last. Linux keeps the running executable inode alive until exit. Session
storage is never part of the installation transaction. On ordinary I/O errors,
restore committed paths in reverse order. Preserve staging backups and report
their location if rollback fails. This is not a power-loss journal for every
resource directory, so do not claim a fully atomic multi-path update.

The UI prevents process exit while its installation worker is active, even if
the dialog is hidden. Once the worker finishes, normal checkpoint/close gating
continues. Updates use no elevation and store no GitHub credentials.

Tests cover version ordering, release metadata, public/private transport fixtures,
checksum rejection, real GNU tar preflight, compatibility checks, locks, external
replacement, symlinks, injected rollback failure paths, portable/prefix layouts,
preservation of custom themes and session data, production packager compatibility,
manual-only UI behavior, compact layouts, and exit gating. They run through
`make test` and `make ci` without contacting GitHub. Runtime update requirements
are curl or authenticated GitHub CLI, plus GNU tar; Python is only a test/build
dependency.

## Contribution guidelines

Keep documentation in three entry points: `README.md` is the user landing page,
`USER_GUIDE.md` explains product use, and this file holds development, architecture,
branding, and roadmap details. Avoid creating separate topic guides that scatter
these subjects again.

Test changes that can lose buffers, overwrite external edits, or invalidate
recovery references. Keep tests isolated in temporary directories. Run `make ci`
before committing, and fix any failed checks. Preserve
upstream license notices and state actual limits rather than presenting planned
features as available.

## Roadmap

### Delivered

Native editor and Cargo build; periodic recovery with fallback/preservation;
named unsaved files and a searchable shelf; guarded atomic saves; bounded large-file
pages, log following, literal search, and bookmarks; bounded persistent undo; automatic
syntax detection and persistent per-buffer syntax selection; customizable themes
and SVG identity; persistent folder trees with multiple roots, file/subfolder creation, and fuzzy
quick open; CUDA tools; Linux packaging
and consolidated user/developer guides.

### Reliability and daily use

- Durable edit journal, recovery checksums, filesystem fault injection, and soak tests.
- Bookmark anchors that follow undo/external changes.
- Streaming encoding and line-ending conversion for large paged files.
- Continuous large-file scrolling, global line indexes, and log filtering.
- X11/Wayland visual validation, accessibility, DEB/RPM packages, and release checks.

### Editor and GPU workbench

#### Edit and View comparison with Sublime Text

The v0.1.3 menus cover common line operations, comments, case conversion,
indentation conversion, LF/CRLF conversion, visibility toggles, column rulers,
wrap columns, zoom, and focus modes. They do not establish full Sublime parity.
The remaining controls from the comparison are now implemented as follows.

| Control | v0.1.3 implementation | Current limits |
| --- | --- | --- |
| Multiple cursors and rectangular selection | Occurrence selection, Alt+click/Alt+Shift+drag, batch typing/clipboard/deletion, cursor undo and recovery | Up to 1,000 selections; fully loaded files |
| Split panes | Two columns or rows, focus/move actions, independent view positions on shared documents | Two groups; paged views share their loaded page |
| Indentation | Automatic detection, smart Enter, soft Backspace, 2/4/8 tab rendering and insertion | Detection examines 10,000 lines; smart Enter uses bracket heuristics |
| Folding and guides | Indentation blocks, manual selections, fold/unfold all, original line numbers | Fully loaded files; edits invalidate fold ranges |
| Minimap | Click/drag scrolling, sampled viewport overview | Current page only for paged files |

The comparison uses Sublime's official descriptions of
[multiple selections and split views](https://www.sublimetext.com/),
[column selection](https://www.sublimetext.com/docs/column_selection.html),
[indentation settings](https://www.sublimetext.com/docs/indentation.html),
[rulers and guides](https://www.sublimetext.com/docs/color_schemes.html), and
[distraction-free mode](https://www.sublimetext.com/docs/distraction_free.html).
Its [API reference](https://www.sublimetext.com/docs/api_reference.html) also
describes folding and minimap controls. Keep unavailable commands out of the
menus until their underlying editing behavior is implemented.

- Richer split layouts and syntax-aware folding across pages.
- More language lexers and incremental multiline state across pages.
- NVIDIA hardware/driver testing, source synchronization, gutter breakpoints,
  structured variables/watches, GPU execution views, and remote setup workflows.

### Daily-use features in 0.1.3

Terminal launches always detach. A child first probes the existing recovery lock;
when another process owns it, the child submits an atomic mailbox request under
`session/requests`. Requests retain native path bytes and resolve relative paths
against the invoking directory. The GUI imports stdin on a worker, opens queued
paths, checkpoints, and acknowledges. Only the session owner writes snapshots.
Lock destruction explicitly unlocks before closing, preventing a concurrently
forked child from briefly retaining an inherited recovery lock before exec.

Search uses the shared regex engine with Unicode boundaries and capture-group
replacement. Folder workers honor Git ignore files, deduplicate overlapping
roots, skip personal `.hedit-workspace` files, and cap files/results/retained text.
Replacement plans fingerprint original bytes. Applying rechecks disk and buffer
state, skips dirty/followed buffers, and commits candidate documents only after a
successful guarded save. Changes across several files are individually atomic,
with partial success reported; unopened files have no retained editor undo.

Project definitions hold roots and settings; the adjacent workspace holds bounded
personal text/views and remaps restored document IDs in split-pane state. Current
unsaved buffers go to the shelf before switching. First-save formatting resolves
rules against the chosen destination, and cleanup uses the normal undo engine.
User and language defaults are overlaid by project settings, then matching parent
EditorConfig sections. EditorConfig support covers common formatting properties
and globs with indentation widths of 1–8; it is not a full conformance claim.

Internal pieces and recovery files stay UTF-8. Encoding metadata controls disk
output; UTF-16 conversion is explicit or BOM-detected and legacy encodings are
never guessed. Conversion is bounded at 32 MiB and refuses lossy output. Format
state, including encoding/BOM, follows undo. Fully loaded documents checkpoint up
to 64 undo/redo states and 32 MiB in each direction. Historical text is coalesced
into immutable, content-addressed sources so typing does not multiply open file
descriptors or manifest entries. Paged history remains process-local.

Git uses bounded read-only subprocesses and background refresh. Symbol navigation
uses source-line heuristics; completion uses open buffers, and snippets support
selection insertion plus a final cursor. These features do not require an LSP.
Run `cargo test quality` for the backend/UI contracts. To inspect native layouts:

```sh
HEDIT_PREVIEW_DIR=/tmp/hedit-quality-previews cargo test --bin hedit export_quality_previews -- --ignored
```

## Provenance and license

The file engine, session foundation, and folder utilities derive from
[rsedit](https://github.com/goodappsstore/rsedit), based on commit
`63a9ee4966d3fe1cfcd7d4af70e963e10aa297ff`. hedit's native UI, workspace layer,
branding, and development direction are independent work. Preserve the original
MIT notices and attribution in [LICENSE](LICENSE).
