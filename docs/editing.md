# Editing and navigation

## Text editing

The editor supports a single selection, clipboard operations, indentation,
undo/redo, word wrap, and adjustable font size. Use **Ctrl+Z** to undo and
**Ctrl+Shift+Z** or **Ctrl+Y** to redo. Up to 256 edit steps are kept per buffer
while hedit is running.

Toggle wrap through **More → Word wrap** or **Ctrl+Shift+W**. When wrap is off,
long lines scroll horizontally. **Ctrl++** and **Ctrl+-** change the editor font
size. Your wrap and font preferences are kept between launches.

UTF-8 text is supported, including an existing UTF-8 BOM and LF/CRLF bytes.
UTF-16 and binary files are rejected. Editing can introduce LF line breaks into
CRLF files; explicit line-ending conversion is not available yet.

CUDA (`.cu`, `.cuh`) and C/C++ (`.c`, `.h`, `.cc`, `.cpp`, `.cxx`, `.hpp`, `.hxx`,
`.hh`, `.inl`) files have lexical syntax highlighting. Other files currently use
plain text. This does not require a language server or a plugin.

## Find and replace

1. Press **Ctrl+F** or click **Find**.
2. Enter a literal, case-sensitive query, up to 4,096 UTF-8 bytes.
3. Click **Next / wrap** to select the next match. Search runs across the whole
   file, including pages that are not currently displayed, and wraps at the end.
4. Enter replacement text and click **Replace selected** to replace the selected
   matching occurrence. Find another match to continue.

Regular expressions, case-insensitive search, and replace-all controls are not
available in this UI yet. Long searches can be cancelled from the status bar.
**Esc** closes the search panel.

## Go to a line or another buffer

Press **Ctrl+G** to show the line controls in the Find panel. Enter a positive,
one-based line number and click **Go**. This searches the complete document;
line numbers shown in a large-file page remain relative to that page.

Press **Ctrl+P** to filter open buffers by name. Press Enter to select the first
matching buffer, or click one. **Ctrl+Shift+P** opens the same palette with editor
commands available below the buffer list.

## Bookmarks

Place the cursor at a useful position and press **Ctrl+F2** to toggle a bookmark.
**F2** goes to the next bookmark; **Shift+F2** goes to the previous one. Navigation
wraps at the first and last bookmark in the active buffer.

Click **Bookmarks (n)** above the editor or choose **More → Bookmarks…** to browse
bookmarks and jump to one by its text label. Remove one by navigating to it and
pressing **Ctrl+F2** again. Each buffer can hold up to 1,000 bookmarks.

Bookmarks are kept with the buffer between launches and when it moves to the
shelf. They store byte positions, with a short label captured when added. Ordinary
edits adjust their positions; undo/redo or replacing the file externally can
change which text is at a saved position. Review bookmarks after those changes.
