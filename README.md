<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/hedit-lockup-dark.svg">
    <img src="assets/hedit-lockup-light.svg" width="320" alt="hedit">
  </picture>
</p>
<p align="center"><strong>Keep your train of thought.</strong></p>
<p align="center">
  <a href="https://github.com/MohamedElashri/hedit/releases">Download</a> ·
  <a href="#quick-start">Quick start</a> ·
  <a href="USER_GUIDE.md">User guide</a> ·
  <a href="DEVELOPER_GUIDE.md">Developer guide</a>
</p>

hedit is a native text editor for Linux and macOS, built for the work you leave open: logs to
inspect, snippets to revisit, and notes that do not need a filename yet. Keep a
workspace of files between sessions, find your place in large logs, and
work with CUDA source without leaving the editor.

See the [changelog](CHANGELOG.md) for what each release adds.

## What you can do

- **Edit folders of code.** Open several project folders, browse their trees,
  create files and subfolders, and search across roots with **Ctrl+P**. Folder
  icons distinguish directories from documents. Hide the tree with **Ctrl+Alt+B**;
  your folders, expanded directories, and visibility return after restarting.

- **Keep notes for later.** Create a new file without choosing a path, name its
  tab, and return to it after restarting. Browse the shelf to reopen closed files.
- **Inspect large logs.** Read files in manageable pages, follow new output, search
  across the file, and bookmark positions you want to revisit.
- **Find your way around.** Search folder contents with regex and filename filters,
  review replacements, jump to symbols, and navigate back and forward. Save named
  projects with their tabs and split views; later terminal launches open in the
  running window.
- **Handle everyday file work.** Pin and reorder tabs, preview sidebar files,
  rename/move/duplicate entries, send them to the system trash, and compare edits
  with disk or Git HEAD. Customize keyboard shortcuts, snippets, and formatting.
- **Read code in color.** Syntax is selected from the filename. Choose a language
  from the searchable bottom-bar menu for unsaved snippets, or switch to plain
  text. Common programming, web, configuration, build, and scientific formats
  are included.
- **Make it comfortable.** Choose a light or dark theme, customize individual
  colors, import or export palettes, and adjust font size, line numbers, and word wrap.
  Familiar menus and a single Preferences window keep settings easy to find.
- **Work with CUDA.** Read highlighted C/C++ and CUDA source, control CUDA-GDB,
  run Compute Sanitizer, and inspect GPU information from the CUDA panel.

## Install

Install the latest stable release:

```sh
curl -fsSL https://raw.githubusercontent.com/MohamedElashri/hedit/main/install.sh | sh
```

The installer verifies the download's checksum and selects the right package for your
system. On Linux it picks the x86-64 or ARM64 build and installs the application,
launcher, themes, and guides under `~/.local`. On macOS it installs the universal
**hedit.app** in `~/Applications` and adds a `hedit` command in `~/.local/bin`.
Ensure `~/.local/bin` is on your `PATH`, then launch `hedit`.

Prefer a manual download? Get an archive and its checksum from
[GitHub Releases](https://github.com/MohamedElashri/hedit/releases). The
[user guide](USER_GUIDE.md#installation) also covers custom installation paths,
pinned versions, private-repository access, and building from source.

Linux releases are built on Ubuntu 24.04 and need compatible Linux libraries, a graphical
X11 or Wayland session, and OpenGL. NVIDIA hardware and CUDA tools are optional
for ordinary editing.

The macOS app needs macOS 11 or newer and contains both Apple Silicon and Intel code.
To install it by hand, download `hedit-<version>-macos-universal.zip` and its checksum,
verify it with `shasum -a 256 -c`, then unzip it and move **hedit.app** to Applications.
Initial packages use an ad-hoc development signature; Developer ID signing
and notarization are pending. Native GUI validation is also pending. See
[macOS installation](USER_GUIDE.md#macos-installation) for details.

Choose **Help → Check for updates…** to review and install a
newer Linux release. Update checks, installations, and restarts are always manual.
See [updating hedit](USER_GUIDE.md#updates) for requirements and upgrading
releases that predate the updater.

## Quick start

Open a log, jump to a source location, or send a snippet from your terminal:

```sh
hedit .
hedit application.log
hedit kernel.cu:42:8
printf 'keep this snippet\n' | hedit -
```

The terminal prompt returns after the window starts; closing the terminal leaves
hedit running.

Inside the editor:

1. Press **Ctrl+N** to create a file. Use **File → Name unsaved file…** to name its tab.
2. Paste your notes and check that the status reads **Recovery up to date** before
   ending your session. Your workspace returns on the next launch.
3. Open a log and choose **View → Follow log (read only)** to keep up with
   incoming output.
4. Use **Ctrl+F2** to bookmark a position and **F2** to return to it.
5. Open **Preferences → Settings…** for editor options, appearance, CUDA tools,
   and recovery in one place.

Close a tab with **Ctrl+W** and reopen it with **Ctrl+Shift+T**, or use
**File → Browse closed files…** to find an older closed file. Use **Ctrl+S**
whenever you want a normal saved file.

## Everyday shortcuts

On macOS, use **Cmd** in place of **Ctrl** and **Option** in place of **Alt**.

| Shortcut | Action |
| --- | --- |
| Ctrl+N / Ctrl+O | New file / open a file |
| Ctrl+Shift+O | Add one or more folders |
| Ctrl+B / Ctrl+Alt+B | Toggle sidebar / folder section |
| Ctrl+S | Save to a file |
| Ctrl+P / Ctrl+Shift+P | Find files / open the command palette |
| Ctrl+F / Ctrl+G | Find text / go to a line |
| Ctrl+W / Ctrl+Shift+T | Close a tab / reopen the latest closed tab |
| Ctrl+, | Open preferences |
| Ctrl+Shift+D / Ctrl+Shift+K | Duplicate selection or line / delete lines |
| Ctrl+Shift+Up / Ctrl+Shift+Down | Move lines up / down |
| Ctrl+/ | Toggle line comments |
| F11 / Shift+F11 | Full screen / distraction-free mode |

**Edit** groups selection, line operations, text transformations, and indentation.
**View** groups sidebar, editor, and font controls. Hide tabs or the status bar,
show whitespace, choose a column ruler and wrap width, and highlight the current
line. **Edit → Line endings** converts the current file between LF and CRLF. Use
**Ctrl+D** to select the next occurrence, **View → Layout** for split panes, and
**View → Code folding** to collapse blocks. Indentation detection, smart Enter,
and configurable tab stops support daily code editing.

## CUDA tools

On Linux, choose **Tools → CUDA panel**, select a debug executable, and start CUDA-GDB or a
sanitizer run. Install NVIDIA's CUDA Toolkit and a compatible driver separately.
macOS keeps CUDA syntax highlighting and code editing; local CUDA debugging,
Compute Sanitizer, and NVIDIA GPU tools are disabled there.
See the [CUDA guide](USER_GUIDE.md#cuda-tools) for compilation flags, debugger
controls, and tool requirements. Device debugging still needs validation on
NVIDIA hardware.

## Your workspace

hedit keeps recovery copies of unsaved files and edits between sessions. These
copies are separate from files you save to disk. Checkpoints are periodic, so an abrupt
kill or power loss can lose edits since the last completed checkpoint. Keep
independent saved copies and backups for important work.

hedit supports UTF-8 and BOM-marked UTF-16, with explicit legacy encoding choices.
Fully loaded buffers retain bounded undo/redo history across restarts;
multiple cursors, rectangular selections, split panes, indentation folding, guides,
and a minimap are available. Read the
[user guide](USER_GUIDE.md) for everyday use and recovery, or the
[developer guide](DEVELOPER_GUIDE.md) for contributing and the roadmap.

## LICENCE

This project is using [MIT license](LICENSE)
