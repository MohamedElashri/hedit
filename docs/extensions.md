# Extension manifests

Write hedit extensions as **JSON manifests**. A manifest declares built-in
actions that insert snippets or create unsaved files.

## Example

Import [markdown-task.json](../examples/extensions/markdown-task.json) from
**Settings → Extensions → Import manifest…**. Review the command and select
**Install**. Run **Insert Markdown task** from the Command Palette, then type
the task and press **Tab** to finish.

## Reference

| Field | Purpose |
| --- | --- |
| `api_version` | Set to `1`. |
| `id` | Unique extension ID. |
| `commands` | One to 16 commands, each with an `id`, `title`, and `action`. |
| `action` | Set `kind` to `insert_snippet` or `new_file` and supply a text `body`. The `new_file` action accepts a `language` ID for syntax highlighting. |
| `when` | Optional conditions. `languages` lists language IDs, `filename` matches a filename using `*` and `?`, and `selection` is a boolean. All supplied conditions must match. |

Use `${1:default}` for an editable field, `$1` to mirror it, and `$0` for the
final caret. Snippets can insert `${SELECTION}` and `${FILE_BASENAME}` as literal
text. The basename omits the final file extension.

Run commands by title or `extension-id.command-id` in the Command Palette.
Assign shortcuts in **Settings → Key Bindings**. Find file templates under
**New File from Template…**.

IDs start with a lowercase letter and use up to 48 ASCII lowercase letters,
digits, or hyphens. Bodies are limited to 2 KiB and 16 lines, with a 128 KiB
expansion limit. hedit loads up to 32 manifests of at most 32 KiB each.
