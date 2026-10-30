# hedit developer guide

Build, test, and release hedit. For using it, see the [user guide](USER_GUIDE.md).

- [Build and install](#build-and-install)
- [Checks](#checks)
- [macOS builds](#macos-builds)
- [Windows builds](#windows-builds)
- [Code map](#code-map)
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
session directory. Linux is the validated platform; macOS builds and passes CI
but still needs hands-on GUI review. Windows builds have native CI checks;
hands-on GUI validation is pending there too.

## Checks

Run `make ci` before committing. It checks formatting, Clippy with warnings
denied, the native-only policy, documentation links, script syntax, Rust tests,
and the offline installer tests, then builds and smoke-tests a release package.

| Command | Purpose |
| --- | --- |
| `make static` | Formatting, policy, docs, and script checks; no compiling |
| `make lint` | `make static`, then Clippy |
| `make test` | Rust tests and offline installer tests |
| `make test-macos` | The tests that apply on macOS (the CI macOS job) |
| `make check` | Lint and tests, without a release build |
| `make package` / `make smoke` | Build a release package under `dist/` / install it in a temporary prefix and run it |
| `make release-check RELEASE_TAG=vX.Y.Z` | Check a tag against `Cargo.toml` |
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

On macOS, run `make test-macos` instead of `make ci`: the updater tests expect
Linux layouts and GNU tar. Updates are Linux-only; Mac users replace the app.
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
Windows upgrades replace the portable folder; in-app updates remain Linux-only.

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
| Search and folders | `search.rs` (search, guarded replacement), `folder.rs` (listings, fuzzy matching), `projects.rs` (projects, EditorConfig) |
| Syntax and look | `syntax.rs`, `syntax/` (grammars, lexer, keywords), `theme.rs`, `fonts.rs`, `brand.rs` |
| Process and updates | `cli.rs`, `launcher.rs`, `ipc.rs`, `windows.rs`, `update.rs`, `update/`, `build.rs`, `debugger.rs` |
| Window | `app.rs` (frame loop, shortcuts, editor drawing), `app/chrome.rs` (menus, tabs, sidebar, status bar), `app/commands.rs` (every command, the palette) |
| App features | One file each in `src/app/`, named after the feature: `auto_complete.rs`, `definitions.rs`, `spell_check.rs`, `macros.rs`, `snippet_fields.rs`, `find_tools.rs`, `changes.rs`, `fold_gutter.rs`, `font_picker.rs`, and so on |
| Markdown | `app/markdown.rs` (source/preview layout, background preparation, links, local images), `app/markdown_scroll.rs` (bidirectional split scrolling), `app/markdown_media.rs` (cached native math and Mermaid rendering), `app/markdown_tests.rs`; per-tab modes are stored in `session.rs` |
| Settings | `settings.rs` (the `settings.json` schema), `app/settings_store.rs`, `app/preferences.rs`, `app/settings_pages.rs` |

Every menu item, shortcut, and palette entry goes through `Command` in
`app/commands.rs`, so availability and behavior stay the same everywhere. A new
command needs a variant, a label, a default shortcut (if any), an `execute` arm,
and, when it can be unavailable, a `command_enabled` arm.

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
dropped. An idle window must not redraw on its own; repaint only when there is
input or a worker result.

**Positions are characters.** Editing code uses Unicode scalar (character)
indices; convert to bytes with `advanced::byte` only at slicing. Test with CRLF
text, text without a final newline, and non-Latin text.

**No shell.** Pass subprocess arguments with `Command::arg`. Debugger, Git, build,
and NVIDIA tool output is bounded.

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
the catalog.

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

The CI workflow runs Linux, macOS, and Windows checks on every push. Linux and
macOS use Makefile targets; Windows uses Cargo and Python directly, without
requiring GNU Make. Pushing a version tag runs the release workflow, which builds
Linux x86-64 and ARM64 tarballs, the universal macOS app, and a Windows x86-64 ZIP, tests them, and
publishes them with checksums. Tags with a hyphen (`vX.Y.Z-rc.1`) publish as
prereleases.

To release:

1. Set the version in `Cargo.toml`, run `cargo check` to update `Cargo.lock`, and
   date the version's section in `CHANGELOG.md`.
2. Run `make ci` and `make release-check RELEASE_TAG=vX.Y.Z`.
3. Commit, push, then tag:

   ```sh
   git tag -a vX.Y.Z -m "hedit X.Y.Z"
   git push origin vX.Y.Z
   ```

The changelog lists only changes users of the previous release will notice: a
bug fixed during development of a feature that has not shipped is not a fix.

The in-app updater (`Help → Check for updates…`) never downloads or installs
without the user asking. It checks the SHA-256 file from the same release, runs
the new binary's `--version` before installing, rejects unsafe archive entries
before extracting, and swaps the binary in last, rolling back on failure.
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
