# Changelog

## [0.1.4] - 2026-10-24

### Added

- Right-to-left text: Arabic and Hebrew follow the Unicode Bidirectional
  Algorithm, Arabic letters join (including lam-alef), and right-to-left text
  mixes with English and numbers on the same line. Clicks, the caret, Left and
  Right, selections, and Find highlights follow the text on screen, while the
  file keeps the typed order. Plain text, Markdown, and LaTeX lines take their
  direction from their first letter; code lines run left to right.
- Eighteen built-in themes, including One Dark, One Light, Dracula, Monokai, Nord,
  Gruvbox, Solarized, Tokyo Night, Catppuccin, GitHub, and Rosé Pine, alongside
  Hedit Ink and Hedit Paper.
- A quick theme switcher (**Preferences → Color theme…**) that previews themes as
  you move through the list and restores the previous theme on Esc.
- A theme gallery with live miniature previews, grouped into dark, light, and
  saved themes, with filtering.
- Optional switching between a dark and a light theme with the system appearance.
- Saved themes in a theme folder: save customized colors as a new theme, update or
  remove saved themes, and keep imported themes available.
- Syntax colors for functions, types, constants, operators, properties and keys,
  and preprocessor directives, attributes, and decorators.
- Partial theme files can start from a built-in theme with `base`.
- Brackets and quotes close automatically, wrap a selection, and are stepped over
  when typed again; Backspace removes an empty pair (`auto_match_enabled`).
- The bracket matching the one beside the cursor is outlined (`match_brackets`).
- Files open with the indentation they already use (`detect_indentation`); new
  files and syntax changes apply the formatting settings.
- The status bar's indentation, line endings, and encoding are clickable.
- “Set syntax: …” entries in the command palette.
- Editor font family (`font_face`, an installed family name or a font file), extra
  line spacing (`line_padding`), cursor width and blinking (`caret_width`,
  `caret_blink`), and scrolling the last line up to the top (`scroll_past_end`,
  on by default).
- Syntax highlighting uses Sublime Text syntax definitions for most languages,
  recognizing embedded languages, multi-line strings and comments, escapes, and
  language constructs. Edits re-highlight only the affected lines, and large files
  are highlighted in the background.
- Import TextMate and Sublime `.tmTheme` color schemes as themes.
- Go to Anything in Go to file (Ctrl+P): `@symbol`, `:line[:column]`, and `#text`
  jump inside the current file with a live preview that Esc undoes, and a file
  name in front (`app.rs@save`, `main.cu:42`) opens that file at the target.
- Find as you type: the Find bar selects the first match after the cursor while
  you type, shows “n of m”, and highlights every match on the page.
- Selecting a word outlines its other occurrences (`match_selection`).
- Selection commands from Sublime: Expand selection (Ctrl+Shift+Space), Expand to
  brackets (Ctrl+Shift+M), Expand to indentation (Ctrl+Shift+J), Jump to matching
  bracket (Ctrl+M), Split into lines (Ctrl+Shift+L), Skip occurrence, and undo or
  redo of selection changes (Ctrl+U, Ctrl+Shift+U).
- Two-key shortcuts as in Sublime, such as Ctrl+K, Ctrl+D (skip occurrence),
  Ctrl+K, Ctrl+U / Ctrl+K, Ctrl+L (upper/lowercase), and Ctrl+K, Ctrl+1 /
  Ctrl+K, Ctrl+J (fold/unfold all). They can be recorded on the Key Bindings page
  and written in keybindings.json as `"Primary+K, Primary+D"`.
- Build systems: Ctrl+B saves and builds with nvcc, g++, gcc, cargo, Python, or
  make (or your own `build_systems`), streaming output to a results panel; F4 and
  Shift+F4 visit the errors, and Ctrl+Shift+B chooses a build system.
- Sublime's editor layouts: one to four columns, two or three rows, and a grid of
  four (Alt+Shift+1–5, 8, 9; Cmd+Option+1–5 on macOS), with Ctrl+1–4 (Control+1–4
  on macOS) to focus a group. Each group keeps its
  own highlighting.
- `hedit -w` (`--wait`) keeps the terminal waiting until the files are closed, so
  hedit can be the editor for `git commit` and other tools.
- File → New window (Ctrl+Shift+N) opens another window with its own tabs and
  recovery data. Windows that held work reopen with the first window.

### Changed

- The Appearance page separates the theme gallery from color customization,
  groups colors into interface, syntax, and status, and offers per-color resets.
- Editing a built-in theme names the result “*name* (modified)” instead of
  “My theme”.
- Theme files are now version 2. Version 1 files still load and look the same.
- The tab width menu items read “Tab width: 2/4/8” instead of “2/4/8 spaces”.
- The sidebar toggle moves from Ctrl+B to Ctrl+K, Ctrl+B, as in Sublime, so
  Ctrl+B can build.
- Select all occurrences moves from Ctrl+Shift+L to Alt+F3 (Control+Cmd+G on
  macOS), as in Sublime; Ctrl+Shift+L now splits a selection into lines.
- Settings live in one readable `settings.json` file, like Sublime Text, in the
  configuration folder (`~/.config/hedit/` on Linux, `~/Library/Application
  Support/hedit.hedit/` on macOS, or `HEDIT_CONFIG_DIR`). Comments and trailing
  commas are allowed, unknown keys are reported, and a broken file is never
  overwritten. Saving it in hedit, or returning to hedit after editing it
  elsewhere, applies it. Earlier preferences are imported on first launch.
- The Settings window has General, Editor, Formatting, Folders & Search,
  Appearance, Key Bindings, Snippets, CUDA Tools, and Recovery pages, each with its own Restore defaults, and an
  **Open settings.json** button.
- Formatting defaults (tab size, indentation, whitespace and newline on save,
  line endings, encoding) can be set in Settings instead of raw JSON.
- Rulers accept any columns, up to eight at once.
- Find options (case sensitivity, regex, whole words) are remembered.
- Key bindings and snippets move to `keybindings.json` and `snippets.json` in the
  configuration folder.
- Settings → Key Bindings lists every command; click a shortcut and press keys to
  change it, with Reset and Unbind. Keys taken from another command unbind it.
- Settings → Snippets edits snippets in place instead of as JSON.
- Formatting rules can be added per syntax from the Formatting page.
- Settings → Folders & Search sets which folders and files are hidden from the
  folder tree, Go to file, and Find in folders (`folder_exclude_patterns`,
  `file_exclude_patterns`). `*.pyc` and `.DS_Store` files are now hidden by default.
- The theme in use, and the dark and light themes when following the system, are
  named in `settings.json` (`color_theme`, `follow_system_appearance`,
  `dark_color_theme`, `light_color_theme`).
- Ctrl+Q quits on Linux, and Ctrl+0 / Cmd+0 resets the editor font size; these
  keys no longer zoom the whole interface.
- On macOS, Replace is Cmd+Option+F (Cmd+H hides the app), full screen is
  Control+Cmd+F, distraction-free mode is Control+Shift+Cmd+F, and Complete word
  is Control+Space (Cmd+Space opens Spotlight). About and Quit appear only in
  macOS's own app menu instead of twice.
- Panels, bars, and banners share one close button (×) on the right: the Find
  bar, build results, CUDA panel, folder sidebar, and error banner (which loses
  its Dismiss button). The Settings window closes from its title bar; the extra
  Done button is gone.
- Dialogs end with Cancel and a highlighted default action on the right, which
  Enter also triggers: Go to line, New file and New folder (which now also has a
  title-bar close button), Name unsaved file, and Rename, Move, Duplicate, and
  Move to trash.

### Fixed

- Arabic, Hebrew, Chinese, Japanese, Korean, Devanagari, Thai, and other scripts
  missing from the built-in fonts showed empty boxes; hedit now uses the system's
  fonts for them.
- On Linux desktops that leave window decorations to the app, such as GNOME on
  Wayland, the window now has a GNOME-style title bar with standard close,
  maximize, and minimize buttons instead of a bare gray frame.
- Menu-bar titles, menu entries, and buttons now show a visible highlight under
  the pointer, and the title of the open menu stays highlighted.
- The command palette, theme switcher, Go to line, Bookmarks, and Closed files close
  when you click elsewhere; clicking a tab while the palette was open used to
  leave keyboard input nowhere.
- Escape closes one window at a time, starting with the most recent, keeps extra
  cursors unless nothing else is open, dismisses the error banner, and works while
  a background job runs.
- Opening a window closes the previous one instead of stacking them.
- Pressing Enter in the Find bar keeps focus in the search field instead of
  typing over the found match; Shift+Enter finds the previous match, and the
  match stays highlighted.
- Choosing Find or Go to line from the command palette focuses its field.
- Enter only triggers Find in folders, Go to symbol, Insert snippet, and the New
  file dialog when their field has focus, instead of overriding the editor.
- “No matches found.” and “Search cancelled.” appear as status-bar messages, not
  errors. The error banner appears once above split panes, and Retry checkpoint
  only appears for checkpoint failures.
- Status messages such as “File saved.” appear in the status bar for a few seconds.
- Clicking a window that floats over a split pane no longer switches panes.
- Git change markers and Tools → Git changes work for files reached through a
  symlinked folder, such as macOS's `/var` → `/private/var`.
- On macOS, navigate back/forward with Control+- and Control+Shift+-, so
  Option+arrows keep moving by word.

### Performance

- An idle window no longer redraws five times a second. hedit redraws on input or
  when work is pending, and a background thread wakes it when another `hedit`
  command sends files.
- Git change markers refresh when the text changes or the window regains focus,
  instead of running git every two seconds.
- Files changed on disk are noticed as soon as the window regains focus.
- The editor no longer copies and compares the whole page on every frame, and the
  folder tree no longer copies folder listings on every frame.
- The CUDA panel's Browse dialog no longer blocks the window.

### Removed

- The Ember theme. A saved Ember appearance keeps its colors.

## [0.1.3] - 2026-10-22

### Added

- Open files, folders, projects, and piped snippets in the running session from
  subsequent terminal commands, bringing its window forward after recovery completes.
- Regex, case-insensitive, whole-word, and selection-scoped search, match counts,
  previous-match navigation, capture-group replacement, and undoable replace all.
- Find in all workspace folders with filename filters, `.gitignore` support,
  cancellation, clickable locations, selectable replacement previews, and guarded writes.
- Named `.hedit-project` definitions, personal `.hedit-workspace` snapshots,
  recent projects, project formatting settings, and restoration of unsaved text,
  tabs, selections, pins, and split views. Switching preserves unrelated unsaved work.
- Clean-file auto-reload and disk/editor comparisons with guarded conflict resolution;
  taking the disk version retains the prior editor text in Undo.
- File/project symbol lists and back/forward navigation across files and positions.
- Reorder tabs by dragging, pin tabs, preview sidebar files, and close other/right tabs.
- Sidebar rename, move, duplicate, system trash, relative-path copy, and file-manager reveal.
- EditorConfig rules and user/project/per-language formatting preferences, including
  undoable save-time cleanup and first-save destination rules.
- Persistent undo/redo for fully loaded buffers, retaining up to 64 states and
  32 MiB in each direction per document in recovery checkpoints.
- Custom keyboard overrides, editable snippets with trigger/selection/cursor support,
  and word completion from open files.
- Git change markers and inline hunk previews, plus a full comparison with HEAD.
- Automatic UTF-16 BOM detection and explicit UTF-8, UTF-16LE/BE, Windows-1252,
  ISO-8859-1, Shift JIS, and GBK conversion; unrepresentable characters are rejected.
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

### Fixed

- Keep open buffers, closed buffers, and workspace roots attached after sidebar
  renames or moves through symlinked parent paths, including macOS `/var` aliases.

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

[0.1.4]: https://github.com/MohamedElashri/hedit/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/MohamedElashri/hedit/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/MohamedElashri/hedit/releases/tag/v0.1.2
[0.1.1]: https://github.com/MohamedElashri/hedit/releases/tag/v0.1.1
[0.1.0]: https://github.com/MohamedElashri/hedit/releases/tag/v0.1.0
