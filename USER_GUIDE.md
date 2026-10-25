# hedit user guide

This guide covers hedit on Linux, macOS, and Windows. Shortcuts use Ctrl on Linux
and Windows; on macOS use **Cmd** for **Ctrl** and **Option** for **Alt** (the exceptions are
listed under [Keyboard shortcuts](#keyboard-shortcuts)). The menus always show
your platform's keys.

- [Installation](#installation)
- [Updates](#updates)
- [Files and folders](#files-and-folders)
- [Editing](#editing)
- [Finding and navigating](#finding-and-navigating)
- [Writing code](#writing-code)
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
download you trust. On macOS, upgrade by running the installer again. See the
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

Remove `~/.local/bin/hedit`, `~/.local/share/applications/hedit.desktop`,
`~/.local/share/icons/hicolor/scalable/apps/hedit.svg`, `~/.local/share/hedit/themes/`,
and `~/.local/share/doc/hedit/`. Your session (unsaved work) is in
`~/.local/share/hedit/session/` on Linux and
`~/Library/Application Support/hedit.hedit/session/` on macOS. On Windows, delete
the extracted app folder; recovery is in `%LOCALAPPDATA%\hedit\hedit\data\session\`.
See
[Recovery](#recovery) before deleting it.

## Updates

On Windows, [replace the portable package](#windows-installation). On macOS,
run the installer again or replace the app.

On Linux, **Help → Check for updates…** looks up the latest stable release,
shows its notes, and installs it when you click **Install update**. hedit never
checks, installs, or restarts on its own. Keep working while it installs; the new
version runs the next time you start hedit.

Updating needs `curl`, GNU `tar`, and a writable installation made by the
installer, a portable package, or `make install`. The download is checked against
its SHA-256 file and the new program is test-run before anything changes; if
installing fails, the old version is restored. For other setups, such as source
builds or system-owned locations, upgrade with the
[installer](#install-from-github-releases).

## Files and folders

### Unsaved files

**Ctrl+N** (or **+** beside the tabs) creates a file you can keep without saving
it: hedit keeps a recovery copy and restores it next time. Its tab is named after
its first line; **File → Name unsaved file…** (**Ctrl+Shift+R**) gives it a name of
your own. **Ctrl+S** saves to disk, asking for a path the first time.

### Opening and switching

Open files with **Ctrl+O**, by dragging them into the window, or from the terminal.
A dot on a tab marks unsaved changes.

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
**Closed Files**. Cancel leaves the tab open. Quitting or closing the window asks
the same question; untitled notes kept without saving are restored next time.
The sole empty untitled tab stays open as the editor's blank state and has no
close button. Add another tab or type text to make it closable.

Closed tabs go on the *shelf* with their selection and bookmarks. After Don't
Save on a file on disk, reopening shows the disk copy; untitled notes reopen with
their text intact. **Ctrl+Shift+T** reopens the last tab, and **File →
Browse closed files…** lists them all. **File → Close all tabs** (**Ctrl+K,
Ctrl+W**) and **Close unmodified tabs** close many at once. The shelf holds 50
files; only saved, unchanged ones are dropped to make room.

### Folders

**File → Open folder…** (**Ctrl+Shift+O**), dragging a folder in, or `hedit .` adds
a folder to the sidebar; you can have several. Right-click an entry to create,
rename, move, duplicate, trash, copy the path of, or reveal it; nothing is
overwritten without asking. **File → Refresh folders** picks up outside changes.
Folders such as `.git`, `node_modules`, and `target` are hidden from the tree and
searches; change the list under **Settings → Folders & Search**.
Open files appear in the tab bar; the sidebar contains folders and a shortcut
to closed-file recovery.

**Ctrl+K, Ctrl+B** shows or hides the sidebar and **Ctrl+Alt+B** just its folder
section. Folders, expanded directories, and what is shown come back next time.

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
commands without a selection act on the whole file.

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
**Ctrl+K, Ctrl+Z** reverts the change at the cursor. **Tools → Git changes…** shows
the whole diff. hedit never changes your repository.

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

hedit knows 57 formats, including C, C++, CUDA, C#, Go, Java, JavaScript and
TypeScript, Kotlin, Python, Rust, Swift, Zig, shell, PowerShell, SQL, HTML, CSS and
Sass, JSON, YAML, TOML, XML, Markdown, LaTeX, CMake, Makefiles, Dockerfiles,
Fortran, Julia, MATLAB, R, Verilog, VHDL, diffs, CSV, and logs. Most are highlighted
with full syntax definitions that understand embedded languages and multi-line
strings; plain text, logs, and CSV use a faster built-in highlighter. Large files
show quick colors first and full colors a moment later.

## Views and layout

- **Layouts:** **Alt+Shift+1–4** for one to four columns, **Alt+Shift+8/9** for two
  or three rows, and **Alt+Shift+5** for a grid of four. **Ctrl+1–4** focuses a
  group, and **Ctrl+Alt+Shift+Right** moves the file to the next group. Each group
  has its own cursor and scroll position on a shared file.
- **Folding:** **Ctrl+Shift+[** and **Ctrl+Shift+]** fold and unfold an indented
  block, and **Ctrl+K, Ctrl+1** / **Ctrl+K, Ctrl+J** fold or unfold everything. With
  the pointer over the gutter, arrows appear beside foldable lines; click one.
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
syntax, and recovery state; click any of them to change it.

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

**Preferences → Settings…** (**Ctrl+,**) has pages for General, Editor, Formatting,
Folders & Search, Appearance, Key Bindings, Snippets, CUDA Tools, and Recovery.
Changes save as you make them, and each page has **Restore defaults**.

**Settings → Key Bindings** lists every command: click a shortcut and press new
keys (two keys in a row make a two-key shortcut such as **Ctrl+K, Ctrl+G**). They
are saved in `keybindings.json` as `{"command": "find_files", "shortcut":
"Primary+Shift+F"}`, where `Primary` is Ctrl on Linux and Cmd on macOS.

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

Unset formatting keys leave each file's style alone. Project settings and
`.editorconfig` rules take precedence.

## Appearance

**Preferences → Color theme…** previews themes as you move through the list;
**Enter** keeps one and **Esc** goes back. **Settings → Appearance** shows a gallery
of every theme and can follow the system's light and dark modes with a theme for
each.

| Dark | Light |
| --- | --- |
| Hedit Ink (default), One Dark, Dracula, Monokai, Nord, Gruvbox Dark, Solarized Dark, Tokyo Night, Catppuccin Mocha, GitHub Dark, Rosé Pine | Hedit Paper, One Light, GitHub Light, Solarized Light, Gruvbox Light, Catppuccin Latte, Rosé Pine Dawn |

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

hedit returns to the prompt as soon as its window is up. If it is already running,
files go to the open window.

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
| Back / forward | Alt+Left / Alt+Right | Control+- / Control+Shift+- |
| Next occurrence / split into lines | Ctrl+D / Ctrl+Shift+L | Cmd+D / Cmd+Shift+L |
| Select all occurrences | Alt+F3 | Control+Cmd+G |
| Add cursor above / below | Alt+Shift+Up / Down | Control+Shift+Up / Down |
| Expand selection / to brackets | Ctrl+Shift+Space / Ctrl+Shift+M | Cmd+Shift+Space / Cmd+Shift+M |
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
| Bookmark / next bookmark / select all bookmarks | Ctrl+F2 / F2 / Alt+F2 | Cmd+F2 / F2 / Option+F2 |
| Build / build with / next error or match | Ctrl+B / Ctrl+Shift+B / F4 | Cmd+B / Cmd+Shift+B / F4 |
| Column layouts and grid | Alt+Shift+1–5 | Cmd+Option+1–5 |
| Two / three rows | Alt+Shift+8 / 9 | Cmd+Option+Shift+2 / 3 |
| Focus a group | Ctrl+1–4 | Control+1–4 |
| Fold / unfold | Ctrl+Shift+[ / Ctrl+Shift+] | Cmd+Shift+[ / Cmd+Shift+] |
| Full screen / distraction-free | F11 / Shift+F11 | Control+Cmd+F / Control+Shift+Cmd+F |
| Wrap long lines | Ctrl+Shift+W | Cmd+Shift+W |
| Bigger / smaller / reset font | Ctrl++ / Ctrl+- / Ctrl+0 | Cmd++ / Cmd+- / Cmd+0 |
| Scroll a line | Ctrl+Up / Down | Control+Option+Up / Down |
| Settings / quit | Ctrl+, / Ctrl+Q | Cmd+, / Cmd+Q |
| Close the newest window or panel, then extra cursors | Esc | Esc |

## Recovery

hedit saves recovery copies of your work every few seconds, separately from your
files, so unsaved text survives a restart or crash. The status bar shows
**Recovery pending**, **Checkpointing…**, **Recovery up to date**, or **Recovery
checkpoint failed** (keep hedit open and fix the cause, such as a full disk).
Closing waits for a final copy. Edits made in the last few seconds before a power
loss can be lost, so save important work.

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
| No window opens | Run in an X11 or Wayland session with OpenGL and read the error in the terminal. |
| The session is locked | Switch to the running window, or use another `--session-dir`. |
| Recovery fails | Check disk space and permissions, keep hedit open, and click **Retry checkpoint**. |
| Save reports an outside change | Use **Save as…** to keep your version, or compare the changes. |
| Open/Save dialogs do not appear | Install and run `xdg-desktop-portal` and your desktop's portal. |
| Text shows as boxes | Install fonts for that script, such as `fonts-noto`. |
| A file is rejected | It is binary or in an unsupported encoding; try **File → File encoding…**. |
| An update fails | Check your connection and read the error; source builds update by reinstalling. |
| A CUDA tool will not start | Check that it is installed and on `PATH`, and read its output. |
