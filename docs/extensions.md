# Extensions

Extensions add snippet commands, file templates, or Lua 5.4 text transforms.
Install a JSON manifest or a versioned `.hedit-plugin` bundle through
**Settings → Extensions → Install from file…**. Commands run from the Command
Palette or a personal key binding; file templates appear under
**New File from Template…**.

Lua works in local buffers on macOS, Linux, and Windows x86-64 and ARM64.
It requires the sandbox support included in the application package. See
[building and testing extensions](../DEVELOPER_GUIDE.md#lua-extension-architecture-and-testing)
for custom builds and platform requirements.

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
from the Command Palette. With previews enabled, review the proposal and choose
**Apply changes**. One Undo restores the original text and selections.

API `3` adds the [document editing SDK](plugin-sdk.md) with explicit document
permissions, structured edits, and declared command inputs. API `2` below keeps
its selection-only contract.

## Lua API

A script returns a function receiving the selected text, language ID, and declared
settings. Return one UTF-8 replacement string without NUL characters:

```lua
return function(selection, language, settings)
  return "> " .. selection
end
```

hedit calls this function for each selected range and preserves the buffer's
line endings when applying edits. Each call receives a fresh settings table with
validated defaults and user overrides. Settings values are booleans, 32-bit
integers, or strings of at most 256 bytes.

Lua exposes `math`, `string`, `table`, `utf8`, and basic assertion, iteration,
conversion, and type functions. File access, networking, process execution,
module loading, dynamic code loading, and editor APIs are unavailable. A new
worker starts for each invocation; no state persists between invocations.

| Limit | Maximum |
| --- | --- |
| Selected ranges | 16 |
| Combined selected text | 64 KiB |
| Combined replacement text | 256 KiB |
| Lua allocations | 16 MiB |
| Instructions | 1,000,000 |
| Invocation deadline | 2 seconds; 400 ms for save hooks |


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

The [extension threat model](extension-security.md) describes isolation, consent,
worker validation, and the limits of these protections.

Lua commands start unapproved. Each approval covers one command and the exact
installed copy’s manifest and source. **Allow once** covers the next attempt in
that window. **Recent Lua activity** shows the latest 64 command outcomes and
durations in the window. **Revoke approval** withdraws access. Disabling an
extension clears its Lua approvals.

**Lua on Save** lets you explicitly allow a persistently approved, save-capable
command to read and replace the whole local file for one language on ordinary
**Save**. A failed hook saves the current editor text without applying the
hook's changes. Updated or reapproved commands need a fresh save binding.
Save hooks do not run on remote, read-only, or paged buffers.

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

Replace `X.Y.Z` with a version for the bundle. `check` validates a manifest and
its referenced source without executing it. `test` runs fixtures inside the same
sandbox used by editor commands. `pack` embeds sibling Lua source into a
self-contained bundle for installation.

A fixture file is a JSON array. Each case names a command and its expected text;
optional `language` and `settings` fields override the declared defaults:

```json
[
  {"command": "markdown-quote.quote", "input": "hello", "expected": "> hello"}
]
```

Install the resulting bundle through **Install from file…**.

For signed releases, use `hedit plugin keygen publisher.key` once and add
`--publisher "Publisher name" --sign-key publisher.key` to the pack command.
Keep the private key outside the files you distribute. Share its printed
fingerprint through a separate channel so users can compare it before trusting
the key. Bundle updates show the release and changed scripts. Signed updates
must use the installed publisher’s key and increase the version. Lua permissions
remain a separate approval.
