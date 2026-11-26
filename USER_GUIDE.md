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

| Installation | How to update |
| --- | --- |
| Writable Linux tarball install | **Help → Check for updates…**. Requires `curl` and GNU `tar`. |
| Writable macOS app | **Help → Check for updates…**. Keep custom themes outside the signed bundle. |
| Linux DEB / RPM | Update through the package manager. |
| Linux AppImage | Replace the AppImage. |
| System-owned tarball or source build | Rerun the installer or rebuild the checkout. |
| Windows portable | Close hedit and [replace the ZIP](#windows-installation). |

The in-app updater runs on request. It verifies checksums, the candidate version,
and the macOS signature. An installation failure restores the previous version.
Choose **Restart hedit** after installation or **Later** to keep working.

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

| What you see | What it means |
| --- | --- |
| Dot on a tab | The file has unsaved changes. |
| **Cancel opening** | Cancels a background open. Esc also works after dismissing overlays. Typing or switching tabs while it loads keeps it from taking focus. |
| **Save**, **Don't Save**, **Cancel** | Closing a modified tab asks what to do. Don't Save reopens a disk file from disk. Untitled text stays in **Closed Files**. |
| **Closed Files** | Keeps about 50 entries and retains unsaved notes beyond that limit. Empty untitled notes are discarded. |

Quitting keeps tabs and edits in recovery. A failed checkpoint keeps the window
open. Windows share settings, key bindings, snippets, macros, and themes.

| Project or file setting | Where |
| --- | --- |
| Folder list and formatting | **Project → Save project as…** writes `.hedit-project`. Open it from Project or the CLI. |
| Personal tabs, unsaved text, layout | The adjacent `.hedit-workspace`. Exclude it from version control. |
| Defaults and language overrides | **Project settings…** offers GUI and JSON views. **Save settings** applies a valid draft. `.editorconfig` can override project settings. |
| Encoding | UTF-8 and BOM-marked UTF-16 open directly. **File → File encoding…** reopens or saves other supported encodings. Unrepresentable text and binary files are refused. |
| Line endings | **Edit → Line endings** converts LF and CRLF. |

### Explorer interaction

Choose **View → Side bar → Focus Explorer**. The VS Code preset uses
**Ctrl+Shift+E** (**Cmd+Shift+E** on macOS). Personal global bindings take priority.

| Key | In Explorer |
| --- | --- |
| Arrows | Navigate, expand, or collapse folders. |
| Home, End, Page Up, Page Down | Move the selection. |
| Type a name | Jump to a matching row. |
| Enter | Open the selected entry. |
| F2 | Rename inline. For a compact chain, rename the last folder. |
| Delete | Open the trash confirmation. |
| Tab / Escape | Leave tree selection / return to the editor. |

Right-click a folder for **New file…** or **New folder…**. Enter submits and
Escape cancels before the operation starts. Invalid names and conflicts keep
the draft editable. Renaming a modified file keeps its unsaved text.

Built-in file and folder icons work in both local and SSH projects.
**Settings → Folders & Search** controls exclusions, automatic reveal, compact
folders, Git badges, and diagnostic counts. Reveal runs while Files is visible.
Diagnostic badges use existing results. Local Git badges follow metadata events
and refreshes. SSH badges require execution trust and refresh on focus or
**Refresh folders**. Hover that control for coverage or indexing limits.
Filename indexing skips directory symlinks. Git badges describe disk state.

## SSH remote workspaces

SSH support is experimental. hedit opens a separate local window. The host
handles files, search, recovery, and any trusted Git, build, or shell commands.

### Connect

Choose **Connect via SSH…** in the Command Palette or **+ Connect…** in
Remote Explorer. Search for a hostname, SSH alias, `user@host`, or saved path.
Use the arrow keys and **Enter** to connect. A host opens its home directory.
A saved workspace opens its saved folder.

<!-- docs-screenshot docs/images/ssh.png | Remote Explorer with SSH hosts and saved workspaces -->

| In Remote Explorer | Use it to |
| --- | --- |
| **Import SSH hosts…** | Show all aliases with **Sync all hosts** (default), or keep a list with **Import selected**. The picker can find every discovered alias. |
| **Reopen last workspace** | Restore tabs and the active file from the last successful host session. |
| **Choose remote folder…** | Preview directories before opening a workspace or acquiring a writer. Each workspace has its own recovery directory. |
| **Connection settings…** | Choose a workspace and override the user, port, trusted config, or private recovery directory. Manual helpers need absolute helper and recovery paths. |
| **SSH configuration…** | Open or refresh SSH config. hedit reads user, system, and included config for aliases. OpenSSH resolves the options at connection time. |

Use trusted SSH config files. Alias discovery skips wildcard and negated entries
and reads configuration without executing `Match exec`. Saved aliases follow
changes to SSH config. Removing an alias hides it until it returns.

Verify the host key with OpenSSH first. Automatic setup transfers the
matching helper to a non-root account with safe directory permissions. hedit
uses its bundled helper when possible. Otherwise it downloads the exact editor
version and verifies SHA-256 locally before transfer. Future connections reuse
the versioned user-local helper. The host needs SSH and compatible storage.
The client handles downloads and compilation is unnecessary. Setup progress
and cancellation appear in the sidebar.

| Connection option | Behavior |
| --- | --- |
| Default authentication | OpenSSH keys or an existing agent. Saved metadata contains host names, paths, and preferences. |
| **Allow interactive SSH authentication** | Linux and macOS clients can prompt for a password, key passphrase, or verification code. Separate SSH connections may prompt again. Use **Reconnect** to retry interactively. Scheduled retries use keys or an agent. |
| **Use SSH config jump hosts** | Linux and macOS clients can use `ProxyJump`. Pin every hop and destination key. CLI flag: `--ssh-use-config-jump`. |
| **Forward SSH config ports to localhost** | Linux and macOS clients can use TCP `LocalForward` for this connection. Use a numeric port or loopback bind, such as `localhost:8080`. Remote Explorer shows active ports and reports conflicts. CLI flag: `--ssh-forward-config-ports`. |

```sh
hedit --ssh devbox
hedit --ssh devbox --ssh-root /home/me/project src/main.rs
```

| CLI flags | Use |
| --- | --- |
| `--ssh-user USER`, `--ssh-port PORT`, `--ssh-config /absolute/config` | Override connection settings. |
| `--ssh-interactive` | Allow interactive authentication on Linux or macOS clients. |
| `--ssh-state` | Keep a private recovery path while managing the helper automatically. |
| `--ssh-helper`, `--ssh-state`, `--ssh-root` | Use all three for a manually installed helper. Set a new recovery path before opening a different workspace. |
| `--ssh-takeover` | Fence the previous writer. Each session retains its own edits. |

| In the remote window | What happens |
| --- | --- |
| **File → Open folder…** | Browse with arrows and **Enter**, or enter a relative path. **Up** stays inside the workspace. **Add Folder** adds it to Files using the same connection and recovery directory. **Tab** reaches the buttons. |
| New file or **Save As** | New files start as recoverable host scratchpads. **Save As** refuses existing targets. Keep at least one remote tab open. |
| **Trust for Git, builds and terminal…** | Allows scripts, hooks, and shells to execute as the SSH account. Revocation stops tracked jobs. Reconnect clears trust. Enable the terminal separately. |
| **Show SSH Output** | Inspect setup and connection events. Retry a launch, reconnect, or open local SSH config. **Copy SSH summary** copies up to 64 fixed event descriptions without host names, paths, raw SSH messages, or credentials. |

### Privacy and recovery

| Status or action | What it means |
| --- | --- |
| **Durable on SSH host** | The host journal has committed the edits. **Save** writes the source file. |
| Connection lost | Pending edits stay in memory. Editing pauses until reconnect reconciles receipts. Transport failures trigger up to five automatic retries with increasing delays. |
| Retries stop | Use **Reconnect**. A changed recovery session or reconciliation conflict stops retries. Automatic retries never take over a writer or repeat an uncertain save. |
| **Disconnect** | Stops retries and keeps pending edits. **Reconnect** resumes the same host recovery session. |
| Save or storage failure | Text stays in the editor. Outside changes, stale writers, lag, quota, and storage errors can pause edits. Try **Save As** if needed. |
| Client crash | Edits without a durable receipt can be lost. Recovery lives on the host. |

Remote drafts, content, undo, searches, patches, and terminal output stay out
of local recovery. Local launch diagnostics may retain host names and paths.
Trust the host with recovery data. OS swap, crash collectors, clipboard
managers, screenshots, and exports are outside the application boundary.

Keep state and SQLite sidecars together. Leave the live state directory alone.
Closing ends tracked tools. Idle workers leave recovery in place and exit
after five minutes.

### Limits

| Area | Limit |
| --- | --- |
| Platforms | Linux and macOS hosts. Linux, macOS, and Windows clients. Jump hosts and localhost TCP forwarding require a Linux or macOS client and opt-in. |
| SSH routes | Supported jump routes use `ProxyJump`. Arbitrary `ProxyCommand`, reverse forwarding, and dynamic forwarding are unavailable. |
| Storage | Private host-local ext-family, XFS, btrfs, ZFS, or APFS. Ancestor permissions are checked. Network filesystems and tmpfs are unsupported. |
| Recovery | Up to 128 retained documents. Each can hold up to 8 MiB of decoded text. Recovery and undo capacity are bounded. |
| Files | Browsing skips symlinks and non-UTF-8 names. Directory duplication and cross-filesystem trash moves are unavailable. |
| Editor tools | Trusted host language servers provide navigation in the workspace. Remote project persistence, Markdown preview, CUDA/debugging, compare-disk, and mini-diff are unavailable. |

See the [developer guide](DEVELOPER_GUIDE.md#ssh-architecture-and-testing) for helper details.

## Editing

Use **Edit** or right-click the text for editing commands. Undo and redo survive
restarts for fully loaded files.

| Task | Shortcut or menu |
| --- | --- |
| Copy or cut a line | Use Copy or Cut with no selection. Pasting a whole line inserts it above the current line. |
| Paste with indentation | **Ctrl+Shift+V** |
| Reuse a clipboard entry | **Ctrl+K, Ctrl+V** shows the last 15 clips. |
| Edit lines | Use **Edit** to duplicate, delete, move, join, comment, indent, or insert lines. With no selection, the command acts on the current line. |
| Clean up text | **Edit** can sort, reverse, deduplicate, change case, trim, reindent, or convert tabs and spaces. Whole-file cleanup applies when there is no selection. |
| Set indentation | **Edit → Indentation** sets width and spaces or tabs. It can also detect the style. Enter follows the style and indents after brackets. |

| Selection task | Shortcut |
| --- | --- |
| Add or skip the next occurrence | **Ctrl+D** / **Ctrl+K, Ctrl+D** |
| Select all occurrences | **Alt+F3** |
| Add more cursors | **Alt+click** or **Alt+Shift+Up/Down** |
| Select a rectangle | **Alt+Shift+drag** |
| Split a selection by line | **Ctrl+Shift+L** |
| Extend or expand | **Shift+click** extends. **Ctrl+Shift+Space** expands through words, quotes, brackets, lines, and the file. |
| Select brackets or matching indentation | **Ctrl+Shift+M** / **Ctrl+Shift+J** |
| Grow an HTML/XML selection | **Ctrl+Shift+A** expands through content, tags, and parent elements. It uses explicit balanced tags, including void and self-closing tags. Inferred tags and JSX are unsupported. |
| Undo selection movement | **Ctrl+U** |
| Return to one cursor | **Esc** after closing panels and dialogs. |

An edit can use up to 1,000 cursors and takes one Undo step. Typing an opening
bracket wraps a selection or inserts its closer. Backspace removes an empty
pair. **Ctrl+M** jumps to the matching bracket.

| Tool | Controls |
| --- | --- |
| Macros | **Ctrl+Alt+Q** starts or stops recording. **Ctrl+Alt+Shift+Q** replays. **Tools → Save macro… / Run saved macro…** keeps named macros. Recording follows typing and keys. |
| Spell check | **F6** checks English prose and code comments or strings. Right-click for suggestions, dictionary additions, or Ignore. **Ctrl+F6** jumps to the next misspelling. Code-like words are skipped. |

Arabic and Hebrew use joined letters and visual caret movement. File order
stays logical. Prose follows its first letter and code stays left-to-right.
Install script fonts if glyphs are missing.

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

<!-- docs-screenshot docs/images/search.png | Search results grouped by file in the sidebar -->

Searches run in the background. New input takes priority. Filename indexes
stay in memory. Notices report incomplete results and unreadable folders.

Enable **Settings → General → Automatically refresh workspace** to watch local
folders and open-file parents. The watcher covers up to 1,024 directories.
It skips exclusions and directory symlinks. Its scan stops after two seconds
or at the path-memory limit. Hover **Refresh folders** for coverage. Refocus
or refresh to update paths outside coverage. Use explicit refresh over SSH.

External changes reload a clean active file. A conflict banner protects
unsaved text and guards the next save.

### Breadcrumbs and Outline

| **View → Editor** | What it does |
| --- | --- |
| **Show breadcrumbs** | Shows the path above each group. Click a segment to reveal it in Files. Hover for the full path. SSH shows the host and relative path. |
| **Show Outline** | Shows functions and types in the active file. Filter and click to jump. Escape returns to editing. |

Outline is off by default. It scans while visible and lists up to 500 symbols
in files up to 2 MiB. Use **Go to symbol** for larger files. Both switches also
appear in **Settings → General**.

## Writing code

| Task | Where to go |
| --- | --- |
| Complete text | **Ctrl+Space** or completions as you type. Choose with Up/Down, accept with Tab or Enter, and dismiss with Esc. |
| Insert a snippet | Type its trigger and press Tab, or use **Tools → Insert snippet…**. Change snippets in **Settings → Snippets**. |
| Inspect a change | Hover a gutter marker. Use **Ctrl+K, Ctrl+.** or **Ctrl+K, Ctrl+,** to move between changes. **Ctrl+K, Ctrl+Z** reverts a change with Undo. |
| Build | **Ctrl+B** saves and builds. **Ctrl+Shift+B** picks a system. Click a diagnostic or use **F4 / Shift+F4**. |

Completion uses language servers, matching snippets, and words from fully
loaded open files. Background indexing prioritizes the active file. Set
`auto_complete_commit_on_tab` to keep Enter for new lines.

| Snippet syntax | Effect |
| --- | --- |
| `$1`, `${1:default}`, `$2` | Fields. Repeated numbers mirror edits. Use Tab or Shift+Tab to move between fields. |
| `$0` | Final cursor |
| `${SELECTION}` | Selected text |
| `\$`, `\}` | Literal characters |

Change markers compare with Git HEAD or the saved file outside Git
(`mini_diff`). Use **Tools → Compare file with Git HEAD…** to compare editor
text. Stage changes in [Source Control](#source-control).

Built-in build systems cover CUDA, C/C++, Rust, Python, and Make. Install the
tools separately. Builds use `sh` on Linux/macOS and `cmd.exe` on Windows.
A custom `build_systems` entry accepts `name`, `shell_cmd`, `selector`, and
`file_regex` (file, line, optional column, and message groups).
Commands expand `$file`, `$file_path`, `$file_name`, `$file_base_name`,
`$file_extension`, and `$folder`. A matching name overrides a built-in.

### Language servers

Open **Settings → Language Servers**, **Preferences → Language servers: manage…**,
or **LSP** in the status bar. Use **Refresh** to update logs. Managed downloads
start when you choose **Install**.

| Install choice | Servers and setup |
| --- | --- |
| Managed | rust-analyzer, clangd, ty, Ruff, LuaLS, Tombi. Install, repair, or remove in Settings. Upstream GitHub releases need a SHA-256 digest. Downloads need `curl`. Updates are manual. |
| External preset | gopls, TypeScript/JavaScript, HTML, CSS, JSON, YAML, shell, pylsp, Taplo. **Configure → Detect installed on PATH** finds an installed executable without running or installing it. |
| Other external server | **Add external server…**. Supply an ID, comma-separated syntax IDs, an absolute executable, arguments as a JSON array, and settings as a JSON object. Install its runtime separately. |

Rust projects need a Rust toolchain and `rust-src` for standard-library sources.
C/C++ projects can use `compile_commands.json`. Use an external server if its
platform has no managed build. A pylsp registration can use
`/absolute/path/to/pylsp` and `[]`. For npm wrappers on Windows, point to
`node.exe` and put the script and `--stdio` in the arguments. Removing a
registration preserves external executables. Use **Configure** to override a
built-in.

| In the server manager | What it changes |
| --- | --- |
| **Use default**, **Enabled**, **Disabled** | Workspace override. Installation, enablement, and running state are separate. |
| **Configure** | Feature switches, priority, arguments, environment strings, root markers, and initialization options. Root discovery stays within the workspace. |
| Initialization options | `{}` sends no extra options. A blank field also sends runtime settings at initialization. Some presets supply a namespace, such as LuaLS `{"Lua":{"runtime":{"version":"LuaJIT"}}}`. |
| **Stop** / **Restart** | Stop for this window or start again. Repeated crashes need an explicit restart. Stop the server in other hedit windows before changing a shared install or registration. |

| Editor action | Where |
| --- | --- |
| Completion | **Ctrl+Space** or automatic suggestions. Supports plain text and additional text edits. |
| Definitions and references | **F12 / Shift+F12**. Local indexing can answer when a server cannot. |
| Hover, format, problems, restart | Tools menu or Command Palette. Choose a diagnostic from Problems to jump to it. |
| Formatting or completion edits | Apply to unsaved text and support Undo and recovery. The editor drops results after relevant context changes. |

Servers start for saved, fully loaded files up to 4 MiB. A workspace synchronizes
up to 32 documents totaling up to 32 MiB and runs up to eight server sessions. SSH uses a
4 MiB total snapshot budget. Paged logs, follow mode, and untitled notes skip
servers. Active files have priority. For each action, the highest-priority
eligible server wins. Server ID breaks ties. Diagnostics are combined and
labeled. Python defaults use ty for intelligence and Ruff for linting/formatting.
Semantic highlighting, server snippets or commands, and unsolicited workspace
edits are unsupported.

In SSH workspaces, install the matching helper. The manager targets the host
and requires execution trust. Revocation or disconnect stops its workers.
The servers live under the helper state's `language-servers` directory.
Executables and runtimes run on the host. Client snapshots, logs, and diagnostics
stay in memory. Navigation stays inside the remote workspace.

```sh
hedit --lsp list
hedit --lsp install rust-analyzer
hedit --lsp remove rust-analyzer
hedit --lsp-dir /absolute/private/server-store --lsp list
```

`list` returns the catalog and registrations as JSON. `install` resolves the
latest stable release and records its version, hash, and source. Temporary
directories and a bounded version check protect the active installation before
atomic activation. If a receipt is uncertain, hedit keeps the package files.
Refresh to inspect the active version. The previous managed package stays
until the next successful update or removal. `remove` deletes only the
registration and hedit-owned packages.

#### Rename Symbol and Quick Fixes

| Command | What to do |
| --- | --- |
| **Tools → Language server → Rename symbol** | Enter a new name and review its edit preview. The VS Code preset uses **F2**. In Explorer, **F2** renames the file. |
| **Tools → Language server → Quick fixes** | Choose an offered text edit and review it. The VS Code preset uses **Primary+Period**. |

Compare **Before** and **After**, then choose **Apply all text changes**. The
preview shows up to 500 characters per line. Changes stay unsaved. Each file has
its own Undo and recovery. Enable these features per server in **Configure**.

Batches accept up to 64 complete files, 2 MiB per file, and 8 MiB of combined
source and proposed text. The editor rejects stale previews, overlapping edits,
paths outside the root, file operations, and server commands. Local batches
apply only after every file passes preflight. SSH also preflights all files.
If a later host edit fails, completed files and their Undo remain visible.

### Syntax highlighting

Click the status-bar language to choose a syntax or return to **Automatic**.
The 58 formats include common languages, CUDA, configuration files, Markdown,
LaTeX, CSV, and logs. Definitions support embedded languages and multiline
strings. Large files show quick colors before full highlighting.

## Bottom panel

| Bottom panel tab | What it shows |
| --- | --- |
| **Problems** | Build locations and language-server diagnostics. Click a row to jump. |
| **Output** | The latest build and its Cancel action. |
| **Terminal** | The running shell. Escape goes to the shell when focused. |

Drag the top edge to resize. Close the panel to hide it while work continues.
Escape also hides Problems or Output.

### Filtering Problems

| Control | Effect |
| --- | --- |
| Text filter | Matches message, filename, or source. |
| Severity and source | Narrows the list. Build messages without a severity prefix are informational. |
| **Current file** | Follows the active editor. |
| **Group by file** | Adds file headers. |
| **Next diagnostic / Previous diagnostic** | Follows active filters and wraps. The VS Code preset uses **F8 / Shift+F8**. Stale language positions are rejected. |

Counts show filtered results and overall severity totals. Explorer badges
include build and language diagnostics. The catalogue holds up to 10,000
entries and 8 MiB.

### Panel and focus controls

| Action | Where |
| --- | --- |
| **Toggle last used panel** | **View → Panel**. The VS Code preset uses **Primary+J**. |
| **Maximize or restore panel** | **View → Panel**. Restore returns to the remembered height. |
| **Focus editor**, **Focus Explorer**, **Focus Search**, **Focus terminal** | Command Palette or your own key bindings. The VS Code preset uses **Primary+1–4** to focus editor groups. |

Hiding the panel preserves the selected tab, output, and shell. Sidebar focus
keeps the current document and panel. Explicit terminal focus can start a
session when enabled. Restoring a disabled terminal shows Problems.

### Notifications and progress

Open **Notifications** from the status bar, **View → Panel**, or the Command
Palette. History stays in this window. It combines consecutive repeats and
keeps up to 128 entries or 256 KiB. Dismiss entries without interrupting work.

| Message or progress | Action |
| --- | --- |
| Failed save | **Retry save** while the same document and path are open. Retrying **Save As** asks for a destination. |
| Build completed | Open **Output**. |
| Language or Git failure | Open its manager or log. |
| Running build, search, or language job | Cancel cooperatively. Cancelling language work clears stale diagnostics and stops owned servers. Refresh in Language Servers to resume. |

Recovery errors and file-writing jobs have their own controls.

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
| Panel close button / toggle shortcut | Hide panel while shells keep running |

<!-- docs-screenshot docs/images/terminal.png | Integrated terminal below a Rust project -->

These keys apply while terminal-focused; macOS uses Cmd for new/close, Control
for tab switching. **Ctrl+C** interrupts; copy/paste uses **Ctrl+Shift+C/V**
(**Cmd+C/V** on macOS). Esc, Tab, arrows, and Control keys go to the shell; click
the editor to resume editing. The command palette remains available.

| Behavior | What to expect |
| --- | --- |
| Paste | Bracketed paste where supported. Limit 256 KiB. |
| Scrollback | Up to 10,000 lines. Hidden tabs still receive output. |
| Titles | Follow the shell by default. **Use shell title** clears a custom title. |
| Stop / Restart | Stop keeps output. Restart clears it. |
| Disable terminal or quit | Ends shells. Sessions and output are discarded. |
| RTL shaping | Changes display. Disable it if a terminal program already reorders text. |
| Graphics or mouse-event forwarding | Unsupported. |

**Settings → Terminal** can use the app font and colors or set them separately.
The app appearance is the default. Terminal font size ranges from 8–40 pt.
Change the terminal colors or a 16-color ANSI palette without restarting local
or SSH shells. A terminal program can still set its own colors.

| `settings.json` key | Example or fields |
| --- | --- |
| `terminal_font` | `{"face":"","size":18}`. An empty face uses the built-in font. A family or font-file path also works. |
| `terminal_colors` | Six-digit hex values for `foreground`, `background`, `cursor`, `selection`, and `selection_text`. Optional `ansi` array of 16 colors. |

Leave the keys out or use `null` to follow the app appearance.

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

<!-- docs-screenshot docs/images/git.png | Source Control with a syntax-colored file diff -->

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

Choose **Source**, **Split**, or **Preview** in **View → Markdown**. Split follows
unsaved source and scrolls with it. Narrow windows stack the panes. Drag the
divider, use its arrow keys, or double-click to restore equal sizes. Choose
Markdown syntax for an untitled note. The mode survives a restart.

<!-- docs-screenshot docs/images/markdown.png | Markdown source beside rendered tables, an equation, and a diagram -->

| Task | Use |
| --- | --- |
| Jump to a heading | **Contents** |
| Copy text or code | Select it or use **Copy** on a code block. |
| Follow a link | Section links stay in the document. Markdown links open in Preview. Other files open in the editor. Right-click to copy the address. |
| Show local images | PNG or JPEG next to the file. **Save As** changes the base path. Remote images, `file://`, and Windows network/device image paths are blocked. |
| Add an equation | Use `$E = mc^2$` inline or `$$ … $$` and fenced `math` for display. Escape a dollar as `\$`. |
| Add a diagram | Use a fenced `mermaid` block. |
| Refresh media | Click **Refresh** or return from Source. |

Tables, lists, tasks, quotes, and GitHub alerts render natively. HTML comments
are hidden. `<br>` adds a line break. Other HTML stays text. Find, Replace,
Go to line, and search results show source.

| Limit | Maximum |
| --- | --- |
| Fully loaded Markdown file | 2 MiB |
| Equation | 16 KiB |
| Diagram | 64 KiB, 1,000 lines, 256 nodes, 1,024 edges |

Equations and diagrams render locally. Unsupported input stays as source with
a hover error and **Copy source**.

## Views and layout

| Feature | Usage |
| --- | --- |
| Panes | **Alt+Shift+1–4**: columns; **Alt+Shift+5**: four-pane grid; **Alt+Shift+8/9**: two/three rows. **Ctrl+1–4** focuses a group; **Ctrl+Alt+Shift+Right** moves a file. Shared files keep separate cursor/scroll positions |
| Folding | Gutter arrows or **Ctrl+Shift+[/]** fold/unfold. **View → Code folding** includes nesting levels and Fold all. **Ctrl+K, Ctrl+1/2/3** folds a level; **Ctrl+K, Ctrl+J** unfolds all |
| Wrap/rulers | **Ctrl+Shift+W** toggles wrapping; View sets wrap column/rulers (up to eight) |
| Focus | **F11** full screen; **Shift+F11** distraction-free; independent modes |
| Font | **Preferences → Editor font…** filters installed fonts by family/script. **Ctrl++ / Ctrl+- / Ctrl+0** changes/resets size |
| Editor details | **View → Editor** toggles numbers, current-line highlight, whitespace, guides, minimap, tabs, and status bar |

<!-- docs-screenshot docs/images/split-panes.png | Two files open in independent editor groups -->

| Workspace control | Behavior |
| --- | --- |
| Search and Source Control | Keep search inputs, results, and commit drafts while switching sidebars. Search has its own query. **More… → Open search window** detaches it. |
| **Git review** tab | Open a changed file from Source Control. Switch back to an editor tab or close the review independently. |
| Activity bar | Open Files, Source Control, Find in files, Remote Explorer, or Settings. Click an active Files or Source Control icon to hide it. Turn off `show_activity_bar` for the text switcher. |

The status bar shows position, indentation, line endings, encoding, syntax, and
recovery; hover for details. Click its recovery dot for settings.

### Tabs in editor groups

Each group has its own tab strip. Open a file in the focused group. The same
document can have different cursor and scroll positions in another group. Text
edits and Undo are shared. Paged files also share the loaded page.

Tab navigation and **Close others**, **Close to the right**, and **Close all**
apply to the focused group. Drag a tab to another group or use **Move file to
other group**. Closing the final dirty view asks before discarding edits.
Empty groups close automatically. Reducing the layout merges the remaining
tabs. Recovery keeps the documents and view positions.

## Large files and logs

| Need | Use |
| --- | --- |
| Browse a file over 2 MiB | **Start**, **Previous**, **Next**, or **End**. Pages are about 256 KiB. |
| Find text or go to a line | Find and **Go to line** can use the whole file. |
| Follow a log | **View → Follow log (read only)** follows the end of a saved file. |
| Handle an outside change | Clean files reload. For a modified file, **Compare changes…** keeps replaced editor text in Undo. |

Paged files support a subset of editor commands. Multiple cursors, code
folding, and Markdown preview require a fully loaded file.

## Settings

Open **Preferences → Settings…** with **Ctrl+,**. Choose **User** or **Project**.

| Need | Use |
| --- | --- |
| Find an option | Search by setting name, description, or JSON key. A selected category narrows the search. Use Up/Down, Home, and End to switch categories. |
| Restore defaults | **Modified** shows changes from defaults. Use the individual **Reset** button where offered. |
| Save the visible scope | **Ctrl+S** saves. **Ctrl+W** closes Settings. Valid user changes also save after a short delay. Language servers use **Save configuration** in their detail view. |
| Edit project formatting | **Project** holds defaults and language overrides. Switching scopes keeps the draft. **Save settings** applies it. **Discard draft and reload** reloads saved values. |
| Check which value wins | Expand **Resolved settings for the active file** under Project or User → Formatting. Open an `.editorconfig` source or choose **Refresh sources**. File detection and tab-specific indentation can differ from these defaults. |
| Adjust formatting rules | Choose **Inherit**, **Enabled**, or **Disabled**. `.editorconfig` can override project rules, which can override user defaults. |
| Rebind a command | Open **Key Bindings**. Use one or two keys. `Primary` means Ctrl or macOS Cmd. Reset one binding or all bindings. |

<!-- docs-screenshot docs/images/settings.png | Settings window with the category list and editable preferences -->

Invalid settings keep the last valid value. Incomplete list entries stay
editable. Project saves refuse disk conflicts or an unsaved project-file tab.
The scope label shows whether a setting belongs to this computer or an SSH host.

### Interface appearance

| Setting in **Appearance → Interface** | Values |
| --- | --- |
| `ui_font_face` | Bundled Noto Sans by default. Choose an installed family or an absolute font-file path. |
| `ui_font_size` | 10–20 logical pixels |
| `ui_density` | `compact`, `default`, `comfortable` |

Use **Choose installed font…** to filter by family or script. Settings apply to
menus, tabs, and panels. Editor zoom and terminal overrides have separate
controls. Font files load in the background with a built-in fallback.

### The settings file

**Open settings.json** opens the current config. Saving applies changes.
External changes reload on focus. Comments and trailing commas are accepted.
hedit reports unknown keys and preserves invalid files for editing.

| Platform | Configuration folder |
| --- | --- |
| Linux | `~/.config/hedit/` |
| macOS | `~/Library/Application Support/hedit.hedit/` |
| Windows | `%APPDATA%\hedit\hedit\config\` |

Use `HEDIT_CONFIG_DIR` to override it. The folder also holds
`keybindings.json`, `snippets.json`, `macros.json`, and `themes/`. Use
`syntax_specific` for entries such as `{"python":{"tab_size":4}}` or
`build_systems` for custom builds. Unset formatting follows the file style.

### Extensions

| Task | Where |
| --- | --- |
| Install, update, disable, or uninstall | **Settings → Extensions**. Disabling or updating revokes approvals. |
| Run a command | Command Palette or a personal key binding. |
| Create from a template | **New File from Template…** |
| Change plugin settings | Configure declared settings in **Extensions**. |
| See outcomes and timing | **Recent Lua activity** in **Extensions**. |

| Access | What to review |
| --- | --- |
| Lua document command | Review each command's permissions. **Allow once** covers one invocation where offered. Edit previews let you inspect changes. Undo restores text and selections. |
| **Lua on Save** | Bind a persistently approved command to a language. It can replace the entire eligible local file on Save. Rebind after update or reapproval. A failed hook saves the current editor text. |
| Provider | Persistent approval lists triggers and document access. Results appear in the editor. Stop or revoke to cancel its work. |
| Scoped command | Review each directory, public endpoint, or native tool image separately. Preview workspace edits file by file before **Apply**. An incomplete application retains a recovery record. |

Lua runs in eligible local buffers on supported macOS, Linux, and Windows builds.
Signature verification identifies the publisher. Command and resource approval
remain separate. See the [extension reference](docs/extensions.md) for examples,
manifest fields, fixture tests, and bundle signing.

## Appearance

**Preferences → Color theme…** previews built-in/custom themes; Enter keeps one,
Esc restores the previous choice. **Settings → Appearance** has a gallery and
system light/dark following with a separate theme for each. Hedit Ink and Hedit
Paper are defaults; [palette credits](assets/licenses/theme-palettes.md) cover
additional themes.

<!-- docs-screenshot docs/images/themes.png | Appearance settings with built-in theme previews -->

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
| `plugin check`, `plugin test`, `plugin pack`, `plugin keygen` | Validate, test, package, and sign [extensions](docs/extensions.md#develop-and-package) without a window |
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

Recovery checkpoints keep tabs, unsaved notes, and fully loaded undo history.
They run every few seconds. A power loss can interrupt the latest checkpoint.
Save important work to write the source file. Closing waits for a final
checkpoint. [SSH recovery](#privacy-and-recovery) lives on the host.

| Situation | What to do |
| --- | --- |
| Recovery dot says **Pending** or **Checkpointing** | Wait for **Up to date**. |
| Checkpoint fails | Keep hedit open. Fix disk space or permissions, then choose **Retry checkpoint**. |
| A tab is missing | Check open tabs, **Ctrl+P**, **Closed Files**, and the session you launched. |
| The latest recovery is unreadable | hedit tries the previous copy and keeps `preserved-*` data. Damaged undo can be dropped with a warning while text survives. |

<!-- docs-screenshot docs/images/recovery.png | Closed Files with an unsaved note available to reopen -->

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
