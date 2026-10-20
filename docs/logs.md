# Working with logs

Open a log with **Open…**, drag it into the window, or run `hedit application.log`.
A growing log can be read without saving anything back to it.

## Navigate a large file

Files above 2 MiB use views of roughly 256 KiB. The page toolbar provides
**Start**, **Previous**, **Next**, and **End**, with the byte range currently shown.
This keeps opening and viewing large logs from requiring the whole file in RAM.

The status bar labels large-file line positions with **Page Ln**. Those numbers
are relative to the displayed page. Use **Go to line** to locate a line in the
whole document, or [Find](editing.md#find-and-replace) to search across all pages.

## Follow incoming output

Enable **Follow log (read only)** on a clean, file-backed buffer. hedit shows the
tail and checks for changes about once per second. Follow mode reloads the latest
file contents and keeps the view at the bottom.

Disable Follow before editing. The checkbox is unavailable for scratchpads and
modified buffers. Save your changes or open a separate clean copy first.

When a log rotates, Follow opens the file currently at the original path. If the
path temporarily disappears, an error is shown and the visible buffer remains.
If the new log contains unsupported encoding or binary bytes, it cannot be read
as UTF-8 text.

## Mark positions worth returning to

Use **Ctrl+F2** to bookmark the cursor position, then **F2** and **Shift+F2** to move
between bookmarks. Bookmarks are useful for a crash report, a request ID, or the
start of an investigation. The **Bookmarks** window lists them by a short text
label and byte position. See [bookmarks](editing.md#bookmarks) for how edits affect
them.

## Handle external changes

Outside Follow mode, hedit flags a changed file when its buffer becomes active.
Use **Reload file** for a clean buffer. For an edited buffer, **Save buffer as…**
keeps your version under another path before you reopen the disk version.

hedit refuses to overwrite a source file that changed externally. It cannot
reconstruct unseen bytes from a large file that changed before a recovery copy
completed. If an edit cannot be applied, it keeps the edited visible page in a
new scratch buffer and explains what happened.
