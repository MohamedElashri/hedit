# hedit user guide

hedit is a native Linux editor for log files and scratch buffers you want to keep
between sessions. You can use it without a GPU. The optional CUDA panel runs
NVIDIA's debugging tools.

## Start here

1. [Install hedit](installation.md).
2. [Open a file or create your first scratchpad](getting-started.md).
3. [Learn how buffers, names, and the closed-buffer shelf work](buffers.md).

## Find the guide for your task

| I want to… | Guide |
| --- | --- |
| Keep snippets without choosing filenames | [Scratchpads and buffers](buffers.md) |
| Read a large file, follow a growing log, or mark a position | [Working with logs](logs.md) |
| Find text, replace a selection, or navigate to a line | [Editing and navigation](editing.md) |
| Change colors or import a theme | [Appearance and themes](themes.md) |
| Debug a CUDA program or run a memory check | [CUDA and GPU tools](cuda.md) |
| Open files from my terminal or pipe in output | [Command line](command-line.md) |
| Learn the keyboard controls | [Keyboard shortcuts](shortcuts.md) |
| Restore work after a crash or a recovery warning | [Recovery](recovery.md) |
| Fix a startup, file, dialog, or tool problem | [Troubleshooting](troubleshooting.md) |

hedit currently reads UTF-8 text. Large files use bounded pages, and line numbers
within a page are relative to that page. Undo history lasts for the current
process; buffer contents and bookmarks survive restarts through recovery.
