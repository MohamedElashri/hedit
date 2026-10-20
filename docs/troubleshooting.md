# Troubleshooting

## hedit cannot open a window

Run it inside a graphical Linux session. X11 or Wayland and OpenGL are required;
a plain SSH session without display access cannot show the editor. Check the
error printed in the terminal. A consumed stdin buffer is checkpointed before
window creation, so reopen the same session after fixing display access.

## The session is locked

Another hedit process is using that session directory. Return to its window and
open the file there. To start an independent workspace, run:

```sh
hedit --session-dir /tmp/hedit-another-session
```

The presence of `session.lock` alone does not mean it is locked; a live process
must hold the lock. Do not remove session storage to work around a lock message.

## Recovery fails or a buffer is missing

See [recovery](recovery.md). Check disk space, directory permissions, the buffer
shelf, and whether you launched the correct session. Keep the editor open if a
checkpoint fails and use Save As for important text.

## Save says the file changed outside hedit

Another process changed or replaced it. hedit keeps your buffer and refuses to
overwrite that version. Use **Save buffer as…** or **More → Save as…** to keep your
text under another path. Reopen the original afterward. A clean buffer offers
**Reload file**.

## Open or Save dialog does not appear

The native file dialogs use the desktop portal service. Ensure
`xdg-desktop-portal` and the backend for your desktop are installed and running.
Launch hedit from the same desktop session. You can pass a file path on the
[command line](command-line.md) to open it without the dialog.

## A file is rejected as binary or invalid text

hedit currently reads UTF-8, optionally with a BOM. UTF-16 and arbitrary binary
files are not supported. Convert a separate copy to UTF-8 with another tool if
needed; hedit does not silently reinterpret the original bytes.

## My custom theme did not load

Check the error banner. A theme needs version `1`, valid six-digit RGB colors,
and a name of 1–80 characters. Unknown fields and files above 64 KiB are rejected.
A failed import keeps the active palette. A bad saved preference falls back to
Hedit Ink so you can still use the editor. See [themes](themes.md).

## CUDA-GDB, Compute Sanitizer, or GPU info will not start

Ordinary editing works without these tools. Install the relevant NVIDIA tool,
ensure it is on `PATH`, and start hedit from a session that can access the GPU.
Enter a custom CUDA-GDB path in the panel if needed. For a CUDA executable, build
with host and device debug information, for example `nvcc -g -G`.

The panel shows the tool's error output and exit status. See the
[CUDA guide](cuda.md) for the workflow and commands.
