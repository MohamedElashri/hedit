# hedit developer guide

Build, test, and release hedit. For using it, see the [user guide](USER_GUIDE.md).

- [Build and install](#build-and-install)
- [Checks](#checks)
- [macOS builds](#macos-builds)
- [Windows builds](#windows-builds)
- [Code map](#code-map)
- [Terminal development](#terminal-development)
- [Rules to keep](#rules-to-keep)
- [Themes and branding](#themes-and-branding)
- [Packaging and releases](#packaging-and-releases)
- [Provenance and license](#provenance-and-license)

## Build and install

You need Rust 1.92 or newer, GNU Make, a C compiler (for the bundled Oniguruma
regex engine), and Python 3.11 or newer for the development scripts. CI uses
Rust 1.92.0 and Python 3.12. X11, Wayland, and OpenGL libraries load at run time,
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
session directory. Linux is the validated platform. macOS and Windows have
native CI build and test jobs; hands-on GUI validation is pending on both.

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

# CPU timings for the syntax menu and for typing with completions on
cargo test --locked --release --bin hedit profile_ -- --ignored --nocapture
```

The offline User Guide renders the repository's Markdown with the same native
renderer as file previews. Edit the source documents directly; do not keep a
second copy of their text in UI code. Rebuild the executable after documentation
changes. Add linked local documents to the bundled allowlist in
`app/user_guide.rs`; the reader never resolves links against the user's disk.
Internal links and Contents stay in the modal, and only explicit external-link
clicks open a browser. Keep Help, About, F1, the command palette, and key bindings
routed through `Command::UserGuide`.

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
| App features | One file each in `src/app/`, named after the feature: `auto_complete.rs`, `definitions.rs`, `spell_check.rs`, `macros.rs`, `snippet_fields.rs`, `find_tools.rs`, `changes.rs`, `fold_gutter.rs`, `font_picker.rs`, and so on |
| Integrated terminal | `terminal.rs` (Ghostty VT and bounded PTY IO); `app/terminal_panel.rs` (tab lifecycle, onboarding, input focus); `app/terminal_tabs.rs` (compact tab strip, reordering, menus); `app/terminal_text.rs` (bidirectional cells and cached Rustybuzz/ab_glyph output); `app/terminal_tests.rs` (UI regression tests) |
| Markdown | `app/markdown.rs` (source/preview layout, background preparation, links, local images), `app/markdown_scroll.rs` (bidirectional split scrolling), `app/markdown_style.rs` (shared reader fonts, contrast, and cached native code blocks), `app/markdown_media.rs` (cached native math and Mermaid rendering), `app/markdown_tests.rs`; per-tab modes are stored in `session.rs` |
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
on background workers and serialize repository actions. Parse porcelain v2 with
NUL records so paths containing spaces, newlines, and non-UTF-8 bytes stay intact
on Unix. Pass literal pathspecs only to path-taking commands: applying
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
Back up regular files before discard/conflict replacement, then report the
backup path. Keep existing Git gutter comparison independent of index operations.
Tests in `tests/git.rs` use disposable repositories and local bare remotes;
`app/source_control_tests.rs` exercises the native UI and unsaved-buffer guards.

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
download fallback. The current process stays open until the user quits. macOS
release signatures are currently ad hoc, so this checks bundle integrity rather
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

The native Markdown renderer includes a local dependency patch for nested block
layout; see [the patch notes](vendor/egui_commonmark/PATCH.md). Keep its
[MIT notice](assets/licenses/egui_commonmark-MIT.txt) with distributed packages.

Document views use bundled Noto Sans regular and semibold fonts, plus Noto Sans
Symbols for symbols. Editor and application fonts keep their existing settings.
Keep the [Noto font license](assets/licenses/Noto-Sans-OFL.txt) and font files
under `assets/fonts/` with distributed packages. The source fonts come from
[the Noto project](https://github.com/notofonts/noto-fonts).

Terminal RTL runs use Rustybuzz for OpenType shaping, with each glyph cluster
fitted to its original Ghostty cells. Unicode bidi layout supplies reversible
cell maps for the cursor and selection. The original text and VT stream are
unchanged. Keep the bundled Noto Sans Arabic font and its
[OFL notice](assets/licenses/Noto-Sans-Arabic-OFL.txt) in distributed packages;
the source is [Google Fonts](https://github.com/google/fonts/tree/main/ofl/notosansarabic).
