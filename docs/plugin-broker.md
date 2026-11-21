# Workspace and service broker

SDK 3 is under implementation. This integration branch accepts experimental
manifest API 5 / SDK 3 ordinary commands; APIs 1–4 retain their existing meanings.
The types in `hedit-plugin::broker` and `hedit-broker` are internal development
interfaces, not a released author contract. See the [current SDK](plugin-sdk.md)
and [security model](extension-security.md) for supported functionality.

## Implementation boundary

The host crate implements scoped workspace access, search, versioned resources,
watch continuations, edit plans, recovery storage, exact-endpoint HTTPS, and a
bounded asynchronous driver. Lua runs in a fresh confined process for each
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
buffer preflight, and off-thread reconciliation. SDK 3 background-provider
routing, recovery UI, author CLI and extracted-package references remain open.
Tool declarations are validated, but tool and language-server requests report
unavailable. Native external-tool isolation remains a separate implementation
and platform gate. None of these gaps is covered by an unsandboxed fallback.

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
worker slots. The worker retains the service role's readiness deadline, 750 ms
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
Compensating recovery never overwrites a competing write.

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

Tools must use a separately reviewed confinement profile, exact executable/runtime
identity, finite reviewed arguments, fixed environment and direct launch without
a shell. Initial profiles should allow no filesystem writes; writable scratch
needs an enforceable aggregate quota. Project reads need a private validated
snapshot, not a live mount of the original workspace. Plugin workers continue to
deny child processes. Language servers also need typed stdio framing, request and
URI/version checks, UTF-16 conversion, and independent lifetime/CPU/restart limits.

The macOS prototype demonstrated native filesystem/network/child restrictions,
but a hard unprivileged native memory ceiling has not been established. VM
isolation is not required for the selected native profile: macOS will use
monitored memory limits, with explicitly documented detection/termination
overshoot. This prototype does not enable tools or establish Windows/Linux tool
support. Unsupported filesystem/process confinement must report unavailable.

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

Required gates still include SDK 3 provider routing and native recovery UI acceptance; fresh native
Windows x64/ARM64 and Linux editor-integration checks; external
tool/LSP confinement and adversarial tests; author/reference/extracted-package
coverage; final sanitizer and sustained performance evidence; and universal macOS
packaging. Startup samples from debug fixtures are exploratory, not whole-editor
latency claims. Scheduled codec-only AddressSanitizer campaigns retain source
hashes, source snapshots, logs, corpus and failure artifacts; they complement
native sandbox and real-editor tests rather than replacing them.
