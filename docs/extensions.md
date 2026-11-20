# Extension manifests

Write commands in Lua 5.4 and declare them in a JSON manifest. JSON can also
define snippets and new-file templates. Lua runs in local buffers on Linux
x86-64 with kernel process isolation (Linux 5.9 or a backport). Declarative
extensions work on all hedit platforms.

## Example

In **Settings → Extensions → Install from file…**, open
[markdown-quote.json](../examples/extensions/markdown-quote.json) and select
**Install**. Under the command, choose **Review permissions…**, then **Approve
command**. Select some text and run **Quote selection as Markdown** from the
Command Palette. Undo restores the original text and selection in one step.

Each Lua source returns a function `function(selection, language)` that takes
the selected UTF-8 text and the editor language ID. Return a UTF-8 replacement
string of up to 256 KiB. hedit preserves the buffer’s line endings. Commands
run only when invoked, with a single selection of up to 64 KiB.

## Manifest fields

| Field | Purpose |
| --- | --- |
| `api_version` | Use `2` for Lua or `1` for declarative commands. |
| `id` | Unique extension ID. |
| `commands` | One to 16 entries with `id`, `title`, and `action`. |
| `action` | For Lua, use `"kind": "lua"` and a `source` key from `scripts`. Declarative actions use `"kind": "insert_snippet"` or `"kind": "new_file"` and a text `body`. A new-file action also accepts a `language` ID. |
| `scripts` | For Lua, map a filename such as `quote.lua` to its Lua source string. |
| `permissions` | For Lua, declare `selection.read` and `selection.replace`. |
| `when` | Optional conditions. `languages` lists language IDs, `filename` matches a filename using `*` and `?`, and `selection` is a boolean. All supplied conditions must match. |

Use `extension-id.command-id` in the Command Palette or assign a shortcut in
**Settings → Key Bindings**. Snippets support `${1:default}`, `$1`, `$0`,
`${SELECTION}`, and `${FILE_BASENAME}`. Find templates under **New File from Template…**.

IDs start with a lowercase letter and use up to 48 ASCII lowercase letters,
digits, or hyphens. Use the same format before `.lua` in script names.
Each JSON manifest is limited to 32 KiB and each Lua script to 16 KiB.
Declarative bodies are limited to 2 KiB and 16 lines, with a 128 KiB expansion
limit. hedit loads up to 32 manifests.

## Permissions

Lua commands start unapproved. Each approval covers one command and the exact
installed copy’s manifest and source. Use **Revoke approval** to withdraw that
command’s access. Disabling an extension clears its Lua approvals. Lua provides
string, table, math, and UTF-8 helpers for local text processing.

Use **Update…** to review and replace an installed manifest from a file.
Updated commands require fresh approval. Use **Uninstall…** to remove the
installed copy and its approvals. hedit keeps your original download.
If the approval file is unreadable, **Reset Lua approvals…** preserves a backup
and clears access so you can review commands again.
