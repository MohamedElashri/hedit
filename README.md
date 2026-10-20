<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/hedit-lockup-dark.svg">
    <img src="assets/hedit-lockup-light.svg" width="320" alt="hedit">
  </picture>
</p>
<p align="center"><strong>Keep your train of thought.</strong><br>A native home for logs, scratchpads, and CUDA.</p>

hedit is a native Rust editor for Linux. Open large logs, keep snippets in buffers
without choosing filenames, and return to them after restarting. The application
and build use Cargo, with no Node.js, npm, webview, or frontend bundler.

**Start with the [user guide](docs/README.md).** Version 0.1.0 is an early release;
remaining work is listed in the [roadmap](ROADMAP.md).

## What you can do

- Keep unnamed or named scratchpads between launches through recovery checkpoints.
- Reopen closed buffers from a searchable shelf, including unsaved text.
- Read large files in bounded pages and follow growing logs in read-only mode.
- Add persistent bookmarks and navigate with F2 and Shift+F2.
- Find literal text across the whole file, go to a line, undo/redo, and toggle wrap.
- Highlight CUDA and C/C++ source without plugins or a language server.
- Customize interface and syntax colors, or import/export JSON themes.
- Run CUDA-GDB, Compute Sanitizer, and GPU inventory from the optional CUDA panel.

## Install and launch

Requires Rust 1.92 or newer and a Linux graphical desktop with OpenGL. On
Debian/Ubuntu, install the build dependencies, then build and run:

```sh
sudo apt-get install build-essential pkg-config libx11-dev libxkbcommon-dev libwayland-dev libegl1-mesa-dev
cargo build --release --locked
./target/release/hedit
```

Use `make install` for an installation under `~/.local`, or `make package` to
create a portable Linux archive and checksum under `dist/`. See
[installation](docs/installation.md) for package installation and other prefixes.

```sh
hedit application.log
hedit kernel.cu:42:8
printf 'keep this snippet\n' | hedit -
hedit --session-dir /tmp/hedit-investigation --new
```

## Guides

| Task | Read |
| --- | --- |
| First session | [Getting started](docs/getting-started.md) |
| Name, keep, close, or reopen snippets | [Buffers and scratchpads](docs/buffers.md) |
| Navigate and follow a log | [Working with logs](docs/logs.md) |
| Search, replace, or bookmark text | [Editing and navigation](docs/editing.md) |
| Change the editor's colors | [Themes](docs/themes.md) |
| Debug a CUDA program | [CUDA and GPU tools](docs/cuda.md) |
| Use terminal arguments | [Command line](docs/command-line.md) |
| Learn controls | [Keyboard shortcuts](docs/shortcuts.md) |
| Restore missing work | [Recovery](docs/recovery.md) |
| Resolve a problem | [Troubleshooting](docs/troubleshooting.md) |

Recovery is periodic: the status bar shows pending, in-progress, and completed
checkpoints. Normal exit waits for a final checkpoint; an abrupt process kill can
lose edits after the last completed one. The shelf has 50 slots and refuses to
silently evict unsaved work. Undo history is limited to the current process.

UTF-8 text is supported. Files above 2 MiB use roughly 256 KiB pages; displayed
page line numbers are relative. Multiple cursors, regex search, split views, and
single-instance file forwarding remain planned. The CUDA workbench provides
textual debugging controls; GPU hardware validation is still outstanding.

## Project information

For development instructions, see [Contributing](CONTRIBUTING.md). Contributor
notes live under `contributor/`; `docs/` is the user manual. The independent
[SVG identity](assets/brand.md) includes icon, wordmark, and monochrome variants.

hedit derives its Rust file engine and recovery foundation from
[rsedit](https://github.com/goodappsstore/rsedit), based on commit
`63a9ee4966d3fe1cfcd7d4af70e963e10aa297ff`. It has an independent native UI,
workspace layer, branding, and development direction. Upstream copyright and
attribution are retained in [LICENSE](LICENSE) and [NOTICE](NOTICE).

MIT licensed. hedit is not affiliated with GoodApps, Sublime HQ, or NVIDIA.
