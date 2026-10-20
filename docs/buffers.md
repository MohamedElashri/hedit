# Scratchpads and buffers

A buffer is the text behind a tab. It can be backed by a file or exist only as a
scratchpad. You do not need to choose a filename to keep a scratchpad between
launches: hedit stores recovery copies in its session folder.

## Name a scratchpad

Choose **More → Name scratchpad…** or press **Ctrl+Shift+R** while an unsaved
scratchpad is active. Enter a name of up to 80 characters and click **Apply name**.
The name appears in the tab, switcher, and buffer shelf, and survives restarts.

The name stays fixed when you edit the text. Leave the name empty and apply it
to return to using the first nonempty line as the tab title. Without either a
name or text, the title is “Untitled” followed by a number.

Naming does not create a file. **Ctrl+S** still asks where to save the text. After
you save a scratchpad to a file, the tab uses the file's name. Use **Save as…** to
write an existing file buffer to another path.

## Close and reopen tabs

Close a tab with its **×** button or **Ctrl+W**. Closing moves the buffer onto the
shelf, including its text, custom name, selection, and bookmarks.

- **Ctrl+Shift+T** or **Reopen latest** restores the most recently closed buffer.
- **Browse shelf…** in the sidebar, or **More → Browse closed buffers…**, shows
  all closed buffers. Filter by name, then click **Reopen** beside the one you want.
- Closing the window keeps open tabs in the session; it does not move them to
  the shelf.

The shelf has 50 slots. When full, hedit may remove a clean file-backed entry to
make room. It refuses to silently evict unsaved scratchpads or modified files.
If the shelf is full of unsaved work, reopen and save some of it before closing
more tabs. There is no expiration based on age.

## Know what has been preserved

Recovery runs in the background while your work changes. Check the status bar
before ending a session. Recovery copies are separate from the original files;
they do not replace saving text you want to share or manage as a normal file.

Undo/redo retains up to 256 edit steps during the current process. The undo stack
is not restored after restart. For recovery errors and session locations, see
[recovery](recovery.md).
