# hedit user guide

This guide covers hedit on Linux, macOS, and Windows. Shortcuts use Ctrl on Linux
and Windows; on macOS use **Cmd** for **Ctrl** and **Option** for **Alt** (the exceptions are
listed under [Keyboard shortcuts](#keyboard-shortcuts)). The menus always show
your platform's keys.

Open **Help → User guide** (**F1**) or **User guide** in About to read this guide
offline. **Contents** jumps to a topic; section links stay in the modal, and
**Back** returns to the previous location. The linked developer guide, palette
credits, and license notices are bundled too. External websites open in your
browser only when you click their links. Close with **Esc**, the X, or a click
outside the modal.

- [Installation](#installation)
- [Updates](#updates)
- [Files and folders](#files-and-folders)
- [SSH remote workspaces](#ssh-remote-workspaces)
- [Editing](#editing)
- [Finding and navigating](#finding-and-navigating)
- [Writing code](#writing-code)
- [Integrated terminal](#integrated-terminal)
- [Source Control](#source-control)
- [Markdown preview](#markdown-preview)
- [Views and layout](#views-and-layout)
- [Large files and logs](#large-files-and-logs)
- [Settings](#settings)
- [Appearance](#appearance)
- [CUDA tools](#cuda-tools)
- [Command line](#command-line)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Recovery](#recovery)
- [Troubleshooting](#troubleshooting)

## Installation

On Linux you need an X11 or Wayland desktop with OpenGL. A GPU and CUDA are
optional.

### Install from GitHub Releases

```sh
curl -fsSL https://raw.githubusercontent.com/MohamedElashri/hedit/main/install.sh | sh
```

The installer picks the right package, verifies its checksum, and installs into
`~/.local` (make sure `~/.local/bin` is on your `PATH`). To read the script first,
pick a version, or install elsewhere:

```sh
curl -fsSL https://raw.githubusercontent.com/MohamedElashri/hedit/main/install.sh -o hedit-install.sh
sh hedit-install.sh --version vX.Y.Z --prefix "$HOME/.local"
```

Run it again to upgrade; close hedit first. Your files and session are kept.
Release builds come from Ubuntu 24.04; if one cannot run on your system, build
from source.

### Install a portable package

Download an archive and its `.sha256` file from
[GitHub Releases](https://github.com/MohamedElashri/hedit/releases), then:

```sh
sha256sum -c hedit-X.Y.Z-linux-x86_64.tar.gz.sha256
tar -xzf hedit-X.Y.Z-linux-x86_64.tar.gz
cd hedit-X.Y.Z-linux-x86_64
sh install.sh            # or run ./hedit from here
```

### Install from source

Follow the [build instructions](DEVELOPER_GUIDE.md#build-and-install), then run
`make install`.

### macOS installation

The universal app runs on macOS 11 or newer, on Apple Silicon and Intel. The
installer command above works on macOS too: it puts **hedit.app** in
`~/Applications` (`--app-dir /Applications` for all users) and a `hedit` command
in `~/.local/bin`. To install by hand, download
`hedit-X.Y.Z-macos-universal.zip` and its `.sha256` file, then:

```sh
shasum -a 256 -c hedit-X.Y.Z-macos-universal.zip.sha256
ditto -x -k hedit-X.Y.Z-macos-universal.zip .
```

and move **hedit.app** to Applications. The app is not notarized yet, so macOS may
block it the first time; use **Open Anyway** in Privacy & Security only for a
download you trust. Use **Help → Check for updates…** to upgrade a writable app,
or run the installer again. See the
[macOS build notes](DEVELOPER_GUIDE.md#macos-builds) for what is still being
tested.

### Windows installation

Download `hedit-X.Y.Z-windows-x86_64.zip` and its `.sha256` file from
[GitHub Releases](https://github.com/MohamedElashri/hedit/releases).
The portable build needs Windows 10 or newer (x86-64) and an OpenGL-capable
graphics driver. In PowerShell, verify and extract it:

```powershell
$archive = 'hedit-X.Y.Z-windows-x86_64.zip'
$expected = (Get-Content "$archive.sha256").Split(' ')[0]
if ((Get-FileHash $archive -Algorithm SHA256).Hash -ne $expected) { throw 'Checksum mismatch' }
Expand-Archive $archive -DestinationPath .
.\hedit-X.Y.Z-windows-x86_64\hedit.exe
```

Keep the extracted folder anywhere writable. Add it to your user `PATH` if you
want to open files with `hedit.exe` from a terminal. To upgrade, close all hedit
windows and extract the newer ZIP into a new folder. Recovery and settings live
in your Windows profile, separately from the executable. Windows upgrades use
the ZIP; the shell installer supports Linux and macOS, and in-app installation
supports Linux.

Windows packages are unsigned. Native CI builds, tests, and checks the extracted
executable; Open/Save dialogs, clipboard, display scaling, and recovery after
closing a real window still need hands-on validation.

### Uninstall

On Linux, remove `~/.local/bin/hedit`,
`~/.local/share/applications/hedit.desktop`,
`~/.local/share/icons/hicolor/scalable/apps/hedit.svg`,
`~/.local/share/hedit/themes/`, and `~/.local/share/doc/hedit/`.
On macOS, remove **hedit.app** from the application folder and the
`~/.local/bin/hedit` launcher installed by the script. On Windows, delete the
extracted portable folder. Adjust these paths if you installed elsewhere.

Uninstalling leaves settings and recovery data in your profile. See
[Recovery](#recovery) for the platform's session path before deleting any of it.

## Updates

On Windows, [replace the portable package](#windows-installation).

On Linux and macOS, **Help → Check for updates…** looks up the latest stable release,
links to release notes, and installs it when you click **Install update**. hedit
never checks, installs, or restarts on its own. Keep working while it installs,
then choose **Restart hedit** to start the updated version with your open files
and unsaved work restored. Choose **Later** to keep working and use the new
version the next time you start hedit.

On Linux, updating needs `curl`, GNU `tar`, and a writable installation made by the
installer, a portable package, or `make install`. On macOS, updating uses the
universal app ZIP and needs write access to the folder containing the release
app; move the app to Applications and reopen it if macOS is running
it from a temporary location. The app signature and bundle version are checked,
and the whole bundle is replaced together. Keep custom themes in your user theme
folder rather than inside the signed app.

The download is checked against its SHA-256 file and the new program is test-run
before anything changes; if installing fails, the old version is restored.
For executables run directly from
a source checkout or installed in a system-owned location, rebuild from source
or upgrade with the
[installer](#install-from-github-releases).

## Files and folders

### Unsaved files

**Ctrl+N** (or **+** beside the tabs) creates a file you can keep without saving
it: hedit keeps a recovery copy and restores it next time. Its tab is named after
its first line; **File → Name unsaved file…** (**Ctrl+Shift+R**) gives it a name of
your own. **Ctrl+S** saves to disk, asking for a path the first time.

### Opening and switching

Open files with **Ctrl+O**, by dragging them into the window, or from the terminal.
A dot on a tab marks unsaved changes. Hover over it to reveal the close button.

- **Ctrl+P** finds open files and files in your folders by letters in order
  (`srcprj` finds `src/project.rs`).
- **Ctrl+Tab** goes back to the file you used before; hold Ctrl and press Tab
  again to go further back. **Ctrl+PageDown/PageUp** step through the tabs, and
  **Alt+1–9** pick a tab by position.
- **File → Open recent file…** lists the last 20 files.
- Drag tabs to reorder them. A tab's right-click menu can pin it (pinned tabs
  survive closing many at once), close the others or those to its right, copy its
  path, or reveal it in the sidebar.
- Clicking a file in the sidebar opens an italic preview tab that the next preview
  replaces; double-click, pin, or edit it to keep it.

### Closing and the shelf

Closing a modified file or a non-empty untitled tab (**Ctrl+W**) asks whether to
**Save**, **Don't Save**, or **Cancel**. Save writes your changes before closing
and opens Save As for untitled tabs; cancelling Save As leaves the tab open.
For a file on disk, Don't Save discards edits and keeps the disk copy. For an
untitled note, it closes the tab without writing a file and keeps its text in
**Closed Files**. Cancel leaves the tab open. Quitting or closing the window
preserves all open tabs and unsaved changes automatically, without a save prompt.
They are restored when you reopen hedit, including untitled notes and edits to
files on disk. Quitting does not write those edits to the original files. If
recovery cannot be written, the window stays open and shows an error.
If a note's text is intact but its recovery undo history is damaged, hedit restores
the note and warns that its undo history is unavailable.
The sole empty untitled tab stays open as the editor's blank state and has no
close button. Add another tab or type text to make it closable.

Empty untitled tabs are discarded when closed and do not appear in Closed Files.
Other closed tabs go on the *shelf* with their selection and bookmarks. After Don't
Save on a file on disk, reopening shows the disk copy; untitled notes reopen with
their text intact. **Ctrl+Shift+T** reopens the last tab, and **File →
Browse closed files…** lists them all. **File → Close all tabs** (**Ctrl+K,
Ctrl+W**) and **Close unmodified tabs** close many at once. The shelf normally keeps
50 files; only saved, unchanged ones are dropped to make room. Unsaved notes are
kept beyond that limit, so a full shelf never prevents closing a tab.

### Folders

**File → Open folder…** (**Ctrl+Shift+O**), dragging a folder in, or `hedit .` adds
a folder to the sidebar; you can have several. Right-click an entry to create,
rename, move, duplicate, trash, copy the path of, or reveal it; nothing is
overwritten without asking. **File → Refresh folders** picks up outside changes.
Folders such as `.git`, `node_modules`, and `target` are hidden from the tree and
searches; change the list under **Settings → Folders & Search**.
Open files appear in the tab bar. The sidebar contains folders and a shortcut
to closed-file recovery.

Click the sidebar icon at the top left, beside **File**, to hide or restore the
sidebar in Files or Source Control. Its filled left pane indicates that the
sidebar is visible; hover for the action and your current shortcut.
**Ctrl+K, Ctrl+B** also shows or hides the sidebar and **Ctrl+Alt+B** just its
folder section. Showing the sidebar exits distraction-free mode. Folders,
expanded directories, and what is shown come back next time.

### Projects

**Project → Save project as…** writes a `.hedit-project` file with your folders
and project formatting settings; reopen it with **Open project…**, **Recent
projects…**, or `hedit file.hedit-project`. The `.hedit-workspace` file beside it
keeps your own tabs, unsaved text, and layout for that project; keep it out of
version control.

**Project → Project settings…** has **GUI** and **JSON** tabs for the same draft.
In the GUI, edit **Project defaults** or choose **Add language…** for an override.
Controls cover indentation, tab width, line endings, encoding, final newlines,
and trailing whitespace. **Inherit** uses the project default for a language
override, or editor settings for project defaults. Matching `.editorconfig`
files take precedence.

Choose **Save settings** to apply the draft and update the open `.hedit-project`
file. Without an open project, settings apply to the current workspace. Closing
without saving discards the draft. Invalid JSON stays available to correct and
does not apply settings.

### Windows

**File → New window** (**Ctrl+Shift+N**) opens a window with its own tabs, folders,
and recovery. Settings, key bindings, snippets, and themes are shared. Windows that
had work in them reopen with the first one; `hedit FILE` opens in the first window.

### Encodings and line endings

hedit reads UTF-8 (with or without a BOM) and UTF-16 with a BOM. **File → File
encoding…** reopens or saves a file in another encoding, such as Windows-1252,
ISO-8859-1, Shift JIS, or GBK; characters that cannot be converted are refused,
never replaced. **Edit → Line endings** converts a file to LF or CRLF. Binary
files are not supported.

## SSH remote workspaces

Choose **Connect via SSH…** in the SSH bar to open a remote workspace in a
separate native window. Files, folder search and replacement, Source Control,
builds, and terminal processes use the SSH host. Rendering and editing views
stay local. There is no hedit cloud, relay, or account.

### Connect to a host

Build the helper on a Linux or macOS destination from the same checkout as the
editor. Create its destination directory first:

```sh
cargo build --release --locked -p hedit-remote
mkdir -p "$HOME/.local/bin"
install -m 700 target/release/hedit-remote "$HOME/.local/bin/hedit-remote"
```

Verify the host key through your normal OpenSSH workflow before connecting.
hedit refuses unknown and changed keys. Authentication uses OpenSSH keys or your
existing agent; password and interactive authentication prompts are unsupported.

Supply the host or alias, absolute workspace and installed helper paths, and an
absolute private state directory on the host. A state directory belongs to one
workspace; use a different directory for each workspace. Select the port
explicitly when it differs from the default, including aliases with a custom
port. An optional SSH configuration must be a trusted absolute local file.
Configuration and its includes can execute `Match exec` commands; do not use a
configuration supplied by a remote repository.

You can also connect from the command line:

```sh
hedit --ssh devbox \
  --ssh-root /home/me/project \
  --ssh-helper /home/me/.local/bin/hedit-remote \
  --ssh-state /home/me/.local/state/hedit/project \
  src/main.rs
```

Optional arguments are `--ssh-user USER`, `--ssh-port PORT`,
`--ssh-config /absolute/local/config`, and `--ssh-takeover`. Takeover explicitly
fences the previous writer. A new writer waits for cancelled tool jobs to finish.

The SSH bar identifies the host and workspace. Use the folder tree, Go to
Anything, or **Open remote path…** to open files. Paths entered in remote dialogs
are relative to the workspace, or absolute paths inside it. **New remote file**
creates an untitled, host-recoverable scratchpad. Save As creates a new host file
and refuses an existing target.

Git, builds, and terminals require **Trust for Git, builds and terminal…** for
each connection. Trust permits execution with the SSH account's permissions,
including Git hooks, filters, configuration, shells, and build scripts. It is
not a sandbox. Revoking trust cancels jobs and stops tracked processes;
reconnecting clears trust. The terminal also needs its normal opt-in setting.

### Privacy and recovery

Remote content, unsaved edits, undo history, tab state, searches, Git patches and
drafts, and terminal output are not written to local recovery or GUI state.
A temporary local session directory can contain launch and IPC metadata and
diagnostics. Connection arguments contain paths and host metadata, not keys.
The operating system, swap, hibernation, crash collectors, clipboard managers,
screenshots, and explicit copy/paste or exports are outside this application-level
storage guarantee. Trust the host and SSH account with their own files and recovery.

OpenSSH uses strict host-key checks. Agent, X11, tunnel and port forwarding,
existing multiplexed connections, local commands, ProxyCommand and ProxyJump are
disabled. The helper uses a private Unix socket rather than a host network
listener. Recovery storage requires private ownership and Unix permissions;
on macOS, extended ACL grants on that storage or its ancestors are also rejected.

The SSH bar shows outstanding edit receipts. **Durable on SSH host** means the
host has committed accepted edits to its recovery journal. A disconnected editor
retains pending edits in memory and pauses editing. **Reconnect** checks the
original recovery identity and reconciles the same edit sequences. If another
writer advanced a document, editing stays paused to preserve the stale view.

Editing also pauses when outstanding work reaches 64 edits, 1 MiB, or two seconds
without catching up. A client crash can lose edits without a durable receipt;
there is deliberately no local recovery fallback. Accepted host transactions
continue if their client disappears, and committed text can recover without the
source file.

Saving checks the expected source digest, preserves ownership, mode and supported
ACLs, and syncs an atomic file replacement and its parent directory. A worker
crash after publication is reconciled before saved state is published. Uncertain
saves and tool mutations are not blindly retried. External changes, permission
failures and symlinks preserve editor text; use Save As to keep a conflicting
version under a new name. The final source check and rename are separate OS
operations, so an uncooperative external writer can still race that interval.

Renames record inode identity and reconcile recovery paths after crashes.
Corrupt or ambiguous recovery, quota exhaustion, and filesystem errors retain
recovery data for inspection. Keep SQLite WAL and sidecars together; do not
manipulate a live state directory. Back it up using a SQLite-aware procedure
after stopping its worker.

Build and terminal output uses separate bounded streams. Losing a stream stops
its tracked process; losing the control connection stops tracked shell/build
groups and cancels jobs. Intentionally detached processes remain managed by the
SSH account. Terminal output drains before exit is reported. A worker with no
connections exits after five minutes, leaving recovery on disk.

### Supported scope and limits

- Direct SSH to Linux or macOS hosts; Windows can be a client. Windows hosts,
  jump hosts, automatic helper installation, passwords and interactive MFA are
  not implemented.
- Recovery storage must use an approved host-local filesystem: ext-family, XFS,
  btrfs or ZFS on Linux, or APFS on macOS. Network filesystems, tmpfs and unknown
  filesystems cannot hold durable recovery. Projects may use separate storage,
  subject to its save semantics.
- Up to 128 retained documents, 8 MiB of decoded text per document, a 128 MiB
  logical journal budget, 16 socket peers, and eight tool jobs/processes per
  session. Individual pending edits share the 1 MiB client queue budget.
- Compaction runs every 512 edits, retaining up to 256 undo/redo changes within
  a 16 MiB history budget and 256 recent deduplication receipts. Quotas fail
  closed; unsaved recovery is not automatically deleted.
- The folder browser skips non-UTF-8 names and symlinks. Git transport preserves
  Unix filename bytes, but the folder UI does not cover every such path. File
  duplication is supported; directory duplication is not. Trash moves into host
  state and refuses cross-filesystem moves.
- Keep one remote tab open; create another scratchpad before closing the last
  tab. Remote project-file persistence, Markdown preview, CUDA/debugging,
  definition/reference lookup, compare-disk and mini-diff tools are unavailable.
  Regular editing, syntax highlighting, folder search, Source Control, builds,
  and terminals remain available.

See [SSH architecture and testing](DEVELOPER_GUIDE.md#ssh-architecture-and-testing)
for implementation invariants and live test instructions.

## Editing

Every change is part of the file's undo history (**Ctrl+Z**, **Ctrl+Shift+Z** or
**Ctrl+Y**), which is kept across restarts for files that are fully loaded. The
**Edit** menu holds all the commands below, under Selection, Line, Text, and
Indentation, and right-clicking the text gives the common ones.

### Clipboard

With nothing selected, **Ctrl+C** and **Ctrl+X** copy or cut the whole line, and
pasting that line puts it above the current line. **Ctrl+Shift+V** pastes and fits
the text to the current indentation. **Ctrl+K, Ctrl+V** lists the last 15 things
you copied in hedit to paste one again.

### Lines and text

| Shortcut | Action |
| --- | --- |
| Ctrl+L | Select the line (again to add the next) |
| Ctrl+Shift+D / Ctrl+Shift+K | Duplicate / delete lines |
| Ctrl+Shift+Up / Down | Move lines up or down |
| Ctrl+Enter / Ctrl+Shift+Enter | New line below / above |
| Ctrl+J | Join lines |
| Ctrl+] / Ctrl+[ | Indent / unindent |
| Ctrl+/ / Ctrl+Shift+/ | Toggle line comment / block comment |
| Alt+Q | Rewrap the paragraph at the ruler, keeping comment markers and bullets |
| Ctrl+T | Swap the characters around the cursor, or the text of selections |
| Ctrl+Shift+Backspace / Ctrl+K, Ctrl+K | Delete to the start / end of the line |
| Ctrl+K, Ctrl+U / Ctrl+K, Ctrl+L | Uppercase / lowercase |
| F9 / Ctrl+F9 | Sort lines ignoring case / exactly |

The menus also have Title case, Swap case, Reverse lines, Remove duplicate lines,
Trim trailing whitespace, and Reindent lines (for bracket-based languages). Line
commands such as Duplicate, Delete, and Move act on the current line when
nothing is selected. Sort, Reverse, Remove duplicates, Trim trailing whitespace,
and indentation conversion apply to the whole file when nothing is selected.

### Multiple cursors

- **Ctrl+D** selects the word, then each press adds its next occurrence; **Ctrl+K,
  Ctrl+D** skips one, and **Alt+F3** selects them all.
- **Alt+click** adds a cursor, **Alt+Shift+Up/Down** adds one on the line above or
  below, and **Alt+Shift+drag** selects a rectangle.
- **Ctrl+Shift+L** splits a selection into one cursor per line.
- **Esc** goes back to one cursor.

Typing, deleting, pasting, and line commands work on every cursor at once, as one
undo step, for up to 1,000 cursors.

### Selecting

**Ctrl+Shift+Space** grows the selection step by step: word, quotes, brackets,
line, file. **Ctrl+Shift+M** selects inside brackets, **Ctrl+Shift+J** the lines at
the same indentation, and **Ctrl+M** jumps between matching brackets. **Ctrl+U**
and **Ctrl+Shift+U** undo and redo selection changes.

In HTML or XML files, **Edit → Selection → Expand selection to HTML/XML tag**
(**Ctrl+Shift+A**, **Cmd+Shift+A** on macOS) selects an element's contents, then
its complete tags, then the enclosing element on repeated use. It works with
multiple cursors and preserves reversed selections. Comments, CDATA, quoted
attributes, and HTML script/style content do not introduce false nested tags.
HTML void elements and XML self-closing tags select as a whole. The command uses
explicit balanced tags; it does not infer omitted HTML closing tags or parse JSX
and template expressions. It is disabled for paged files and other syntaxes.

### Brackets and indentation

Typing a bracket or quote adds its closer, wraps a selection, or steps over an
existing closer; Backspace in an empty pair removes both. Single quotes are not
paired in prose or Rust. The bracket matching the one at the cursor is outlined.

Enter keeps the indentation and adds a level after an opening bracket; if you move
on without typing, the added indentation is removed again. Files open with the
indentation they already use; your `tab_size` and `translate_tabs_to_spaces`
settings apply to new files. **Edit → Indentation** switches between spaces and
tabs, sets the width, converts a file, or detects its style again; the same
choices are on the status bar.

### Macros

**Ctrl+Alt+Q** starts recording what you type and the keys you press in the editor;
press it again to stop. **Ctrl+Alt+Shift+Q** plays it at the cursor. **Tools → Save
macro…** keeps it by name in `macros.json`, and **Run saved macro…** plays a saved
one. Commands chosen from menus are not recorded; use their shortcuts.

### Spell check

**F6** underlines misspelled English words with a red wave: everywhere in plain
text, LaTeX, and Markdown (except code), and in comments and strings in code.
Code-like words, abbreviations, web addresses, and other scripts are skipped.
Right-click a word for suggestions, **Add to dictionary** (kept in `added_words`),
or **Ignore**; **Ctrl+F6** and **Ctrl+Shift+F6** jump between misspellings. The
dictionary is built in.

### Arabic, Hebrew, and other scripts

Arabic and Hebrew read right to left with joined letters, and mix with English and
numbers on one line. Only the display is reordered: the file keeps the order you
typed. In prose files each line takes the direction of its first letter; in code
every line runs left to right, so a comment keeps `//` at its start. The caret,
clicks, and arrow keys follow what is on screen.

Scripts the built-in font lacks, including Chinese, Japanese, Korean, Devanagari,
and Thai, are drawn with your system's fonts (install `fonts-noto` if you see
boxes). This works with any editor font you choose.

## Finding and navigating

### Find and replace

**Ctrl+F** opens the search panel: the first match after the cursor is selected
as you type, and every match is highlighted. **Enter** and **Shift+Enter** go to
the next and previous match. **Ctrl+H** adds Replace; **Replace all** is one undo
step. Options are Regex (with `${1}` groups in replacements), Case sensitive,
Whole words, and Selection only.

- **Alt+Enter** puts a cursor on every match.
- **Ctrl+E** / **Ctrl+Shift+E** use the selection as the Find / Replace text.
- **Ctrl+F3** selects the word at the cursor, then its next copy on each press
  (**Ctrl+Shift+F3** for the previous).

### Find in folders

**Ctrl+Shift+F** searches every open folder, including unsaved changes in open
files, optionally limited to files such as `*.rs, *.cu`. Git ignore rules apply.
Results appear in collapsible file groups, with line numbers and highlighted
matches. Turn on **Context** to see surrounding lines. Click a match to open it,
or use **Next** / **Previous** (**F4** / **Shift+F4**) to step through results.

To replace, turn on **Replace**, enter the replacement text, and choose
**Preview**. Select files in the list, review the diff beside it, and choose
**Apply to … files**. Open files change in the editor, unsaved, so you
can undo them there; other files are written only if they have not changed since
the preview, and **Undo replacements** puts them back.

### Go to anything

- **Ctrl+P** goes to a file. Add `@name` for a symbol, `:42` or `:42:7` for a line
  and column, or `#text` for a line containing text, as in `main.rs@save`; on their
  own they work in the current file. Results preview as you move, and **Esc** goes
  back.
- **Ctrl+Shift+P** opens the command palette. Type a few letters of a command, in
  order (`tglc` finds Toggle line comment).
- **Ctrl+G** goes to a line, **Ctrl+R** lists the file's symbols (or the whole
  project's), and **Alt+Left/Right** go back and forward through where you have been.

### Definitions and references

**F12** goes to where the name at the cursor is defined, searching open files and
then your folders; when there are several, it lists them. **Shift+F12** lists every
line that uses the name. Resting the pointer on a name shows its definitions from
the open files. These work by reading source lines, without a language server, so
they cover common definitions (functions, types, macros, declarations) rather
than every language rule.

### Switch header and implementation

**Goto → Switch header/implementation** (**Alt+O**, or **Option+O** on macOS)
jumps between files with the same stem, such as `solver.cpp` and `solver.hpp`,
or `kernel.cu` and `kernel.cuh`. The command also appears in the command palette
and can be rebound in **Settings → Key Bindings**.

hedit checks the current file's directory first. If no match is beside it, it
searches the nearest containing open project folder, respecting Git ignores and
folder exclusions. Without an open folder, it checks siblings only. A single
match opens directly; several matches appear in a filterable picker. **Esc**
cancels the picker. Existing tabs keep their unsaved edits and cursor positions,
and **Navigate back** returns to your starting location.

Headers use `.h`, `.hh`, `.hpp`, `.hxx`, `.h++`, or `.cuh`; implementations use
`.c`, `.cc`, `.cpp`, `.cxx`, `.c++`, `.cu`, `.m`, or `.mm`. Untitled files and
other file types leave this command disabled.

A named source buffer can find its header before its first save. If a matching
file disappears before it opens, hedit reports the error and keeps the source
buffer active.

### Open a file path under the cursor

**Goto → Open file under cursor** (**Alt+D**, or **Option+D** on macOS) opens a
local path in the current line or selection. It is also in the editor's
right-click menu and the command palette. Quote paths containing spaces, such
as `"data/run results.csv"`; quoted filenames preserve literal punctuation.
Includes such as `<lib/solver.h>` and locations
such as `src/main.cpp:42:7` work too.

Relative paths are checked beside the current document first, then under each
open folder and its `include` directory. Absolute paths work without an open
folder. Multiple matches appear in a filterable picker. Existing buffers keep
unsaved edits; a location suffix moves their cursor to that line and column.
Only existing files open: missing paths, folders, and web URLs leave the current
document unchanged. References are limited to one line and 4 KiB.

### Bookmarks

**Ctrl+F2** sets or removes a bookmark, **F2** and **Shift+F2** go to the next and
previous, and **Goto → Bookmarks…** lists them. **Alt+F2** puts a cursor on every
bookmark, and **Ctrl+Shift+F2** clears them. Bookmarks are kept across restarts.

## Writing code

### Completions

While you type a word, a list at the cursor offers words from your open files and
matching snippets. **Up/Down** choose, **Tab** or **Enter** accepts, and a space or
**Esc** closes it; **Ctrl+Space** opens it at once. Turn it off with
`auto_complete`, or set `auto_complete_commit_on_tab` so Enter always starts a new
line.

### Snippets

Type a snippet's trigger and press **Tab**, or choose **Tools → Insert snippet…**.
Snippets can contain:

- `$1`, `$2`, … for fields, and `${1:text}` for a field with starting text. After
  inserting, **Tab** and **Shift+Tab** move between fields; a number used twice is
  filled in everywhere at once.
- `$0` for where the cursor ends, and `${SELECTION}` for the selected text.
- `\$` and `\}` for literal characters.

Edit snippets under **Settings → Snippets** (saved in `snippets.json`); a
snippet's syntax limits it to one language.

### Changes against Git

Markers in the gutter show lines changed since Git's last commit, or since the
last save for files outside Git (`mini_diff`). Hover a marker to see the change.
**Ctrl+K, Ctrl+.** and **Ctrl+K, Ctrl+,** go to the next and previous change, and
**Ctrl+K, Ctrl+Z** reverts the change at the cursor as an undoable editor edit.
**Tools → Compare file with Git HEAD…** compares the current editor text with
the last commit. For staging and repository operations, use [Source Control](#source-control).

### Build systems

**Ctrl+B** saves and builds the file, showing the output in a panel; click an error,
or press **F4** / **Shift+F4**, to go to it. Built-in build systems cover CUDA
(`nvcc`), C++ (`g++`), C (`gcc`), Rust (`cargo build`), Python, and Make, chosen by
syntax; **Ctrl+Shift+B** picks one and remembers it. Commands run through `sh`
on Linux/macOS and `cmd.exe` on Windows; use the matching shell syntax and
install your compiler or interpreter separately. The Python command is `python`
on Windows and `python3` elsewhere. Add your own in
[settings.json](#the-settings-file):

```json
"build_systems": [
  {
    "name": "CUDA release",
    "shell_cmd": "nvcc -O3 \"$file\" -o \"$file_base_name\"",
    "file_regex": "^(..[^(]*)\\((\\d+)\\): (.*)$",
    "selector": ["cuda"]
  }
]
```

`shell_cmd` can use `$file`, `$file_path`, `$file_name`, `$file_base_name`,
`$file_extension`, and `$folder`; `file_regex` captures the file, line, optional
column, and message. A build system named like a built-in one replaces it.

### Syntax highlighting

The language is chosen from the file name. Click it in the status bar to pick
another, by name, extension, or file name such as `Makefile`; choose
**Automatic** to go back, or **Plain text** for no colors. A choice you make stays
with the file.

hedit knows 58 formats, including C, C++, CUDA, C#, Go, Java, JavaScript and
TypeScript, Kotlin, Python, Rust, Swift, Zig, shell, PowerShell, SQL, HTML, CSS and
Sass, JSON, YAML, TOML, XML, Markdown, LaTeX, CMake, Makefiles, Dockerfiles,
Fortran, Julia, MATLAB, R, Verilog, VHDL, diffs, CSV, and logs. Most are highlighted
with full syntax definitions that understand embedded languages and multi-line
strings; plain text, logs, and CSV use a faster built-in highlighter. Large files
show quick colors first and full colors a moment later.

## Integrated terminal

The integrated terminal is **disabled by default**. A new installation offers
**Keep disabled** and **Enable terminal** on first launch. You can change the
choice in **Preferences → Settings → Terminal** or **Tools → Terminal settings…**
at any time. Existing installations skip the first-launch prompt and keep the
terminal disabled until you enable it. Enabling the feature does not launch a
shell; **Open terminal** on the settings page opens it when you are ready.

Once enabled, use **Tools → Toggle terminal** or **Ctrl+`** (**Control+`** on macOS)
to open a resizable bottom panel. It starts your local default shell in the
active file's project folder, the first open folder, or the file's directory.
With no file or folder open it uses hedit's launch directory. Each new tab chooses
its starting folder from the editor's current file and folders. Windows uses the
default command shell. The shell has the same permissions as hedit. Drag the
panel's top edge to resize it; click the editor to return to editing.

Type commands directly into the panel. Escape, Tab, arrow keys, and Ctrl shortcuts
go to the shell while it has focus; **Ctrl+C** interrupts a command. On macOS,
use Control+C to interrupt and Cmd+C to copy. Drag across text to select it,
then use **Copy selection**, **Ctrl+Shift+C**, or **Cmd+C** on macOS. Paste with
**Ctrl+Shift+V** or **Cmd+V**; pastes use bracketed paste when the shell supports
it and are limited to 256 KiB. Scroll to read up to 10,000 lines of history.

Each of the terminal's tabs keeps its own process, directory, selection, and
scrollback. Output continues in inactive or hidden tabs; a dot marks new output.
Hover a tab for its full title, starting directory, and shell status.

| Control | Action |
| --- | --- |
| **+** | Open another shell, up to 16 tabs |
| Tab | Select it; drag to reorder, or middle-click to close |
| Tab's **X** | Close that shell; shown on the selected tab and on hover |
| **…** | List all tabs, including those outside the visible strip; access the selected tab's actions and **Copy selection** |
| Right-click a tab | Rename it, restore its shell title, restart or stop its shell, or close it |
| Down chevron | Hide the panel and keep all shells running |

Tab names follow the shell's title until you set **Tab name** in either menu.
Choose **Use shell title** to restore automatic naming. **Restart shell** replaces
only that tab's shell and clears its output, keeping its name and starting
directory. **Stop shell** ends it and keeps its output available to read and copy.
A small outlined square marks a stopped or exited shell.

While the terminal has focus, **Ctrl+Shift+T** opens a tab, **Ctrl+Shift+W** closes
it, and **Ctrl+Page Up / Page Down** switches tabs. On macOS, use **Cmd+Shift+T**,
**Cmd+Shift+W**, and **Control+Page Up / Page Down**. These commands also appear
in the command palette, along with **Terminal: restart shell**. Their default
keys keep their editor behavior when the editor has focus. **Ctrl+Shift+P**
(**Cmd+Shift+P** on macOS) still opens the command palette from the terminal.
Escape is sent to the shell; use the down chevron or toggle shortcut to hide it.

The down chevron or toggle shortcut hides all tabs and keeps their shells running.
The X beside a tab closes that shell. Disabling the terminal or quitting hedit
ends all sessions. Shells, tab names, running commands, and terminal scrollback
are not restored after quitting.

Arabic and other right-to-left text display in reading order alongside Latin
text and numbers. Arabic uses OpenType shaping, including joining, ligatures,
and diacritics, with a bundled font available on every platform. This changes
display only: shell input, cursor commands, and copied text keep their original
logical Unicode order. Pointer selection and the displayed cursor follow the
visual cell mapping. Input-method composition is shown before it is committed.
**Settings → Terminal → Bidirectional text and Arabic shaping** is enabled by
default. Turn it off for applications that already send text in visual order.

Ghostty supplies terminal emulation; hedit supplies the PTY and native rendering.
The MIT license notices for [Ghostty](assets/licenses/ghostty-MIT.txt) and its
[Rust bindings](assets/licenses/libghostty-rs-MIT.txt) are bundled.
Noto Sans Arabic is bundled under its [SIL Open Font License](assets/licenses/Noto-Sans-Arabic-OFL.txt).
This initial panel supports text and ANSI colors. It does not render terminal
graphics or forward mouse events to terminal applications. Builds made without
terminal support explain this on the Terminal settings page.

## Source Control

Use the **Files / Source Control** switch at the top of the sidebar, **Tools →
Source Control**, or **Ctrl+Shift+G** (**Cmd+Shift+G** on macOS). The active segment
is highlighted; narrow sidebars shorten Source Control to **Git**. Switching views
keeps your editor tabs and commit message. Each Git view remembers its filter and
selected file or commit. Contextual history and comparison links offer **Back to…**
to return to the previous review. This native workspace uses
Git installed on your `PATH` (Git 2.30 or newer). Your existing Git identity,
credential helpers, SSH agent, signing settings, hooks, and remotes apply.
It does not require a browser or a hosting-service account.

The repository selector discovers repositories from your workspace folders and
the active file. Its **…** menu holds **Refresh**, **Open repository…**, **Clone
repository…**, and remote actions. Click the branch name to search and switch
branches or create a new one. Selecting a remote branch reuses an existing local
tracking branch; otherwise a dialog lets you choose its local name. Git dialogs
focus the first field. Enter submits a one-field dialog or the final single-line
field; Ctrl+Enter (Cmd+Enter on macOS) submits completed forms, including tags.
Destructive confirmations still require choosing the named action. Local repositories show **Add remote…**; connected
branches show relevant Pull/Push counts and **Fetch**, or **Publish branch** when
an upstream is missing. Counts reflect the last fetch. Diverged branches offer
**Compare** before you choose a merge or rebase. Clone a URL/local repository
into a new folder. Enter the full destination path when cloning. Clone progress
shows a Cancel action; a failed or cancelled clone keeps the previous repository,
review, and commit draft. A partially created destination may remain and can be
inspected before choosing a new destination. A folder without
Git can be initialized from the sidebar. **Files** or **Ctrl+W** returns to the
editor; open tabs, cursor positions, and unsaved text remain available.

### Review, stage, and commit

**Changes** separates conflicts, staged files, and working changes. Select a file
to compare the **working tree with the index**, or the **index with HEAD** for
staged files. New files have a text preview; binary files show a summary. **Unified**
and **Side by side** share syntax coloring, character-change highlights, line
numbers, and horizontal scrolling. Narrow windows use Unified automatically. **Copy diff** copies the
complete patch, including headers hidden from the visual review.

Choose **Review changes** to start at the first matching file. **Previous** and
**Next**, or **Alt+Up / Alt+Down** (**Option+Up / Option+Down** on macOS), step through
file comparisons in Changes, commit reviews, and branch comparisons. The counter
shows your position. Navigation follows the file filter; a file with both staged
and working edits has a separate comparison for each side. Clear a filter with its
close button to see the full list again.

Both layouts offer **Copy line** and **Copy diff** from the right-click menu;
**Copy line** copies the saved source without the diff marker. Line numbers stay
fixed while code scrolls horizontally. In compact windows, Unified is shown as
the active layout; widening the review area restores your side-by-side preference.

Diffs and conflict previews use the active editor theme, font, font size, line
spacing, and tab display settings. Added and removed lines use the theme's success
and error colors; selected lines use its selection colors. Gutter typography and
line-number visibility follow the editor settings too.

**Tree** groups files into collapsible folders and compacts single-child folder
chains. Folder counts include all changed descendants. Filtering reveals matching
files inside collapsed folders without losing your expansion state. **Expand all**
opens every folder; **List** shows full paths instead. Both layouts draw only
visible rows. Your Tree/List preference is remembered. Commit review uses the same
tree, with a separate **Filter files…** field.

- **+** stages a file; **−** unstages it. Section actions stage/unstage all files
  in that section, including files hidden by the filter.
- **Stage hunk** and **Unstage hunk** select individual changes in ordinary text
  files. New/deleted files, mode changes, binary files, and some unusual path
  encodings use whole-file staging. Changed diffs must be refreshed before a hunk
  can be applied.
- Click added or removed lines to select them in either diff layout; click again
  to deselect. Shift-click extends the selection over changed lines. **Stage
  selected** or **Unstage selected** applies only those additions and deletions
  to the index. Select both sides of a replacement to include the entire edit.
  **Clear selection** resets the selection. It also resets when the reviewed diff
  changes, or when you switch files or views. The same whole-file restrictions
  and stale-diff checks as hunk staging apply.
- The review follows a file into its new group after staging or unstaging, and
  clears after the file has no remaining changes. Unstaging only changes the
  index and can be done while the editor has unsaved text.
- Enter a message and choose **Commit staged**, or press **Ctrl+Enter**
  (**Cmd+Enter** on macOS). Only the index is committed. Unsaved editor edits are
  not included. The message below the button explains what is needed when it is
  disabled. The commit button’s **…** menu offers **Amend last commit**, which
  asks for confirmation because it changes the commit ID. Git commit and
  next/previous file commands also appear in the command palette and can be
  customized in Key Bindings.
- **Discard working changes…** restores a tracked file from the index after
  confirmation. A copy of an existing regular file is kept under the session's
  `git-backups` directory; **Git output** reports its path. Staged text is kept.
  Untracked files, links, and submodules are not deleted by this action.

Save edited files before staging them. Actions that can change working files
(switching branches, merging, stashing, pulling, or restoring) are blocked while
the repository has unsaved open buffers. Save or close those tabs first; closing
nonempty unsaved text preserves it in Closed Files. Git can also refuse operations
when saved working changes would be overwritten.

### History, branches, and stashes

**History** shows commits reachable from local branches, remote branches, tags,
and HEAD, with a parent graph and reference labels. Select a commit, then a file,
to review its diff against its first parent. Root commits compare with an empty
tree. Copy a commit ID, cherry-pick a commit, or create a reverting commit from
the detail toolbar. Merge commits that need a mainline selection should be
cherry-picked/reverted in a terminal. History initially loads 200 commits;
**Load more commits** extends this up to 2,000.

**Branches** creates and switches local branches, creates local tracking branches
from remote branches, merges into the current branch, and deletes merged local
branches. The **…** menu beside a branch also offers comparison, scoped history,
renaming, setting an upstream, and rebasing the current branch onto it. Rebase
rewrites commit IDs and asks for confirmation; conflicts use the same Continue
and Abort controls in Changes. Save or close unsaved repository files first.
Git refuses deletion of an unmerged branch. Remotes can be added and removed here too.

**More → Stashes** saves tracked and untracked working changes with a description.
**Apply** restores the saved working/index state and keeps the stash; **Drop…**
removes it after confirmation. If applying produces conflicts, resolve and stage
them before continuing other work.

### Compare branches and revisions

Choose **More → Compare**, or **Compare with current branch** from a branch’s
**…** menu. Pick a Base and Head using branch pickers, or enter tags, commit IDs,
or revisions such as `HEAD~3`. **Compare** lists the affected files and shows
how many commits Head is ahead of or behind Base. Opening Compare chooses the
upstream, another main branch, or HEAD’s parent as an initial base when available.
**Swap** reverses the inputs and updates the review immediately. Press Enter in a
revision field or choose Compare to apply edited inputs; a hint identifies inputs
that have not yet been applied.

**Changes since common ancestor** reviews Head’s contribution since the branches
diverged, like a pull request. Turn it off to compare the current contents of the
two revisions directly. Select a file to use the normal Unified or Side by side
diff with syntax colors and Copy diff. These comparisons are read-only. Reviews
are pinned to exact commit IDs; refreshing a moving branch clears an outdated
file selection so a previous patch cannot appear under the new comparison.

### File history and line authorship

In **History**, expand **History scope** to limit commits to a branch or revision,
a repository-relative file or folder, or both. **Apply scope** applies the inputs;
**All history** clears them. The regular Filter field searches the loaded commits.
A file’s context menu also offers **File history**, including for files in commit
reviews. File history follows the selected path; it does not follow earlier names
across renames.

Choose **More → Line history**, enter a repository-relative file and revision,
then **Show line history**. **Active file** uses the current editor file. A changed
file’s context menu offers the same view. Each line shows its author, commit ID,
and source text; select a commit ID to inspect that commit. Filtering matches
source text, authors, IDs, and commit summaries. The view uses committed UTF-8
text at the selected revision, so saved working changes and unsaved editor text
are excluded. It follows the editor’s theme, font, spacing, and tab settings.

### Tags and recovery

**More → Tags** lists local tags. **Tag HEAD…**, or **More… → Tag…** in a commit’s
detail bar, creates an annotated tag with a message. A tag’s **…** menu inspects its commit,
compares it with HEAD, creates a branch there, copies its name, or deletes the local
tag. Deletion checks the reviewed tag object, so a tag moved by another Git client
must be refreshed first. Tags are managed locally; publishing tags uses a terminal.

**More → Recovery** shows the local HEAD reflog, including movements caused by
commits, resets, switches, and rebases. **Recover branch…** gives an earlier commit
a new branch name without switching the current checkout or changing saved or
unsaved files. Selecting a reflog commit ID opens its history and file diffs, even
when the commit is no longer reachable from another branch. Reflog entries are
local and may expire according to Git’s configuration.

**Undo last commit…** moves the current branch to its first parent while preserving
the index, working files, and unsaved editor text. The undone changes remain staged,
alongside any existing staged edits. This asks for confirmation and checks that
HEAD still matches the reviewed commit. The initial commit and operations already
in progress cannot be undone here. Use **Revert…** for commits shared with others.
**More… → Branch here…** in any commit’s detail bar also creates a branch without
switching.

### Conflicts and remotes

Conflicts appear in their own group. Select one to review **Current · ours**,
**Incoming · theirs**, and the common ancestor where available. **Use Current…**
or **Use Incoming…** replaces the working file after confirmation and backup;
review and stage it afterward. **Open to resolve** opens the normal editor to
combine both sides manually. Save, then **Stage resolved file** or **+** to mark
it resolved. For a deletion conflict, keep/delete the file in the editor or a
terminal, then stage the result. Binary, non-UTF-8, link, submodule, and large
conflicts may need a terminal. During rebase, Current is the branch being rebased
onto and Incoming is the commit being replayed. An operation banner offers
**Continue** and **Abort…** for merges, cherry-picks, reverts, and rebases.
Replacing a conflict checks both the working file and the reviewed index sides;
if another Git client changes either, refresh and review before trying again.

**Fetch** updates remote references. **Pull** only fast-forwards the upstream;
divergent histories require an explicit merge or rebase from Branches. **Push** uses
Git's configured destination. **Publish** lets a branch without an upstream pick
a remote and sets its upstream. These actions never force-push. Configure
credentials outside hedit if Git reports that authentication is unavailable;
terminal password prompts are disabled, while configured credential helpers and
SSH agents remain usable.

Git work runs in the background. **Refresh** in the repository’s **…** menu reloads
repository state, which also refreshes every four seconds while this workspace is
visible. Automatic refresh uses a small branch-row indicator and keeps file/view
navigation available. Actions show their progress by name and a short completion
message; confirmation buttons name the action they perform. Operation controls
for conflicts remain visible when switching Git views. **Cancel** stops
the current operation; refresh afterward to inspect its outcome. Git commands
time out after 30 seconds locally or 120 seconds for network operations and have
a 24 MiB output limit. **Git output** keeps recent action results. The normal
editor and offline guide work without Git installed.
Actions stay bound to the selected repository root. If a nested repository is
removed or moved, hedit asks you to reopen it and refuses to run its actions
against an enclosing repository. Cancelling the repository picker keeps your
current selection and draft.

## Markdown preview

Markdown files (`.md`, `.markdown`, and `.mdown`) have a toolbar with **Source**,
**Split**, and **Preview**. Split shows editable source alongside the rendered
document and stacks the views in narrow windows. Preview fills the editor area
with the rendered document; switch back to Source or Split to type. Unsaved
changes appear live, and each tab's view mode is kept between launches. For an untitled note, choose
**Markdown** from the language picker in the status bar.

In Split, scrolling either pane also scrolls the other to the corresponding part
of the document. This works side by side and in the stacked layout. Horizontal
scrolling stays independent. Drag the divider to resize the panes, or focus it
and use the arrow keys (hold Shift for larger steps). Double-click the divider
to restore equal sizes. Side-by-side and stacked layouts keep separate sizes
for each document and editor group during the session.

The preview and offline guide use a document layout modeled on VS Code's
Markdown preview: compact text, consistent block spacing, a padded page that
uses the available pane width, and rules below the first two heading levels.
Emphasis uses real italic and semibold fonts. Inline code shares the surrounding
text size and baseline. Links use your theme's accent color and underline on
hover or keyboard focus; right-click a link to copy its address. Editor zoom
also changes the reading size.
Use **Contents** in the preview toolbar to jump to a heading or the top of the
document without moving the source caret.

Tables size their columns to the content, wrap long cells, and scroll
horizontally when the table is too wide. Code blocks use your syntax colors;
**Copy** appears on hover or keyboard focus. Long lines scroll horizontally
inside the block, and tall samples remain part of the document's normal vertical
scroll. Text stays selectable across paragraphs and code blocks. The preview
also renders nested lists, task lists, quotes, and GitHub-style alerts. Section links scroll to their headings, including headings in
quotes and lists. Markdown file links stay in Preview and honor `#section`
fragments; other file links open in the editor. Local PNG and JPEG images resolve
relative to the Markdown file's folder; Save As updates that folder. Images
from web URLs or `file://` URLs are not fetched. On Windows, network-share and
device image paths are also blocked. HTML comments are hidden, `<br>` inserts a
line break, and other embedded HTML is displayed as text. Preview
is available for fully loaded documents up to 2 MiB; larger or paged files remain
editable in Source.

Use `$E = mc^2$` for inline LaTeX math and `$$ … $$` for a displayed equation.
Fenced `math` blocks also render equations. Inline equations align with the text
baseline, including fractions and matrices. Display equations are centered and
keep their font size; wide equations scroll inside their own block. Right-click
an equation or diagram and choose **Copy source** to copy its original notation.
Escape literal dollar signs as `\$`
when writing prices or other text that should not become math. Fenced `mermaid`
blocks render diagrams, for example:

````markdown
```mermaid
flowchart LR
    A[Write Markdown] --> B[Preview]
```
````

Equations and diagrams render locally in the background and follow the preview's
theme. Unsupported or malformed input stays visible as source with an error on
hover. Editing prose reuses unchanged equations and diagrams; changing the theme
or closing a document releases its obsolete media. Native Mermaid rendering supports common diagram types, including
flowcharts and sequence diagrams; some Mermaid features may be unsupported.
Individual equations are limited to 16 KiB, and diagrams to 64 KiB, 1,000 lines,
256 nodes, and 1,024 edges.

Use **Refresh** in the Markdown toolbar or **View → Markdown** to reload the
preview, equations, diagrams, and local images, including images that were missing
or changed on disk. Switching from Source back to Split or Preview also refreshes
rendered media.

The same commands are in **View → Markdown** and the command palette. Find,
Replace, Go to line, and search-result navigation expose the source when used
from Preview. Cancelling a Go to Anything preview restores the original view mode.

## Views and layout

- **Layouts:** **Alt+Shift+1–4** for one to four columns, **Alt+Shift+8/9** for two
  or three rows, and **Alt+Shift+5** for a grid of four. **Ctrl+1–4** focuses a
  group, and **Ctrl+Alt+Shift+Right** moves the file to the next group. Each group
  has its own cursor and scroll position on a shared file.
- **Folding:** **Ctrl+Shift+[** and **Ctrl+Shift+]** fold and unfold an indented
  block. **View → Code folding → Fold level 1 / 2 / 3** collapses blocks at that
  nesting depth, leaving their parents visible. Use **Ctrl+K, Ctrl+1 / 2 / 3**
  (**Cmd+K, Cmd+1 / 2 / 3** on macOS); **Ctrl+K, Ctrl+J** unfolds everything.
  **Fold all blocks** remains in the menu and palette. Choosing a level replaces
  existing folds and moves a hidden cursor to the fold boundary without changing
  text. With the pointer over the gutter, arrows appear beside foldable lines;
  click one. Tabs, spaces, and uneven indentation all count nesting steps.
- **Wrap and rulers:** **Ctrl+Shift+W** toggles wrapping; **View** sets the wrap
  column and rulers (up to eight in settings).
- **Full screen and focus:** **F11** toggles full screen, and **Shift+F11**
  distraction-free mode, which hides the sidebar, tabs, status bar, and minimap and
  centers the text. They work independently.
- **Fonts:** **Preferences → Editor font…** lists every installed font, monospace
  first, with the scripts each covers; type to filter, such as `mono` or `arabic`.
  **Ctrl++**, **Ctrl+-**, and **Ctrl+0** change the size.
- **Scrolling:** **Ctrl+Up/Down** scroll a line without moving the cursor.
- **View → Editor** also toggles line numbers, the current-line highlight,
  whitespace, indentation guides, the minimap, the tab bar, and the status bar.

The status bar shows the cursor position, indentation, line endings, encoding,
and syntax. The small dot at the right shows recovery state; hover for details
or click to open recovery settings. Hover over the cursor position for file size.

## Large files and logs

Files over 2 MiB open in pages of about 256 KiB, with **Start**, **Previous**,
**Next**, and **End** buttons; **Ctrl+G** and Find still cover the whole file.
Some commands that need the whole text, such as multiple cursors and folding, are
off for paged files.

**View → Follow log (read only)** shows the end of a saved file and adds new output
as it arrives. hedit notices when a file changes on disk: unchanged files reload,
and changed ones offer **Compare changes…** to take the disk version (your text
stays in Undo) or keep yours.

## Settings

**Preferences → Settings…** (**Ctrl+,**) has pages for General, Terminal, Editor, Formatting,
Folders & Search, Appearance, Key Bindings, Snippets, and Recovery. Linux also
has a CUDA Tools page.
Changes save as you make them. General, Editor, Formatting, and Folders & Search
have **Restore defaults**; Key Bindings has per-command **Reset** and **Reset all**.

**Settings → Key Bindings** lists every command: click a shortcut and press new
keys (two keys in a row make a two-key shortcut such as **Ctrl+K, Ctrl+G**). They
are saved in `keybindings.json` as `{"command": "find_files", "shortcut":
"Primary+Shift+F"}`, where `Primary` is Ctrl on Linux and Windows, and Cmd on macOS.

### The settings file

**Open settings.json** opens your settings as a file; saving it applies them, and
edits from other editors apply when hedit regains focus. It lives in
`~/.config/hedit/` on Linux, `~/Library/Application Support/hedit.hedit/` on
macOS, and `%APPDATA%\hedit\hedit\config\` on Windows (or `HEDIT_CONFIG_DIR`),
with `keybindings.json`, `snippets.json`,
`macros.json`, and your themes. Comments and trailing commas are allowed, unknown
keys are reported, and a file with errors is never overwritten.

| Key | Default | Meaning |
| --- | --- | --- |
| `font_face` | `""` | A font family or font file; empty uses the built-in font |
| `font_size`, `line_padding` | `14`, `0` | Font size (10–28) and extra line spacing (0–20) |
| `caret_width`, `caret_blink` | `1.5`, `true` | Cursor width (1–4) and blinking |
| `word_wrap`, `wrap_width` | `false`, `0` | Wrap long lines, at a column (`0` is the window edge) |
| `rulers` | `[]` | Ruler columns, such as `[80, 120]` |
| `scroll_past_end` | `true` | Let the last line scroll to the top |
| `line_numbers`, `highlight_line` | `true`, `true` | Line numbers and current-line highlight |
| `draw_white_space`, `draw_indent_guides`, `show_minimap` | `false`, `true`, `false` | Whitespace, indentation guides, minimap |
| `auto_indent`, `smart_indent`, `smart_backspace` | `true` | Keep indentation, indent after brackets, Backspace by levels |
| `trim_automatic_white_space` | `true` | Remove indentation Enter added if the line stays empty |
| `detect_indentation` | `true` | Open files with the indentation they use |
| `auto_match_enabled`, `match_brackets`, `match_selection` | `true` | Close brackets, outline the matching bracket, outline copies of the selected word |
| `copy_with_empty_selection` | `true` | Copy and cut the line when nothing is selected |
| `auto_complete`, `auto_complete_delay`, `auto_complete_commit_on_tab` | `true`, `50`, `false` | Completions, their delay in milliseconds, Tab-only accepting |
| `show_definitions` | `true` | Show definitions when the pointer rests on a name |
| `spell_check`, `added_words` | `false`, `[]` | Spell check and words it accepts |
| `mini_diff` | `"auto"` | Change markers against Git, else the saved file (`"saved"`, `"off"`) |
| `show_tab_bar`, `show_status_bar`, `show_sidebar`, `show_folders` | `true` | Window parts |
| `tab_size`, `translate_tabs_to_spaces` | unset | Indentation width (1–8), spaces or tabs |
| `trim_trailing_white_space_on_save`, `ensure_newline_at_eof_on_save` | unset | Clean up when saving |
| `line_ending`, `encoding` | unset | Convert line endings (`"lf"`, `"crlf"`) or encoding when saving |
| `syntax_specific` | `{}` | Formatting per syntax, such as `{"python": {"tab_size": 4}}` |
| `folder_exclude_patterns`, `file_exclude_patterns` | see Settings | Names or globs hidden from the tree and searches |
| `find_case_sensitive`, `find_regex`, `find_whole_word` | `true`, `false`, `false` | Find options |
| `color_theme` | `"Hedit Ink"` | The theme |
| `follow_system_appearance`, `dark_color_theme`, `light_color_theme` | `false`, `"Hedit Ink"`, `"Hedit Paper"` | Switch themes with the system's light or dark mode |
| `build_system`, `build_systems` | `"automatic"`, `[]` | Build system for Ctrl+B, and your own |
| `cuda_gdb_path` | `"cuda-gdb"` | CUDA-GDB program (Linux) |
| `terminal_enabled` | `false` | Allow opening a local integrated terminal |
| `terminal_bidi` | `true` | Display bidirectional terminal text and shape Arabic; keeps PTY input and copied Unicode unchanged |
| `terminal_onboarding_complete` | `false` on new installs | Remember the first-launch terminal choice; existing settings files skip onboarding |

Unset formatting keys leave each file's style alone. Project settings and
`.editorconfig` rules take precedence.

## Appearance

**Preferences → Color theme…** previews themes as you move through the list;
**Enter** keeps one and **Esc** goes back. **Settings → Appearance** shows a gallery
of every theme and can follow the system's light and dark modes with a theme for
each.

| Dark | Light |
| --- | --- |
| Hedit Ink (default) | Hedit Paper |
| One Dark | One Light |
| Dracula | GitHub Light |
| Monokai | Solarized Light |
| Nord | Gruvbox Light |
| Gruvbox Dark | Catppuccin Latte |
| Solarized Dark | Rosé Pine Dawn |
| Tokyo Night | Ayu Light |
| Catppuccin Mocha | Light Owl |
| GitHub Dark | Quiet Light |
| Rosé Pine | Everforest Light |
| Ayu Dark | Flexoki Light |
| Night Owl | Tomorrow |
| Cobalt2 | Horizon Bright |
| Everforest Dark | Iceberg Light |
| Kanagawa Wave | PaperColor Light |
| Tomorrow Night | Alabaster |
| Horizon | — |
| Iceberg Dark | — |
| PaperColor Dark | — |
| SynthWave ’84 | — |

The [additional palette sources](assets/licenses/theme-palettes.md) list upstream projects
and their licenses.

**Customize colors** changes any interface, syntax, or status color with a live
preview and shows text contrast. **Save as new theme…** keeps your version under
**Your themes**. **Import…** accepts hedit theme files and TextMate
`.tmTheme` color schemes; **Export…** saves the current colors.

### Theme files

Themes are JSON files in the `themes` folder beside
[settings.json](#the-settings-file) (or `HEDIT_THEMES_DIR`). A theme can start
from a built-in one and change a few colors:

```json
{
  "version": 2,
  "name": "Violet Ink",
  "base": "Hedit Ink",
  "accent": "#B4A0F5",
  "selection": "#413657"
}
```

Colors are `background`, `surface`, `panel`, `elevated`, `border`, `text`, `muted`,
`accent`, `selection`, `selection_text`, `warning`, `error`, `success`, `comment`,
`keyword`, `string`, `number`, `cuda`, `function`, `type`, `constant`, `operator`,
`property`, and `preprocessor`, each as `#RRGGBB`.

## CUDA tools

**Tools → CUDA panel** runs NVIDIA's `cuda-gdb`, `compute-sanitizer`, and
`nvidia-smi` (Linux only; set the debugger path under **Settings → CUDA Tools**).
CUDA editing and highlighting work everywhere.

To debug, build with debug information, for example
`nvcc -g -G examples/cuda/vector_add.cu -o /tmp/vector-add`, then:

1. Choose the **Executable** and its **Args** (a JSON list such as `["--size", "1024"]`).
2. Click **Start CUDA-GDB**; it stops at `main`. Add breakpoints by function or
   `file.cu:42`.
3. Use **Run**, **Continue**, **Pause**, **Step**, **Next**, and **Finish**, and
   inspect **Stack**, **Locals**, **Devices**, **Kernels**, **Threads**, and **Warps**.
   The console takes commands such as `print i` or `cuda thread (0,0,0)`.

**Run sanitizer** checks the program with memcheck (memory errors), racecheck
(shared-memory races), initcheck (uninitialized reads), or synccheck
(synchronization). **GPU info** shows the GPU, driver, memory, and temperature.
See NVIDIA's [CUDA-GDB](https://docs.nvidia.com/cuda/cuda-gdb/index.html) and
[Compute Sanitizer](https://docs.nvidia.com/compute-sanitizer/ComputeSanitizer/index.html)
guides for details.

## Command line

```text
hedit [--session-dir DIR] [--new] [-w] [FILE[:LINE[:COLUMN]] | FOLDER ...] [-]
```

| Argument | Effect |
| --- | --- |
| `FILE`, `FOLDER` | Open a file (created on save if new) or add a folder |
| `FILE:LINE[:COLUMN]` | Open at a location |
| `-` | Read standard input into an unsaved file |
| `--new` | Add an empty unsaved file |
| `-w`, `--wait` | Wait until the files are closed, for use as `git config --global core.editor "hedit -w"` |
| `--session-dir DIR` | Use a separate session and window (default from `HEDIT_SESSION_DIR`) |
| `--` | Treat the rest as file names |
| `-h`, `--version` | Help and version |

hedit returns to the prompt as soon as its window is up. Terminal launches restore
the window and request focus. On Wayland, hedit passes the terminal's activation
token to the window, including an already running session. If the terminal does
not provide a token, the desktop decides whether to focus the window or show an
attention notification.

If it is already running, files go to the open window.

## Keyboard shortcuts

Change any shortcut under **Settings → Key Bindings**. Two-key shortcuts such as
**Ctrl+K, Ctrl+D** mean: press the first, then the second.

| Action | Linux / Windows | macOS |
| --- | --- | --- |
| New file / open file / new window | Ctrl+N / Ctrl+O / Ctrl+Shift+N | Cmd+N / Cmd+O / Cmd+Shift+N |
| Save / save as | Ctrl+S / Ctrl+Shift+S | Cmd+S / Cmd+Shift+S |
| Close tab / reopen closed tab / close all tabs | Ctrl+W / Ctrl+Shift+T / Ctrl+K, Ctrl+W | Cmd+W / Cmd+Shift+T / Cmd+K, Cmd+W |
| Recent tab / next and previous tab | Ctrl+Tab / Ctrl+PageDown, Ctrl+PageUp | Control+Tab / Control+PageDown, Control+PageUp |
| Tabs 1–9 | Alt+1–9 | Cmd+1–9 |
| Add folder / sidebar / folder section | Ctrl+Shift+O / Ctrl+K, Ctrl+B / Ctrl+Alt+B | Cmd+Shift+O / Cmd+K, Cmd+B / Cmd+Option+B |
| Go to file / commands / line / symbol | Ctrl+P / Ctrl+Shift+P / Ctrl+G / Ctrl+R | Cmd+P / Cmd+Shift+P / Cmd+G / Cmd+R |
| Find / find in folders | Ctrl+F / Ctrl+Shift+F | Cmd+F / Cmd+Shift+F |
| Replace | Ctrl+H | Cmd+Option+F |
| Select all matches / use selection for Find / quick find | Alt+Enter / Ctrl+E / Ctrl+F3 | Option+Enter / Cmd+E / Cmd+F3 |
| Go to definition / references | F12 / Shift+F12 | F12 / Shift+F12 |
| Switch header/implementation | Alt+O | Option+O |
| Open file under cursor | Alt+D | Option+D |
| Back / forward | Alt+Left / Alt+Right | Control+- / Control+Shift+- |
| Next occurrence / split into lines | Ctrl+D / Ctrl+Shift+L | Cmd+D / Cmd+Shift+L |
| Select all occurrences | Alt+F3 | Control+Cmd+G |
| Add cursor above / below | Alt+Shift+Up / Down | Control+Shift+Up / Down |
| Expand selection / to brackets | Ctrl+Shift+Space / Ctrl+Shift+M | Cmd+Shift+Space / Cmd+Shift+M |
| Expand selection to HTML/XML tag | Ctrl+Shift+A | Cmd+Shift+A |
| Jump to bracket | Ctrl+M | Control+M |
| Copy / cut / paste | Ctrl+C / Ctrl+X / Ctrl+V | Cmd+C / Cmd+X / Cmd+V |
| Paste and indent / paste from history | Ctrl+Shift+V / Ctrl+K, Ctrl+V | Cmd+Shift+V / Cmd+K, Cmd+V |
| Undo / redo / undo selection change | Ctrl+Z / Ctrl+Shift+Z / Ctrl+U | Cmd+Z / Cmd+Shift+Z / Cmd+U |
| Line comment / block comment | Ctrl+/ / Ctrl+Shift+/ | Cmd+/ / Cmd+Option+/ |
| Wrap paragraph | Alt+Q | Cmd+Option+Q |
| Transpose | Ctrl+T | Control+T |
| Delete to the start of the line | Ctrl+Shift+Backspace | Cmd+Backspace |
| Complete word | Ctrl+Space | Control+Space |
| Record / play macro | Ctrl+Alt+Q / Ctrl+Alt+Shift+Q | Control+Q / Control+Shift+Q |
| Spell check / next misspelling | F6 / Ctrl+F6 | F6 / Cmd+F6 |
| Next change / revert change | Ctrl+K, Ctrl+. / Ctrl+K, Ctrl+Z | Cmd+K, Cmd+. / Cmd+K, Cmd+Z |
| Source Control | Ctrl+Shift+G | Cmd+Shift+G |
| Toggle integrated terminal (once enabled) | Ctrl+` | Control+` |
| Bookmark / next bookmark / select all bookmarks | Ctrl+F2 / F2 / Alt+F2 | Cmd+F2 / F2 / Option+F2 |
| Build / build with / next error or match | Ctrl+B / Ctrl+Shift+B / F4 | Cmd+B / Cmd+Shift+B / F4 |
| Column layouts and grid | Alt+Shift+1–5 | Cmd+Option+1–5 |
| Two / three rows | Alt+Shift+8 / 9 | Cmd+Option+Shift+2 / 3 |
| Focus a group | Ctrl+1–4 | Control+1–4 |
| Fold / unfold | Ctrl+Shift+[ / Ctrl+Shift+] | Cmd+Shift+[ / Cmd+Shift+] |
| Fold level 1 / 2 / 3 | Ctrl+K, Ctrl+1 / 2 / 3 | Cmd+K, Cmd+1 / 2 / 3 |
| Unfold all | Ctrl+K, Ctrl+J | Cmd+K, Cmd+J |
| Full screen / distraction-free | F11 / Shift+F11 | Control+Cmd+F / Control+Shift+Cmd+F |
| Wrap long lines | Ctrl+Shift+W | Cmd+Shift+W |
| Bigger / smaller / reset font | Ctrl++ / Ctrl+- / Ctrl+0 | Cmd++ / Cmd+- / Cmd+0 |
| Scroll a line | Ctrl+Up / Down | Control+Option+Up / Down |
| Offline user guide | F1 | F1 |
| Settings / quit | Ctrl+, / Ctrl+Q | Cmd+, / Cmd+Q |
| Close the newest window or panel, then extra cursors | Esc | Esc |

With the terminal focused, some keys have different actions:

| Terminal action | Linux / Windows | macOS |
| --- | --- | --- |
| New / close terminal tab | Ctrl+Shift+T / Ctrl+Shift+W | Cmd+Shift+T / Cmd+Shift+W |
| Previous / next terminal tab | Ctrl+PageUp / Ctrl+PageDown | Control+PageUp / Control+PageDown |
| Copy selection / paste | Ctrl+Shift+C / Ctrl+Shift+V | Cmd+C / Cmd+V |
| Interrupt the running command | Ctrl+C | Control+C |
| Send Escape to the shell | Esc | Esc |

## Recovery

hedit saves recovery copies of your work every few seconds, separately from your
files, so unsaved text survives a restart or crash. Hover over the small dot at
the right of the status bar to read **Recovery pending**, **Checkpointing…**, or
**Recovery up to date**; click it for Recovery settings. A failed checkpoint
shows an error banner with **Retry checkpoint**. Keep hedit open and fix the
cause, such as a full disk. Closing waits for a final copy. Edits made in the
last few seconds before a power loss can be lost, so save important work.

If work seems missing, check the open tabs, **Ctrl+P**, and **File → Browse closed
files…**, and make sure you opened the same session (`--session-dir`). If the
newest recovery data cannot be read, hedit uses the one before it and keeps the
old data in `preserved-*` folders, which are never deleted automatically.

On Linux the session is in `~/.local/share/hedit/session/` (or
`$XDG_DATA_HOME/hedit/session`). On macOS it is in
`~/Library/Application Support/hedit.hedit/session/`; on Windows it is in
`%LOCALAPPDATA%\hedit\hedit\data\session\`.
To back it up or rescue text by hand, close hedit and copy the whole folder first;
small files are plain UTF-8 text in `buffers-*`, named in `session.json`. Recovery
data is not encrypted.

## Troubleshooting

| Problem | What to do |
| --- | --- |
| No window opens | Check graphics-driver support and read the startup error in the terminal. On Linux, use an X11 or Wayland desktop. Later errors are in `hedit.log` in the session folder. |
| The session is locked | Switch to the running window, or use another `--session-dir`. |
| Recovery fails | Check disk space and permissions, keep hedit open, and click **Retry checkpoint**. |
| Save reports an outside change | Use **Save as…** to keep your version, or compare the changes. |
| Open/Save dialogs do not appear on Linux | Install and run `xdg-desktop-portal` and your desktop's portal. |
| Text shows as boxes | Install fonts for that script, such as `fonts-noto`. |
| A file is rejected | It is binary or in an unsupported encoding; try **File → File encoding…**. |
| An update fails | Read the dialog error and check your connection. Rebuild executables run from a source checkout, or reinstall with the platform's installer or package. |
| Terminal commands are disabled | Enable **Settings → Terminal → Enable integrated terminal**. If the page says support is unavailable, use a build with the terminal feature. |
| A terminal shell exits or will not start | Read the error or hover its tab for status. Check that its starting directory still exists, then use **Restart shell** in the tab menu. |
| A terminal application rearranges Arabic or Hebrew incorrectly | Turn off **Settings → Terminal → Bidirectional text and Arabic shaping** if the application already arranges the text itself. |
| A CUDA tool will not start | Check that it is installed and on `PATH`, and read its output. |
