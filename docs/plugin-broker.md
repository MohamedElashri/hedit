# Workspace and service broker

API 5 / SDK 3 adds scoped workspace operations, public HTTPS, and supervised
native tools. Commands and providers share the same continuation contract.
Review each directory, endpoint, or exact native image separately. See the
[threat model](extension-security.md) for enforcement and residual risks.

## Author contract

An API 5 command declares a `broker` object with explicit `capabilities`,
optional `tools`, and optional `endpoints`. There is no package-wide permission
inheritance. Omit legacy `private_storage` and Save-hook fields; automatic work
uses a provider event. Command arguments, settings, package-local modules and
text resources retain their SDK 1 forms. Providers use scalar `cache` instead
of command arguments or legacy private storage.

```json
{
  "api_version": 5,
  "sdk_version": 3,
  "id": "scoped-format",
  "commands": [{
    "id": "format",
    "title": "Format with reviewed tool",
    "broker": {
      "capabilities": ["document.read", "document.edit", "tools.run"],
      "tools": {
        "formatter": {
          "protocol": "filter",
          "workspace": false,
          "arguments": [{"literal": "--format"}]
        }
      }
    },
    "action": {"kind": "lua", "source": "format.lua"}
  }],
  "script_files": ["format.lua"]
}
```

The user selects/imports the exact native image and separately grants execution.
The manifest cannot select executable paths. Filter arguments may reference
finite declared `choices`; language-server arguments are literal only. A project
snapshot (`workspace: true`) also requires workspace discovery/read capabilities
and a user-selected physical directory.

```lua
return function(ctx)
  if ctx.broker.step == 0 then
    return {intent = {tool = {
      profile = "formatter", input = ctx.document.text, arguments = {}
    }}}
  end
  local reply = ctx.broker.response
  if reply.failed then return {intent = {finish = {}}} end
  assert(reply.tool.status == 0)
  return {intent = {finish = {
    transaction = hedit.replace(reply.tool.output)
  }}}
end
```

The first callback has `ctx.broker.step == 0` and no response. Return exactly one
intent and optional scalar `cache`; the VM exits before any host operation.
A fresh confined callback receives `ctx.broker.response`, incremented step and
cache. Only that cache persists. `ctx.request` is present for providers with the
same kind/event/position/action fields as SDK 2. A final `finish` contains a
document transaction, a typed result, or neither for an ordinary no-op command.
Close callbacks return `result = "closed"` and receive empty document text/cache
without I/O or edit authority.

| Intent | Required capability | Response |
| --- | --- | --- |
| `discover` | `workspace.discover` | `discovered`: entries and truncation flag |
| `search` | `workspace.read` | `searched`: bounded literal matches |
| `open`, `open_many`, `read` | `workspace.read` | `opened`, `opened_many`, `read` |
| `release` | Existing resource | `released` |
| `stage_edits`, `review_edits` | `workspace.edit` | `staged`, `edited`: file count |
| `watch`, `wait_changes`, `unwatch` | `workspace.watch` | `watching`, `changed`, `unwatched` |
| `tool` | `tools.run` and exact filter profile | `tool`: status and UTF-8 output |
| `language_server` | `tools.run`, `document.read` and exact server profile | `language_server`: typed SDK result |
| `https` | `network.https` and exact endpoint | `https`: status and bounded body |
| `finish` | Corresponding edit/presentation authority | Terminal |

The [Lua annotations](../schemas/hedit-sdk.lua) specify intent/response fields.
The [manifest schema](../schemas/plugin-manifest.schema.json) checks authoring
structure; `hedit plugin check` also validates capability dependencies, argument
choices, protocol constraints, resources and byte limits. Missing/revoked
resources produce fixed `failed.code` values: `unavailable`, `denied`, `changed`,
`cancelled`, `limit` or `failed`. Never apply incomplete/truncated edit plans.

SDK 3 also provides bounded `hedit.json_encode`, `hedit.json_decode`,
`hedit.json_array` and `hedit.json_null`. They preserve JSON array/null identity
and reject duplicate keys, non-finite values, cycles and metatable execution.
They supply no network/filesystem access. SDK 2's Lua UTF-16 helpers are specific
to SDK 2; the SDK 3 LSP host converts protocol positions to snapshot byte ranges.

### Offline author fixtures

Use `hedit plugin test MANIFEST FIXTURES.json --timings` for API 5 and earlier
APIs. Cases retain `command`, `input`, `expected`, optional settings/arguments/
selections/cache and expected cache/result. Add `broker.exchanges`, an ordered
array of exact `{intent, response}` pairs. Each exchange runs a fresh confined
worker and compares its complete intent before supplying the fixture response.
Wrong response kinds, undeclared profiles/capabilities, missing/extra exchanges,
invalid typed results and wrong cache/text fail the case.

At most 31 exchanges precede the final callback, with a ten-second active-work
limit. `broker.scope: true` supplies a synthetic workspace handle for a declared
workspace capability. Fixtures use document `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`,
revision `"0"`, request `ffffffffffffffffffffffffffffffff` and synthetic scope
`cccccccccccccccccccccccccccccccc`. Production handles are random/session-local.

Transcripts perform no filesystem effects, HTTPS requests, tool launches or
approval-store writes. Responses and resource identities are synthetic fixture
data. Native broker/installed/extracted-package tests separately establish
confinement and resource-policy behavior.

## Reference integrations

Build the workspace, then use the author CLI to check, test and pack each
manifest under `examples/extensions`. Install the resulting bundle in Extensions.
Source approval and each directory/tool/endpoint grant are separate steps.

Open the complete sources and fixtures for
[JSON tools](../examples/extensions/scoped-json.json),
[project replacement](../examples/extensions/scoped-rename.json), and
[Markdown conversion](../examples/extensions/scoped-markdown-html.json).
The manifests name their sibling Lua sources and companion fixture files.


| Reference | Behavior | Extra resource review |
| --- | --- | --- |
| `scoped-json.format` | Formats the current JSON document through the native filter | Select `hedit-json-tools`; literal `--format` |
| `scoped-json.diagnostics`, `hover`, `symbols` | Persistent JSON diagnostics, project summary and root-key navigation | Select `hedit-json-tools`; literal `--project-lsp`; choose a project directory with discovery/read rights |
| `scoped-rename.replace` | Reviews ASCII identifier replacement across existing UTF-8 files | Choose a project directory with discovery/read/edit rights |
| `scoped-markdown-html.convert` | Sends only the selected Markdown to GitHub and replaces that selection with returned HTML source | Approve the exact public GitHub Markdown endpoint |

Packaged native images are located beside the editor: `hedit-json-tools` in a
Linux installation's `bin` directory, `hedit-json-tools.exe` in a Windows portable
directory, and `hedit.app/Contents/MacOS/hedit-json-tools` on macOS. Mac packages
contain both native architecture slices. Select/import the image explicitly for
each approved command/profile; installation or an app update never executes or
silently grants the reference tool.

The native JSON image has no shell, network or child-process interface. The
project mode reads at most 32 top-level JSON files, each at most 64 KiB, from the
approved private projection. Hover reports its activation-time project summary;
live unsaved document changes use the LSP channel. Refresh the provider explicitly
to rebuild the fixed project copy. This is a bounded reference, without JSON Schema
completion or cross-file semantic analysis. Formatting sorts object keys, uses
two-space indentation and appends LF. Invalid/duplicate/over-complex JSON produces
diagnostics; an unformattable or oversized formatting result produces no edits.

Project replacement is case-sensitive textual identifier replacement, not
language-aware symbol renaming. It excludes matches adjacent to ASCII identifier
characters or non-ASCII bytes, refuses truncated search/more than 32 files, and
uses each opened resource's version. The complete preview and recoverable commit
flow apply before any file changes. A conflict requires a fresh run; no partial
search result becomes an accepted plan.

The HTTPS reference uses GitHub's
[public Markdown raw endpoint](https://docs.github.com/en/rest/markdown/markdown#render-a-markdown-document-in-raw-mode),
which supports unauthenticated public-resource requests. It transmits the selected
text only, never an editor path, account token or environment secret. GitHub sees
that selection and can rate-limit/fail the request; the original document then
stays unchanged. Returned HTML is inserted as text, never executed or rendered by
a webview. No credentials are required by these references.

Companion `*.fixtures.json` files test the complete continuation exchanges
offline, including Unicode and CRLF. Native tests separately run the actual JSON
image through import, confinement, persistent typed results and cleanup on every
supported platform.

## Implementation boundary

| Component | Owns |
| --- | --- |
| Editor | Installation, consent, live document revisions, previews, and UI |
| Broker | Policy checks, resources, file/network effects, recovery, and native service lifetimes |
| Confined Lua worker | One callback with bounded input. It returns one intent and exits. |

The host driver handles filesystem and network work off the UI thread.
Ordinary commands and providers use the same resource, revocation, and commit
checks. Native filters need exact-image approval. Persistent LSP owners also
bind to the source and document. Confinement failure stops the invocation.

## Authority and threat model

Treat package source, replies, filesystem entries and races, and endpoint
responses as untrusted. The [threat model](extension-security.md) lists the
trusted components and residual risks. It assumes a trusted editor, OS, and
user configuration. System routing, DNS, and CA trust also apply.

| At | Broker check |
| --- | --- |
| Session start | Exact installed package and command, capabilities, physical root, and policy generation. `Definition` requests access. Trusted Rust `Config`, `Root`, and `Endpoint` supply it. |
| Each operation | Current policy guard, request nonce, and callback step. Resource, version, watch, and request IDs are random 128-bit values local to the session. |
| Change or revocation | Publish cancellation before waiting for the exclusive lock. File events cancel transports without UI polling. Pending mutations block admission. A writer lock serializes mutations, and orphan repair advances the generation. |
| Result consumption | Recheck authorization and live scope before displaying or editing. Updates, scope changes, closure, and Stop discard pending work. |
| Close callback | Empty text and cache. It cannot request edits or I/O. |

Revocation stops new effects. It leaves disclosed text, completed requests, and
committed edits as they are.

## Continuation and scheduling contract

A callback returns one intent and exits. The next fresh worker gets the approved
response and explicit scalar cache. The SDK 3 private frame limit is 2 MiB.

| Budget | Limit |
| --- | --- |
| Continuation chain | 32 callbacks, 10 seconds of active work, 64 MiB of I/O |
| Contexts and operations | 16 contexts, four active operations. Providers can use 12 and three, respectively. |
| Native service callbacks | Four slots globally. Background work leaves one for an ordinary command. |
| Each worker | Service startup deadline, 750 ms processing, 200,000 Lua instructions, 16 MiB Lua allocations, and native confinement limits |
| UI handoff | One pending event and one preview decision per host. Terminal results replace stale previews. |

Admission is cancellable. Retrying capacity preserves callback input and avoids
repeating completed operations. Stop and Drop signal cancellation without waiting.
Keep setup, filesystem work, watcher teardown, and worker waits off the UI thread.
Wake callbacks must return promptly.

Watchers coalesce native changes into a dirty flag. `wait_changes` releases its
worker and policy guard while parked. A fresh authorized scan returns changed
relative names at most twice per second. Idle waits expire after 30 seconds.
Scans spend active-work time. Watch approval covers names and changes.

## Scoped workspace access

Discovery, read, existing-file edit and watch are separate grants on a root chosen
by the host. Edit requires explicit read authority. Create/delete/rename are not
included. Names use `/`; coordinates are zero-based UTF-8 bytes with exclusive
ends. No implicit remote/paged-document or original-workspace tool access exists.

Root/component handles are pinned, with no symlink/reparse traversal. Reads admit
single-link regular UTF-8 files only. NUL, special files, hidden components,
Windows devices/streams and ambiguous path components are refused on every OS.
Identity, size/time stamps and hashes detect changes; current paths and root
identity are rechecked before publishing data. Hard links are rejected rather
than treated as an alternative grant. Case/normalization aliases cannot create
additional edit targets because plans reject duplicate physical identities.

| Resource | Bound |
| --- | --- |
| File before and after edits | 1 MiB |
| Relative path | 4 KiB, 32 components, 255 bytes per component |
| Discovery | 20,000 visited entries, 16 levels, 512 returned entries |
| Directory scan handles | One active iterator per depth, not per queued sibling |
| Literal search | 128-byte query, 16 matches/file, 512 total |
| Live resources | 128/session; 16 MiB/session and 64 MiB global retention |
| Response disclosure | 64 KiB aggregate names/text per continuation |
| Batch open | 32 files within the aggregate response limit |
| Native watch | 128 directories/watch, one watch/session, 16 globally |

Chunks preserve UTF-8/CRLF boundaries and consume the same cumulative read budget,
including repeated reads of cached snapshots. A resource's contents remain an
immutable versioned snapshot; later disk edits do not silently change that version.
Large batches must use bounded chunks. Truncation never implies a complete index.

The configuration root and this window's session/recovery storage are excluded,
even when custom session storage is inside the selected workspace. Private editor
data and journal directories have durable physical-identity
exclusions, refreshed across editor processes. Persist an exclusion before writing
sensitive stage contents. Moves/renames do not grant access. The owner-only registry
has a 4,096-identity ceiling; corruption, partial records or exhaustion fail closed.
IDs remain excluded after cleanup, so inode reuse can conservatively deny an
unrelated new file. Removing the registry while private stages survive is unsafe.

## Installed providers

The command approval binds the kind, triggers, debounce, capabilities, and
source digest. Automatic document events require `document.read`.
A presentation provider without it is manual-only and gets an empty snapshot.
Directory and endpoint grants are separate.

The scheduler coalesces events and checks document, revision, and selections.
It preserves Stop, Resume, quarantine, idle expiry, and per-owner work limits.
Close gets empty text and cache and can only return closed. Successful edits
carry their package ID to suppress feedback. Workspace edits always need a
complete preview and acceptance.

Only the selected provider shows details. Diagnostics have validated byte
ranges. Results expire after 120 seconds. Parked hosts wake from native
notifications. Capacity retries stay off the UI thread. Preview dwell spends
no active-work time and holds no worker, document lock, or policy lock.

## Workspace edits and recovery

Plans use at most 32 existing files and 64 edits per file. The 256 KiB aggregate
replacement limit applies to raw and normalized text. Each edit binds the resource version,
physical identity, and content hash. Files must be on the root volume.

| Stage | Host behavior |
| --- | --- |
| Preview | Show complete before/after text in pages. The exact plan ID expires after 120 seconds. Preparing a preview leaves disk untouched. |
| Preflight | Compare physical identities and complete before text for up to 4,096 open or shelved buffers in five seconds. Refuse dirty, followed, paged, or non-UTF-8 matches. Charge the chain budget. |
| Commit | Freeze affected clean buffers with a generation-checked lease. Keep other buffers editable. Flush replacements and the immutable journal before changing originals. |
| Reconcile | Release leases on success, cancellation, or failure. Replaced files advance revision and clear their history. Unchanged files keep Undo. Unexpected disk contents preserve original buffer text unsaved. Reread deferred opens off-thread. |

The commit captures each original, then publishes its replacement. Both
renames refuse overwrites. **The destination can be absent between renames.**
A crash, competing creator, or I/O failure can leave a partial plan. The host
attempts a no-clobber restore on ordinary failures and retains the journal
when recovery is needed. Other windows remain external writers.

| Reservation | Limit |
| --- | --- |
| Journals | 32 globally, 64 MiB of logical payload plus per-journal overhead. Failed preparation retains its reservation. |
| Clean-buffer backup and refresh | 64 MiB globally. Document locks are released during I/O. |
| Recovery views | 16 MiB per view from a 64 MiB pool. Views expire after 120 seconds. |
| Recovery jobs | Two globally. Closing a review cancels pending work. |

Open **Settings → Extensions → Workspace recovery…** and select the original
physical directory. Compare the original, replacement, and current file.
New journals list the command and approved source digest. Older journals may
lack attribution. Inspection works after uninstall and runs no plugin code.

Resume, restore, and history removal authenticate the journal, recheck directory
identity and birth time, and compare the current view with the reviewed one.
Recovery cancels plugin sessions under the policy mutation guard. It uses the
same buffer leases and off-thread reconciliation as commit.

History removal refuses competing contents, missing destinations, and unknown
or damaged stages. Incomplete-preparation cleanup removes only verified
replacement-only stages without receipts, a ledger, or captured originals.
Ambiguous data keeps its reservation. A missing version that existed only in
a live file cannot be reconstructed. Recovery requires a user action. There is
no global multi-file Undo. Unix flushes files and directories. Windows file
flushes provide no directory-fsync or power-loss guarantee.

## Network boundary

Approve the exact public HTTPS URL, GET/POST method, content type, and limits.
Combining text reads and network access discloses what can leave the editor.

| Check | Limit |
| --- | --- |
| URL and transport | Port 443, text or JSON. Redirects, proxies, arbitrary headers, cookies, compression, raw sockets, and credentials are unavailable. |
| DNS and TLS | Bounded answers and a DNS deadline. All addresses must be public. The broker pins them and verifies the peer and TLS hostname. |
| Bodies and headers | Up to 64 KiB each for upload and response. Strict UTF-8 without NUL. Response headers have separate byte and count caps. |
| Time and rate | Eight seconds for transport, ten for the request. Four global concurrent requests. Five seconds per owner and 32 requests per 30 seconds globally. |
| Failure or cancellation | Failures spend rate budget. Cancelling drops the request and disclosure guard. |

DNS, system routing, and native CA trust remain dependencies. The JSON helpers
are bounded data transforms and grant no network or execution access.

## External tools and validation

Native filters use separately reviewed images, finite literal/choice arguments,
a fixed minimal environment and direct launch. The broker's sealed Rust grant
binds command, source approval token, profile and recipe; worker-supplied paths,
identities and permission assertions cannot construct it. Session construction
rejects mismatched command/source/recipe grants.

The editor imports a checked private image without running a version command.
Import alone creates no execution authority. Resource review shows its filename,
digest, scope and argument choices before approval; reapproval cannot reuse an
old source token. Each invocation retains a distinct executable and immutable
project projection through process termination. Workspace projections require
discover/read grants and refuse truncation or unsafe/incomplete content.

Filters accept at most 64 KiB of input/output. Success requires a successful exit,
complete input delivery, bounded UTF-8 output and output EOF. Native failures
return fixed broker codes rather than tool diagnostics. Image verification/copy
work has a separate 256 MiB session budget; native launch and execution consume
the existing ten-second active-work budget. Current source authority protects
disclosure throughout; cancellation terminates the process before bundle cleanup.

The [native threat model](extension-security.md#native-tool-process-boundary)
documents Seatbelt, Landlock/seccomp, LPAC/write restrictions, resource limits,
private storage admission and residual OS risks. macOS uses monitored memory
limits under the selected native model, with explicit overshoot rather than a VM
or hard unprivileged memory-cap claim. Unknown/corrupt storage is preserved.

The persistent LSP host implements bounded framing/JSON-RPC, exact issued URI
and revision checks, UTF-8/16/32 conversion, typed results, source/document
lifetimes and soft cancellation fencing. Native service limits are separate from
finite filters: 60 cumulative CPU seconds, a 30-minute wall lifetime, two CPU
seconds and 8 MiB in each direction per ten-second interval, and no automatic
restarts. The rolling implementation conservatively retains ten to fifteen
seconds of charges. See the threat model for platform enforcement and overshoot.
Installed native editor acceptance exercises the shipped formatter and project
LSP through real source/resource grants, unsaved document updates and revocation
without UI polling. Extracted-package smoke tests use the packaged image and
editor bootstrap rather than a development-only launcher.

Run the shared and native continuation tests with:

```sh
cargo test --locked -p hedit-broker
cargo clippy --locked -p hedit-broker -p hedit-plugin --all-targets -- -D warnings
```

The custom native test handles the trusted helper entry point on Linux, prepares
an ad-hoc sandboxed test helper on macOS, and copies its headless worker test image
on Windows. It uses disposable Unicode/CRLF files and a synthetic translation
response. The transport tests separately exercise real loopback TLS; production
endpoint policy cannot opt into that test route.

Native tool probes run on macOS, Linux x86-64/ARM64 and Windows x64/ARM64.
Author conformance, editor integration, recovery and extracted-package checks
complement those adversarial policy probes. Scheduled codec-only AddressSanitizer
campaigns retain source hashes, snapshots, logs, corpus and failure artifacts.

Reproduce optimized scoped-provider and broker measurements on macOS with:

```sh
python3 scripts/profile-plugin-ui.py --frames 1200 --out-dir target/plugin-validation/scoped-ui
python3 scripts/profile-plugin-broker.py --trials 25 --out-dir target/plugin-validation/broker
```

The UI probe interleaves independent disabled/native views over three trials
using a near-64-KiB Unicode/CRLF JSON document. Frame order balances both edits
and native requests; disabled/disabled A/A controls expose comparison noise.
The native/disabled pair shares native background load, so its comparison
measures incremental UI cost. Process CPU and footprint belong to both views.
It records actual egui CPU-frame work, UI-thread allocations, numeric activity
phases, host physical footprint, published results and Stop cleanup. It does not
measure GPU presentation, physical input latency, other-thread allocations or
aggregate child-process memory. The broker probe separates reply validation,
bookkeeping, data copies and recoverable edit preparation/application using
real confined callbacks and disposable files. Keep sources fixed while either
script runs; both retain samples, source hashes and environment metadata.
