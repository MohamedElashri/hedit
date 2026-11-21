# Provider SDK (API 4 / SDK 2)

Providers run against the current local, editable document, up to 64 KiB. Paged,
remote, followed, and unavailable editor contexts do not activate a provider.
Ordinary commands keep API 2/3 behavior. Provider approval is persistent,
command-specific, bound to the reviewed package, and explicitly lists triggers
and document edit authority. An ordinary command grant never activates a provider.

See [the editing SDK](plugin-sdk.md) for snapshots, UTF-8 byte offsets,
transactions, package modules, typed settings, and fixed diagnostic categories.
The [document-assist example](../examples/extensions/document-assist.json) is an
executable reference, with confined [fixtures](../examples/extensions/document-assist.fixtures.json).

## Registration

Declare `api_version: 4`, `sdk_version: 2`. Each Lua command may declare:

```json
"provider": {
  "kind": "diagnostics",
  "events": ["open", "change", "close", "manual"],
  "may_edit": true,
  "priority": 0,
  "debounce_ms": 150
}
```

Kinds are `diagnostics`, `completions`, `hover`, `symbols`, `formatting`,
`code_actions`, and `presentation`. Triggers also include `selection` and `save`.
`interaction` is reserved for a host-validated action on a current result.
Providers cannot declare ordinary command arguments, save hooks, or persistent
private storage. Package-local modules/resources and typed settings are shared
with SDK 1. The package permission ceiling is exactly the union of its roles:
`document.read`, `events.observe`, `ui.present`, and `document.edit` for editing
providers. Formatting/completion require edit authority. Ordinary SDK commands
may coexist, retaining their separate approvals and optional private storage.

Registration uses existing language/filename/selection conditions. Priority
ranges from -100 to 100; higher priority runs first, then command ID breaks ties.
At most 32 approved providers register per window; the results window reports an
excess. Results remain grouped by owner; one provider cannot replace another's
results. Editor decorations deduplicate identical ranges/messages across owners and show
at most 256 ranges across priority order. Result lists render visible rows for
one selected provider; long output uses bounded pages.
There is no implicit call to a built-in LSP, executable, or internal editor command.

## Callback and state

A script returns a function taking `ctx`, with the SDK 1 document/settings API and:

- `ctx.request`: `id`, `kind`, `trigger`, `position` (UTF-8 byte offset), plus
  `action`/`choice` only for an interaction.
- `ctx.cache`: explicit scalar state from this provider's last accepted callback.
- `hedit.to_utf16(byte)` returns zero-based line and UTF-16 column;
  `hedit.from_utf16(line, column)` returns a byte offset. Both reject surrogate,
  UTF-8, and CRLF splits. A shared index is built once per callback.

Return `{result = {diagnostics = items}, cache = ctx.cache}`, using the registered
kind as the result tag. Omitting `cache` preserves its input value. Cache has at
most 16 scalar entries, 256 UTF-8 bytes per string, and signed 32-bit integers.
It is scoped to the exact provider grant and active document. It survives accepted
callbacks/revisions, but clears on scope changes, idle expiry, stop, policy/source
changes, or quarantine. Lua globals and module state never survive a callback.

Open/close describe entry into and exit from an eligible active-document scope,
including pane/document switches. Close is a coalescible, best-effort notification:
it receives an empty snapshot and cache, cannot edit, and must return
`{result='closed'}` with no cache. Revocation/exit terminates work immediately;
it does not run plugin cleanup code. Change/selection/save events coalesce into
one latest pending event. Provider edits are tagged by package; their own
change/save feedback is suppressed. Callbacks never apply an edit automatically.
A manual formatting request opens the ordinary edit review flow.

## Results and native UI

All ranges are `{start=byte, finish=byte}` in the input snapshot. Counts are capped
at 128 items and eight actions per item. Result JSON is capped at 256 KiB. Lua result conversion also caps tables at
16 levels and 4,096 values; combined limits apply even when individual item
counts fit.

| Kind | Item/result fields |
| --- | --- |
| diagnostics | `range`, `severity` (`error`, `warning`, `information`, `hint`), `message` (1 KiB), optional `actions` |
| completions | `label` (256 bytes), optional `detail` (1 KiB), `range`, `insert` (4 KiB) |
| hover | `range`, `text` (16 KiB) |
| symbols | `name` (256 bytes), optional `detail` (1 KiB), `range` |
| formatting | SDK transaction |
| code_actions | action items `{title, action}` |
| presentation | optional `status` (256 bytes), `output` (16 KiB), `annotations`, `quick_list`, `input` |

An action is exactly one of `{edit=transaction}`, `{select={anchor,head}}`, or
`{callback={name=action_id,value=scalar}}`. Annotations have `range` and `text`
(1 KiB). A quick list has `title` and `items` (action items). An input has `title`,
`name`, scalar `default`, and optional same-type `choices` (up to eight, unique,
including the default). Host widgets collect choices; interaction launches a new
callback with the selected action/value. Empty Lua arrays use `{}` in array fields.

The status-bar Extensions button opens native results with trusted owner captions,
Stop/Resume, diagnostics, quick fixes, completions, hover, navigation lists, text
output, and prompts. Diagnostics have gutter markers/underlines; annotations
have underlines and bounded inline text. Results expire after 120 seconds and
are rejected when their scope/revision changes. Caret-sensitive results also
expire on selection changes. Actions must still belong to the current result
nonce, owner, document, revision, and selection context. Edits/selections recheck
approval and reuse the SDK validator, review policy, and single-document Undo.
Strings are plain text: no HTML, Markdown renderer, embedded images, scripts,
filesystem links, network fetches, or arbitrary host command strings.

## Lifecycle and limits

The host lazily manages sessions and rotates a fresh native worker/VM for every
callback. A trusted launch argument selects this role before reading IPC. The
worker sends one fixed readiness byte only after isolation succeeds; the parent
allows two seconds for startup and then 750 ms for request processing, response,
EOF, and exit. The callback also has 200,000 Lua instructions, 16 MiB Lua memory,
and the existing native 256 MiB/two-second CPU ceilings. Unix service workers
lower their scheduling priority; Linux cannot change it after seccomp. Windows
uses a below-normal Job Object priority and a 10% hard CPU-rate cap. There is no
resident VM whose cumulative CPU ceiling is silently removed.

Four provider workers may run globally, including authorization. Callbacks
serialize per package within a window. Each provider starts at most every 100 ms,
uses a declared 50–1000 ms debounce, and has a conservative 10-second wall-work
budget per 30 seconds. Idle cache expires after 30 seconds. Failures back off;
three failures quarantine the provider until explicit Resume. Stop clears pending
work, results and cache. A permission/source watcher invalidates registrations;
watcher failure disables providers. Authorization is checked before delivery,
before publishing results, and before effects. Provider disclosure/publication
guards permit concurrent readers; policy changes and private storage writes stay
exclusive. UI authorization receipts expire after 200 ms if unconsumed, so an
inactive window cannot retain a policy lock indefinitely. User prompt/preview
dwell holds no such receipt. Filesystem checks and Lua/IPC
never wait on the UI thread. Revocation cannot retract already disclosed data.

## Fixtures and declarative packs

`hedit plugin test manifest.json fixtures.json` launches the distinct confined
role. Provider fixtures specify `expected_result`, optional `cache` /
`expected_cache`, `trigger`, and interaction `action`/`choice`. Formatting may use
ordinary `expected` text. Fixture document handles are 32 `a` characters and
revision `0`, for reproducible expected transactions. Fixtures confer no editor
grants and never run unconfined.

Declarative API 1 additionally supports `language` and `theme` command actions.
They require no executable grants. A language `definition` has `name`, 1–16 simple
lowercase `extensions`, up to 256 ASCII `keywords`, optional punctuation
`line_comment` (1–8 bytes), and `quoted_strings`. It associates otherwise plain-text
files automatically, or can be selected from the command palette for a document.
The scanner is linear, has no regex/backtracking/nesting, and falls back to plain
text above 64 KiB. Built-in language services are not implicitly enabled for a
custom syntax. Themes use the existing bounded native theme JSON as `definition`
and are applied only when selected. Unknown fields and external resources are
rejected. See the [visual pack](../examples/extensions/demo-visuals.json).

SDK 2 grants no workspace, tool, HTTPS, credential, multi-file edit or remote
document authority. Those operations require separately declared SDK 3
capabilities and resource review; credentials and remote documents remain
unsupported.

## Scoped providers

API 5 / SDK 3 providers use the scoped broker.
They reuse these typed provider results and scheduling semantics while declaring
capabilities per command. Workspace and HTTPS access require separate resource
review. Document-free presentation providers are manual-only and receive an
empty snapshot. See the [broker contract](plugin-broker.md#installed-providers)
for the authority boundary, foreground reserves and native service limits.
