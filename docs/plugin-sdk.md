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

| Request | Scope |
| --- | --- |
| `document.read` | The current eligible local document, up to 64 KiB. |
| `document.edit` | Proposed edits and selections in that document. |
| Approval | Exact installed copy and command. Selection-only approval covers API 2 commands. |

Use `schemas/plugin-manifest.schema.json` and `schemas/hedit-sdk.lua` when
writing a plugin. Run `hedit plugin check` for the editor's checks on limits,
languages, resources, and argument values. SDK versions and private wire
versions are independent. Unsupported versions fail before evaluation.

## Command contract

```lua
return function(ctx)
  return hedit.replace(ctx.arguments.prefix .. ctx.document.text)
end
```

| Context field | Value |
| --- | --- |
| `ctx.document` | Opaque `id` and string `revision`, plus `text`, `language`, `crlf`, and `selections`. |
| `ctx.settings`, `ctx.arguments` | Declared, validated values. Changing either Lua table has no editor effect. |

Return one transaction. `hedit.replace(text)` replaces the snapshot. Use
`hedit.transaction` with `hedit.edit` for multiple edits.

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

| Field or helper | Rule |
| --- | --- |
| Offset | Zero-based UTF-8 byte index into the snapshot. |
| `start`, `finish` | Include start, exclude finish. Keep edits ordered with distinct starts and no overlaps. Endpoints must fall outside UTF-8 and CRLF interiors. |
| `hedit.line_column(byte)`, `hedit.offset(line, column)` | Convert zero-based lines and UTF-8 byte columns. Line content excludes its ending. A trailing newline creates an empty line. |
| Replacement text | Newlines follow the document policy. Lone carriage returns stay as text. |
| Resulting `selections` | Use final, normalized text. Invalid selections reject the transaction. |
| Omitted `selections` | The caret follows the last edit, moving before a split CRLF. If text is unchanged, selections stay in place. |
| Undo | One step for an applied text edit. Selection-only transactions leave text Undo alone. |

| Resource | Limit |
| --- | --- |
| Snapshot | 64 KiB |
| Edits and resulting selections | 64 edits and 16 selections |
| Replacement text | 256 KiB combined. The host checks raw and normalized data. |
| Source and Lua allocations | 16 KiB source and 16 MiB allocations |
| Instructions and time | One million instructions. Two seconds including startup and transport. |
| Save hook | 400 ms and an explicit language binding |

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

| Need | Manifest field | Lua |
| --- | --- | --- |
| Lua module | `modules` or `module_files` with simple sibling `.lua` names | `hedit.import("helper.lua")` evaluates reviewed source. It caches the returned value for the invocation. Cycles and depth above eight fail. |
| Text resource | `resources` or `resource_files` with lowercase IDs. Files can be sibling `.txt`, `.md`, `.json`, or `.csv`. | `hedit.resource("defaults")` returns reviewed text. |

Use one inline or file-backed form for each field. The CLI embeds file contents.
Installed workers receive bytes and resolve no paths or URIs. Module state
ends with the invocation.

| Limit | Size |
| --- | --- |
| Module and resource counts | 16 modules and 16 resources |
| Each module or resource | 16 KiB |
| Command source, modules, and resources | 64 KiB combined |
| API 3 manifest and bundle | 128 KiB and 256 KiB |
| API 1/2 manifest and bundle | 32 KiB and 96 KiB |

## Private command storage

Declare `storage.private` at the package level and `private_storage: true` on
the command. Persistent approval is required. Save hooks and **Allow once**
are unavailable for that command.

`ctx.storage` holds up to 16 scalar entries. Use lowercase setting IDs.
Values are booleans, signed 32-bit integers, or strings up to 256 bytes.

To replace the bucket:

```lua
return function(ctx)
  local result = hedit.transaction({})
  result.storage = {count = (ctx.storage.count or 0) + 1}
  return result
end
```

| Rule | Effect |
| --- | --- |
| Storage proposal | Submit it without edits or resulting selections. |
| Commit | The host checks approval and compares the current bucket with the snapshot. It atomically replaces that command's bucket off the UI thread. Conflicts fail. |
| Scope | The persistent command approval identifies the bucket. Text, revision, and text Undo are unchanged. |
| Revocation, disable, uninstall, or update | Removes buckets for revoked approvals. Reapproval starts empty. |
| Unsafe storage | Fails closed. Revoke to clear the bucket before reviewing again. |

Quotas cover entries, serialized files, and active buckets. Host-managed files
use owner-only permissions on Unix and inherited owner-only ACLs on Windows.
macOS rejects extended ACL grants. Lua gets no storage path or handle.

Fixtures use `storage` and `expected_storage`. They leave real buckets alone
and fail on unexpected proposals or mismatches.

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

Failures use fixed categories and messages. A syntax error can include a host-verified
`plugin:line` or module location. Runtime values, document text, and tracebacks
stay out of public errors.

SDK replies use a separate versioned envelope. Both SDK and legacy workers
use bounded frames, one request, native confinement, and host validation.

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
campaign is smoke evidence, not a completed security or performance gate.
Native platform enforcement, provider lifetimes, consent timing and sustained
egui CPU-frame measurements have separate tests. See the
[broker validation guide](plugin-broker.md#external-tools-and-validation) for
scope and reproduction commands.

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
