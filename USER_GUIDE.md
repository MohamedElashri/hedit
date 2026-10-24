# hedit user guide

Keep unsaved files between sessions, inspect large logs, and work with CUDA source.
This guide covers hedit on Linux and its universal macOS build.
Shortcuts use Linux labels below; on macOS, substitute **Cmd** for **Ctrl** and
**Option** for **Alt**. The menus display the platform's labels.

- [Installation](#installation)
- [Updates](#updates)
- [Files and unsaved changes](#files-and-unsaved-changes)
- [Logs and bookmarks](#logs-and-bookmarks)
- [Editing and navigation](#editing-and-navigation)
- [Settings](#settings)
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

The installer script lives in the repository root and always downloads the
latest stable release. Install it with:

```sh
curl -fsSL https://raw.githubusercontent.com/MohamedElashri/hedit/main/install.sh | sh
```

On Linux, the installer selects x86-64 or ARM64, verifies the package's SHA-256 checksum,
and installs under `~/.local`. It needs `curl`, `tar`, and standard Linux utilities;
no compiler is required. On macOS, the same command installs the universal app; see
[macOS installation](#macos-installation). Release binaries are built on Ubuntu 24.04 and require
compatible system libraries. If they cannot run on your system, install from source.
CUDA tools are installed separately.

To inspect the script first, select a version, or choose another prefix:

```sh
curl -fsSL https://raw.githubusercontent.com/MohamedElashri/hedit/main/install.sh -o hedit-install.sh
sh hedit-install.sh --version vX.Y.Z --prefix "$HOME/.local"
```

Omit `--version` for the latest stable release. Prereleases require their full
tag, such as `--version vX.Y.Z-rc.1`. Run the installer again to upgrade. Close
hedit before upgrading; your saved files and session data are kept.

### Private repository access

Private releases require an account with repository access. With GitHub CLI
installed, sign in and download the installer through the authenticated API:

```sh
gh auth login --hostname github.com
gh api -H 'Accept: application/vnd.github.raw' repos/MohamedElashri/hedit/contents/install.sh > hedit-install.sh
sh hedit-install.sh --github-cli
```

Add `--version vX.Y.Z` to the second command for a pinned version.

Public `curl` URLs cannot access a private repository.

### Install a portable package

Download the matching archive and `.sha256` file from
[GitHub Releases](https://github.com/MohamedElashri/hedit/releases), verify its
checksum, extract it, and run the included installer. For the x86-64 package,
replacing `X.Y.Z` with the release version:

```sh
sha256sum -c hedit-X.Y.Z-linux-x86_64.tar.gz.sha256
tar -xzf hedit-X.Y.Z-linux-x86_64.tar.gz
cd hedit-X.Y.Z-linux-x86_64
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
Intel code in one app. Download `hedit-X.Y.Z-macos-universal.zip` and its `.sha256`
file from [GitHub Releases](https://github.com/MohamedElashri/hedit/releases).
In the download directory, verify and extract it:

```sh
shasum -a 256 -c hedit-X.Y.Z-macos-universal.zip.sha256
ditto -x -k hedit-X.Y.Z-macos-universal.zip .
```

Move **hedit.app** into Applications and open it. For command-line use, invoke
`/Applications/hedit.app/Contents/MacOS/hedit` with the same arguments as on Linux.

The shell installer from [Install from GitHub Releases](#install-from-github-releases)
also works on macOS. It verifies the checksum and the app signature, installs
**hedit.app** in `~/Applications`, and creates a `hedit` command in `~/.local/bin`.
Use `--app-dir /Applications` to install for all users, and `--prefix` to place
the command elsewhere. Quit hedit, then run the installer again to upgrade. The in-app
updater supports Linux only.

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

In-app updates are available on Linux. If your hedit has no **Check for updates…**
item, it predates the updater: close hedit, run the release installer again to
install the latest release, and reopen hedit.

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

From the keyboard, **Ctrl+Tab** returns to the file you used before. Keep Ctrl
held and press Tab again to go further back through recently used files;
**Ctrl+Shift+Tab** goes the other way. **Ctrl+PageDown** and **Ctrl+PageUp** move
to the next or previous tab in the tab strip, and **Alt+1** to **Alt+9** choose a
tab by its position. On macOS these are **Control+Tab**, **Control+PageDown**,
and **Cmd+1** to **Cmd+9**.

Drag a tab to reorder it. Its context menu offers **Pin tab**, **Close other tabs**,
and **Close tabs to the right**; group closing keeps pinned tabs. A single click
on a sidebar file opens an italic preview tab, replacing the previous clean,
unpinned preview. Double-click, pin, or edit the preview to keep it.

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

Right-click an entry to rename, move, duplicate, copy its absolute/relative path,
or reveal it in the system file manager. **Move to trash…** uses the system trash
and asks for confirmation; open text remains recoverable. Folders also offer
**New file…**, **New folder…**, and **Refresh folder**. Creation, moving, and
duplication refuse to overwrite existing entries. Moves require the same
filesystem; duplication does not follow symlinks and is limited to 50,000 entries
and 32 folder levels. **Save as…**
starts in the folder containing the current file, or the first root for an unsaved
file. After changes made outside hedit, use **File → Refresh folders** or the **↻**
button to reload the tree and clear cached quick-open results.

**Ctrl+P** searches paths across all roots, including unopened files, even when
the folder sidebar is hidden. With multiple roots, result paths include the root
name; roots with the same name use full paths to distinguish them.
Letters can be nonconsecutive: `srcprj` can find `src/project.rs`. Open files
appear first; folder results show paths so matching filenames are distinguishable.
Use **Up/Down** and **Enter**, or click a result. Search skips the hidden folders and
files set under **Settings → Folders & Search** (by default `.git`, `.hg`, `.svn`,
`node_modules`, `target`, `.venv`, `__pycache__`, `*.pyc`, and `.DS_Store`), and does not follow
directory symlinks. Hidden folders and files are left out of the folder tree too.

Folder paths, expanded directories, collapsed roots, and folder-sidebar visibility
survive restart. If a folder is moved or unavailable, its tree shows an error
while recovered buffers and the other roots remain usable. Right-click a root
and choose **Remove folder from workspace** to remove just that root, or choose
**File → Close all folders** to remove all trees. Both actions keep tabs open
and leave files on disk untouched.

Click **×** beside FOLDERS or use **View → Side bar → Show folders in the sidebar** (**Ctrl+Alt+B**)
to hide or reveal the folder section independently of Open Files. **View → Side bar →
Show sidebar** (**Ctrl+K, Ctrl+B**) toggles the entire sidebar. Both options are also in
**Preferences → Settings… → General**.

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

Outside Follow mode, hedit checks the active file about once per second and
automatically reloads clean files. Modified files keep their text and offer
**Compare changes…** or **Save file as…**. The comparison lets you take the disk
version, retaining your prior text in Undo, or explicitly save the editor version
over the compared disk version. If either side changes again, compare again.
Conflict resolution requires a fully loaded file; paged files retain Save As.

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

Use one or more selections for clipboard operations and indentation.
**Edit → Paste from history…** (**Ctrl+K, Ctrl+V**) lists the last 15 pieces of
text copied or cut in hedit, newest first; type to filter, then press **Enter**
to paste one at every cursor. The pasted text also goes back on the clipboard.
The history lasts until hedit quits. **Ctrl+Z**
undoes; **Ctrl+Shift+Z** or **Ctrl+Y** redoes. Up to 256 edit steps per file are
kept during the current process. Undo history is not restored after restart.

Toggle wrap through **View → Editor → Word wrap** (**Ctrl+Shift+W**). Long lines scroll
horizontally when wrap is off. **Ctrl++** and **Ctrl+-** change font size. Wrap,
font size, line numbers, tabs, status bar, current-line highlighting, whitespace
markers, and sidebar visibility are kept between launches.
Use **Preferences → Settings…** to manage them together.

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
| Ctrl+Shift+/ | Toggle a block comment around the selection or the current line |
| Alt+Q | Rewrap the selected lines, or the paragraph at the cursor, at the ruler |
| Ctrl+T | Swap the characters around the cursor, or the text of several selections |
| Ctrl+Shift+Backspace / Ctrl+K, Ctrl+K | Delete to the start / end of the line |
| F9 / Ctrl+F9 | Sort lines ignoring case / by exact characters |

**Line → Sort lines**, **Sort lines, ignoring case**, **Reverse lines**, and
**Remove duplicate lines** affect selected lines, or the whole file without a
selection. Duplicate removal keeps the first occurrence of each line and
preserves their order. **Delete to start of line** and **Delete to end of line**
remove the text between the cursor and the line's edge; at the edge itself, they
join the line with its neighbor.

**Text → Uppercase/Lowercase** transforms the selection. **Title case** and
**Swap case** change the selection, or the word at the cursor. **Transpose**
swaps the two characters around the cursor (at the end of a line, the two before
it); with several selections, their text moves one place along.
**Trim trailing whitespace** affects selected lines or the whole file.

**Text → Wrap paragraph at ruler** rewraps the selected lines, or the paragraph
around the cursor, at the wrap column (`wrap_width`), else the first ruler, else
80 columns. Indentation, comment markers such as `//` and `#`, quote markers
in Markdown, and list bullets are kept on every line, and a paragraph ends where
the marker changes. **Text → Reindent lines** indents the selected lines, or the
current line, by their brackets, starting from the line above; it is available
for languages whose blocks use brackets, such as C, Rust, JavaScript, and JSON.

**Toggle block comment** wraps the selection in `/* */` (or the language's own
markers, such as `<!-- -->` in HTML and Markdown), and removes them from a
selection that is already a comment. With no selection it comments the current
line, or removes the comment around the cursor. Languages without block comments
toggle line comments instead.

Under **Indentation**, choose spaces or tabs and **Tab width: 2/4/8**
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
occurrence. **Edit → Selection → Skip occurrence** drops the last one and selects
the next instead. **Alt+F3** selects all occurrences of the selected text or word,
and **Ctrl+Shift+L** splits a selection into one cursor per line.

**Ctrl+Shift+Space** expands the selection step by step: the word, the text inside
quotes, the quotes, the text inside brackets, the brackets, the line, and the whole
file. **Ctrl+Shift+M** selects inside the nearest brackets (press again to include
them, then the next pair out), **Ctrl+Shift+J** selects the surrounding lines at
the same or deeper indentation, and **Ctrl+M** jumps between matching brackets.
**Ctrl+U** and **Ctrl+Shift+U** step back and forward through selection changes,
like Sublime's soft undo for cursor movement.
Use **Alt+click** to add a cursor. **Alt+Shift+Up** and **Alt+Shift+Down**
(**Control+Shift+Up/Down** on macOS) add a cursor on the line above or below at the
same column; pressing again keeps that column even past shorter lines. Drag with **Alt+Shift** to select a rectangle,
or make a normal selection and choose **Edit → Selection → Rectangular selection**.
Short lines clamp to their ends. **Esc** or **Single selection** keeps just the
primary cursor.

Typing, pasting, deletion, indentation, line commands, and arrow-key navigation
apply across the selections. Copy joins selected ranges with newlines; paste
inserts the clipboard text at every cursor. Each batch is one undo step, with
cursor positions restored by undo/redo during the current process. Additional
selections survive recovery and reopening closed files. There is a limit of
1,000 selections per view; these controls require a fully loaded file.

### Windows

**File → New window** (**Ctrl+Shift+N**) opens another hedit window with its own
tabs, folders, and layout. Each window keeps its own recovery data, so unsaved
work is safe in each; settings, key bindings, snippets, and themes are shared.
When the first window starts, it reopens the other windows that were open last
time. A window closed with nothing in it is not reopened. Files opened from a
terminal with `hedit FILE` go to the first window.

### Split panes, folding, and minimap

**View → Layout** offers Sublime's layouts: a single group (**Alt+Shift+1**), two,
three, or four columns (**Alt+Shift+2/3/4**), two or three rows
(**Alt+Shift+8/9**), and a grid of four (**Alt+Shift+5**). On macOS use
**Cmd+Option+1–5**, and **Cmd+Option+Shift+2/3** for rows. New groups show the
current file. Use a group's file dropdown to show another open file, click an
editor to focus it, press **Ctrl+1–4** (**Control+1–4** on macOS) to focus a
group, or use **Ctrl+Alt+Right** to go to the next group. **Move file to other
group** (**Ctrl+Alt+Shift+Right**) displays the current file in the next group and
focuses it. If that group showed a different file, the two files exchange groups. Views keep independent cursor, selection,
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
**Settings → Editor** (or `rulers` in `settings.json`) accepts up to eight columns.
**View → Word wrap column** chooses 80, 100, 120, or the window width and enables
wrap. A narrow window wraps earlier to fit. Turning **Word wrap** off keeps your
chosen column for the next time you enable it. Ruler and wrap-column choices
survive restart and are also available in Editor preferences.

**F11** toggles full screen. **Shift+F11** toggles distraction-free mode, temporarily
hiding the sidebar, tabs, and status bar, entering full screen, and centering
text within 100 columns. Toggle it again to restore your chosen layout and
previous full-screen state. The menu remains available in both modes.

UTF-8 text is supported, including an existing BOM and LF/CRLF bytes. Binary files are
rejected; see [Encodings](#encodings-and-persistent-undo) for UTF-16. **Edit → Line endings** converts the entire loaded
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

Most languages are colored with Sublime Text syntax definitions (from the
collection the `bat` tool uses), so embedded languages, multi-line strings and
comments, escape sequences, and language constructs are recognized as Sublime
recognizes them. CUDA uses the C++ definition plus CUDA qualifiers and built-ins.
Plain text, logs, CSV, PowerShell, Visual Basic, and lines longer than 20,000
characters use hedit's quicker built-in highlighter, which recognizes functions,
types, and constants by form. When a large file opens or a large paste lands,
the quick colors show first and the full colors follow within a moment; typing
only re-highlights the lines it affects. Highlighting restarts at page boundaries
in very large paged files. Customize colors in
**Preferences → Settings… → Appearance**. The menu scrolls to the current syntax
when opened and adapts its height to smaller windows.

### Find and replace

1. Press **Ctrl+F** or choose **Find → Find…** to open the bottom search panel.
2. Enter a query of up to 4,096 UTF-8 bytes and choose search options.
3. As you type, the first match after the cursor is selected and every match on
   the page is highlighted (in large paged files, search starts when you press
   Enter). Press **Enter** or click **Find next** to select the next match across
   the whole file; **Shift+Enter** selects the previous one. The cursor stays in the search
   field, so you can keep pressing Enter or refine the query. Enter in the
   replacement field also finds the next match.
4. Enable **Replace** (or press **Ctrl+H**), enter replacement text, and click
   **Replace selected** to change that occurrence.

Search wraps at the end. Enable **Regex**, **Case sensitive**, or **Whole words**;
**Selection only** uses the selection captured when opening Find. Use **Previous**
or **Shift+F3** to go backward. The panel shows the current match as “n of m” while you type.
Regex replacements support capture groups such as `${1}`. **Replace all** is one
undoable edit. **Esc** closes the panel. Advanced search supports up to 32 MiB
and 20,000 matches; advanced replacement requires a fully loaded file. Literal,
case-sensitive Find continues to stream across larger paged files and supports
cancellation from the status bar.

**Find → Find in folders…** (**Ctrl+Shift+F**) searches disk contents across all
workspace roots. Enter comma-separated filename/path globs, such as `*.rs, *.cu`,
or leave Files empty. Git ignore rules apply, including outside Git repositories;
directory symlinks are not followed. Click a result to open its location.
Search reads UTF-8 and BOM-marked UTF-16 files, skipping unsupported/binary files
and files over 16 MiB. It reports skipped files and result limits. Open unsaved
text is not included in folder searches.

Choose **Preview replacements**, select the files to change, and click a filename
to inspect its diff before **Apply reviewed replacements**. Changing the query,
replacement, filters, or options requires a new preview. Files changed externally,
open with unsaved text, or in Follow mode are skipped and reported. Preview text
is limited to 128 MiB; applying across files is not a single transaction. Open,
fully loaded buffers retain undo for successful replacements; unopened disk files
should be restored through version control or backups if necessary.

### Saved projects and formatting

Use **Project → Save project as…** to create a `.hedit-project` file containing
folder roots and project settings. **Open project…**, **Recent projects…**, and
`hedit path/to/demo.hedit-project` restore it. The adjacent `.hedit-workspace`
stores personal tabs, unsaved text, selections, pins, split views, wrap, and font
size; keep it out of version control. Workspace state updates with recovery
checkpoints. Switching projects preserves unrelated unsaved buffers on the closed
shelf. A full shelf of unsaved work must be saved before switching. Workspace
snapshots support 200 tabs, 16 MiB per unsaved tab, and 32 MiB of unsaved text.

**Project → Project settings…** edits project formatting defaults and per-language
overrides. Your own defaults are under **Preferences → Settings… → Formatting** and
in [the settings file](#the-settings-file). Project formatting properties are
`indent_style` (`spaces` or `tabs`), `indent_size` (1–8), `end_of_line` (`lf`/`crlf`),
`insert_final_newline`, `trim_trailing_whitespace`, and `charset`.

hedit also reads matching `.editorconfig` sections from parent folders, honoring
`root = true`, common filename globs, and the formatting properties above.
EditorConfig uses `space`/`tab`; `tab_width` supplies a width when no indentation
size is set. Matching EditorConfig rules override project and user defaults.
Save-time cleanup is undoable and uses the destination rules on the first Save As.
Paged files cannot apply cleanup or line-ending conversion requiring a whole-file
rewrite; hedit reports that limitation instead of silently changing them.

### Symbols, Git, snippets, and keyboard settings

**Goto → Go to symbol…** (**Ctrl+R**) lists functions, classes, and types in the
current file; enable **All project folders** to scan the workspace. These are
source-line heuristics, not language-server definitions. Project indexing follows
the same ignore and per-file size limits as folder search and lists up to 20,000
symbols. **Alt+Left/Right** (**Control+-** / **Control+Shift+-** on macOS, where
Option+arrows move by word) navigates back/forward through up to 200 recorded
locations, reopening shelved files when needed.

**Goto → Go to definition** (**F12**) finds where the name at the cursor is
defined: functions, classes, and types, `#define` macros, `typedef`s, and
variable and constant declarations. It searches the open files first, then the
open folders, and jumps straight to a single result or lists several to choose
from. **Goto → Go to references…** (**Shift+F12**) lists every line that uses the
name as a whole word, up to 2,000. Both use the same source-line heuristics as
Go to symbol, skip binary files and files over the folder-search size limit, and
record where you were, so **Alt+Left** returns. Resting the pointer on a name for
a moment lists its definitions from the open files; click one to go there
(`show_definitions` turns this off).

With Git installed, changed-line markers refresh as you edit the active fully
loaded file, and when hedit's window regains focus. Hover a marker for its inline hunk. **Tools → Git changes…**
opens a comparison of editor text with HEAD, including staged and unsaved changes.
These operations do not stage, commit, or otherwise modify the repository.

While you type a word, a list at the cursor offers matching words from the open
files and snippets whose trigger matches. Words that start with what you typed
come first, then words that contain its letters in order. **Up** and **Down**
choose, **Tab** or **Enter** accepts, and **Esc**, a space, or punctuation closes
the list. **Tools → Complete word…** (**Ctrl+Space**, **Control+Space** on macOS)
opens it at once, even after one letter. The words are collected from the open
files in the background a moment after you stop typing, up to the first 4 MB of
each file, so large files never slow typing. Turn the list off with
`auto_complete`, or set `auto_complete_commit_on_tab` so that **Enter** always
starts a new line.
**Tools → Insert snippet…** inserts a configured template into the current selection.
Alternatively, type its trigger and press Tab. Templates support `${SELECTION}`
and `$0` for the final cursor; a `language` value restricts them to a syntax ID.

Snippets can have fields to fill in: `$1`, `$2`, … mark places, and
`${1:text}` gives one a starting text, which may contain other fields. After
inserting, the first field is selected; type to replace it, then press **Tab**
for the next field or **Shift+Tab** for the previous one. The last **Tab** moves
to `$0` (or the end of the snippet). A number used more than once is filled in
everywhere at once, so `for (${1:i} = 0; $1 < ${2:n}; $1++)` renames the loop
variable in all three places. **Esc**, or moving the cursor out of the field,
stops field navigation. Write `\$` for a literal dollar sign before a number,
and `\}` inside a field's text. Lines after the first take the indentation of
the line the snippet is inserted on, and a tab at the start of a body line
becomes the file's indentation.
Completion and snippets operate on fully loaded files.

**Settings → Key Bindings** lists every command with its shortcut. Click a
shortcut and press the new keys; **Esc** cancels. Press a second key within about
a second to record a two-key shortcut such as **Ctrl+K, Ctrl+G**. If another command used those
keys, it is unbound and the status bar says so. **Reset** restores a default, and
**Unbind** removes a shortcut. Bindings are saved in `keybindings.json`, where an
entry looks like
`{"command":"find_files","shortcut":"Primary+Shift+F","editor_only":false}`.
`Primary` means Ctrl on Linux and Command on macOS, and an empty shortcut unbinds
a command. Separate the keys of a two-key shortcut with a comma:
`"Primary+K, Primary+D"`.

Like Sublime, some commands use two keys: press the first, then the second. The
status bar shows that hedit is waiting for the second key; **Esc** cancels. The
defaults are **Ctrl+K, Ctrl+D** (skip occurrence), **Ctrl+K, Ctrl+U** /
**Ctrl+K, Ctrl+L** (uppercase / lowercase), and **Ctrl+K, Ctrl+1** /
**Ctrl+K, Ctrl+J** (fold / unfold all).

**Settings → Snippets** edits each snippet's trigger, description, syntax, and
body, and can add, remove, or restore the default snippets. They are saved in
`snippets.json`.

### Brackets and indentation

Typing an opening bracket or quote also inserts its closer, typing a selected
text's quote or bracket wraps the selection, and typing a closer that is already
next to the cursor steps over it. Backspace between an empty pair removes both.
Single quotes are not paired in plain text, Markdown, LaTeX, logs, or Rust, where
they are usually apostrophes or lifetimes. The bracket matching the one beside
the cursor is outlined. Turn either off under **Settings → Editor**.

When a file opens, hedit uses the indentation it already has; your `tab_size` and
`translate_tabs_to_spaces` settings apply to new files and files without indented
lines. Project settings and `.editorconfig` rules still take precedence.

### Build systems

**Tools → Build** (**Ctrl+B**) saves the file and runs its build system. Output
appears in a panel at the bottom as it arrives, ending with the time taken and any
exit code. Errors and warnings are highlighted; click one, or press **F4** /
**Shift+F4**, to go to the next or previous one. **Esc** hides the panel, **Tools →
Show build results** shows it again, and **Cancel** (or **Tools → Cancel build**)
stops the build and the commands it started.

hedit includes build systems for CUDA (`nvcc`), C++ (`g++`), C (`gcc`), Rust
(`cargo build`), Python, and Make. By default the first one for the file's syntax
is used. **Build with…** (**Ctrl+Shift+B**) chooses one, and hedit remembers it for
Ctrl+B; choose **Automatic** to go back to choosing by syntax.

Add your own under `build_systems` in [the settings file](#the-settings-file). A
build system with the same name as a built-in one replaces it, and yours are
chosen before the built-in ones:

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

`shell_cmd` runs through `sh`. It can use `$file`, `$file_path` (its folder),
`$file_name`, `$file_base_name` (without extension), `$file_extension`, and
`$folder` (the open folder containing the file). `working_dir` defaults to the
file's folder. `file_regex` captures the file, the line, and optionally the column
and message. `selector` lists the syntax IDs the system builds automatically.
Build systems come only from your own settings, never from a project's files.

### Encodings and persistent undo

UTF-8 remains the default. BOM-marked UTF-16LE/BE opens automatically. Use
**File → File encoding…** to open or reopen a file with an explicit encoding, or
choose its encoding for the next save. Supported choices include Windows-1252,
ISO-8859-1, Shift JIS, and GBK. Conversion supports files up to 32 MiB and rejects
unrepresentable characters rather than substituting them. Reopening with a
different encoding requires saving unsaved changes first.

Recovery checkpoints preserve undo and redo for fully loaded files: up to 64
states and 32 MiB of historical text in each direction per document. Older history
is trimmed; paged-file undo remains available during the current process.
Closing to the shelf preserves retained history as well as text. The previous
release's recovery snapshots remain readable, with no historical undo to restore.

### Arabic, Hebrew, and other scripts

hedit finds system fonts for scripts its built-in fonts lack, such as Arabic,
Persian, Urdu, Hebrew, Devanagari, Thai, Chinese, Japanese, and Korean, so their
text appears instead of empty boxes. On Linux it asks fontconfig and prefers Noto
fonts; on macOS it uses the system fonts. Install a font package such as
`fonts-noto` if a script still shows boxes.

Right-to-left text follows the Unicode Bidirectional Algorithm, and Arabic letters
join into their connected forms, including the lam-alef ligature. The file keeps
the text in the order you typed it; only the display changes, so copying,
searching, and saving work on the text as written.

- In plain text, Markdown, and LaTeX, each line reads in the direction of its first
  letter: a line that starts in Arabic runs right to left, and numbers and English
  words inside it stay left to right.
- In code, every line runs left to right, so an Arabic comment keeps `//` at its
  start and Arabic strings stay between their quotes.
- Clicking places the caret where you click, and **Left** and **Right** move by
  what is on screen. Where Arabic meets English, the caret has two places, as in
  GTK: the full-height caret shows where the next letter you type in the current
  direction appears, and a short one marks the other side.
- Lines are always aligned to the left.

Find, Replace, the command palette, tab titles, and the folder sidebar show
Arabic text joined and in order too.

### Go to a line or switch files

Press **Ctrl+G**, enter a positive, one-based line number, and click **Go**. This
locates a line in the entire document, even when only a page is displayed.

**Ctrl+P** opens Go to file, including all open folders; **Ctrl+Shift+P** opens commands. Type a
filter, use **Up/Down** to select a result, and press **Enter**, or click a result.
Like Sublime's Go to Anything, Go to file also jumps inside files: `@name` lists
the current file's symbols, `:42` (or `:42:7`) goes to a line and column, and
`#text` lists lines containing the text. The cursor previews each result as you
move through the list, and **Esc** returns to where you were. Put a file name
first to jump inside another file, for example `app.rs@save`, `main.cu:42`, or
`notes#todo`. Type `>` in the file switcher to search commands instead. **Esc** or a click
outside closes either. Go to line, Bookmarks, and Closed files also close when you
click elsewhere.
The menus show the same commands and keyboard shortcuts.

## Settings

Choose **Preferences → Settings…** or press **Ctrl+,** (**Cmd+,** on macOS).
Settings are grouped in one window, and every change saves automatically:

| Page | Settings and actions |
| --- | --- |
| General | Tab bar, status bar, sidebar, folders in the sidebar, update checks with GitHub CLI |
| Editor | Font size, line numbers, current line, whitespace, indentation guides, minimap, wrapping, rulers, typing behavior |
| Formatting | Indentation, tab size, whitespace and final newline on save, line endings, encoding, and the same rules per syntax |
| Folders & Search | Folders and files hidden from the folder tree and searches, default find options |
| Appearance | Theme gallery, system light/dark switching, custom colors, saved themes, import/export |
| Key Bindings | Every command's shortcut; press keys to change one |
| Snippets | Snippet triggers, descriptions, syntaxes, and bodies |
| CUDA Tools (Linux) | CUDA-GDB path and a shortcut to the tool panel |
| Recovery | Checkpoint status, checkpoint now, closed files, recovery guidance |

Each page has **Restore defaults** for its own settings.

### The settings file

Like Sublime Text, hedit keeps its settings in a readable file. Click **Open
settings.json** to edit it in a tab; saving it applies the changes immediately.
hedit also picks up edits made in another editor when its window regains focus.

The file lives in `~/.config/hedit/` on Linux and
`~/Library/Application Support/hedit.hedit/` on macOS, next to `keybindings.json`,
`snippets.json`, and your saved themes. Set `HEDIT_CONFIG_DIR` to use another folder.
The first launch after updating imports your earlier preferences into these files.

Comments (`//` and `/* */`) and trailing commas are allowed. Delete a key to restore
its default; hedit reports unknown keys and ignores them. If the file has an error,
hedit shows it, keeps your previous settings, and does not overwrite the file until
you fix it.

| Key | Default | Meaning |
| --- | --- | --- |
| `font_face` | `""` | An installed font family, such as `"JetBrains Mono"`, or a `.ttf`/`.otf` path; empty uses the built-in font |
| `font_size` | `14` | Editor font size, 10–28 |
| `line_padding` | `0` | Extra space between lines, 0–20 |
| `caret_width`, `caret_blink` | `1.5`, `true` | Cursor width (1–4) and blinking |
| `scroll_past_end` | `true` | Let the last line scroll up to the top of the editor |
| `word_wrap` | `false` | Wrap long lines |
| `wrap_width` | `0` | Wrap column; `0` wraps at the window edge |
| `rulers` | `[]` | Up to 8 ruler columns, such as `[80, 120]` |
| `line_numbers` | `true` | Show the line number gutter |
| `highlight_line` | `true` | Highlight the current line |
| `draw_white_space` | `false` | Show spaces and tabs |
| `draw_indent_guides` | `true` | Show indentation guides |
| `show_minimap` | `false` | Show the minimap |
| `auto_indent` | `true` | Keep the previous line's indentation on Enter |
| `smart_indent` | `true` | Indent after an opening bracket or colon |
| `smart_backspace` | `true` | Backspace in leading whitespace removes one level |
| `auto_match_enabled` | `true` | Close brackets and quotes, wrap selections in them, and step over closers |
| `match_brackets` | `true` | Outline the bracket matching the one beside the cursor |
| `match_selection` | `true` | Outline other occurrences of a selected word |
| `auto_complete` | `true` | Offer completions in a list while typing |
| `auto_complete_delay` | `50` | Milliseconds after typing before the list opens, 0–2000 |
| `auto_complete_commit_on_tab` | `false` | Accept completions with Tab only; Enter starts a new line |
| `show_definitions` | `true` | List a name's definitions when the pointer rests on it |
| `detect_indentation` | `true` | Use the indentation a file already has instead of `tab_size` and `translate_tabs_to_spaces` |
| `show_tab_bar`, `show_status_bar` | `true` | Show the tab bar and status bar |
| `show_sidebar`, `show_folders` | `true` | Show the sidebar and its folder section |
| `tab_size` | unset | Indentation width, 1–8 |
| `translate_tabs_to_spaces` | unset | Indent with spaces (`true`) or tabs (`false`) |
| `trim_trailing_white_space_on_save` | unset | Remove trailing whitespace when saving |
| `ensure_newline_at_eof_on_save` | unset | End files with a newline when saving |
| `line_ending` | unset | Convert to `"lf"` or `"crlf"` when saving |
| `encoding` | unset | Convert to an encoding such as `"utf-8"` when saving |
| `syntax_specific` | `{}` | Formatting keys per syntax ID, such as `{"python": {"tab_size": 4}}` |
| `folder_exclude_patterns` | `[".git", ".hg", ".svn", "node_modules", "target", ".venv", "__pycache__"]` | Folder names or globs hidden from the folder tree and searches |
| `file_exclude_patterns` | `["*.pyc", ".DS_Store"]` | File names or globs hidden from the folder tree and searches |
| `find_case_sensitive`, `find_regex`, `find_whole_word` | `true`, `false`, `false` | Find options, remembered as you change them |
| `color_theme` | `"Hedit Ink"` | Theme name, built in or saved under Your themes |
| `follow_system_appearance` | `false` | Switch between the dark and light themes with the system |
| `dark_color_theme`, `light_color_theme` | `"Hedit Ink"`, `"Hedit Paper"` | Themes used when following the system |
| `build_system` | `"automatic"` | The build system for Ctrl+B, or `"automatic"` to choose by syntax |
| `build_systems` | `[]` | Your own build systems; see [Build systems](#build-systems) |
| `cuda_gdb_path` | `"cuda-gdb"` | CUDA-GDB executable (Linux) |
| `update_with_github_cli` | `false` | Check for updates through GitHub CLI |

Unset formatting keys leave each file's own style alone. A project's settings and
matching `.editorconfig` rules override these defaults.

View commands provide quick toggles for the same editor settings. The status bar
shows the cursor position, indentation, line endings, encoding, size, syntax, and
recovery state. Click the indentation to switch between spaces and tabs, change
the tab width, detect it from the file, or convert it; the line endings to convert
them; the encoding to reopen or save with another one; the syntax to change it;
or the recovery status to open its settings. In the command palette, type
`set syntax` to choose a syntax by name.
For paged files, gutter line numbers
and **Page Ln** refer to the displayed page. The sidebar lists open files and
provides a **Closed Files** link. Right-click a tab for its context menu, or
middle-click its title to close it to the shelf.

## Appearance

### Switch themes quickly

Choose **Preferences → Color theme…**, or run **Color theme…** from the command
palette. The list shows every theme with a strip of its colors. Use **↑**/**↓**
to preview themes in the editor as you move, type to filter by name or by
“dark”/“light”, press **Enter** to keep the highlighted theme, or press **Esc** to
return to the theme you had. **Customize…** opens the full Appearance page.

### Browse the gallery

Open **Preferences → Settings…** (**Ctrl+,**) and select **Appearance**. The
**Themes** tab shows each theme as a miniature editor, grouped into dark themes,
light themes, and **Your themes**. Click a card to use it; filter the gallery with
the search field.

hedit includes these themes:

| Dark | Light |
| --- | --- |
| Hedit Ink (default), One Dark, Dracula, Monokai, Nord, Gruvbox Dark, Solarized Dark, Tokyo Night, Catppuccin Mocha, GitHub Dark, Rosé Pine | Hedit Paper, One Light, GitHub Light, Solarized Light, Gruvbox Light, Catppuccin Latte, Rosé Pine Dawn |

### Follow the system appearance

Select **Match the system light or dark appearance** to switch themes when your
desktop changes between light and dark. Choose **When dark** or **When light**,
then pick or customize the theme for that appearance. While the Appearance page is
open, the editor shows the appearance you are editing.

### Customize colors

Select the **Customize colors** tab. A live sample shows the result as you change
colors with the color pickers or `#RRGGBB` fields. Colors are grouped:

- **Interface:** backgrounds, sidebar, controls, borders, text, accent/cursor, and selection.
- **Syntax:** comments, keywords, strings, numbers, CUDA qualifiers, functions,
  types, constants, operators, properties/keys, and preprocessor
  directives/attributes.
- **Status:** warning, error, and success colors.

Invalid hex input keeps the last valid color. Changing a built-in theme renames it
to “*name* (modified)”; **↺** beside a color, or **Reset all**, restores the
original value. Text and selection contrast ratios appear below the colors, and
values below 4.5:1 are flagged without blocking your choices. **Dark controls**
switches menus, scroll bars, and other native controls between light and dark styles.

To keep your changes, choose **Save as new theme…**, enter a name, and press
**Save**. Saved themes appear under **Your themes** and in the quick switcher.
When you edit a saved theme, **Save changes** updates it. Right-click a saved
theme to export it or move it to the trash.

**Revert to** *name* restores the theme that was active when Preferences opened.
Settings, including the theme, save to `settings.json` shortly after each change
and on exit.

### Theme files

Saved and imported themes are JSON files in the `themes` folder beside
[the settings file](#the-settings-file). Set `HEDIT_THEMES_DIR` to use another
folder. Settings name the theme in use (`color_theme`); a customized theme you
have not saved keeps its colors in hedit's own state until you save it. hedit reads the folder when it starts.

**Import…** copies a theme file into the folder and selects it; a name that
matches an existing theme gets a number. It also accepts TextMate and Sublime
`.tmTheme` color schemes, converting their colors to hedit's roles. **Export…** saves the current colors
anywhere you choose. The bundled `themes/` directory has every built-in theme as
a complete file, plus a custom example.

A theme can name a built-in `base` and set only the colors that differ:

```json
{
  "version": 2,
  "name": "Violet Ink",
  "base": "Hedit Ink",
  "accent": "#B4A0F5",
  "selection": "#413657",
  "selection_text": "#F2ECFF",
  "function": "#D4C6FA"
}
```

Without `base`, omitted colors come from Hedit Ink. Color keys are
`background`, `surface`, `panel`, `elevated`, `border`, `text`, `muted`, `accent`,
`selection`, `selection_text`, `warning`, `error`, `success`, `comment`,
`keyword`, `string`, `number`, `cuda`, `function`, `type`, `constant`,
`operator`, `property`, and `preprocessor`. Version 1 files still load: their
functions, types, and operators use `text`, constants use `number`, and
properties and preprocessor directives use `keyword`, so they look as before.

Theme names need 1–80 characters. Colors are opaque, six-digit RGB values; a
leading `#` is optional on input. Unsupported versions, unknown fields or bases,
and files above 64 KiB are rejected. A failed import keeps your current theme.

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
hedit [--session-dir DIR] [--new] [-w] [FILE[:LINE[:COLUMN]] | FOLDER ...] [-]
```

| Argument | Effect |
| --- | --- |
| `FILE` | Open a file or start a named file |
| `FILE:LINE[:COLUMN]` | Open at a one-based location |
| `-` | Read UTF-8 stdin until EOF into an unsaved file |
| `--new` | Add an empty unsaved file to the restored session |
| `-w`, `--wait` | Keep the terminal waiting until the files are closed in hedit |
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

With `-w`, the command returns only after you close the files' tabs (or quit
hedit), so hedit can be the editor for other tools:

```sh
git config --global core.editor "hedit -w"
```

Without `-w`, hedit returns the terminal prompt as soon as the window starts. Closing
the terminal leaves the editor running. Help, version, and
startup errors still appear in the invoking terminal; background diagnostics
are appended to `hedit.log` in the session directory.

An existing literal filename takes precedence over location syntax. CLI columns
use UTF-16 units; out-of-range locations clamp to the document or line end. A
missing file opens only if its parent exists and is created on **Save**.

One process owns each session directory. Subsequent launches forward files,
folders, projects, new-buffer requests, and stdin to its existing window and
bring it forward. The command returns after the request and recovery checkpoint
complete. Use `--session-dir` for an independent window and workspace.
`HEDIT_SESSION_DIR` changes the default directory; the CLI option takes precedence.

With `-`, hedit reads until EOF before releasing the terminal. Stdin is
checkpointed before window creation, so consumed text remains in the session if
the graphical window cannot start.

## Keyboard shortcuts

On macOS, use Cmd for Ctrl and Option for Alt, with these exceptions that follow
macOS and Sublime Text conventions: Replace is **Cmd+Option+F** (Cmd+H hides the
app), full screen is **Control+Cmd+F**, distraction-free mode is
**Control+Shift+Cmd+F**, Complete word is **Control+Space** (Cmd+Space opens
Spotlight), select all occurrences is **Control+Cmd+G**, jump to bracket is
**Control+M**, navigate back/forward is **Control+-** / **Control+Shift+-**,
block comments are **Cmd+Option+/**, wrapping is **Cmd+Option+Q**, transpose is
**Control+T**, delete to the start of a line is **Cmd+Backspace**,
tab switching uses **Control+Tab** and **Control+PageDown** / **Control+PageUp**, and
tabs 1–9 are **Cmd+1–9**. About
and Quit are in macOS's own hedit menu. Change any shortcut under
**Settings → Key Bindings**.

| Shortcut | Action |
| --- | --- |
| Ctrl+N / Ctrl+O | New file / open file |
| Ctrl+Shift+N | New window |
| Ctrl+S / Ctrl+Shift+S | Save / Save As |
| Ctrl+W / Ctrl+Shift+T | Close to shelf / reopen latest |
| Ctrl+Tab / Ctrl+Shift+Tab | Recently used tabs, forward / back (hold Ctrl to go further) |
| Ctrl+PageDown / Ctrl+PageUp | Next / previous tab |
| Alt+1–9 | Tab 1–9 |
| Ctrl+Shift+R | Name the active unsaved file |
| Ctrl+Shift+O | Add one or more folders |
| Ctrl+K, Ctrl+B / Ctrl+Alt+B | Toggle entire sidebar / folder section |
| Ctrl+B / Ctrl+Shift+B | Build / build with a chosen build system |
| F4 / Shift+F4 | Next / previous build error |
| Ctrl+P / Ctrl+Shift+P | Find files / command palette |
| Ctrl+F / Ctrl+H / Ctrl+G | Find / Replace / Go to line |
| Ctrl+Z / Ctrl+Shift+Z or Ctrl+Y | Undo / redo |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy / cut / paste |
| Ctrl+K, Ctrl+V | Paste from history |
| Ctrl+A | Select all in the focused text field |
| Ctrl+D / Alt+F3 | Select next / all occurrences |
| Ctrl+Shift+L | Split the selection into lines |
| Ctrl+Shift+Space / Ctrl+Shift+M / Ctrl+Shift+J | Expand selection / to brackets / to indentation |
| Ctrl+M | Jump to the matching bracket |
| Ctrl+U / Ctrl+Shift+U | Undo / redo a selection change |
| Ctrl+K, Ctrl+D | Skip occurrence |
| Ctrl+K, Ctrl+U / Ctrl+K, Ctrl+L | Uppercase / lowercase |
| Ctrl+Shift+/ / Alt+Q / Ctrl+T | Block comment / wrap paragraph / transpose |
| Ctrl+Shift+Backspace / Ctrl+K, Ctrl+K | Delete to the start / end of the line |
| F9 / Ctrl+F9 | Sort lines ignoring case / exactly |
| Ctrl+K, Ctrl+1 / Ctrl+K, Ctrl+J | Fold / unfold all |
| Alt+click / Alt+Shift+drag | Add a cursor / rectangular selection |
| Alt+Shift+Up / Alt+Shift+Down | Add a cursor on the line above / below |
| Ctrl+Alt+Right / Ctrl+1–4 | Focus the next editor group / group 1–4 |
| Alt+Shift+1–5, 8, 9 | Layout: 1–4 columns, grid of four, 2 or 3 rows |
| Ctrl+Alt+Shift+Right | Show the current file in the other group |
| Ctrl+Shift+[ / Ctrl+Shift+] | Fold / unfold |
| F11 / Shift+F11 | Full screen / distraction-free mode |
| Ctrl+Shift+W | Toggle wrap |
| Ctrl++ or Ctrl+= / Ctrl+- / Ctrl+0 | Increase / decrease / reset editor font |
| Ctrl+, | Settings |
| Ctrl+Q | Quit (after a final recovery checkpoint) |
| F12 / Shift+F12 | Go to definition / references |
| Ctrl+F2 | Toggle a bookmark at the cursor |
| F2 / Shift+F2 | Next / previous bookmark |
| Esc | Close the topmost window or the search panel, then the error banner, then extra cursors |

Opening a window closes any other window, so only one is open at a time; the
search panel stays open beside it. Each **Esc** closes one thing, starting with
the most recent. Escape works even while a search or save is running. Short
messages such as “File saved.” appear in the status bar for a few seconds.

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
| File is rejected | Convert a separate copy to UTF-8 if needed. Binary files are unsupported. |
| Theme is rejected | Check the version, name length, hex values, and 64 KiB limit. Your current palette stays active. |
| Update check fails | Check the connection and access method; private releases need authenticated GitHub CLI. Read the error in the update dialog. |
| Update installation is unavailable | Use a writable release installation or upgrade manually; source builds and managed/symlink layouts cannot update in place. |
| CUDA tool cannot start | Install it, check `PATH` or the debugger path, and read the tool's output for driver/device/permission errors. |

hedit refuses to overwrite external changes. For a large source that changes
before a recovery copy completes, unseen bytes may be unavailable. If an edit
cannot be applied, the edited visible page is kept in a new file and an
error explains the result.
