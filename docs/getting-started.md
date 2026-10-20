# Your first session

Launch `hedit` from your application launcher or terminal. The first session
opens with an empty scratch buffer. Later sessions restore your open buffers.

## Keep a snippet without saving a file

1. Click **+ New**, or press **Ctrl+N**.
2. Type or paste your text.
3. Optionally choose **More → Name scratchpad…** (**Ctrl+Shift+R**) and give it a
   name such as “Investigation notes”. This does not create a file.
4. Watch the status bar. **Recovery pending** means the latest changes are waiting;
   **Checkpointing…** means a recovery copy is being written;
   **Recovery up to date** means the current work has a completed checkpoint.

You can leave the buffer open, close its tab to the shelf, or close the window.
On normal exit, hedit waits for a final recovery checkpoint. If recovery fails,
it keeps the window open and shows an error. See [buffers](buffers.md) for more.

## Open and save files

Click **Open…** or press **Ctrl+O**. You can also drag files into the window or
pass filenames from the [command line](command-line.md).

Use **Save** (**Ctrl+S**) to write the current buffer to a file. A scratchpad asks
for a filename the first time. **More → Save as…** (**Ctrl+Shift+S**) writes to a
chosen path. Naming a scratchpad is separate from saving it.

Click a tab or a row in **Workspace** to switch buffers. **Ctrl+P** opens the
buffer switcher. The dot on a tab indicates unsaved changes.

## Make the editor comfortable

Open **Appearance** (**Ctrl+,**) to choose a palette or customize colors. Toggle
word wrap through **More → Word wrap** (**Ctrl+Shift+W**). Use **Ctrl++** and
**Ctrl+-** to adjust the editor font size.

For a long log, start with [working with logs](logs.md). For GPU debugging, open
the **CUDA** panel and follow the [CUDA guide](cuda.md).
