+++
title = "Your first project"
description = "Open a folder, find your way around, and keep unfinished work available."
weight = 20
template = "page"
+++

## Open your work

After [installation](@/getting-started/install.md), open hedit and choose
**File → Open…**, drag a file or folder into the window, or open a project from a shell:

```sh
hedit .
hedit src/main.rs:12:5
```

The first command opens the current folder. The second opens a file at a line and column.
Create an unsaved note with **Ctrl+N** (**Cmd+N** on macOS).

## Find files and commands

Use **Ctrl+P** to find a file and **Ctrl+Shift+P** to run a command.
On macOS, use **Cmd** in these shortcuts. The Command Palette shows the current
bindings, and **Settings → Key Bindings** lets you change them.

Select a file in the Explorer to preview it. Double-click it, pin it, or start
editing to keep its tab open. [Files, folders, and projects](@/guide/files.md)
explains project files, tab behavior, and encodings.

## Set up language support

Syntax highlighting works without a language server. For completion, diagnostics,
hover, and navigation, open **Settings → Language Servers** and configure a server
for your language. See [language support](@/guide/language-support.md) for supported
features and installation options.

## Make the workspace yours

- Open **Preferences → Settings…** with **Ctrl+,** to choose fonts, formatting, and appearance.
- Use [editor groups](@/guide/layout.md) to keep related files side by side.
- Enable the [integrated terminal](@/guide/terminal.md) in Settings when you need a shell.
- Open [Markdown preview](@/guide/markdown.md) to keep technical notes beside their rendered form.

## Save and resume

**Ctrl+S** saves the current file. Quitting preserves open tabs and unsaved edits
for your next session without writing them to their original files.
Recovery checkpoints are periodic; save important work explicitly.

Reopen a closed tab with **Ctrl+Shift+T**, or use **File → Browse closed files…**.
Read [Recover your work](@/guide/recovery.md) for what recovery retains and its limits.
