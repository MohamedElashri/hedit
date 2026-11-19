# hedit user guide

Open **Help → User guide** (**F1**) for this guide offline. **Contents** jumps to
a section; **Back** restores your place. **Esc** closes the guide. External links
open in your browser only when clicked.

Shortcuts below use Linux/Windows keys. On macOS, use **Cmd** for **Ctrl** and
**Option** for **Alt**, except where the [shortcut table](#keyboard-shortcuts)
says otherwise. Menus show your platform's keys; the command palette
(**Ctrl+Shift+P**) lists commands, and **Settings → Key Bindings** can rebind them.

- [Installation](#installation) · [Updates](#updates)
- [Files and folders](#files-and-folders) · [SSH workspaces](#ssh-remote-workspaces)
- [Editing](#editing) · [Finding and navigating](#finding-and-navigating)
- [Writing code](#writing-code) · [Terminal](#integrated-terminal) · [Git](#source-control)
- [Markdown preview](#markdown-preview) · [Layout](#views-and-layout) · [Large files](#large-files-and-logs)
- [Settings](#settings) · [Appearance](#appearance) · [CUDA](#cuda-tools)
- [Command line](#command-line) · [Shortcuts](#keyboard-shortcuts)
- [Recovery](#recovery) · [Troubleshooting](#troubleshooting)

## Installation

Download packages and their `.sha256` checksums from
[Releases](https://github.com/MohamedElashri/hedit/releases).

| Platform | Requirements | Package |
| --- | --- | --- |
| Linux | X11 or Wayland desktop, OpenGL; releases built on Ubuntu 24.04 | x86-64 or ARM64 DEB, RPM, AppImage, or tarball |
| macOS | macOS 11+, Apple Silicon or Intel | Universal app ZIP |
| Windows | Windows 10+, x86-64, OpenGL-capable graphics driver | Portable ZIP |

### Linux and macOS

```sh
curl -fsSL https://raw.githubusercontent.com/MohamedElashri/hedit/main/install.sh | sh
```

The installer verifies checksums and installs under `~/.local` on Linux or
`~/Applications` on macOS. Add `~/.local/bin` to `PATH` for the `hedit` command.
To inspect the script, select a release, or change the destination:

```sh
curl -fsSL https://raw.githubusercontent.com/MohamedElashri/hedit/main/install.sh -o hedit-install.sh
sh hedit-install.sh --version vX.Y.Z --prefix "$HOME/.local"
```

On macOS, `--app-dir /Applications` installs for all users. For manual installation,
verify with `sha256sum -c ARCHIVE.sha256` (Linux) or `shasum -a 256 -c ARCHIVE.sha256`
(macOS), then extract the archive. On Linux, run the included `install.sh` or
`./hedit`; on macOS, move **hedit.app** to Applications. The app is not notarized;
use **Open Anyway** in Privacy & Security only for a download you trust.

### Linux packages

Verify the matching `.sha256` file before installing. Choose one format and your
architecture; Linux builds require glibc 2.39+ and a working desktop OpenGL driver.

```sh
sudo apt install ./hedit-*-linux-*.deb       # Debian/Ubuntu
sudo dnf install ./hedit-*-linux-*.rpm       # Fedora
chmod +x hedit-*-linux-*.AppImage            # Portable; then double-click or run it
```

If FUSE is unavailable, run `./hedit-*-linux-*.AppImage --appimage-extract-and-run`.
DEB/RPM install `/usr/bin/hedit` and desktop resources; use your package manager
to upgrade/remove them. Replace an AppImage to upgrade it. Settings and recovery
stay in your user profile. Tarballs and the installer remain available.

### Windows installation

Verify the ZIP in PowerShell before extracting it:

```powershell
$archive = 'hedit-X.Y.Z-windows-x86_64.zip'
$expected = (Get-Content "$archive.sha256").Split(' ')[0]
if ((Get-FileHash $archive -Algorithm SHA256).Hash -ne $expected) { throw 'Checksum mismatch' }
Expand-Archive $archive -DestinationPath hedit
```

Run **hedit.exe** from the extracted folder; optionally add its folder to `PATH`.
Windows packages are unsigned. To upgrade, close all windows and extract the new
ZIP into a new folder. Settings and recovery stay in your Windows profile.

### Source and removal

For source builds, follow the [developer guide](DEVELOPER_GUIDE.md#build-and-install).
To uninstall, remove the portable folder or macOS app and its `~/.local/bin/hedit`
launcher. A default Linux installation also uses `~/.local/bin/hedit`,
`~/.local/share/applications/hedit.desktop`,
`~/.local/share/icons/hicolor/scalable/apps/hedit.svg`,
`~/.local/share/hedit/themes/`, and `~/.local/share/doc/hedit/`.
Settings and [recovery data](#recovery) are retained; back them up before deletion.

## Updates

**Help → Check for updates…** installs stable releases on Linux and macOS when
you request it. Choose **Restart hedit** after installation or **Later** to keep
working. Checks, downloads, and restarts never happen automatically. Checksums,
the candidate version, and macOS signatures are verified; installation failures
restore the previous version.

Linux needs `curl`, GNU `tar`, and a writable packaged installation. On macOS,
use a writable application folder and keep custom themes outside the signed app.
Use the package manager for DEB/RPM and replace AppImages. Rebuild source
checkouts or rerun the installer for system-owned tarball installations,
and [replace the portable ZIP](#windows-installation) on Windows.

## Files and folders

| Task | How |
| --- | --- |
| Create or name a note | **Ctrl+N**; **File → Name unsaved file…** gives it a title |
| Open | **Ctrl+O**, drag files/folders into the window, or `hedit .` |
| Switch files | **Ctrl+P** searches; **Ctrl+Tab** visits recent tabs; **Ctrl+PageUp/PageDown** steps through tabs |
| Organize tabs | Drag to reorder; right-click to pin, copy a path, reveal, or close other tabs |
| Preview a file | Single-click in the sidebar; double-click, pin, or edit to keep it |
| Reopen a closed tab | **Ctrl+Shift+T** or **File → Browse closed files…** |
| Add folders | **Ctrl+Shift+O**; right-click entries to create, rename, move, duplicate, trash, or reveal |
| Refresh or hide folders | **File → Refresh folders**; **Ctrl+K, Ctrl+B** toggles the sidebar |
| Create another window | **Ctrl+Shift+N**; each window has its own tabs and recovery |

A dot marks unsaved changes. File opening runs in the background; **Cancel opening**
or **Esc** cancels pending opens after dismissing overlays. Typing, scrolling, or
switching tabs while a file loads keeps it from taking focus. Folder trees and
filename searches are bounded; notices identify incomplete results. Exclusions
live in **Settings → Folders & Search**; filename indexing skips directory symlinks.

Closing a modified tab asks **Save**, **Don't Save**, or **Cancel**. Don't Save
reopens disk files from disk; untitled notes keep their text in **Closed Files**.
Empty notes are discarded. The shelf normally keeps 50 files, retaining unsaved
notes beyond that limit. Quitting preserves open tabs and edits for next time
without writing them to their original files. Failed recovery keeps the window
open. Settings, key bindings, snippets, macros, and themes are shared by windows.

**Project → Save project as…** stores folders and formatting in `.hedit-project`.
Open it through Project or the CLI. Its adjacent `.hedit-workspace` holds personal
tabs, unsaved text, and layout; exclude it from version control. **Project settings…**
has GUI/JSON views, project defaults, and language overrides. **Save settings**
applies the draft; invalid JSON stays available to fix. `.editorconfig` takes
precedence over project settings, then editor settings.

UTF-8 and BOM-marked UTF-16 open directly. **File → File encoding…** reopens or
saves legacy encodings; unrepresentable characters are refused. **Edit → Line
endings** converts LF/CRLF. Binary files are unsupported.

## SSH remote workspaces

Experimental SSH workspaces use a separate native window. Files, search, Git,
builds, terminals, and recovery run on the host; the interface runs locally.

### Connect

Choose **Connect via SSH…** from the Command Palette or **+ Connect…** in
**Remote Explorer**. Type a hostname, SSH alias, `user@host`, or part of a saved
workspace path. Use the arrow keys to select a result, **Enter** to connect, and
**Escape** to dismiss the picker. A host opens the remote account's home
directory. A saved workspace reopens its saved folder. You can also select either
directly in Remote Explorer.

hedit discovers aliases from `~/.ssh/config`, the system config, and included
files. User and port come from OpenSSH unless explicitly overridden. Wildcard and
negated selectors do not appear as destinations.

Choose **Import SSH hosts…** in Remote Explorer to select which aliases appear
in the sidebar. **Import selected** saves that list across restarts. **Sync all
hosts** also shows aliases added to SSH config later and is the default. Changes
to SSH config and included files update the list automatically. A selected
alias disappears when removed from SSH config and returns if you add it back.
Connections use the current OpenSSH settings for the alias. The connection
picker can still find any discovered alias.

Remote Explorer groups saved workspaces under their SSH host. **Reopen last
workspace** uses the last successful connection and restores its open tabs and
active file from host recovery. **Choose remote folder…** previews directories
before opening a workspace or acquiring a writer. A new workspace uses its own
recovery directory. If you use a manually installed helper, enter that new
directory in connection settings before opening it.

hedit checks the host platform, installs the matching helper for the non-root SSH
account, and opens a separate remote window. It uses a bundled helper when the
platform matches, or downloads the exact editor version and verifies its SHA-256
checksum locally before transferring it over SSH. The host needs no Cargo,
compiler, internet access, or elevated privileges. Later connections reuse the
versioned user-local installation. Setup progress and cancellation appear in the
sidebar. Automatic setup refuses root accounts and unsafe directory permissions.

Open **Connection settings…** from the picker or a host's context menu to choose
a workspace and override the user, port, trusted config, or private recovery
directory. Existing recovery paths are retained when enabling automatic helper
management. The host context menu also has **SSH configuration…**.
For an existing custom installation, enable **Use a manually installed helper**
and supply absolute helper and private recovery paths. Saved workspaces also
have right-click actions to edit or remove them. Filter by host or workspace path.
SSH configuration settings provide **Open SSH config…** and **Refresh hosts**.
Discovery reads files without executing `Match exec`; OpenSSH resolves options
when connecting, so configuration files must be trusted.

Host keys must already be verified with OpenSSH. Authentication uses keys or an
existing agent; passwords and interactive MFA remain unsupported. Unlock the
agent before connecting. Saved metadata contains host names and paths, without
keys, passwords, or document content. Corrupt metadata is reported and preserved.

The command line uses the same automatic setup:

```sh
hedit --ssh devbox
hedit --ssh devbox --ssh-root /home/me/project src/main.rs
```

Advanced flags: `--ssh-user USER`, `--ssh-port PORT`,
`--ssh-config /absolute/config`, and `--ssh-takeover`. `--ssh-state` can retain an existing private recovery directory while managing
the helper automatically. Supplying `--ssh-helper` also requires `--ssh-state` and
`--ssh-root` and selects a manually managed helper. Takeover fences the previous writer and does not merge edits.

The **SSH · host** menu and Remote Explorer show connection status, pending edits,
reconnect, disconnect, and execution trust. Disconnect retains documents and
pending edits and stops automatic retries; Reconnect resumes the same host
recovery session. Paths stay inside the workspace. New files start as recoverable
host scratchpads, and Save As refuses existing targets. Keep at least one remote
tab open. **Trust for Git, builds and terminal…** permits host scripts, hooks and
shells to execute as the SSH account. Reconnect clears trust; revoking it stops
tracked jobs. Terminals also need their regular opt-in setting.

In a remote window, choose **File → Open folder…** to browse within the active
SSH workspace. Use the arrow keys and **Enter** to browse subfolders, or type a
path relative to the workspace and press **Enter**. **Up** returns to the parent
inside the workspace. **Add Folder** shows the current folder in the Files
sidebar and keeps the same SSH connection and recovery directory. Use **Tab**
to reach the buttons and **Escape** to dismiss the browser.

### Privacy and recovery

Remote content, drafts, undo history, searches, patches, and terminal output stay
out of local recovery. Local launch metadata/diagnostics may contain host names
and paths. OS swap, crash collectors, clipboard managers, screenshots, and explicit
exports are outside this application guarantee. Trust the host with recovery data.

**Durable on SSH host** means accepted edits committed to the host journal;
**Save** separately writes the source file. Disconnects preserve pending edits in
memory and pause editing until reconnect reconciles receipts. Transport failures
trigger up to five automatic reconnect attempts with increasing delays. Retries
stop on recovery-session changes or reconciliation conflicts; they never take over
another writer or repeat uncertain saves. After retries stop, use **Reconnect** to
try again. Reconnect always clears execution trust. A client crash can
lose edits without a durable receipt; there is no local recovery fallback.
Outside changes, stale writers, lag, quota, or storage errors pause edits rather
than discarding recovery. Failed saves retain editor text; use Save As if needed.
Keep state and SQLite sidecars together; never manipulate a live state directory.
Closing ends tracked tools; idle workers exit after five minutes, leaving recovery.

### Limits

- Direct SSH to Linux/macOS hosts; Windows clients work. No jump hosts, Windows
  helpers, forwarding, local commands, or reuse of
  multiplexed connections.
- Private host-local ext-family, XFS, btrfs, ZFS (Linux) or APFS (macOS) recovery
  storage; ancestor permissions are checked. Network filesystems and tmpfs are
  unsupported. Recovery and undo capacity are bounded.
- Up to 128 retained documents, each with at most 8 MiB decoded text.
- Remote browsing skips symlinks and non-UTF-8 names; directory duplication and
  cross-filesystem trash moves are unavailable.
- No remote project persistence, Markdown preview, CUDA/debugging, definition or
  reference lookup, compare-disk, or mini-diff tools.

Helper details and tests: [developer guide](DEVELOPER_GUIDE.md#ssh-architecture-and-testing).

## Editing

Undo/redo survives restarts for fully loaded files. The **Edit** menu groups
selection, line, text, and indentation commands; right-click offers common actions.

| Feature | Usage |
| --- | --- |
| Clipboard | With no selection, Copy/Cut takes the line; pasting places it above. **Ctrl+Shift+V** adjusts indentation; **Ctrl+K, Ctrl+V** opens the last 15 clips |
| Line editing | Duplicate, delete, move, join, comment, indent, and add lines above/below. With no selection these act on the current line |
| Text cleanup | Sort/reverse/deduplicate lines, change case, trim whitespace, reindent, and convert tabs/spaces; whole-file cleanup applies when nothing is selected |
| Multiple cursors | **Ctrl+D** adds the next occurrence; **Ctrl+K, Ctrl+D** skips; **Alt+F3** selects all. **Alt+click**, **Alt+Shift+Up/Down**, or **Alt+Shift+drag** adds cursors/rectangles; **Ctrl+Shift+L** splits lines. Up to 1,000 cursors, one undo step |
| Selection | **Shift+click** extends; **Ctrl+Shift+Space** expands by word/quotes/brackets/line/file; **Ctrl+Shift+M** selects brackets; **Ctrl+Shift+J** selects equal indentation; **Ctrl+U** undoes selection changes |
| HTML/XML selection | **Ctrl+Shift+A** grows through contents, tags, then enclosing elements. Uses explicit balanced tags, including self-closing/void elements; no inferred closing tags or JSX parsing |
| Brackets | Typing a pair wraps selections or adds its closer; Backspace removes empty pairs. **Ctrl+M** jumps to the match |
| Indentation | Enter preserves/detects style and indents after brackets. **Edit → Indentation** changes width, spaces/tabs, or detects again |
| Macros | **Ctrl+Alt+Q** records editor typing/keys, then stops; **Ctrl+Alt+Shift+Q** replays. **Tools → Save macro… / Run saved macro…** keeps named macros. Menu clicks are not recorded |
| Spell check | **F6** checks English prose and code comments/strings, excluding code-like words. Right-click for suggestions, dictionary additions, or Ignore; **Ctrl+F6** jumps to the next |

Arabic/Hebrew use joined letters and visual caret navigation while keeping logical
file order. Prose follows the first letter's direction; code lines stay left to
right. System font fallback covers other scripts; install fonts if glyphs are
missing. **Esc** returns to one cursor after dismissing open panels/dialogs.

## Finding and navigating

| Task | Usage |
| --- | --- |
| Find/replace | **Ctrl+F / Ctrl+H**; Enter/Shift+Enter navigates. Options: regex, case, whole words, selection only. Replacements support `${1}` groups; Replace all is one undo step |
| Select matches | **Alt+Enter**; Find highlights up to 20,000 matches but navigates beyond. Replacement/select-all requires at most 20,000; narrow larger queries |
| Find in folders | **Ctrl+Shift+F** searches open folders and unsaved buffers, honoring Git ignores. Filter with `*.rs, *.cu`; Context shows surrounding lines; **F4/Shift+F4** navigates |
| Replace in folders | Enable Replace, enter text, then **Preview**. Review/select files and **Apply**. Open files remain unsaved/undoable; unopened files are written only if unchanged since preview, with **Undo replacements** available |
| Go to anything | **Ctrl+P** finds by letters in order (`srcprj` → `src/project.rs`). Append `@symbol`, `:42:7`, or `#text`; alone these search the current file. Esc cancels previews |
| Commands/symbols/lines | **Ctrl+Shift+P / Ctrl+R / Ctrl+G**; **Alt+Left/Right** navigates history |
| Definitions/references | **F12 / Shift+F12** uses the configured language server. Local files fall back to source indexing when a server cannot answer; pointer hover also shows indexed definitions |
| Header/implementation | **Alt+O** switches matching C/C++/CUDA/Objective-C stems, checking siblings then the nearest project; multiple matches open a picker |
| Path under cursor | **Alt+D** opens a path/include/`file:line:column` in the line or selection. Quote spaces. Checks beside the file, then open folders and `include/`; existing files only, no URLs |
| Bookmarks | **Ctrl+F2** toggles; **F2/Shift+F2** navigates; **Goto → Bookmarks…** lists; **Alt+F2** adds cursors; **Ctrl+Shift+F2** clears |

Large find, file-picker, symbol, and text searches run in the background. New
input takes priority over pending navigation. Filename indexes stay in memory. **Settings → General → Automatically refresh
workspace** enables debounced native watching of local roots and open-file
parents. Watches cover up to 1,024 directories, skip directory symlinks and
excluded folders, and stop scanning after two seconds or the path-memory limit.
Hover Refresh folders for coverage status. Refocusing or Refresh folders updates
caches beyond watch coverage; SSH workspaces continue to use explicit refresh.
Clean active files reload after external changes; unsaved text is kept with a
conflict banner and the existing save guard. Notices report
indexing progress, unreadable folders, and incomplete results.

## Writing code

**Completions:** language-server suggestions, words from fully loaded open files,
and matching snippets appear as you type. Up/Down chooses, Tab/Enter accepts, Esc dismisses; **Ctrl+Space** opens
the list. Background indexing prioritizes the active file and notes incomplete
results. Set `auto_complete_commit_on_tab` to keep Enter for new lines.

**Snippets:** type a trigger then Tab, or **Tools → Insert snippet…**. Edit in
**Settings → Snippets**. `$1`, `${1:default}`, `$2` define fields; repeated numbers
mirror edits; `$0` is the final cursor and `${SELECTION}` inserts selected text.
Tab/Shift+Tab moves between fields; `\$` and `\}` escape literals.

**Change markers:** gutter markers compare with Git HEAD, or the saved file outside
Git (`mini_diff`). Hover to inspect; **Ctrl+K, Ctrl+.** / **Ctrl+K, Ctrl+,** navigates changes,
**Ctrl+K, Ctrl+Z** reverts as an undoable edit. **Tools → Compare file with Git HEAD…**
compares editor text; use [Source Control](#source-control) for staging.

**Builds:** **Ctrl+B** saves and builds; **Ctrl+Shift+B** picks a system. Built-ins
cover CUDA, C/C++, Rust, Python, and Make. Click diagnostics or use F4/Shift+F4.
Install the tools separately. Commands use `sh` on Linux/macOS and `cmd.exe` on
Windows. Custom `build_systems` in settings accept `name`, `shell_cmd`, `selector`,
and `file_regex` (file, line, optional column, message capture groups).
Commands expand `$file`, `$file_path`, `$file_name`, `$file_base_name`,
`$file_extension`, and `$folder`; matching a built-in name overrides it.

### Language servers

Open **Settings → Language Servers**, **Preferences → Language servers: manage…**,
or the **LSP** button in the status bar. The manager lists available and registered
servers, installation source/version, workspace status, failures, and logs
(**Refresh** updates logs). No server downloads until you select **Install**.

- **Managed installs:** rust-analyzer (Rust), clangd (C/C++/CUDA), ty and Ruff
  (Python), LuaLS (Lua), and Tombi (TOML). Select
  **Install**, **Update / Repair**, or **Remove**. Packages come from the upstream
  GitHub release, require a release-provided SHA-256 digest, and install under
  hedit's user data directory. Downloads require `curl`; updating is manual.
  Rust projects also need a Rust toolchain (and `rust-src` for standard-library
  sources); C/C++ projects benefit from `compile_commands.json`. Unsupported
  OS/architecture combinations report an error and can use an external server.
- **External servers:** presets cover gopls, TypeScript/JavaScript, HTML, CSS, JSON,
  YAML, shell, pylsp, and Taplo. Select **Configure → Detect installed on PATH**
  to find a preset on the current computer or SSH host. Detection does not install
  dependencies or run the executable. For other servers, select **Add external
  server…**, enter an ID, comma-separated syntax IDs such as `python` or `go`,
  an absolute executable path, arguments as a
  JSON array, and settings as a JSON object. For example, a Python server may use
  `/absolute/path/to/pylsp` with `[]`; a Node-based server may need `["--stdio"]`.
  On Windows, npm wrappers need an explicit `node.exe` path with the server script
  and `--stdio` in the argument array. Install runtimes/dependencies separately.
  Removing a registration preserves external executables. **Configure** can also override a built-in server.
- **Control:** installed, enabled, and running are separate states. Change defaults
  or choose **Use default**, **Enabled**, or **Disabled** for the current workspace.
  **Configure** opens a dedicated detail view for feature switches and priority. Advanced settings include
  environment overrides (a JSON object of strings), project root markers (files or
  directories), and initialization options separate from runtime server settings.
  Presets scaffold the server's settings namespace (for example, LuaLS uses
  `{"Lua": {"runtime": {"version": "LuaJIT"}}}`). Root discovery stays within the
  workspace. Arguments can override managed defaults too. Empty initialization options (`{}`) send no extra options; a blank field
  preserves the older behavior of also sending server settings at initialization.
  **Stop** lasts for this window; **Restart** starts it again. Repeated crashes require an explicit
  restart. Stop a server in other hedit windows before changing its registration
  or installation; shared installations are protected by cross-window locks.

**Ctrl+Space** and automatic completion include plain-text LSP completions and
additional text edits. **F12 / Shift+F12** requests definitions/references.
Use the command palette or Tools menu for **Language server: show hover
information**, **format document**, **toggle problems**, and **restart**.
The Problems panel lists diagnostics for open files; select one to navigate.
Formatting and accepted completion edits remain unsaved and use normal Undo and
recovery. Delayed results are discarded after relevant document, cursor,
selection, pane, or workspace changes. Source indexing and word completion remain
available without a server.

Servers start as needed for saved, fully loaded files up to 4 MiB. A workspace
synchronizes up to 32 documents / 32 MiB and runs up to eight server sessions;
the active file is prioritized. SSH uses a 4 MiB aggregate snapshot budget to
stay within its bounded JSON transfer limit. Paged logs, follow mode, and untitled notes do
not start servers. Multiple enabled servers can share a file. Each action uses
the highest-priority server that supports it and has that feature enabled; ties
use server-ID order. Diagnostics are combined and labeled by server. Python
defaults pair ty for language intelligence with Ruff for linting and formatting.
The session cap favors focused documents, then server priority. Rename, code actions, semantic
highlighting, server-supplied snippets/commands, and unsolicited workspace edits are not yet
supported.

In **SSH workspaces**, the manager targets the host. Execution trust is required
for management and execution; revoking trust or disconnecting stops its workers.
Packages and registrations live in the helper state's `language-servers` directory;
executable paths and runtime requirements refer to the host. LSP document text,
logs, and diagnostics remain memory-only on the client. Results outside the
remote workspace are excluded from navigation. Install the matching updated
helper; the language-server feature uses the updated SSH protocol.

Local management also works without opening a window:

```sh
hedit --lsp list
hedit --lsp install rust-analyzer
hedit --lsp remove rust-analyzer
hedit --lsp-dir /absolute/private/server-store --lsp list
```

`list` returns JSON containing the catalog and registrations. `install` resolves
the latest stable release and records its exact version, hash, and source;
`remove` deletes only hedit-owned package directories and the registration.
Downloads install through temporary directories, a bounded executable version
check, and atomic activation. Failures or cancellation before activation preserve
the active installation. If a receipt write cannot be confirmed, hedit preserves
package files and reports the uncertainty; refresh the manager to inspect the
active version. The previous managed
installation is retained until the next successful update or removal.

### Syntax highlighting

Click the status-bar language to override detection or return to
Automatic. The 58 formats include common programming languages, CUDA, web/config
formats, Markdown, LaTeX, CSV, and logs. Embedded languages and multiline strings
use syntax definitions; large files show quick colors before full highlighting.

## Bottom panel

**Problems**, **Output**, and **Terminal** share one resizable bottom panel.
Choose a tab to keep the editor visible while reviewing diagnostics, build output,
or a shell. Problems combines build locations and language-server diagnostics;
click a row to navigate. Output retains the latest build and its Cancel action.
The close button hides the panel without stopping a build or shell. Escape hides
Problems or Output; while focused in Terminal, Escape goes to the shell.

## Integrated terminal

Enable **Preferences → Settings → Terminal**, then **Tools → Toggle terminal**
or **Ctrl+`** (**Control+`** on macOS). It is disabled by default. Shells start in
the active project/file directory, or the launch directory; SSH shells run on the
trusted host. Drag the panel edge to resize it.

| Control | Action |
| --- | --- |
| **+** / Ctrl+Shift+T | New shell, up to 16 tabs |
| Tab | Select, drag to reorder, middle-click to close |
| **X** / Ctrl+Shift+W | Close that shell |
| Ctrl+PageUp/PageDown | Previous/next terminal tab |
| **…** / right-click | Rename, restart, stop, close, copy selection |
| Chevron / toggle shortcut | Hide panel while shells keep running |

These keys apply while terminal-focused; macOS uses Cmd for new/close, Control
for tab switching. **Ctrl+C** interrupts; copy/paste uses **Ctrl+Shift+C/V**
(**Cmd+C/V** on macOS). Esc, Tab, arrows, and Control keys go to the shell; click
the editor to resume editing. The command palette remains available.

Pastes use bracketed paste where supported, capped at 256 KiB; scrollback keeps
10,000 lines. Hidden/inactive tabs drain output. Titles follow the shell unless
renamed; **Use shell title** restores that. Restart clears output; Stop retains
it. Disabling the feature or quitting ends shells; sessions/output are not restored.
RTL shaping affects display only; turn it off if a terminal app already reorders
text. Graphics and mouse-event forwarding are unsupported.

**Settings → Terminal** also has independent appearance controls. **Use app font
and size** and **Use app colors** are enabled by default. Turn either off to
choose a terminal font (family or font-file path, 8–40 pt), foreground, background,
cursor and selection colors, or an optional 16-color ANSI palette. Copy a theme
as a starting point and adjust individual colors. Terminal programs can still
set their own colors. Changes apply to existing local and SSH shells without
restarting them; app/editor colors and fonts keep their own settings.

In `settings.json`, omitted or `null` `terminal_font` and `terminal_colors` use
the app appearance. For example, a separate font can be set with
`"terminal_font": {"face": "", "size": 18}` (empty face uses the built-in font).
Color objects use six-digit hex values for `foreground`, `background`, `cursor`,
`selection`, and `selection_text`; `ansi` is an optional array of 16 colors.

## Source Control

**Ctrl+Shift+G** opens the Git workspace; **Files** or **Ctrl+W** returns to editing.
Install Git 2.30+. Select an open repository, **Open repository…**, **Clone
repository…** into a new directory, or Initialize a folder. A failed clone may
leave files for inspection. Credentials use your Git helpers/SSH agent; terminal
prompts are disabled. Signing and hooks follow Git configuration.

| Task | Usage |
| --- | --- |
| Review | Changes groups conflicts, staged, and working files. Choose Unified/Side by side; narrow windows use Unified. Tree/List and filters organize paths; Previous/Next or Alt+Up/Down navigates. Right-click to copy source lines or patches |
| Stage | **+ / −** for files, Stage/Unstage hunk for ordinary text, or select changed lines (Shift extends) and Stage/Unstage selected. Select both sides for a replacement. Section actions include filtered-out files |
| Discard | **Discard…** confirms and backs up the working version; unsaved editor text is retained. Review before saving over the result |
| Commit | Enter a message; Commit includes staged changes only. Amend confirms. Errors retain draft/staging. Refresh stale diffs before partial staging; unusual/binary changes may require whole-file staging |
| History | Select commits to review files/diffs; History scope filters revision/path, All history clears. File history does not follow renames |
| Branches | Picker switches/creates; **…** renames, merges, rebases, sets upstream, or deletes. Save/close unsaved repository files before worktree changes. Conflicted operations offer Continue/Abort |
| Compare | **More → Compare** accepts branches, tags, IDs, or `HEAD~3`. Apply inputs with Compare, reverse with Swap; choose common-ancestor or direct comparison. Read-only reviews pin exact commits; refresh moving references |
| Line history | **More → Line history** shows committed UTF-8 source with authors/IDs; excludes unsaved/worktree edits. Filter source, authors, IDs, summaries |
| Stashes | **More → Stashes** includes tracked/untracked changes. Apply keeps the stash and restores index/worktree; Drop confirms. Resolve/stage conflicts before continuing |
| Tags | **More → Tags** lists local tags; Tag HEAD or commit menu creates annotated tags. Inspect, compare, branch, copy, delete from menus; publish tags in a terminal |
| Recovery | **More → Recovery** lists HEAD reflog. Recover branch names an older commit without switching or changing files; Git may expire reflog entries |
| Undo/revert | Undo last commit confirms the reviewed HEAD, moves to its first parent, and leaves changes staged. Unavailable for initial commits/operations in progress. Revert shared commits; Branch here creates without switching |
| Remotes | Fetch updates references; Pull fast-forwards only; Push uses the configured destination; Publish sets upstream. No force-push; merge/rebase divergence explicitly |

Conflicts show **Current · ours**, **Incoming · theirs**, and the ancestor.
**Use Current/Incoming…** confirms and backs up; **Open to resolve** combines in
the editor. Review and stage afterwards, including deletions. During rebase,
Current is the destination and Incoming the replayed commit. Binary, symlink,
submodule, non-UTF-8, or large conflicts may need a terminal. Refresh externally
changed sides before resolving.

Git jobs run in the background. Cancel stops work; refresh to check its outcome.
**Git output** keeps recent results. If a repository moves, reopen it. Offline
help and editing work without Git installed.

## Markdown preview

Markdown has **Source**, **Split**, and **Preview** modes (also in **View →
Markdown**). Split follows unsaved edits and synchronizes source/preview scroll;
narrow windows stack the panes. Drag the divider, use its focused arrow keys, or
double-click to equalize. Preview fills the editor; switch back to type. Modes
survive restarts. Choose Markdown syntax for untitled notes.

**Contents** jumps to headings. Text/code are selectable; code blocks offer Copy.
Tables wrap/scroll; lists, tasks, quotes, and GitHub alerts render natively.
Section links stay in-document; Markdown links open in Preview with fragments,
other files in the editor. Right-click links to copy addresses.

Local PNG/JPEG images resolve beside the document, changing base after Save As.
Web/`file://` images and Windows network/device paths are blocked. Comments are
hidden, `<br>` inserts a break, other HTML stays text. Preview requires a fully
loaded file up to 2 MiB.

Use `$E = mc^2$` inline, `$$ … $$` or fenced `math` for display equations, and
fenced `mermaid` for diagrams. Escape dollars as `\$`. Rendering is local and
asynchronous; unsupported input stays as source with a hover error and **Copy
source**. Limits: 16 KiB/equation; 64 KiB, 1,000 lines, 256 nodes, and 1,024 edges
per diagram. **Refresh** reloads media, including repaired images; returning
from Source also refreshes. Find/Replace/Go to line/search results expose source.

## Views and layout

| Feature | Usage |
| --- | --- |
| Panes | **Alt+Shift+1–4**: columns; **Alt+Shift+5**: four-pane grid; **Alt+Shift+8/9**: two/three rows. **Ctrl+1–4** focuses a group; **Ctrl+Alt+Shift+Right** moves a file. Shared files keep separate cursor/scroll positions |
| Folding | Gutter arrows or **Ctrl+Shift+[/]** fold/unfold. **View → Code folding** includes nesting levels and Fold all. **Ctrl+K, Ctrl+1/2/3** folds a level; **Ctrl+K, Ctrl+J** unfolds all |
| Wrap/rulers | **Ctrl+Shift+W** toggles wrapping; View sets wrap column/rulers (up to eight) |
| Focus | **F11** full screen; **Shift+F11** distraction-free; independent modes |
| Font | **Preferences → Editor font…** filters installed fonts by family/script. **Ctrl++ / Ctrl+- / Ctrl+0** changes/resets size |
| Editor details | **View → Editor** toggles numbers, current-line highlight, whitespace, guides, minimap, tabs, and status bar |

Search and Source Control stay beside the document while you switch tools.
Search inputs, results and commit drafts survive sidebar switches. Selecting a
changed file opens a reusable **Git review** tab; select a file tab to resume
editing, or close the review without closing your document. Search has its own
query and options, separate from Find within the current file. Search's **More…**
menu can detach it into a window.

The activity bar provides Files, Source Control, Find in files and Settings.
Click the active Files or Source Control icon to hide its sidebar. Turn off
`show_activity_bar` in Settings to restore the text workspace switcher.

The status bar shows position, indentation, line endings, encoding, syntax, and
recovery; hover for details. Click its recovery dot for settings.

## Large files and logs

Files over 2 MiB use roughly 256 KiB pages, with Start/Previous/Next/End controls.
Find and Go to line cover the whole file; whole-document commands such as multiple
cursors, folding, and Markdown preview are unavailable. **View → Follow log (read
only)** follows a saved file's end. External changes reload unchanged files;
modified files offer **Compare changes…**, keeping replaced editor text in Undo.

## Settings

**Preferences → Settings…** (**Ctrl+,**) opens a resizable settings window.
Choose **User** for shared preferences or **Project** for workspace formatting and
language overrides. Switching scopes preserves the project draft. **Save settings**
applies that draft; **Discard draft and reload** replaces it with saved values.
Ctrl+S saves the visible scope and Ctrl+W closes Settings without closing a file.
Project saves refuse conflicting disk changes or an unsaved project-file buffer.

Expand **Resolved settings for the active file** in Project or User → Formatting
to see the saved defaults and their winning sources, including language overrides
and `.editorconfig`. Click an `.editorconfig` source to open it; **Refresh sources**
rereads external changes. The inspector runs only while expanded. File detection
and per-file indentation choices can differ from these formatting defaults.
Search by setting name, description, or JSON key; searches span categories, and
selecting a category narrows the results. Focus a category and use **Up/Down**,
**Home** or **End** to navigate without leaving the keyboard. **Modified** shows values that differ
from defaults. Common settings show their default and offer an individual
**Reset**. Invalid edits display an explanation and keep the last valid value;
incomplete list entries remain available to correct after changing focus.

The scope label distinguishes personal preferences on this computer from
language-server settings on an SSH host. User preferences apply across projects;
project settings, per-language rules and `.editorconfig` can override formatting.
Formatting save rules offer **Inherit**, **Enabled**, and **Disabled**, including
explicitly disabling a rule inherited from the user defaults.

Categories include General, Terminal, Editor, Formatting, Folders & Search,
Appearance, Key Bindings, Snippets, Language Servers, Recovery and Linux CUDA
Tools. Valid personal changes save after a short delay. Server configuration uses
**Save configuration** in its dedicated detail view; advanced JSON options are
collapsed until needed. Key bindings retain per-command Reset and Reset all.
Two-key bindings are supported; `Primary` means Ctrl or macOS Cmd.

### Interface appearance

**Settings → Appearance → Interface** controls `ui_font_face`, `ui_font_size`
(10–20 logical pixels), and `ui_density` (`compact`, `default`, or `comfortable`).
The bundled Noto Sans is the default UI font; an installed family or absolute
font-file path can replace it. Use **Choose installed font…** to filter families
by name or supported script, or type a family or font-file path directly.
These preferences affect menus, tabs and panels
independently of editor zoom, editor fonts and terminal overrides. Font files
load in the background, with the built-in font as fallback when unavailable.

### The settings file

**Open settings.json** opens the editable configuration. Saving applies changes;
external edits reload on focus. Comments/trailing commas are accepted; unknown
keys are reported and invalid files are never overwritten. Unset formatting
preserves file style; project settings and `.editorconfig` override editor defaults.

| Platform | Configuration folder |
| --- | --- |
| Linux | `~/.config/hedit/` |
| macOS | `~/Library/Application Support/hedit.hedit/` |
| Windows | `%APPDATA%\hedit\hedit\config\` |

Override with `HEDIT_CONFIG_DIR`. The folder also holds `keybindings.json`,
`snippets.json`, `macros.json`, and `themes/`. Use the settings pages to discover
options/defaults; JSON also supports `syntax_specific` formatting overrides,
such as `{"python": {"tab_size": 4}}`, and custom `build_systems`.

### Extensions

Extensions can add snippet commands to the Command Palette. To try the
experimental extension API, copy [markdown-task.json](examples/extensions/markdown-task.json)
into `extensions/` inside your configuration folder. In the bundled guide, use
**Copy code** and save the contents as `markdown-task.json` in that folder.
Restart hedit, open the Command Palette, and choose **Insert Markdown task** in
a writable editor with one caret or selection. Type over the prompt and press
**Tab** to finish the snippet. Remove the file and restart to uninstall it.

The [extension manifest reference](docs/extensions.md) describes the fields and limits.

## Appearance

**Preferences → Color theme…** previews built-in/custom themes; Enter keeps one,
Esc restores the previous choice. **Settings → Appearance** has a gallery and
system light/dark following with a separate theme for each. Hedit Ink and Hedit
Paper are defaults; [palette credits](assets/licenses/theme-palettes.md) cover
additional themes.

**Customize colors** previews interface/syntax/status colors and contrast.
**Save as new theme…** keeps a copy; Import accepts hedit JSON and TextMate
`.tmTheme`; Export saves current colors. Themes live in the configuration's
`themes/` folder (or `HEDIT_THEMES_DIR`) and can inherit a built-in theme:

```json
{"version": 2, "name": "Violet Ink", "base": "Hedit Ink", "accent": "#B4A0F5"}
```

## CUDA tools

CUDA highlighting works everywhere. Linux **Tools → CUDA panel** uses separately
installed `cuda-gdb`, `compute-sanitizer`, and `nvidia-smi`. Set the debugger path
in Settings. Build with debug symbols, e.g.
`nvcc -g -G examples/cuda/vector_add.cu -o /tmp/vector-add`.

Choose Executable and Args (a JSON list), Start CUDA-GDB, and set function or
`file.cu:line` breakpoints. Run/Continue/Pause/Step/Next/Finish control execution;
Stack/Locals/Devices/Kernels/Threads/Warps and the command console inspect state.
Run sanitizer selects memcheck/racecheck/initcheck/synccheck; GPU info reports
device status. See NVIDIA's [CUDA-GDB](https://docs.nvidia.com/cuda/cuda-gdb/index.html)
and [Compute Sanitizer](https://docs.nvidia.com/compute-sanitizer/ComputeSanitizer/index.html)
references.

## Command line

```text
hedit [--session-dir DIR] [--new] [-w] [FILE[:LINE[:COLUMN]] | FOLDER ...] [-]
```

| Argument | Effect |
| --- | --- |
| File/folder | Open, or create a file on first save; `:LINE[:COLUMN]` sets the location |
| `-` | Read stdin into an unsaved file |
| `--new` | Add an empty note |
| `-w`, `--wait` | Wait until files close; use `git config --global core.editor "hedit -w"` |
| `--session-dir DIR` | Separate session/window; default from `HEDIT_SESSION_DIR` |
| `--lsp list`, `--lsp install ID`, `--lsp remove ID` | Manage local language servers without a window; optional `--lsp-dir DIR` selects another server store |
| `--` | Remaining arguments are filenames |
| `-h`, `--version` | Help/version |

Launches return after window readiness; existing sessions receive files in their
first window. Wayland focus uses the launching terminal's activation token;
without one, the desktop decides whether to focus or show an attention notice.
For remote flags, see [SSH workspaces](#ssh-remote-workspaces).

## Keyboard shortcuts


Choose **Settings → Key Bindings → Shortcut preset** for Hedit or **VS Code ·
common commands** (`keybinding_preset`: `hedit` or `vscode`). Existing settings keep
Hedit. The optional preset maps sidebar visibility, build, symbols, Problems,
Output, splitting, group focus and document formatting to the corresponding
[VS Code shortcuts](https://code.visualstudio.com/docs/reference/default-keybindings),
including platform differences. Other commands retain Hedit bindings. Personal
bindings take priority; conflicting default shortcuts are suppressed and shown
as unbound. Menus, the palette and tooltips show effective shortcuts. Reset restores
the selected preset; switching presets preserves `keybindings.json`. Shell focus
keeps editor shortcuts with the terminal, and editor-only chords stay out of Settings.

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
| Select line / duplicate / delete | Ctrl+L / Ctrl+Shift+D / Ctrl+Shift+K | Cmd+L / Cmd+Shift+D / Cmd+Shift+K |
| Move lines up / down | Ctrl+Shift+Up / Down | Cmd+Shift+Up / Down |
| Insert line below / above | Ctrl+Enter / Ctrl+Shift+Enter | Cmd+Enter / Cmd+Shift+Enter |
| Join lines / indent / unindent | Ctrl+J / Ctrl+] / Ctrl+[ | Cmd+J / Cmd+] / Cmd+[ |
| Uppercase / lowercase | Ctrl+K, Ctrl+U / Ctrl+K, Ctrl+L | Cmd+K, Cmd+U / Cmd+K, Cmd+L |
| Sort lines / case-sensitive sort | F9 / Ctrl+F9 | F9 / Cmd+F9 |
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

Recovery checkpoints preserve open files, unsaved notes, and fully loaded undo
history separately from source files. They run every few seconds; the last edits
before power loss can be lost. Save important work. The status dot reports
Pending/Checkpointing/Up to date. If it fails, keep hedit open, fix disk space or
permissions, then **Retry checkpoint**. Closing waits for a final checkpoint.
[SSH recovery](#privacy-and-recovery) lives on the host instead.

For missing work, check tabs, Ctrl+P, Closed Files, and the session you launched.
Unreadable latest recovery falls back to the previous copy; `preserved-*` data is
retained. Damaged undo history can be dropped with a warning while retaining text.

| Platform | Default local session |
| --- | --- |
| Linux | `~/.local/share/hedit/session/` (`$XDG_DATA_HOME/hedit/session`) |
| macOS | `~/Library/Application Support/hedit.hedit/session/` |
| Windows | `%LOCALAPPDATA%\hedit\hedit\data\session\` |

Close hedit and copy the whole folder before backup/manual rescue. Small buffers
are UTF-8 files under `buffers-*`, indexed by `session.json`. Recovery is not encrypted.

## Troubleshooting

| Problem | Action |
| --- | --- |
| No window | Check graphics support, terminal startup errors, and `hedit.log` in the session folder |
| Session locked | Switch to its open window or use another `--session-dir` |
| Recovery fails | Keep hedit open; check space/permissions and Retry checkpoint |
| Outside change on save | Compare changes or Save As to retain your version |
| Missing Linux file dialog | Run `xdg-desktop-portal` and your desktop's portal |
| Missing glyphs | Install fonts for the script, e.g. `fonts-noto` |
| File rejected | Binary/unsupported encoding; try File encoding |
| Update fails | Read the error; check network/write permissions, reinstall, or rebuild source |
| Terminal unavailable | Enable it in Settings; builds without the feature explain this there |
| Shell exits/won't start | Hover its tab/read the error, check starting directory, then Restart shell |
| Terminal RTL reordered twice | Disable terminal bidirectional shaping |
| SSH fails | Check pinned host key, unlocked agent, helper version, explicit port, and private durable state storage |
| CUDA tool unavailable | Install the tool, check PATH, and read its output |

### Tabs in editor groups

Each editor group has its own tab strip. Opening a file adds it to the focused
group; the same file can appear in several groups with independent cursor and
scroll positions. Edits and undo belong to the shared document. Tab position,
next/previous tab, Close others, Close to the right and Close all apply to the
focused group. Drag a tab to another group’s strip or editor, or use Move file
to other group. Closing a shared tab removes that view; the final dirty tab
asks before discarding. Empty groups close automatically. Reducing the layout
merges remaining tabs, and recovery retains their documents and view metadata.
Git reviews use a reusable tab in one group while other groups remain editable.
Paged files share the currently loaded page across their groups.

### Breadcrumbs and Outline

**View → Editor → Show breadcrumbs** displays the file location above each
group. Click a segment to reveal it in an open folder’s Files tree; hover for
the full path. SSH breadcrumbs include the host and relative file path.
**Show Outline** opens a filterable list of functions and types for the active
file. Click a symbol to navigate; Escape in its filter returns typing to the
editor. Outline is off by default, scans only while visible, and shows up to
500 symbols for files up to 2 MiB. Use Go to symbol for larger files. Both
visibility preferences are also available in Settings → General.

### Explorer interaction

**View → Side bar → Focus Explorer** gives the folder tree keyboard focus. The
optional VS Code preset uses Ctrl+Shift+E (Cmd+Shift+E on macOS). Arrows expand,
collapse and navigate; Home/End and Page Up/Down move the selection. Type a name
to jump to a matching row, Enter opens it, F2 renames it inline, Delete opens the
existing trash confirmation, and Escape returns to the editor. Tab leaves the
roving tree selection. Personal global bindings retain priority.

Right-click a folder for inline **New file…** or **New folder…**; rename is inline
too. Enter or the action button submits the name, and Escape cancels before the
operation starts. Invalid names and filesystem conflicts keep the draft available
to correct. Renaming an open dirty file retargets it without saving its text.

Settings → Folders & Search controls automatic active-file reveal, compact
single-child folder chains, Git badges and diagnostic counts. Automatic reveal
runs when Files is visible and leaves Search/Source Control inputs intact. In a
compact chain, F2 renames the final segment. Badges are cached and computed in
background/on-demand work; showing diagnostic badges never starts a language
server. Git refresh follows filesystem and Git-metadata events, explicit refresh,
and refocusing; SSH Git badges require execution trust and refresh on focus or
explicit refresh. Decoration budgets bound large repositories; hover Refresh
folders for status. Unsaved editor text is independent of disk Git status.

### Filtering Problems

The Problems panel combines build results and language-server diagnostics in one
list. Filter by message, filename or source, select a severity or source, and use
Current file to follow the active editor. Group by file adds location headers.
Counts show the filtered total and overall severity totals. Build messages with
no explicit error or warning prefix appear as information.

Next diagnostic and Previous diagnostic follow the current filters and wrap at
the ends. The optional VS Code shortcut preset binds them to F8 and Shift+F8.
These commands preserve the Problems panel and reject stale language positions.
Explorer diagnostic badges include build results as well as language diagnostics.
The combined catalogue is limited to 10,000 entries and 8 MiB.

### Rename Symbol and Quick Fixes

Tools → Language server includes Rename symbol and Quick fixes. The optional
VS Code shortcut preset uses F2 and Primary+Period. In Explorer, F2 still renames
the selected file. Each server's Configure page can enable or disable these
features independently.

Rename requests a new name and opens a preview. Quick fixes lists available
text edits; choose one to review it. Inspect each file's Before and After text,
then choose Apply all text changes. The preview shows the first 500 characters
of long lines. Changes stay unsaved, and each changed file has its own normal
Undo history and session recovery. Opening a preview never changes file text.

The editor rejects stale previews, stale versions, overlapping edits and paths
outside the project root. File resource operations and server commands are
unavailable. A batch supports up to 64 complete files, 2 MiB per file and 8 MiB of
combined source and proposed text. Local batches preflight every source and
commit no edits if preparation fails. SSH batches preflight every source too;
if a later host edit fails, the editor reports the completed files and keeps
their text and Undo history visible. Files are never saved automatically.

### Panel and focus controls

View → Panel includes Toggle last used panel and Maximize or restore panel.
Closing or toggling the panel preserves its selected tab, output and shell.
Maximize expands the panel while remembering its normal height; Restore returns
to that height. These layout choices are remembered for the current window.
The VS Code shortcut preset uses Primary+J to toggle the panel.

Focus editor, Focus Explorer, Focus Search and Focus terminal are available in
the command palette and custom keybindings. Primary+1 through Primary+4 in the
VS Code preset move from the terminal to an editor group. Sidebar focus leaves
the panel and active document intact. Focusing a terminal shows its existing
session, or starts one when explicitly requested and enabled. A disabled terminal
never starts when restoring the last panel; Problems is shown instead.

### Notifications and progress

Use Notifications in the status bar, View → Panel, or the command palette to
review recent messages. Messages do not take keyboard focus. The window keeps
up to 128 entries and 256 KiB of history, combines consecutive repeated messages,
and shows unread counts. Dismiss individual entries or clear the history; this
does not dismiss recovery errors or cancel running work. History lasts for the
current window and is not written to disk.

Failed saves provide Retry save when the original document is still open at the
same path. Save As retries ask for a destination again. Build completions link to
Output; language and Git failures link to their existing management/log views.
Progress controls can cancel builds, folder searches and language-server work
cooperatively. Cancelling server work clears stale diagnostics and stops owned
servers; Refresh in Language servers resumes them. Generic file-writing jobs
are not cancelled by notification controls.
