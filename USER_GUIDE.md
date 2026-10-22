# hedit user guide

Keep unsaved files between sessions, inspect large logs, and work with CUDA source.
This guide covers the upcoming hedit 0.1.3 on Linux and its initial universal macOS build.
Shortcuts use Linux labels below; on macOS, substitute **Cmd** for **Ctrl** and
**Option** for **Alt**. The menus display the platform's labels.

- [Installation](#installation)
- [Updates](#updates)
- [Files and unsaved changes](#files-and-unsaved-changes)
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

On Linux, you need a graphical session with X11 or Wayland and OpenGL. A GPU and CUDA
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
sh hedit-install.sh --version v0.1.3 --prefix "$HOME/.local"
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
gh release download v0.1.3 --repo MohamedElashri/hedit --pattern install.sh --output hedit-install.sh
sh hedit-install.sh --github-cli --version v0.1.3
```

Use a fresh filename or remove the previous downloaded script before downloading
again. Public `curl` URLs cannot access a private repository.

### Install a portable package

Download the matching archive and `.sha256` file from
[GitHub Releases](https://github.com/MohamedElashri/hedit/releases), verify its
checksum, extract it, and run the included installer. For the x86-64 package:

```sh
sha256sum -c hedit-0.1.3-linux-x86_64.tar.gz.sha256
tar -xzf hedit-0.1.3-linux-x86_64.tar.gz
cd hedit-0.1.3-linux-x86_64
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

### macOS installation

The macOS package targets macOS 11 or newer and contains both Apple Silicon and
Intel code in one app, starting with v0.1.3. Once released, download `hedit-0.1.3-macos-universal.zip` and its `.sha256`
file from [GitHub Releases](https://github.com/MohamedElashri/hedit/releases).
In the download directory, verify and extract it:

```sh
shasum -a 256 -c hedit-0.1.3-macos-universal.zip.sha256
ditto -x -k hedit-0.1.3-macos-universal.zip .
```

Move **hedit.app** into Applications and open it. For command-line use, invoke
`/Applications/hedit.app/Contents/MacOS/hedit` with the same arguments as on Linux.
The shell installer and in-app updater support Linux; upgrade the Mac app by
quitting hedit and replacing the bundle with the newly downloaded version.

Initial bundles have an ad-hoc development signature, with no Developer ID or
notarization ticket. Gatekeeper may block a downloaded app; use macOS's explicit
Open Anyway action in Privacy & Security only for a download you trust. Native
GUI validation is pending; see the [macOS build notes](DEVELOPER_GUIDE.md#macos-builds)
for the current checks and remaining platform work.

Recovery data lives in `~/Library/Application Support/hedit.hedit/session/`.
Removing or replacing the app does not remove that directory.

### First session

Launch hedit from your terminal or application launcher. A first session opens
an empty unsaved file; later sessions restore your workspace.

1. Press **Ctrl+N** to create a file and type or paste some text.
2. Optionally choose **File → Name unsaved file…** to give it a recognizable name.
3. Check the recovery status before ending your session.
4. Open **Preferences → Settings…** to customize the editor, or **File → Open file…** to read a file.

### Uninstall

For the default installation, remove `~/.local/bin/hedit`,
`~/.local/share/applications/hedit.desktop`,
`~/.local/share/icons/hicolor/scalable/apps/hedit.svg`,
`~/.local/share/hedit/themes/`, and `~/.local/share/doc/hedit/`.

Session data normally lives in `~/.local/share/hedit/session/`. Keep it if you may
need your unsaved files again; see [Recovery](#recovery) before removing it.

## Updates

On Linux, in-app updates are available from version **0.1.1**. Version 0.1.0 needs one
normal installer upgrade to get this feature. Close 0.1.0, run the release
installer again to install 0.1.3, and reopen hedit.

### Check and install manually

1. Choose **Help → Check for updates…**, or search for **Check for updates…** in
   the command palette. This requests the latest stable release from GitHub.
2. Review the available version, release notes, and installation location.
3. Click **Install update** to download, verify, and install that release.
4. When installation finishes, quit and reopen hedit when you are ready. The
   **Quit hedit** button uses the normal recovery process before closing.

There are **no background update checks, automatic installations, or automatic
restarts**. Opening the update action checks for a release; checking alone never
installs it. Prereleases and downgrades are not offered.

You can close the dialog and continue editing during installation. The running
editor keeps its current version until you restart. If you quit while an
installation is in progress, closing waits for it to finish. Your session data,
unsaved files, saved appearance, and additional theme files are kept.

### Requirements and private releases

Updates support Linux x86-64 and ARM64. Public downloads need `curl` and **GNU
tar**; private releases need GNU tar and an authenticated **GitHub CLI**. For a
private release, run `gh auth login --hostname github.com`, enable **Use GitHub
CLI authentication**, and click **Check again**. Credentials stay with GitHub CLI.

Installation must be writable and recognizable as a release package: either an
extracted portable folder or a prefix installed by the included installer or
`make install`. Source builds, unrecognized layouts, system-owned locations you
cannot write, and installation paths containing resource symlinks need a manual
upgrade. hedit does not request administrator privileges. You can still check
versions and open the release page from those installations.

The updater verifies the archive's SHA-256 checksum and checks that the new
executable runs before changing the installation. Download, checksum, and
compatibility failures leave the old installation in place. Installation failures
attempt to restore the previous files. If rollback cannot finish, the error gives
the location of retained recovery files; keep them until the installation is
repaired. Use the [release installer](#install-from-github-releases) for a manual
upgrade, closing hedit first.

## Files and unsaved changes

### Keep text without choosing a filename

Use **File → New file**, the **+** beside the tabs, or **Ctrl+N** to create a file.
hedit keeps recovery copies of your text so you can return after restarting,
even before choosing where to save it on disk.

Choose **File → Name unsaved file…** (**Ctrl+Shift+R**) to name it. Names can have
up to 80 characters and stay fixed as you edit. Apply an empty name to return to
using the first nonempty line as the tab title. This changes the tab name;
**Save** chooses a path and writes the file to disk.

### Open, save, and switch

Use **File → Open file…** (**Ctrl+O**), drag a file into the window, or pass its path in the
terminal. **Save** (**Ctrl+S**) writes the current file to disk, asking for a path
if it has never been saved. **File → Save as…** (**Ctrl+Shift+S**) chooses another
path. After saving, the tab uses the filename from disk.

Click a tab or an Open Files row to switch files. **Ctrl+P** finds open files
and files in all open folders. A dot on a tab indicates unsaved changes.

### Open a folder of code

Choose **File → Open folder…** (**Ctrl+Shift+O**), drag a folder into the window,
or run `hedit .` or `hedit /path/to/client /path/to/server`. You can select
several folders in the dialog or add more later with the **+** beside FOLDERS.
Opening another folder adds a root to the workspace and keeps your open tabs
and unsaved text. Reopening an existing root refreshes it without adding a duplicate.

File tabs sit above the editor region. The sidebar starts below the menu bar
and shows Open Files above the folder trees. Amber folder icons and
outlined document icons distinguish directories from files. Click a chevron or folder name
to expand or collapse it; click a file to open it in a tab. Directories load as
you expand them, with folders listed first and hidden files included. Opening
an already open file keeps its selection and unsaved edits. A file on the closed
shelf is reopened with its recovery text.

Right-click a folder for **New file…**, **New folder…**, **Copy path**, and
**Refresh folder**. Creation refuses to overwrite an existing entry. **Save as…**
starts in the folder containing the current file, or the first root for an unsaved
file. After changes made outside hedit, use **File → Refresh folders** or the **↻**
button to reload the tree and clear cached quick-open results.

**Ctrl+P** searches paths across all roots, including unopened files, even when
the folder sidebar is hidden. With multiple roots, result paths include the root
name; roots with the same name use full paths to distinguish them.
Letters can be nonconsecutive: `srcprj` can find `src/project.rs`. Open files
appear first; folder results show paths so matching filenames are distinguishable.
Use **Up/Down** and **Enter**, or click a result. Search skips `.git`, `.hg`,
`.svn`, `node_modules`, `target`, `.venv`, and `__pycache__`, and does not follow
directory symlinks. These directories remain available in the tree.

Folder paths, expanded directories, collapsed roots, and folder-sidebar visibility
survive restart. If a folder is moved or unavailable, its tree shows an error
while recovered buffers and the other roots remain usable. Right-click a root
and choose **Remove folder from workspace** to remove just that root, or choose
**File → Close all folders** to remove all trees. Both actions keep tabs open
and leave files on disk untouched.

Click **×** beside FOLDERS or use **View → Side bar → Show folder sidebar** (**Ctrl+Alt+B**)
to hide or reveal the folder section independently of Open Files. **View → Side bar →
Show sidebar** (**Ctrl+B**) toggles the entire sidebar. Both options are also in
**Preferences → Settings… → Editor**.

Directory listings show at most 2,000 entries per directory. Quick open returns
up to 80 folder matches, scanning at most 50,000 entries or about one second.
The UI reports incomplete scans and unreadable folders. These limits keep
large source trees manageable; they do not limit the files you can open directly.

### Close and reopen files

Close a tab with **×** or **Ctrl+W** to put it on the shelf, including its text,
name, selection, and bookmarks. Closing the window keeps open tabs in the session
rather than moving them onto the shelf.

- **Ctrl+Shift+T** or **File → Reopen closed tab** restores the latest closed file.
- **File → Browse closed files…**, also available from **Closed Files** in
  the sidebar, lets you filter the shelf by name and reopen any entry.

The shelf has 50 slots. A clean, saved entry may be removed to make room; unsaved
files and modified files are never silently evicted. If all slots hold
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

Enable **View → Follow log (read only)** on a clean, saved file to show its tail
and check for new output about once per second. Disable Follow before editing.
The menu item is unavailable for unsaved files and modified files.

When a log rotates, Follow reads the file currently at the original path. If that
path disappears or the replacement is not UTF-8 text, an error is shown and the
visible file remains.

Outside Follow mode, hedit flags external changes when the file becomes active.
A clean file offers **Reload file**. For a modified file, use **Save file
as…** to keep your version before reopening the disk version.

### Bookmark a position

Press **Ctrl+F2** to toggle a bookmark at the cursor. **F2** goes to the next one;
**Shift+F2** goes to the previous one, wrapping within the current file.
**Goto → Bookmarks…** lists saved positions
by text label. Navigate to a bookmark and press **Ctrl+F2** again to remove it.

Each file can hold 1,000 bookmarks. They survive restarts and moves to the
shelf. Labels are captured when added. Bookmarks store byte positions and adjust
for ordinary edits; undo/redo or an external replacement can change which text
sits at a saved position.

## Editing and navigation

Use one or more selections for clipboard operations and indentation. **Ctrl+Z**
undoes; **Ctrl+Shift+Z** or **Ctrl+Y** redoes. Up to 256 edit steps per file are
kept during the current process. Undo history is not restored after restart.

Toggle wrap through **View → Editor → Word wrap** (**Ctrl+Shift+W**). Long lines scroll
horizontally when wrap is off. **Ctrl++** and **Ctrl+-** change font size. Wrap,
font size, line numbers, tabs, status bar, current-line highlighting, whitespace
markers, and sidebar visibility are kept between launches.
Use **Preferences → Settings… → Editor** to manage them together.

### Edit commands

**Edit** groups commands under **Selection**, **Line**, **Text**, and **Indentation**.
All text changes participate in the current file's undo history and recovery.

| Shortcut | Action |
| --- | --- |
| Ctrl+L | Select the current line or extend the selection to whole lines |
| Ctrl+Shift+D | Duplicate the selection, or the current line with no selection |
| Ctrl+Shift+K | Delete selected lines or the current line |
| Ctrl+Enter / Ctrl+Shift+Enter | Insert an indented line after / before the cursor |
| Ctrl+Shift+Up / Ctrl+Shift+Down | Move selected lines or the current line |
| Ctrl+J | Join selected lines, or the current and next line |
| Ctrl+] / Ctrl+[ | Indent / unindent lines |
| Tab / Shift+Tab | Insert indentation or indent selected lines / unindent |
| Ctrl+/ | Toggle line comments for languages with a supported line-comment prefix |

**Line → Sort lines**, **Reverse lines**, and **Remove duplicate lines** affect
selected lines, or the whole file without a selection. Duplicate removal keeps
the first occurrence of each line and preserves their order.
**Text → Uppercase/Lowercase** transforms the selection.
**Trim trailing whitespace** affects selected lines or the whole file.

Under **Indentation**, choose spaces or tabs and a 2/4/8-space indentation step
for the current file. These choices survive restart. Conversions affect leading
indentation on selected lines, or the whole file with no selection; tabs within
code or strings stay intact. The indentation step controls inserted spaces and
conversions and the rendered tab stops without replacing tab characters in the file.
Opening a source file detects its indentation from up to 10,000 lines; choose
**Detect indentation from file** to run detection again. **Automatic indentation**
copies leading whitespace on Enter. **Smart indentation** adds a level after an
opening bracket and places a matching closing bracket on its own line. Turn
these options off independently in the submenu or Editor preferences.
**Backspace to indentation stops** removes spaces back to the previous stop.
Bulk text commands are unavailable in paged files and Follow mode.

### Multiple cursors and rectangular selections

Press **Ctrl+D** to select the word at the cursor, then repeat to add the next
occurrence. **Ctrl+Shift+L** selects all occurrences of the selected text or word.
Use **Alt+click** to add a cursor. Drag with **Alt+Shift** to select a rectangle,
or make a normal selection and choose **Edit → Selection → Rectangular selection**.
Short lines clamp to their ends. **Esc** or **Single selection** keeps just the
primary cursor.

Typing, pasting, deletion, indentation, line commands, and arrow-key navigation
apply across the selections. Copy joins selected ranges with newlines; paste
inserts the clipboard text at every cursor. Each batch is one undo step, with
cursor positions restored by undo/redo during the current process. Additional
selections survive recovery and reopening closed files. There is a limit of
1,000 selections per view; these controls require a fully loaded file.

### Split panes, folding, and minimap

**View → Layout** offers a single pane, two columns, or two rows. A split initially
shows the current file in both groups. Use a group's file dropdown to show
another open file, click an editor to focus it, or use **Ctrl+Alt+Right** to switch
groups. **Move file to other group** (**Ctrl+Alt+Shift+Right**) displays the current
file there and focuses that group. If the other group showed a different file,
the two displayed files exchange groups. Views keep independent cursor, selection,
fold, and scroll positions; edits and undo share one document. Layout and view
positions save with window preferences. Paged files share the currently loaded
page across views.

**View → Code folding** folds the current indentation block or selection, folds
all indentation blocks, and unfolds one or all regions. **Ctrl+Shift+[** folds;
**Ctrl+Shift+]** unfolds. Click a folded marker to expand it. Folded text remains
in the file, in copy operations, and in recovery. Editing clears fold ranges so
stale ranges cannot hide unrelated text. A search or cursor jump into a fold
reveals it. Automatic folds use indentation rather than a language parser;
select a region to fold code without indentation. Folding requires a fully
loaded file.

Toggle **Show indentation guides** and **Show minimap** under **View → Editor**
or in preferences. Click or drag in the minimap to scroll. Its drawing is sampled
to the available height; for paged files it shows the current page and is labeled
**PAGE**, rather than reading the whole large file.

### View controls

**View → Editor** toggles tabs, status bar, line numbers, current-line highlighting,
whitespace markers, and wrap. Whitespace markers show spaces as dots and tabs as
arrows without changing the file. **View → Font size** includes zoom and reset.

**View → Ruler** shows a vertical guide at column 80, 100, or 120, or hides it.
**View → Word wrap column** chooses 80, 100, 120, or the window width and enables
wrap. A narrow window wraps earlier to fit. Turning **Word wrap** off keeps your
chosen column for the next time you enable it. Ruler and wrap-column choices
survive restart and are also available in Editor preferences.

**F11** toggles full screen. **Shift+F11** toggles distraction-free mode, temporarily
hiding the sidebar, tabs, and status bar, entering full screen, and centering
text within 100 columns. Toggle it again to restore your chosen layout and
previous full-screen state. The menu remains available in both modes.

UTF-8 text is supported, including an existing BOM and LF/CRLF bytes. UTF-16 and
binary files are rejected. **Edit → Line endings** converts the entire loaded
file to **LF (Unix)** or **CRLF (Windows)**, regardless of the current selection.
The conversion is undoable, preserves an existing BOM, and updates the status
bar and recovery metadata. Enter follows the file's selected LF/CRLF policy;
pasted text may still contain mixed endings. Conversion is unavailable in
paged files and Follow mode.

### Syntax highlighting

hedit selects syntax from the filename when you open or save a file. Click the
language name at the bottom right, such as **Plain text ▾** or **Python ▾**, to
choose a different syntax for the current file. The larger menu has a search
field: type a language name, extension such as `.hpp`, or filename such as
`Makefile`. Press **Enter** when the filter leaves one result, or click a row.
This also works for unsaved files: paste a snippet, then choose its language without saving it.

Choose **Automatic (…)** to return to filename detection, or **Plain text** to
turn off highlighting for that file. A manual choice stays with the file
through Save As, page navigation, recovery, and reopening from the closed-file
shelf. It does not change the text or the choices in other tabs. Unsaved file
titles do not determine syntax; new files start as plain text.

| Syntax | Automatically recognized filenames |
| --- | --- |
| Assembly | `.asm`, `.s`, `.inc` |
| Batch | `.bat`, `.cmd` |
| C | `.c`, `.h`, `.i` |
| C++ | `.cc`, `.cpp`, `.cxx`, `.c++`, `.hpp`, `.hxx`, `.hh`, `.h++`, `.inl`, `.ipp`, `.tpp`, `.ii` |
| C# | `.cs`, `.csx` |
| CMake | `.cmake`, `CMakeLists.txt`, `CMakeCache.txt` |
| CSS | `.css` |
| CSV / TSV | `.csv`, `.tsv` |
| CUDA | `.cu`, `.cuh` |
| Dart | `.dart` |
| Diff / Patch | `.diff`, `.patch` |
| Dockerfile | `.dockerfile`, `.containerfile`, `Dockerfile`, `Containerfile` |
| Elixir | `.ex`, `.exs` |
| Erlang | `.erl`, `.hrl` |
| Fortran | `.f`, `.for`, `.f77`, `.f90`, `.f95`, `.f03`, `.f08` |
| Go | `.go` |
| GraphQL | `.graphql`, `.gql` |
| Groovy | `.groovy`, `.gradle`, `Jenkinsfile` |
| Haskell | `.hs`, `.lhs` |
| HCL / Terraform | `.hcl`, `.tf`, `.tfvars` |
| HTML | `.html`, `.htm`, `.xhtml`, `.shtml` |
| INI / Config | `.ini`, `.cfg`, `.conf`, `.properties`, `.env`, `.editorconfig`, `.gitconfig`, `.gitignore`, `.gitattributes`, `.npmrc`, `.npmignore` |
| Java | `.java` |
| JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` |
| JSON | `.json`, `.jsonl`, `.ndjson`, `.ipynb`, `.geojson` |
| Julia | `.jl` |
| Kotlin | `.kt`, `.kts` |
| LaTeX | `.tex`, `.sty`, `.cls`, `.bib` |
| Less | `.less` |
| Log | `.log` |
| Lua | `.lua` |
| Makefile | `.mk`, `.mak`, `Makefile`, `GNUmakefile`, `BSDmakefile` |
| Markdown | `.md`, `.markdown`, `.mdown` |
| MATLAB / Octave | `.m` |
| Nix | `.nix` |
| Objective-C | `.mm` |
| Perl | `.pl`, `.pm`, `.t` |
| PHP | `.php`, `.phtml`, `.php3`, `.php4`, `.php5`, `.phps` |
| PowerShell | `.ps1`, `.psm1`, `.psd1` |
| Protocol Buffers | `.proto` |
| Python | `.py`, `.pyi`, `.pyw`, `.pyx`, `.pxd`, `SConstruct`, `SConscript` |
| R | `.r`, `.rmd`, `.rprofile`, `.Rprofile` |
| Ruby | `.rb`, `.rake`, `.gemspec`, `.ru`, `Gemfile`, `Rakefile`, `Guardfile`, `Vagrantfile`, `Brewfile`, `.irbrc` |
| Rust | `.rs` |
| Scala | `.scala`, `.sc`, `.sbt` |
| SCSS / Sass | `.scss`, `.sass` |
| Shell | `.sh`, `.bash`, `.zsh`, `.ksh`, `.fish`, `.bashrc`, `.bash_profile`, `.profile`, `.zshrc`, `.zprofile`, `.zshenv`, `.bash_aliases` |
| SQL | `.sql` |
| Swift | `.swift` |
| TOML | `.toml`, `Cargo.lock`, `uv.lock`, `poetry.lock`, `Pipfile` |
| TypeScript | `.ts`, `.tsx`, `.mts`, `.cts` |
| Verilog / SystemVerilog | `.v`, `.vh`, `.sv`, `.svh` |
| VHDL | `.vhd`, `.vhdl` |
| Visual Basic | `.vb`, `.vbs`, `.bas` |
| XML | `.xml`, `.xsd`, `.xsl`, `.xslt`, `.svg`, `.plist`, `.csproj`, `.fsproj`, `.vbproj`, `.props`, `.targets`, `.ui`, `.qrc`, `.rss`, `.atom` |
| YAML | `.yaml`, `.yml` |
| Zig | `.zig`, `.zon` |

C (`.c`, `.h`) and C++ have separate syntax modes. Uppercase `.C` and `.H` use
C++; other extensions and special filenames are matched case-insensitively.
`.M` uses Objective-C; `.m` defaults to MATLAB / Octave, and you can choose
Objective-C manually for those files. `Dockerfile.*`, `Containerfile.*`, and
`.env.*` variants are also recognized. Unrecognized filenames use plain text.

These 57 syntax modes cover common code, web, scientific, configuration, build,
and text files. Highlighting covers keywords, strings, numbers, and comments;
HTML/XML tags and attributes, CSS properties, CSV separators, and diff additions
and deletions have format-specific colors. Log mode colors common severity
words. Markdown highlights headings, quotes, and code.

These are basic syntax modes: embedded languages, JSX/TSX markup, language
validation, and complete language grammars are not supported. Multiline
highlighting can reset at page boundaries in large files. Customize colors in
**Preferences → Settings… → Appearance**. The menu scrolls to the current syntax
when opened and adapts its height to smaller windows.

### Find and replace

1. Press **Ctrl+F** or choose **Find → Find…** to open the bottom search panel.
2. Enter a literal, case-sensitive query of up to 4,096 UTF-8 bytes.
3. Click **Find next** to select the next match across the whole file.
4. Enable **Replace** (or press **Ctrl+H**), enter replacement text, and click
   **Replace selected** to change that occurrence.

Search wraps at the end. Long searches can be cancelled from the status bar;
**Esc** closes the panel. Regex, case-insensitive search, and replace-all controls
are not yet available.

### Go to a line or switch files

Press **Ctrl+G**, enter a positive, one-based line number, and click **Go**. This
locates a line in the entire document, even when only a page is displayed.

**Ctrl+P** opens Go to file, including all open folders; **Ctrl+Shift+P** opens commands. Type a
filter, use **Up/Down** to select a result, and press **Enter**, or click a result.
Type `>` in the file switcher to search commands instead. **Esc** closes either.
The menus show the same commands and keyboard shortcuts.

## Preferences

Choose **Preferences → Settings…** or press **Ctrl+,**. Settings are grouped in
one window:

| Page | Settings and actions |
| --- | --- |
| Editor | Wrap, line numbers, sidebar/folders, tabs, status bar, current line, whitespace, font size, restore defaults |
| Appearance | Presets, custom colors, theme import/export, contrast feedback |
| CUDA Tools (Linux) | CUDA-GDB path and a shortcut to the tool panel |
| Recovery | Checkpoint status, checkpoint now, closed files, recovery guidance |

View commands provide quick toggles for the same editor settings. The status bar
shows the cursor position, language, encoding, and recovery state; click the
language to select syntax or the recovery status to open its preferences page.
For paged files, gutter line numbers
and **Page Ln** refer to the displayed page. The sidebar lists open files and
provides a **Closed Files** link. Right-click a tab for its context menu, or
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

CUDA syntax highlighting and editing work on both platforms. Local CUDA-GDB,
Compute Sanitizer, and NVIDIA GPU information are disabled on macOS; their
menus and preferences are hidden there.

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
hedit [--session-dir DIR] [--new] [FILE[:LINE[:COLUMN]] | FOLDER ...] [-]
```

| Argument | Effect |
| --- | --- |
| `FILE` | Open a file or start a named file |
| `FILE:LINE[:COLUMN]` | Open at a one-based location |
| `-` | Read UTF-8 stdin until EOF into an unsaved file |
| `--new` | Add an empty unsaved file to the restored session |
| `--session-dir DIR` | Use an independent session |
| `--` | Treat remaining arguments as filenames |
| `-h`, `--help` | Show help |
| `-V`, `--version` | Show the version |

```sh
hedit .
hedit application.log
hedit kernel.cu:42:8
hedit "notes for today.txt"
printf 'keep this snippet\n' | hedit -
hedit --session-dir /tmp/hedit-investigation --new
hedit -- -notes.txt
```

hedit always returns the terminal prompt after the window starts. Closing
the terminal leaves the editor running. Help, version, and
startup errors still appear in the invoking terminal; background diagnostics
are appended to `hedit.log` in the session directory.

An existing literal filename takes precedence over location syntax. CLI columns
use UTF-16 units; out-of-range locations clamp to the document or line end. A
missing file opens only if its parent exists and is created on **Save**.

One process can use a session directory at a time. A second launch does not
forward files to the current window; open them there or use `--session-dir`.
`HEDIT_SESSION_DIR` changes the default directory; the CLI option takes precedence.

With `-`, hedit reads until EOF before releasing the terminal. Stdin is
checkpointed before window creation, so consumed text remains in the session if
the graphical window cannot start.

## Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| Ctrl+N / Ctrl+O | New file / open file |
| Ctrl+S / Ctrl+Shift+S | Save / Save As |
| Ctrl+W / Ctrl+Shift+T | Close to shelf / reopen latest |
| Ctrl+Shift+R | Name the active unsaved file |
| Ctrl+Shift+O | Add one or more folders |
| Ctrl+B / Ctrl+Alt+B | Toggle entire sidebar / folder section |
| Ctrl+P / Ctrl+Shift+P | Find files / command palette |
| Ctrl+F / Ctrl+H / Ctrl+G | Find / Replace / Go to line |
| Ctrl+Z / Ctrl+Shift+Z or Ctrl+Y | Undo / redo |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy / cut / paste |
| Ctrl+A | Select all in the focused text field |
| Ctrl+D / Ctrl+Shift+L | Select next / all occurrences |
| Alt+click / Alt+Shift+drag | Add a cursor / rectangular selection |
| Ctrl+Alt+Right | Focus the other editor group |
| Ctrl+Alt+Shift+Right | Show the current file in the other group |
| Ctrl+Shift+[ / Ctrl+Shift+] | Fold / unfold |
| F11 / Shift+F11 | Full screen / distraction-free mode |
| Ctrl+Shift+W | Toggle wrap |
| Ctrl++ or Ctrl+= / Ctrl+- | Increase / decrease editor font |
| Ctrl+, | Preferences |
| Ctrl+F2 | Toggle a bookmark at the cursor |
| F2 / Shift+F2 | Next / previous bookmark |
| Esc | Close search, the palette, or file-tool windows |

Undo in a search or debugger input edits that input; in the editor it uses the
file's history.

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

Check open tabs, **Ctrl+P**, and **File → Browse closed files…**. Verify you opened the same
session, especially if you used `--session-dir`. Read any startup recovery warning
before dismissing it; a fallback checkpoint can be older than your latest work.

If the latest manifest cannot be read, hedit tries the previous checkpoint and
retains recovery generations. If neither works, old data is archived under
`preserved-*` and a fresh session starts. Failed file restores retain their
recovery references for another attempt.

### Back up or rescue a session

The default is `$XDG_DATA_HOME/hedit/session`, normally
`~/.local/share/hedit/session`. Close hedit and copy the **whole session directory**
before inspecting or modifying it. Small files are UTF-8 files in `buffers-*`;
`session.json` and `session.previous.json` identify their tabs. Large files can
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
| Save reports an external change | Use **Save as…** to keep your version. A clean file can reload the file. |
| Open/Save dialog does not appear | Check that `xdg-desktop-portal` and your desktop's portal backend are installed and running in the same session. |
| File is rejected | Convert a separate copy to UTF-8 if needed. UTF-16 and binary files are unsupported. |
| Theme is rejected | Check the version, name length, hex values, and 64 KiB limit. Your current palette stays active. |
| Update check fails | Check the connection and access method; private releases need authenticated GitHub CLI. Read the error in the update dialog. |
| Update installation is unavailable | Use a writable release installation or upgrade manually; source builds and managed/symlink layouts cannot update in place. |
| CUDA tool cannot start | Install it, check `PATH` or the debugger path, and read the tool's output for driver/device/permission errors. |

hedit refuses to overwrite external changes. For a large source that changes
before a recovery copy completes, unseen bytes may be unavailable. If an edit
cannot be applied, the edited visible page is kept in a new file and an
error explains the result.
