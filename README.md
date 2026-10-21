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

hedit is a text editor for Linux, built for the work you leave open: logs to
inspect, snippets to revisit, and notes that do not need a filename yet. Keep a
workspace of scratchpads between sessions, find your place in large logs, and
work with CUDA source without leaving the editor.

## What you can do

- **Keep notes for later.** Start a scratchpad without choosing a file, give it a
  name, and return to it after restarting. Browse the shelf to reopen closed tabs.
- **Inspect large logs.** Read files in manageable pages, follow new output, search
  across the file, and bookmark positions you want to revisit.
- **Find your way around.** Switch buffers, jump to a line, search and replace
  individual matches, and undo edits during your session.
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
curl -fsSL https://github.com/MohamedElashri/hedit/releases/latest/download/install.sh | sh
```

The installer chooses Linux x86-64 or ARM64, verifies the download's checksum,
and installs the application, launcher, themes, and guides under `~/.local`.
Ensure `~/.local/bin` is on your `PATH`, then launch `hedit`.

Prefer a manual download? Get an archive and its checksum from
[GitHub Releases](https://github.com/MohamedElashri/hedit/releases). The
[user guide](USER_GUIDE.md#installation) also covers custom installation paths,
pinned versions, private-repository access, and building from source.

Releases are built on Ubuntu 24.04 and need compatible Linux libraries, a graphical
X11 or Wayland session, and OpenGL. NVIDIA hardware and CUDA tools are optional
for ordinary editing.

From version 0.1.1, choose **Help → Check for updates…** to review and install a
newer release. Update checks, installations, and restarts are always manual.
See [updating hedit](USER_GUIDE.md#updates) for requirements and upgrading 0.1.0.

## Quick start

Open a log, jump to a source location, or send a snippet from your terminal:

```sh
hedit application.log
hedit kernel.cu:42:8
printf 'keep this snippet\n' | hedit -
```

Inside the editor:

1. Press **Ctrl+N** to start a scratchpad. Use **File → Name scratchpad…** to name it.
2. Paste your notes and check that the status reads **Recovery up to date** before
   ending your session. Your workspace returns on the next launch.
3. Open a log and choose **View → Follow log (read only)** to keep up with
   incoming output.
4. Use **Ctrl+F2** to bookmark a position and **F2** to return to it.
5. Open **Preferences → Settings…** for editor options, appearance, CUDA tools,
   and recovery in one place.

Close a tab with **Ctrl+W** and reopen it with **Ctrl+Shift+T**, or use
**File → Browse closed buffers…** to find an older closed buffer. Use **Ctrl+S**
whenever you want a normal saved file.

## Everyday shortcuts

| Shortcut | Action |
| --- | --- |
| Ctrl+N / Ctrl+O | New scratchpad / open a file |
| Ctrl+S | Save to a file |
| Ctrl+P / Ctrl+Shift+P | Switch buffers / open the command palette |
| Ctrl+F / Ctrl+G | Find text / go to a line |
| Ctrl+W / Ctrl+Shift+T | Close a tab / reopen the latest closed tab |
| Ctrl+, | Open preferences |

## CUDA tools

Choose **Tools → CUDA panel**, select a debug executable, and start CUDA-GDB or a
sanitizer run. Install NVIDIA's CUDA Toolkit and a compatible driver separately.
See the [CUDA guide](USER_GUIDE.md#cuda-tools) for compilation flags, debugger
controls, and tool requirements. Device debugging still needs validation on
NVIDIA hardware.

## Your workspace

Just like the famous sublime text, hedit copies keep unsaved scratchpads and modified buffers between sessions.
They are separate from your saved files. Checkpoints are periodic, so an abrupt
kill or power loss can lose edits since the last completed checkpoint. Keep
independent saved copies and backups for important work.

hedit currently supports UTF-8 text. Undo history lasts for the current process;
regex search, multiple cursors, and split views are planned. Read the
[user guide](USER_GUIDE.md) for everyday use and recovery, or the
[developer guide](DEVELOPER_GUIDE.md) for contributing and the roadmap.

## LICENCE

This project is using [MIT license](LICENSE)
