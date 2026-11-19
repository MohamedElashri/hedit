# Extension manifest reference

The experimental manifest API lets extensions contribute snippet commands and
file templates. Open **Settings → Extensions**, select **Import manifest…**,
and review the commands before installing. You can enable, disable, and reload
extensions from this page. The [Markdown task example](../examples/extensions/markdown-task.json)
uses the original manifest format and can still be installed.

Authors can also edit manifests directly in the `extensions/` subfolder of the
hedit [configuration folder](../USER_GUIDE.md#extensions), then click
**Reload extensions** to apply changes in the current window.

| Field | Purpose |
| --- | --- |
| `api_version` | Set to `1` for this schema. |
| `id` | Unique extension ID. |
| `commands` | Between 1 and 16 command objects. Each has an `id`, a `title`, and an `action`. |
| `commands[].id` | Unique command ID within the extension. Search by `<extension-id>.<command-id>` or by title in the Command Palette. |
| `commands[].title` | The label shown in the Command Palette, up to 128 bytes. |
| `commands[].when` | Optional conditions for showing and running the command. |
| `commands[].action` | An object with a `kind` and a `body` string. File templates can also set `language`. |

## Actions

- `insert_snippet` replaces the active caret or selection. It respects
  indentation and line endings and can be undone in one step.
- `new_file` opens a new unsaved buffer. You can find these commands in the
  Command Palette or under **New File from Template…**. Set `language` to an
  editor language ID such as `python` or `markdown` to assign its syntax.
  In an SSH workspace, the template opens as a host scratchpad.

IDs use up to 48 lowercase ASCII letters, digits, or hyphens and start with a
letter. A body uses up to 2,048 UTF-8 bytes across 16 lines without NUL characters.
Both actions use flat fields numbered 0 to 4,294,967,295, such as
`${1:default}` and `$2`. Repeat a field number to mirror its value and use
`$0` for the final caret position. To insert a literal dollar sign, write
`\\$` in the JSON string.

Snippet actions also support `${SELECTION}` and `${FILE_BASENAME}`. These
expand literally. The basename removes the final extension from the file name
and is empty in an untitled buffer. An expanded extension snippet can use up to
128 KiB.

## Context and shortcuts

Every condition in `when` must match the active editor. `languages` accepts
up to 16 editor language IDs. `filename` matches the filename, with `*` for
any number of characters and `?` for one character. `selection` accepts
`true` to require selected text or `false` to require a caret.

```json
{
  "api_version": 1,
  "id": "writing-tools",
  "commands": [
    {
      "id": "wrap-link",
      "title": "Wrap selection in a link",
      "when": {"languages": ["markdown"], "filename": "*.md", "selection": true},
      "action": {"kind": "insert_snippet", "body": "[${SELECTION}](${1:url})$0"}
    },
    {
      "id": "python-test",
      "title": "Python test",
      "action": {"kind": "new_file", "language": "python", "body": "def test_${1:case}():\n    ${2:pass}\n$0"}
    }
  ]
}
```

Bind an extension command in the **Key Bindings** settings by its title or
namespaced ID, for example `writing-tools.wrap-link`. The command must be
enabled and available in the current editor context when you press the shortcut.

hedit reads up to 32 regular `.json` manifests of at most 32 KiB each in
filename order. The folder can contain up to 256 entries. If two valid
manifests declare the same extension ID, hedit loads the first one.
