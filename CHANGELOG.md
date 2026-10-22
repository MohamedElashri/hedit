# Changelog

## [0.1.3] - Unreleased

### Added

- Terminal launches return the prompt after window startup and keep running when
  the terminal closes. This behavior is automatic for every editor launch.
  Startup errors remain visible, stdin retains recovery protection, and background
  diagnostics are saved in the session directory's `hedit.log`.
- One universal macOS `.app` ZIP for Apple Silicon and Intel, including an ICNS
  icon, bundled guides/themes, checksum, and ad-hoc signing. Native CI builds
  both slices and checks the merged app; Developer ID notarization is pending.
- Edit submenus for selection, line operations, text transformations, and
  indentation. Duplicate, delete, move, join, sort, or reverse lines; change case,
  trim trailing whitespace, convert indentation, and toggle line comments.
- Insert an indented line before/after the cursor (**Ctrl+Shift+Enter** /
  **Ctrl+Enter**) and remove duplicate lines while preserving their order.
- Explicit whole-file LF/CRLF conversion with cursor mapping, undo/redo,
  recovery metadata, and CRLF-aware Enter input.
- Per-file spaces/tabs indentation policy and 2/4/8-space indentation steps.
- View submenus for sidebar, editor, and font controls, with persistent tabs/status
  visibility, current-line highlighting, whitespace markers, full screen (**F11**),
  and distraction-free mode (**Shift+F11**).
- Persistent 80/100/120-column rulers and word-wrap columns, plus window-width
  wrapping. Display wrapping leaves file bytes unchanged.
- Multiple cursors with next/all occurrence selection, Alt+click, rectangular
  selection, batch typing/clipboard/deletion, cursor undo/redo, and recovery.
- Two-column/two-row split panes with independent view positions, group focus,
  file selection, and a shared document/edit history.
- Automatic indentation detection, smart Enter, soft indentation Backspace,
  and per-file tab rendering at the selected indentation width.
- Indentation/selection folding with fold/unfold-all controls, original source
  line numbers, indentation guides, and a sampled clickable minimap.
- Centered distraction-free text with restoration of the previous full-screen state.

### Changed

- macOS menus display Command/Option shortcuts. Local CUDA debugging and NVIDIA
  tools are disabled on macOS while CUDA source editing remains available.
- Release automation includes the universal macOS archive alongside Linux packages.

## [0.1.2] - 2026-10-22

### Added

- Open project folders through **File → Open folder…** (**Ctrl+Shift+O**),
  the command line (`hedit .`), or drag and drop. Browse a lazy folder tree,
  create files and subfolders, add or remove roots, and close folders independently
  of open tabs. Restore roots, expanded directories, and collapsed states after restart.
- Hide or show the folder section independently with **Ctrl+Alt+B**, its header
  close button, or **View → Show folder sidebar**. Toggle the entire sidebar with
  **Ctrl+B**.
- Native vector icons distinguish open/closed folders and document files.
- **Ctrl+P** fuzzy path search across files in all open folders, with background
  scanning, relative-path results, and keyboard navigation.

### Fixed

- Keep the file tab strip above the editor region, with the sidebar extending
  directly below the menu bar. Tabs resize with the editor when the sidebar is
  shown, hidden, or resized.
- Opening an existing tab preserves its cursor and edits. Opening a file from
  the closed shelf restores its recovery text instead of making a duplicate.

## [0.1.1] - 2026-10-21

### Added

- Manual updates through **Help → Check for updates…**, with release notes,
  checksum verification, and installation for Linux x86-64 and ARM64. Checks,
  installations, and restarts happen only when requested.
- Authenticated update downloads through GitHub CLI for private repositories.
- Automatic filename detection for 57 syntax modes covering programming, web,
  scientific, configuration, build, and text files.
- A larger bottom-bar syntax selector with a searchable, scrollable menu. Filter
  by language, extension, or filename, and press Enter to choose a single match.
- Per-file syntax choices for saved files and unsaved snippets, including
  **Automatic** and **Plain text**. Choices survive recovery, page navigation,
  and reopening closed files, and use the active theme's syntax colors.

### Changed

- Split C and C++ into separate syntax modes. `.c` and `.h` default to C;
  uppercase `.C` and `.H` use C++.
- Renamed the **CUDA C++** syntax label to **CUDA**.
- Use **New file**, **Name unsaved file…**, and **Closed files** consistently in
  the interface and guides.
- Reduced highlighting overhead by combining adjacent tokens with the same color.

### Fixed

- Escape now dismisses open menus as well as editor dialogs.
- Selecting syntax with Enter no longer inserts a newline into the open file.
- Syntax-picker redraws reuse unchanged editor layouts and filtered results,
  draw only visible choices, and keep the popup's position and size stable.

To upgrade from **0.1.0**, run the release installer again or install the new
package. In-app updating becomes available after installing **0.1.1**. See the
[update instructions](USER_GUIDE.md#updates) and the
[supported syntax modes](USER_GUIDE.md#syntax-highlighting).

## [0.1.0] - 2026-10-21

Initial release of hedit.

### Added

- Native Linux text editor with its own SVG branding, without Node.js or npm.
- Named unsaved files, session recovery checkpoints, fallback recovery, and a
  searchable shelf for closed files, including unsaved text.
- Guarded atomic saves and UTF-8 editing, with preservation of existing BOM and
  LF/CRLF bytes when saving unchanged content.
- Paged large-file editing, read-only log following, literal search and individual
  replacements, bookmarks, and go-to-line navigation.
- Clipboard commands, indentation, process-local undo/redo, command-line file
  locations, and snippets piped through standard input.
- Menus, tabs, an open-files sidebar, line numbers, a command palette, and unified
  preferences for editing, appearance, CUDA tools, and recovery.
- Light and dark themes, custom colors, theme import/export, font sizing, and
  word wrap.
- C/C++ and CUDA highlighting, a CUDA-GDB control panel, Compute Sanitizer runs,
  and NVIDIA GPU information tools.
- Linux x86-64 and ARM64 release packages, checksum-verified installation scripts,
  desktop integration, and tagged GitHub Actions releases.
- User and developer guides, plus Makefile commands for formatting, linting,
  tests, packaging, and CI validation.

[0.1.3]: https://github.com/MohamedElashri/hedit/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/MohamedElashri/hedit/releases/tag/v0.1.2
[0.1.1]: https://github.com/MohamedElashri/hedit/releases/tag/v0.1.1
[0.1.0]: https://github.com/MohamedElashri/hedit/releases/tag/v0.1.0
