# Editing SDK

Choose the SDK for the capabilities your extension needs. Each manifest version
has an exact SDK version; unsupported combinations fail before evaluation.

| Manifest API | SDK | Contract |
| --- | --- | --- |
| 1 | none | Declarative contributions |
| 2 | none | Selection-only Lua transforms |
| 3 | 1 | [Document editing](#command-contract) |
| 4 | 2 | [Typed providers, events and presentation](plugin-services.md) |
| 5 | 3 | [Scoped workspace, tools, LSP and HTTPS](plugin-broker.md#author-contract) |

The editing helpers below remain SDK 1's contract and are shared by later SDKs.
A newer SDK does not inherit earlier approval grants or gain undeclared authority.

## Permissions and compatibility

SDK commands request `document.read` and `document.edit`. Approval lets one command
read the whole current local document, up to 64 KiB, and propose edits/selections
anywhere in it. It never authorizes a different document, filesystem path, or
remote/paged buffer. Existing selection-only approvals do not authorize SDK
commands. Installation and updates retain exact-copy, command-specific consent.

Use `schemas/plugin-manifest.schema.json` for authoring and
`schemas/hedit-sdk.lua` for Lua annotations. `hedit plugin check` remains the
runtime authority for byte limits, language IDs, resource references, and argument
constraints. SDK versions and private worker framing are separate contracts;
unsupported versions fail before evaluation.

## Command contract

```lua
return function(ctx)
  return hedit.replace(ctx.arguments.prefix .. ctx.document.text)
end
```

The context supplies `document`, declared `settings`, and validated `arguments`.
`document.id` is an opaque invocation handle; `document.revision` is an opaque
string token, not a Lua number. The document contains `text`, `language`, `crlf`,
and `selections`. Mutating Lua context tables never changes editor state.

A command returns one transaction. `hedit.replace(text)` replaces the complete
snapshot. For multiple edits, return:

```lua
return function(ctx)
  return hedit.transaction({
    hedit.edit(0, 0, "prefix "),
    hedit.edit(#ctx.document.text, #ctx.document.text, " suffix")
  })
end
```

Transaction helpers supply the original document handle and revision. The host
checks them independently along with current approval, pane, document revision,
and selections. Forged or stale output is rejected before application.

## Coordinates and transactions

All positions are zero-based UTF-8 byte offsets into the exact snapshot. Ranges
are half-open: `start` is included and `finish` is excluded. Edits must be ordered,
non-overlapping, and have distinct starts. No endpoint may split a UTF-8 code
point or a CRLF pair. `hedit.line_column(byte)` and `hedit.offset(line, column)`
convert zero-based line/UTF-8-byte-column coordinates. Line endings are excluded
from line content; a trailing newline creates an empty last line.

Replacement newlines follow the host document's line-ending policy; lone carriage
returns remain text. Optional transaction `selections` refer to the final,
normalized text. Omitted selections become a caret after the last edit, or remain
unchanged when the text is unchanged. If an omitted caret would fall inside a
new CRLF pair, it moves before that pair. Invalid final selections reject the complete
transaction. A successful document edit uses one Undo step; selection-only
transactions do not add a text Undo entry.

The host validates both raw and normalized output before effects. Limits are:
64 KiB snapshot text, 64 edits, 16 resulting selections, 256 KiB combined normalized
replacement text, 16 KiB script source, 16 MiB Lua allocations, one million
instructions, and two seconds including worker startup and transport. Save hooks
retain their 400 ms deadline and explicit language binding.

## Declared inputs

A command may declare up to 16 `arguments`, each with a label and scalar default.
Optional `choices` must contain unique values of the default's type and include
the default. Values are booleans, signed 32-bit integers, or UTF-8 strings up to
256 bytes without NUL. The host collects inputs before starting the worker and
rechecks context afterward. Input and preview dwell do not consume worker time.
Save hooks use argument defaults and never display prompts.

See `examples/extensions/sdk-markdown.json` and its fixture file. Test it with:

```sh
hedit plugin check examples/extensions/sdk-markdown.json
hedit plugin test examples/extensions/sdk-markdown.json examples/extensions/sdk-markdown.fixtures.json
```

Fixtures retain `input` and `expected` text. API 3 additionally accepts argument
overrides and snapshot `selections` in byte coordinates. Unknown arguments and
invalid choices/types are rejected. Fixture execution uses the same confined
worker as editor commands; it does not create persistent approvals.

## Verified modules and text resources

API 3 may declare `modules` mapping simple `.lua` names to source and `resources`
mapping lowercase IDs to UTF-8 text. `hedit.import("helper.lua")` evaluates only a
reviewed module from that package, caches its returned value for this invocation,
and rejects cycles or import depth above eight. A module must return a value.
`hedit.resource("defaults")` returns the named text resource. Neither operation
opens a path, resolves a URI, loads native code, or accesses another package.
Module state is discarded with the worker.

Use `module_files` for a list of simple sibling Lua files and `resource_files`
for a map from resource IDs to sibling `.txt`, `.md`, `.json`, or `.csv` files.
These are authoring conveniences for CLI check/test/pack. Packaging embeds the
reviewed contents; installed bundles never interpret paths. Do not combine an
inline map with its corresponding file map/list.

A package permits at most 16 modules and 16 text resources, each up to 16 KiB.
Total command source, modules, and resources together must fit 64 KiB. API 3
manifests may occupy up to 128 KiB and bundles up to 256 KiB. APIs 1/2 retain their
32 KiB manifest and 96 KiB bundle limits, and their script limits are unchanged.

## Private command storage

A package may declare `storage.private`; each command that uses it must also set
`private_storage: true`. Other commands in that package do not receive storage.
The approval review shows this extra authority. Storage requires persistent
command approval; Allow once and save hooks are unavailable for such commands.

Authorized invocations receive `ctx.storage`, a snapshot of up to 16 named scalar
values. Keys use the same lowercase ID rules as settings, and values have the
same 256-byte string / 32-bit integer / boolean limits. To replace the bucket:

```lua
return function(ctx)
  local result = hedit.transaction({})
  result.storage = {count = (ctx.storage.count or 0) + 1}
  return result
end
```

Storage proposals cannot include document edits or explicit resulting selections.
The host reviews the proposal, rechecks approval and the storage snapshot, and
atomically replaces only that command's private bucket on a background thread.
A conflicting update rejects the proposal; no last-writer-wins overwrite occurs.
Document text/revision and text Undo history are unaffected. This separation does
not promise a transaction spanning a document and a filesystem write.

Buckets are keyed by the persistent command approval, not a worker-supplied name.
Commands do not share buckets automatically. Revocation, disablement, uninstall,
or an update that removes that approval clears its bucket. Reapproval starts with
empty storage; it does not transfer private data to a replacement package.
Corrupt or unsafe storage fails closed. Revoke approval to clear its bucket before
reviewing it again. Quotas apply to keys, values, serialized files, and active
buckets. Unix owner-only permissions (with extended ACL grants rejected on macOS)
and Windows owner-only inherited ACLs protect
these host-managed files; no path or handle is exposed to Lua.

Fixtures may provide `storage` and `expected_storage` for storage-capable commands.
They execute against fixture data and never read or write configuration buckets.
An unexpected storage proposal or a mismatch fails the fixture.

## Compatibility policy

Manifest API, SDK, portable bundle, and private wire versions are separate.
SDK versions are integer contracts. Changed coordinates, callback/result shapes
or authority semantics require a new supported SDK version. Additional optional
helpers can be additive when existing sources retain identical behavior and
limits; the manifest checker remains the availability check. Keep existing
conformance fixtures and publish migrations before retiring a version.
APIs 1–4 remain supported; none is deprecated by API 5. Signing
does not grant capabilities, and a supported version never bypasses permission
review. Bundle format 1 and private protocol versions are independent of SDK numbers.

The documented helper set and manifest schema are the contract; application
internals and private worker framing are not extension APIs. The first reference
plugins are `sdk-markdown.json`, `sdk-table.json`, and `sdk-trim.json`, each with a
companion fixture file. The table converter handles tab-separated text; it is not
a general CSV parser. The formatter supports explicitly bound Save hooks.

## Errors and diagnostics

SDK failures have fixed error categories and messages. Syntax errors may identify
`plugin:line` or a reviewed module name and line. The host verifies that name and
line against the original package before displaying them. Worker responses never
supply arbitrary diagnostic prose. Runtime errors, tracebacks, document contents,
settings, arguments, and storage values are not included in public errors.

The private SDK reply has its own versioned success/failure envelope. It cannot be
interpreted as a legacy replacement array. Both paths retain bounded messages,
one request per worker, native confinement, and host validation before effects.

## Validation and performance probes

Run the SDK editor tests and actual confined CLI fixtures with:

```sh
cargo test --locked --no-default-features --bin hedit app::extensions
cargo test --locked --no-default-features --test plugin_sdk
```

Native helper tests require the platform sandbox and signing tools; run them
outside a restrictive development runner that prevents child sandbox creation.
They must not be replaced with an in-process fallback.

An opt-in synthetic latency probe saves every sample and its build/toolchain
metadata. It measures host transaction preparation separately from complete CLI
invocations, including process startup, confinement, IPC, Lua, and exit:

```sh
HEDIT_SDK_PROBE_OUT=/tmp/hedit-sdk-latency.json cargo test --locked --no-default-features --test plugin_sdk_performance -- --ignored
python3 scripts/fuzz-plugin-codec.py --seconds 300
```

The fuzz command retains its log, metadata, corpus, and crash artifacts under
`target/plugin-fuzz`. It uses LLVM coverage on the host target without AddressSanitizer;
`fuzz/README.md` also documents the nightly `cargo-fuzz` workflow. A short clean
campaign is smoke evidence, not a completed security or performance gate. GUI
frame/input latency, consent timing, native platform parity, and longer sanitizer
campaigns require separate validation before expanding to persistent services.

Phase profiling is available with `hedit plugin test MANIFEST FIXTURES --timings`
and under Recent Lua activity in the extension manager. Samples contain only a
fixed phase name, elapsed microseconds, and count. Host authorization, request
encoding, process launch, request delivery, response transport, validation,
proposal preparation, commit, and prompt/preview dwell are recorded separately.
The worker's evaluation time is explicitly a worker-reported measurement; it is
bounded and never used to establish authority, enforce a deadline, or accept a
result. Transport includes worker execution and process exit, so phases are not
an additive wall-time partition. Collection uses fixed atomic counters and does
not write files or acquire a lock on the UI thread.

For CPU frame and allocation evidence from the actual egui frame path:

```sh
HEDIT_PLUGIN_UI_PROBE_OUT=/tmp/plugin-ui.json cargo test --locked --no-default-features --bin hedit profile_plugin_frames -- --ignored --test-threads=1
```

This opt-in test alternates three warmed workloads over three trials: extensions
disabled, installed idle, and stale-result rejection during typing. It records
per-frame CPU time, allocation count, and requested allocation bytes using a
test-only allocator. It does not measure other-thread allocation, retained heap,
OS event delivery, GPU rendering, or display latency. Normal builds do not
install that allocator.

A nightly Rust compiler and a compatible native Clang runtime are needed for the
sanitizer campaign. Choose `CC`/`CXX` explicitly when Apple Clang and Rust's LLVM
sanitizer runtimes differ:

```sh
python3 scripts/fuzz-plugin-codec.py --sanitizer address --seconds 600 --out-dir target/plugin-fuzz-asan
```

The runner records its Rust and native build settings. Keep the metadata, full
log, generated corpus, and any failure artifacts together. Sanitizer campaigns
complement the native confinement tests; they do not execute Lua or prove OS
sandbox policy.
