# hedit user guide

Keep scratchpads between sessions, inspect large logs, and work with CUDA source.
This guide covers the current Linux release.

- [Installation](#installation)
- [Scratchpads and files](#scratchpads-and-files)
- [Logs and bookmarks](#logs-and-bookmarks)
- [Editing and navigation](#editing-and-navigation)
- [Preferences](#preferences)
- [Appearance](#appearance)
- [CUDA tools](#cuda-tools)
- [Command line](#command-line)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Recovery](#recovery)
- [Troubleshooting](#troubleshooting)

## Installation

You need a graphical Linux session with X11 or Wayland and OpenGL. A GPU and CUDA
are optional; ordinary editing works without them.

### Install from GitHub Releases

Install the latest stable release with:

```sh
curl -fsSL https://github.com/MohamedElashri/hedit/releases/latest/download/install.sh | sh
```

The installer selects x86-64 or ARM64, verifies the package's SHA-256 checksum,
and installs under `~/.local`. It needs `curl`, `tar`, and standard Linux utilities;
no compiler is required. Release binaries are built on Ubuntu 24.04 and require
compatible system libraries. If they cannot run on your system, install from source.
CUDA tools are installed separately.

To inspect the script first, select a version, or choose another prefix:

```sh
curl -fsSL https://github.com/MohamedElashri/hedit/releases/latest/download/install.sh -o hedit-install.sh
sh hedit-install.sh --version v0.1.0 --prefix "$HOME/.local"
```

Omit `--version` for the latest stable release. Prereleases require their full
tag, such as `--version v0.2.0-rc.1`. Run the installer again to upgrade. Close
hedit before upgrading; your saved files and session data are kept.

### Private repository access

Private releases require an account with repository access. With GitHub CLI
installed, sign in and download the installer through the authenticated API:

```sh
gh auth login --hostname github.com
gh release download --repo MohamedElashri/hedit --pattern install.sh --output hedit-install.sh
sh hedit-install.sh --github-cli
```

For a pinned version, give the same tag to both commands:

```sh
gh release download v0.1.0 --repo MohamedElashri/hedit --pattern install.sh --output hedit-install.sh
sh hedit-install.sh --github-cli --version v0.1.0
```

Use a fresh filename or remove the previous downloaded script before downloading
again. Public `curl` URLs cannot access a private repository.

### Install a portable package

Download the matching archive and `.sha256` file from
[GitHub Releases](https://github.com/MohamedElashri/hedit/releases), verify its
checksum, extract it, and run the included installer. For the x86-64 package:

```sh
sha256sum -c hedit-0.1.0-linux-x86_64.tar.gz.sha256
tar -xzf hedit-0.1.0-linux-x86_64.tar.gz
cd hedit-0.1.0-linux-x86_64
sh install.sh
```

The default destination is `~/.local`. Ensure `~/.local/bin` is on your `PATH`,
then run `hedit`. You can also run `./hedit` directly from the extracted package,
or choose an absolute installation prefix with `sh install.sh /home/you/.local`.
Packages must match your processor and require compatible Linux system libraries.

### Install from source

If you have a source checkout, follow the [build instructions](DEVELOPER_GUIDE.md#build-and-install),
then use `make install`. It installs the application, desktop entry, icon, themes,
and documentation under `~/.local`.

### First session

Launch hedit from your terminal or application launcher. A first session opens
an empty scratchpad; later sessions restore your workspace.

1. Press **Ctrl+N** to start a scratchpad and type or paste some text.
2. Optionally choose **File → Name scratchpad…** to give it a recognizable name.
3. Check the recovery status before ending your session.
4. Open **Preferences → Settings…** to customize the editor, or **File → Open file…** to read a file.

### Uninstall

For the default installation, remove `~/.local/bin/hedit`,
`~/.local/share/applications/hedit.desktop`,
`~/.local/share/icons/hicolor/scalable/apps/hedit.svg`,
`~/.local/share/hedit/themes/`, and `~/.local/share/doc/hedit/`.

Session data normally lives in `~/.local/share/hedit/session/`. Keep it if you may
need your unsaved buffers again; see [Recovery](#recovery) before removing it.

## Scratchpads and files

### Keep text without choosing a filename

Use **File → New scratchpad**, the **+** beside the tabs, or **Ctrl+N** for a scratchpad. hedit keeps recovery copies so you
can return to it after restarting without first saving it as a normal file.

Choose **File → Name scratchpad…** (**Ctrl+Shift+R**) to name it. Names can have
up to 80 characters and stay fixed as you edit. Apply an empty name to return to
using the first nonempty line as the tab title. Naming does not create a file.

### Open, save, and switch

Use **File → Open file…** (**Ctrl+O**), drag a file into the window, or pass its path in the
terminal. **Save** (**Ctrl+S**) writes the current buffer to a file; a scratchpad
asks for a filename. **File → Save as…** (**Ctrl+Shift+S**) chooses another path.
After a scratchpad is saved to a file, its tab uses that file's name.

Click a tab or an Open Files row to switch buffers. **Ctrl+P** filters open buffers
by name. A dot on a tab indicates unsaved changes.

### Close and reopen buffers

Close a tab with **×** or **Ctrl+W** to put it on the shelf, including its text,
name, selection, and bookmarks. Closing the window keeps open tabs in the session
rather than moving them onto the shelf.

- **Ctrl+Shift+T** or **File → Reopen closed tab** restores the latest closed buffer.
- **File → Browse closed buffers…**, also available from **Closed Buffers** in
  the sidebar, lets you filter the shelf by name and reopen any entry.

The shelf has 50 slots. A clean, saved entry may be removed to make room; unsaved
scratchpads and modified files are never silently evicted. If all slots hold
unsaved work, reopen and save some of it before closing more tabs. Entries do
not expire based on age.

## Logs and bookmarks

### Read a large file

Open a log with **File → Open file…** or `hedit application.log`. Files above 2 MiB use views
of roughly 256 KiB. **Start**, **Previous**, **Next**, and **End** navigate the
file; the toolbar shows the current byte range.

**Page Ln** in the status bar is relative to the displayed page. Use **Go to
line** for a line in the whole file, or **Find** to search across every page.

### Follow incoming output

Enable **View → Follow log (read only)** on a clean, file-backed buffer to show its tail
and check for new output about once per second. Disable Follow before editing.
The menu item is unavailable for scratchpads and modified buffers.

When a log rotates, Follow reads the file currently at the original path. If that
path disappears or the replacement is not UTF-8 text, an error is shown and the
visible buffer remains.

Outside Follow mode, hedit flags external changes when the buffer becomes active.
A clean buffer offers **Reload file**. For a modified buffer, use **Save buffer
as…** to keep your version before reopening the disk version.

### Bookmark a position

Press **Ctrl+F2** to toggle a bookmark at the cursor. **F2** goes to the next one;
**Shift+F2** goes to the previous one, wrapping within the current buffer.
**Goto → Bookmarks…** lists saved positions
by text label. Navigate to a bookmark and press **Ctrl+F2** again to remove it.

Each buffer can hold 1,000 bookmarks. They survive restarts and moves to the
shelf. Labels are captured when added. Bookmarks store byte positions and adjust
for ordinary edits; undo/redo or an external replacement can change which text
sits at a saved position.

## Editing and navigation

Use a single selection for clipboard operations and indentation. **Ctrl+Z**
undoes; **Ctrl+Shift+Z** or **Ctrl+Y** redoes. Up to 256 edit steps per buffer are
kept during the current process. Undo history is not restored after restart.

Toggle wrap through **View → Word wrap** (**Ctrl+Shift+W**). Long lines scroll
horizontally when wrap is off. **Ctrl++** and **Ctrl+-** change font size. Wrap, font size, line numbers, and sidebar visibility are kept between launches.
Use **Preferences → Settings… → Editor** to manage them together.

CUDA (`.cu`, `.cuh`) and C/C++ (`.c`, `.h`, `.cc`, `.cpp`, `.cxx`, `.hpp`, `.hxx`,
`.hh`, `.inl`) files have syntax highlighting; other files currently use plain text.
UTF-8 text is supported, including an existing BOM and LF/CRLF bytes. UTF-16 and
binary files are rejected. Edits can introduce LF breaks into CRLF files;
line-ending conversion is not available yet.

### Find and replace

1. Press **Ctrl+F** or choose **Find → Find…** to open the bottom search panel.
2. Enter a literal, case-sensitive query of up to 4,096 UTF-8 bytes.
3. Click **Find next** to select the next match across the whole file.
4. Enable **Replace** (or press **Ctrl+H**), enter replacement text, and click
   **Replace selected** to change that occurrence.

Search wraps at the end. Long searches can be cancelled from the status bar;
**Esc** closes the panel. Regex, case-insensitive search, and replace-all controls
are not yet available.

### Go to a line or switch buffers

Press **Ctrl+G**, enter a positive, one-based line number, and click **Go**. This
locates a line in the entire document, even when only a page is displayed.

**Ctrl+P** opens the buffer switcher; **Ctrl+Shift+P** opens commands. Type a
filter, use **Up/Down** to select a result, and press **Enter**, or click a result.
Type `>` in the buffer switcher to search commands instead. **Esc** closes either.
The menus show the same commands and keyboard shortcuts.

## Preferences

Choose **Preferences → Settings…** or press **Ctrl+,**. Settings are grouped in
one window:

| Page | Settings and actions |
| --- | --- |
| Editor | Word wrap, line numbers, open files sidebar, font size, restore defaults |
| Appearance | Presets, custom colors, theme import/export, contrast feedback |
| CUDA Tools | CUDA-GDB path and a shortcut to the tool panel |
| Recovery | Checkpoint status, checkpoint now, closed buffers, recovery guidance |

View commands provide quick toggles for the same editor settings. The status bar
shows the cursor position, language, encoding, and recovery state; click the
recovery status to open its preferences page. For paged files, gutter line numbers
and **Page Ln** refer to the displayed page. The sidebar lists open buffers and
provides a **Closed Buffers** link. Right-click a tab for its context menu, or
middle-click its title to close it to the shelf.

## Appearance

Choose **Preferences → Color theme…**, or open **Preferences → Settings…**
(**Ctrl+,**) and select **Appearance**. The **Color theme…** command also appears
in the command palette. Pick **Hedit Ink**, **Hedit Paper**, **Nord**, or **Ember**;
Paper uses light controls and the others use dark controls.

Use color pickers or `#RRGGBB` fields to change the background, surfaces, text,
accent/cursor, selection, recovery indicators, and syntax colors. Scroll the list
for syntax roles. Valid changes preview immediately; invalid hex input keeps
the last valid color. Editing a preset creates **My theme**, which you can rename.

Text and selection contrast ratios appear below the controls. Values below
4.5:1 are flagged without blocking your choices. **Dark controls** changes widget
defaults without replacing your colors.

**Revert theme** restores the palette that was active when the window opened.
**Done** or the close button keeps your valid changes. Preferences save
automatically every two seconds and on normal exit.

### Share or keep another palette

Use **Import JSON…** and **Export JSON…**. The bundled `themes/` directory contains
complete presets and a custom example. A partial theme inherits omitted colors
from Hedit Ink:

```json
{
  "version": 1,
  "name": "Violet Ink",
  "accent": "#B4A0F5",
  "selection": "#413657",
  "selection_text": "#F2ECFF",
  "cuda": "#B4A0F5"
}
```

Theme names need 1–80 characters. Colors are opaque, six-digit RGB values; a
leading `#` is optional on input. Unsupported versions, unknown fields, and
files above 64 KiB are rejected. A failed import keeps your current palette.

## CUDA tools

Choose **Tools → CUDA panel**. You need NVIDIA's CUDA Toolkit and a compatible
driver for device debugging. The panel uses `cuda-gdb`, `compute-sanitizer`, and
`nvidia-smi` from `PATH`; a custom debugger path is configured under
**Preferences → Settings… → CUDA Tools**.
The panel’s **Tool settings…** button opens the same preferences page.

### Debug a program

Build a separate debug executable with host and device information. From a
checkout or extracted package, the bundled example can be compiled with:

```sh
nvcc -g -G examples/cuda/vector_add.cu -o /tmp/hedit-vector-add
```

`-G` changes optimization and performance. See NVIDIA's
[CUDA-GDB manual](https://docs.nvidia.com/cuda/cuda-gdb/index.html) for supported
hardware, permissions, and compilation details.

1. Open the source and choose the compiled **Executable** in the CUDA panel.
2. Keep Args as `[]`, or supply strings such as `["--size", "1024", "a path with spaces"]`.
3. Click **Start CUDA-GDB** to load it and add a breakpoint at `main`.
4. Enter a function name or `file.cu:42`, then click **Add breakpoint**.
5. Click **Run**, then **Continue** to reach later breakpoints.

**Pause**, **Step**, **Next**, and **Finish** control execution. **Stack** and
**Locals** inspect the current frame; **Devices**, **Kernels**, **Threads**, and
**Warps** inspect CUDA state. The console accepts commands such as `print i`,
`info cuda blocks`, `cuda kernel 0`, and `cuda thread (0,0,0)`.

Use **Stop** to end a tool session. Closing the panel only hides it. Output is
selectable text, with the most recent 512 KiB retained; fast output may be
truncated. Programs needing interactive terminal input are not supported here.

### Check memory or inspect the GPU

Stop any active tool, select the executable and arguments, choose a sanitizer,
and click **Run sanitizer**. This immediately launches the program.

| Tool | Checks |
| --- | --- |
| memcheck | Memory-access errors |
| racecheck | Shared-memory race hazards |
| initcheck | Uninitialized device global-memory reads |
| synccheck | Invalid synchronization |

Read the output and final process status. See the
[Compute Sanitizer guide](https://docs.nvidia.com/compute-sanitizer/ComputeSanitizer/index.html)
for coverage. **GPU info** starts a separate session showing the GPU name, driver,
memory usage, and temperature.

The workbench currently provides textual debugging controls. Source-gutter
breakpoints, structured watch trees, and lane visualizations are not available.
Device debugging has not yet been validated on NVIDIA hardware in this project.

## Command line

```text
hedit [--session-dir DIR] [--new] [FILE[:LINE[:COLUMN]] ...] [-]
```

| Argument | Effect |
| --- | --- |
| `FILE` | Open a file or start a named file buffer |
| `FILE:LINE[:COLUMN]` | Open at a one-based location |
| `-` | Read UTF-8 stdin until EOF into an unsaved buffer |
| `--new` | Add an empty scratchpad to the restored session |
| `--session-dir DIR` | Use an independent session |
| `--` | Treat remaining arguments as filenames |
| `-h`, `--help` | Show help |
| `-V`, `--version` | Show the version |

```sh
hedit application.log
hedit kernel.cu:42:8
hedit "notes for today.txt"
printf 'keep this snippet\n' | hedit -
hedit --session-dir /tmp/hedit-investigation --new
hedit -- -notes.txt
```

An existing literal filename takes precedence over location syntax. CLI columns
use UTF-16 units; out-of-range locations clamp to the document or line end. A
missing file opens only if its parent exists and is created on **Save**.

One process can use a session directory at a time. A second launch does not
forward files to the current window; open them there or use `--session-dir`.
`HEDIT_SESSION_DIR` changes the default directory; the CLI option takes precedence.

Stdin is checkpointed before window creation, so consumed text remains in the
session if the graphical window cannot start.

## Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| Ctrl+N / Ctrl+O | New scratchpad / open file |
| Ctrl+S / Ctrl+Shift+S | Save / Save As |
| Ctrl+W / Ctrl+Shift+T | Close to shelf / reopen latest |
| Ctrl+Shift+R | Name the active unsaved scratchpad |
| Ctrl+P / Ctrl+Shift+P | Buffer switcher / command palette |
| Ctrl+F / Ctrl+H / Ctrl+G | Find / Replace / Go to line |
| Ctrl+Z / Ctrl+Shift+Z or Ctrl+Y | Undo / redo |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy / cut / paste |
| Ctrl+A | Select all in the focused text field |
| Ctrl+Shift+W | Toggle wrap |
| Ctrl++ or Ctrl+= / Ctrl+- | Increase / decrease editor font |
| Ctrl+, | Preferences |
| Ctrl+F2 | Toggle a bookmark at the cursor |
| F2 / Shift+F2 | Next / previous bookmark |
| Esc | Close search, the palette, or buffer-tool windows |

Undo in a search or debugger input edits that input; in the editor it uses the
buffer's history.

## Recovery

Recovery copies are separate from your original files. The status bar tells you
what has completed:

| Status | Meaning |
| --- | --- |
| Recovery pending | Changes are waiting for a checkpoint |
| Checkpointing… | Recovery data is being written |
| Recovery up to date | The current workspace has a completed checkpoint |
| Recovery checkpoint failed | Keep hedit open and address the error |

Checkpoints normally start every two seconds while work changes. Large copies or
slow disks can take longer. An abrupt kill or power loss can lose edits after the
last completed checkpoint. Normal close waits for a final checkpoint; failure
keeps the window open. Use **Save as…** to keep important text elsewhere if needed.

### Find missing work

Check open tabs, **Ctrl+P**, and **File → Browse closed buffers…**. Verify you opened the same
session, especially if you used `--session-dir`. Read any startup recovery warning
before dismissing it; a fallback checkpoint can be older than your latest work.

If the latest manifest cannot be read, hedit tries the previous checkpoint and
retains recovery generations. If neither works, old data is archived under
`preserved-*` and a fresh session starts. Failed buffer restores retain their
recovery references for another attempt.

### Back up or rescue a session

The default is `$XDG_DATA_HOME/hedit/session`, normally
`~/.local/share/hedit/session`. Close hedit and copy the **whole session directory**
before inspecting or modifying it. Small buffers are UTF-8 files in `buffers-*`;
`session.json` and `session.previous.json` identify their tabs. Large buffers can
reference several ranges, so one source file may not contain the full document.

Keep `preserved-*` archives until you rescue the needed text. After a fallback,
`preserved-manifests/` stops automatic cleanup; remove that marker only after
backing up and recovering your text. Preserved archives do not expire.

Recovery data is plaintext, not encrypted. New session directories are private
on Unix. Use regular saved files and your usual backup system for independent,
long-term copies.

## Troubleshooting

| Problem | What to do |
| --- | --- |
| No window opens | Run inside a graphical X11/Wayland session with OpenGL; inspect the terminal error. Reopen the same session to recover consumed stdin. |
| Session is locked | Return to the running window, or use a different `--session-dir`. A stale lock file alone does not hold a process lock. |
| Recovery fails | Check disk space and permissions, keep hedit open, and use **Retry checkpoint** after fixing the cause. |
| Save reports an external change | Use **Save as…** to keep your version. A clean buffer can reload the file. |
| Open/Save dialog does not appear | Check that `xdg-desktop-portal` and your desktop's portal backend are installed and running in the same session. |
| File is rejected | Convert a separate copy to UTF-8 if needed. UTF-16 and binary files are unsupported. |
| Theme is rejected | Check the version, name length, hex values, and 64 KiB limit. Your current palette stays active. |
| CUDA tool cannot start | Install it, check `PATH` or the debugger path, and read the tool's output for driver/device/permission errors. |

hedit refuses to overwrite external changes. For a large source that changes
before a recovery copy completes, unseen bytes may be unavailable. If an edit
cannot be applied, the edited visible page is kept in a new scratchpad and an
error explains the result.
