# Extension manifests

Write commands in Lua 5.4 and declare them in a JSON manifest. JSON can also
define snippets and new-file templates. Lua runs in local buffers on macOS and
Linux x86-64 and ARM64. Linux requires kernel process isolation (Linux 5.9 or a
backport); macOS requires the signed sandbox helper included in the app bundle.
Declarative extensions work on all hedit platforms.

Windows can install and inspect Lua bundles, but cannot execute Lua commands,
fixtures, or save hooks until its native sandbox adapter is available.

## Declarative example

In **Settings → Extensions → Install from file…**, open
[markdown-task.json](../examples/extensions/markdown-task.json) and select
**Install**. Run **Insert Markdown task** from the Command Palette. The
insertion is one undoable edit. Type over the selected description and press
Tab to leave the snippet field. This example works on macOS, Windows, and Linux
and needs no Lua approval.

## Lua example

In **Settings → Extensions → Install from file…**, open
[markdown-quote.json](../examples/extensions/markdown-quote.json) and select
**Install**. Open **Review permissions…** for the command and choose **Allow
once** or **Approve command**. Select text and run **Quote selection as Markdown**
from the Command Palette. Review the preview, then choose **Apply changes**.
One undo restores the original text and selections.

Lua source returns a function `function(selection, language, settings)` and
returns one replacement string per call. hedit calls it for each selected range,
with up to 16 selections and 64 KiB total input. The combined replacement limit
is 256 KiB. hedit preserves the buffer’s line endings.

## Manifest fields

| Field | Purpose |
| --- | --- |
| `api_version` | Use `2` for Lua or `1` for declarative commands. |
| `id` | Unique extension ID. |
| `commands` | One to 16 entries with `id`, `title`, and `action`. |
| `action` | For Lua, use `"kind": "lua"` and a `source` key from `scripts`. Declarative actions use `"kind": "insert_snippet"` or `"kind": "new_file"` and a text `body`. A new-file action also accepts a `language` ID. |
| `scripts` | For Lua, map a filename such as `quote.lua` to its Lua source string. |
| `permissions` | For Lua, declare `selection.read` and `selection.replace`. |
| `settings` | Optional map of keys to `label` and `default`. Defaults can be booleans, integers, or strings. hedit shows controls for these keys in Extensions. |
| `on_save` | Optional command flag. Set to `true` for a Lua command designed to process a full file. |
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
installed copy’s manifest and source. **Allow once** covers the next attempt in
that window. **Recent Lua activity** shows the latest 64 command outcomes and
durations in the window. **Revoke approval** withdraws access. Disabling an
extension clears its Lua approvals.

**Lua on Save** lets you explicitly allow a persistently approved, save-capable
command to read and replace the whole local file for one language on ordinary
**Save**. A failed hook leaves saving to hedit. Updated or reapproved commands
need fresh Save permission.

Use **Update…** to review and replace an installed manifest from a file.
Updated commands require fresh approval. Use **Uninstall…** to remove the
installed copy and its approvals. hedit keeps your original download.
If the approval file is unreadable, **Reset Lua approvals…** preserves a backup
and clears access so you can review commands again.

## Develop and package

To keep Lua in ordinary files, replace `scripts` with `"script_files": ["quote.lua"]`
and put that file beside the manifest. Commands still reference the Lua filename.

```sh
hedit plugin check extension.json
hedit plugin test extension.json fixtures.json
hedit plugin pack extension.json extension.hedit-plugin --version X.Y.Z
```

Replace `X.Y.Z` with a version for the bundle. Fixtures are a JSON array of objects with `command` (the namespaced ID), `input`,
`expected`, and optional `language` and `settings` overrides. Tests run inside
hedit’s Lua sandbox with the declared defaults and validated overrides.
Install the resulting bundle through **Install from file…**.

For signed releases, use `hedit plugin keygen publisher.key` once and add
`--publisher "Publisher name" --sign-key publisher.key` to the pack command.
Keep the private key outside the files you distribute. Share its printed
fingerprint through a separate channel so users can compare it before trusting
the key. Bundle updates show the release and changed scripts. Signed updates
must use the installed publisher’s key and increase the version. Lua permissions
remain a separate approval.
