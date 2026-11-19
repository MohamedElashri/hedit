# Extension manifest reference

The experimental manifest API lets an extension contribute commands that insert
snippets in the active editor. To install an extension, put its `.json` file
directly in the `extensions/` subfolder of the hedit
[configuration folder](../USER_GUIDE.md#extensions) and restart hedit.
The [Markdown task example](../examples/extensions/markdown-task.json) is ready to install.

| Field | Purpose |
| --- | --- |
| `api_version` | Set to `1` for this schema. |
| `id` | Unique extension ID. |
| `commands` | Between 1 and 16 command objects. Each has an `id`, a `title`, and an `action`. |
| `commands[].id` | Unique command ID within the extension. Search by `<extension-id>.<command-id>` or by title in the Command Palette. |
| `commands[].title` | The label shown in the Command Palette, up to 128 bytes. |
| `commands[].action` | An object with `"kind": "insert_snippet"` and a `body` string. |

IDs use up to 48 lowercase ASCII letters, digits, or hyphens and start with a
letter. A body uses up to 2,048 UTF-8 bytes across 16 lines without NUL characters.
Use flat snippet fields numbered 0 to 4,294,967,295, such as `${1:default}` and
`$2`. Repeat a field number to mirror its value and use `$0` for the final caret
position. To insert a literal dollar sign, write `\\$` in the JSON string.

Commands are available with one caret or selection in the text editor. Snippets
respect the document's indentation and line endings. One undo restores the
selection's previous text. Hedit reads up to 32 regular `.json` files of at
most 32 KiB each in filename order. The folder can contain up to 256 entries.
If two valid files declare the same extension ID, hedit loads the first one.
Errors appear in the editor when it starts.
