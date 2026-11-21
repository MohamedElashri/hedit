# Workspace and service broker

Manifest API 5 / SDK 3 adds scoped workspace operations, HTTPS and supervised
native tools to the shared Lua engine. Commands and providers use the same
continuation contract. APIs 1–4 retain their existing meanings. The manager supports exact-image filter/LSP selection. Final package and sustained
performance gates remain under validation on this branch.
See the [security model](extension-security.md) for enforcement and residuals.

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

The host crate implements scoped workspace access, search, versioned resources,
watch continuations, edit plans, recovery storage, exact-endpoint HTTPS, isolated
native filters and a bounded asynchronous driver. Lua runs in a fresh confined process for each
continuation. The driver uses a trusted host thread for filesystem and network
work; that thread does not evaluate plugin source.

```text
Trusted editor: installed source, consent, document revisions, native UI
         |
   Bounded capability host: policy checks, resource tables, review state
         |                       |
   Versioned IPC             Scoped files / HTTPS transport
         |
   Fresh confined Lua worker: one bounded intent, then exit
```

The editor routes ordinary scoped commands through exact-source approval and
separate directory/endpoint review, complete paged workspace previews, clean
buffer preflight, and off-thread reconciliation. Providers share the same source,
resource, revocation and commit checks. Explicit recovery is available in
Extensions settings. The CLI supports offline continuation transcripts; extracted-package references remain open.
Native filters require separate image selection and final exact-source resource
approval. Language-server requests use persistent source/document-bound owners in the host;
the manager exposes separate exact-image review for these profiles. There is no unsandboxed fallback.

## Authority and threat model

Assets include granted document/workspace text, private editor configuration and
recovery copies, original file contents, host credentials, and editor availability.
Treat Lua source and replies, package updates, workspace directory entries,
filesystem races, and endpoint responses as untrusted. Another extension's grant
or a built-in editor service supplies no authority.

The trusted computing base includes the editor's approval UI and policy store,
this broker and its host-only filesystem/DNS/TLS dependencies, the shared worker
supervisor, native sandbox adapters, the OS, and system trust/resolver settings.
An unrestricted process already running as the user, an administrator, a compromised
OS, and a compromised editor are outside the isolation guarantee. Owner-only
storage does not protect against those attackers.

The host must bind every session to the exact installed package copy,
command/provider, explicit capabilities, selected physical root, and current
policy generation. `Definition` is a request, not an approval. `Config`, `Root`,
`Endpoint`, and authorization callbacks are trusted Rust objects; none is
constructed from a plugin's permission assertions. An ordinary command approval
cannot create workspace, tool, endpoint, or background-event grants.

Every sensitive delivery/effect obtains a current shared policy guard. Revocation
publishes an owner-verified cancellation generation before waiting for the
exclusive policy lock. Native file events directly cancel transports even when
the UI is inactive. New sessions reject a pending mutation. A separate writer
lock prevents one mutation from clearing another's intent; orphan repair obtains
both locks and advances the generation before admitting sessions.
Callbacks, updates, scope changes, closure and Stop must invalidate pending data
and registrations. Final document edits and presentation require fresh editor
checks when consumed. Revocation cannot retract disclosed text or completed
network requests, or automatically reverse completed edits.

Opaque resource/version/watch/request IDs are session-local, canonical 128-bit
random values. Copying an ID into another session cannot resolve its resource.
Request nonce and callback step are checked before authorization or I/O. Close
callbacks receive no document text/cache or edit authority and cannot request I/O.

## Continuation and scheduling contract

A callback returns one typed intent and exits. The host performs the approved
operation and passes its bounded response to a fresh callback. Globals,
coroutines, library mutations and Lua heaps do not survive; only the explicit
bounded scalar cache does. Existing command/provider framing remains unchanged.
SDK 3 uses an explicit private protocol version and a 2 MiB frame ceiling.

Limits include 32 callbacks, 10 seconds of aggregate active work, 64 MiB of chain
I/O, 16 bounded session/host contexts, four active broker operations, and four native
worker slots. Providers may occupy at most 12 contexts and three active broker
operations. Background native callbacks (including SDK 2 providers) leave one of
the four service-worker slots available for an ordinary command. Admission is
cancellable; a capacity retry preserves the exact callback input and never repeats
a previously completed host operation. The worker retains the service role's readiness deadline, 750 ms
processing deadline, 200,000 instructions, 16 MiB Lua allocation limit and native
resource/confinement policy. A reply cannot select a weaker launch role.

The driver reserves admission before starting setup. Each host has one pending
UI event and one preview-decision slot. A terminal event replaces a stale preview;
there is no unbounded event queue. Stop/drop signals cancellation without joining
threads. Setup, filesystem scans, hashing, journal writes, watcher teardown and
worker waits belong off the UI thread. UI methods only take a ready event or
submit a decision without blocking. Wake callbacks must be nonblocking.

Watch notifications discard raw native paths and coalesce into one dirty bit and
wake. `wait_changes` parks without a Lua worker, periodic UI polling, filesystem
scan or held policy lock. A fresh authorized scan produces changed relative names
at most twice per second. An idle wait expires after 30 seconds; active scans
consume the chain budget. Watch grants disclose names and changes, not file text.
A provider scheduler must also enforce owner/event rate limits, coalescing,
quarantine and lifecycle invalidation when the editor integration is enabled.

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

API 5 provider declarations bind the provider kind, triggers, debounce, capabilities
and source digest in the command approval. Automatic document events and typed
document providers require `document.read`. A presentation provider without that
capability starts only on an explicit manual action and receives a neutral empty
snapshot, including when the active buffer cannot support document plugins.
Resource grants remain separate; event approval supplies no directory or endpoint.

The editor coalesces events, checks document/revision/selection lifetimes, bounds
per-owner work, and preserves Stop, Resume, quarantine and idle-cache behavior.
Close callbacks receive empty text/cache, have no edit authority, and may only
finish with the closed result. Workspace proposals always use a complete trusted
preview and explicit acceptance, including proposals from automatic callbacks.
Successful edits carry their originating package so providers do not immediately
react to their own changes.

Only the selected provider draws details. Diagnostics use validated byte ranges
converted to editor coordinates. Results expire after 120 seconds; callbacks
recheck source policy and scope. Parked SDK 3 hosts wake the editor through native
notifications rather than periodic 50 ms UI polls. Capacity contention uses a
bounded off-UI retry; human preview dwell is excluded from work accounting.
No Lua worker or document/policy lock is retained during the preview wait.

## Workspace edits and recovery

Plans reuse the shared Unicode/CRLF edit validator: at most 32 existing files,
64 edits/file, and 256 KiB aggregate raw **and normalized** replacement text.
Each proposal binds a resource version, physical identity and content hash.
The initial journal requires files on the selected root's volume; edits across
nested mount points are refused before review.
Preview preparation does not modify disk. A preview expires after 120 seconds;
its exact host-created plan ID must be accepted before journal preparation.

The command UI shows the selected directory and complete before/after text for
every file. Preview pages are indexed during preparation, so drawing a page does
not scan the full file. Host preflight inspects the window's open and shelved
buffers using physical identities and complete before text, refusing dirty,
followed, paged or non-UTF-8 matches. It checks at most 4,096 buffers in five
seconds and charges that work to the chain budget.

A generation-checked lease freezes affected clean buffers and defers incoming
opens/reloads while the journal runs. Unrelated buffers remain editable. No
document mutex is held through journal or reconciliation I/O. Independent buffer
backups have a separate 64 MiB global reservation; this includes pending refresh
pages. Successful replacements advance revisions and clear affected history.
Unchanged files retain Undo; unexpected disk contents preserve the original
buffer text unsaved. Cancellation and errors reconcile and release every lease.
Deferred opens reread disk off-thread, and stale replies cannot roll back a
reconciled page. Other windows/processes remain external writers and receive the
existing external-change/conflict behavior; this is not a cross-window Undo or
transaction service.

All replacements, permission checks and the immutable journal are prepared and
flushed before original-file changes. Retention reserves up to 32 journals and
64 MiB of logical payload plus per-journal overhead globally, including failed
preparations. File counts are bounded; filesystem allocation units, ACLs and other
metadata affect physical disk use. Dropping a session never silently frees a
reservation or deletes recovery data.

Application uses no-clobber capture of the original followed by no-clobber
publication of the replacement. **There is a visibility gap between those two
renames.** It is not a single atomic replacement or a globally atomic transaction.
Ordinary errors attempt a no-clobber restoration while authority is still held.
Crashes, I/O errors or a competing creator can leave an absent destination or a
partial plan; originals and replacements remain available for explicit recovery.
Compensating recovery never overwrites a competing write. An interrupted apply
is reported separately from a fully completed edit, including when affected files
were not open; the UI directs the user to retained workspace recovery.

**Settings → Extensions → Workspace recovery…** lists retained journals.
Choose the original physical directory to inspect complete original/replacement
text and current file state. New journals record the command and exact approved
source digest; legacy journals explicitly lack attribution. Inspection does not
run Lua, make network requests, or resume edits. Views expire after 120 seconds.
They reserve at most 16 MiB each from the same global 64 MiB snapshot budget.

Explicit resume, restore and history deletion reopen the selected directory,
recheck its identity and birth time, authenticate the journal and compare a fresh
view with the reviewed one. The exclusive policy mutation guard cancels plugin
sessions before recovery, and the same per-window buffer lease protects clean
buffers during effects and reconciliation. At most two recovery jobs run globally;
I/O and hashing run off the UI thread. Closing the review cancels pending work.
No plugin/source approval is needed to recover an uninstalled plugin's journal:
this is a new, trusted user action rather than reused plugin authority.

Unavailable original/replacement bytes, damaged metadata, unknown stage entries
or conflicting writes are preserved. An in-place external write can destroy a
version that was only present in the live file; recovery cannot reconstruct it.
The incomplete-preparation action only removes verified replacement-only stages.
There is no automatic repair, quota reset or deletion of ambiguous records. Startup never resumes plugin source or silently applies an old plan.
History discard refuses missing destinations, competing contents and unknown or
tampered private entries. Interrupted-preparation cleanup accepts only recorded
private stages with bounded known replacement files and no ledger, receipts or
captured originals. Ambiguous/moved stages retain their reservation for review.
There is no global multi-file Undo. Unix directory fsyncs and file flushes are
used; Windows file flushes are not a directory-fsync/power-loss guarantee.

## Network boundary

An endpoint approval binds the exact HTTPS URL, GET/POST method, text/JSON content
type, upload limit and response limit. Port 443 only; no redirects, proxies,
arbitrary headers, cookies, compression, raw sockets or environment credentials.
Combining text reads and network access requires disclosure of the data scope
that can leave the editor. JSON helpers are pure, bounded and strict; decoding
untrusted JSON cannot create authority or execute source.

DNS has a deadline, bounded answers and no application cache. Reject mixed or
nonpublic destinations, including local/private/link-local/metadata/transition
ranges. Pin the approved addresses while retaining TLS hostname verification;
verify the connected peer too. System routing, DNS infrastructure and native CA
trust remain assumptions, not protections against a compromised OS.

Upload/response bodies are at most 64 KiB, strict UTF-8 without NUL. Responses
also have header count/byte limits. Transport is bounded by eight seconds and the
whole request by ten, with four global concurrent requests, five seconds between
requests per authenticated owner, and 32 requests per 30 seconds globally.
Cancellation drops the transport and its disclosure guard. Failed requests count
against rate limits. Credential references are not implemented; no plaintext or
ambient-secret fallback exists.

## External tools and remaining release gates

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
Installed native editor acceptance exercises the formatter and project LSP;
final extracted-package and sustained UI/load gates remain open.

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

Required gates still include native provider/recovery acceptance on all supported platforms; fresh native
Windows x64/ARM64 and Linux editor-integration checks; external
persistent LSP and final tool adversarial acceptance; author/reference/extracted-package
coverage; final sanitizer and sustained performance evidence; and universal macOS
packaging. Startup samples from debug fixtures are exploratory, not whole-editor
latency claims. Scheduled codec-only AddressSanitizer campaigns retain source
hashes, source snapshots, logs, corpus and failure artifacts; they complement
native sandbox and real-editor tests rather than replacing them.
