+++
title = "Your first project"
description = "Open a folder, find files, and recover closed tabs."
weight = 20
template = "page"
+++

## Open a project {#open-your-work}

[Install hedit](@/getting-started/install.md). Drag in a folder or use **File → Open folder…**.
From a shell, open the current folder or jump straight to line 12, column 5.

```sh
hedit .
hedit src/main.rs:12:5
```

## Find files and commands

| I want to | Use |
| --- | --- |
| Start an unsaved note | **Ctrl+N** |
| Find a file | **Ctrl+P** |
| Run a command | **Ctrl+Shift+P** |
| Keep an Explorer preview | Double-click, pin, or edit the tab |
| Save a file | **Ctrl+S** |
| Reopen a closed tab | **Ctrl+Shift+T** or **File → Browse closed files…** |

Use **Cmd** in these shortcuts on macOS. **Settings → Key Bindings** shows
and edits the current bindings.

## Set up language support

Use [Settings → Language Servers](@/guide/language-support.md) for completion,
diagnostics, hover, and navigation. Syntax highlighting works independently.

## Configure the workspace {#make-the-workspace-yours}

| Need | Where to go |
| --- | --- |
| Fonts or formatting | [Preferences → Settings…](@/guide/settings.md) (**Ctrl+,**) |
| Side-by-side editing | [Editor groups](@/guide/layout.md) |
| A shell | [Settings → Terminal](@/guide/terminal.md) |
| Rendered notes beside the source | [Markdown preview](@/guide/markdown.md) |

## Save and resume

**Save** writes the source file. Quitting keeps open tabs and unsaved edits in
the session. [Recovery](@/guide/recovery.md) explains when checkpoints run and
where to look for missing work.
