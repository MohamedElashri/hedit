# Extension threat model

Security rules for Lua workers, providers, and native tools. See the
[extension reference](extensions.md) for the author API and the
[developer guide](../DEVELOPER_GUIDE.md#lua-extension-architecture-and-testing)
for build and sandbox adapters. Declarative snippets and templates are data.

## Assets and adversaries

| Treat as untrusted | Protect |
| --- | --- |
| Source, manifests, bundles, and replies, including validly signed packages | Unapproved text, credentials, files, network services, editor state, and UI availability |
| Malformed messages and a compromised worker | The parent process and its authorization decisions |

The trusted boundary includes the broker, installer, permission store, bounded
parser, launcher, native adapters, their dependencies, and the OS. Lua's library
allowlist adds a restriction. Same-account malware, a compromised editor or
kernel, and administrators are outside this boundary. Unsafe control-file checks
protect against known layouts, with the current user and configuration trusted.

## Process and authority boundaries

Each invocation runs in a fresh confined child. Commands, Save hooks, and CLI
fixtures use the same engine and native isolation. SDK 3 passes explicit state
to a new worker per step. A separately approved native language server can
persist within one document scope. Confinement failure stops execution.

| Step | Check |
| --- | --- |
| Install and approve | Validate bounded input. Bind approval to the command, capabilities, digest, installed identity, and a fresh token. A signature identifies a publisher and verifies bytes. |
| Deliver input | Check current installation, enabled state, and approval under the change lock. Hold authority through delivery. Keep the authority handle in the host. |
| Launch worker | Clear environment and inherited handles. Establish confinement before sending or reading the request. |
| Accept a reply | Bound transport and parse independently. Require valid framing, successful exit, complete delivery for success, and valid decoded output. |
| Apply an edit | Recheck approval, revision, pane, page, language, and selections. Hold authority through the edit. Preview does not add permission. |
| Bind on Save | Require persistent approval and a language binding. Recheck both. Skip remote, read-only, paged, and oversized input. |

Legacy commands get selected strings, language, settings, and source. A bound
Save hook can receive the eligible whole document. SDK workers get a bounded
document, declared inputs, reviewed resources, and approved private values.
All effects use a closed typed API. The worker has no editor object or path
handle. The host owns resource selection and effects.

Allow once is single-use. Policy checks continue through application. Updates,
disablement, removal, revocation, and reapproval invalidate authority.
Revocation serializes with delivery and application. Disclosed bytes and
committed edits remain. Approved code can propose destructive text within its
scope. Use preview and Undo to review and recover. A Save binding permits
automatic replacement. A failed hook saves the current editor text.

## Private worker protocol

The protocol in `crates/hedit-plugin/src/runtime/protocol.rs` is private to
matching host and worker builds. Each worker receives one request and sends one
reply. SDK 3 returns one intent and exits. See the
[broker continuation contract](plugin-broker.md#continuation-and-scheduling-contract)
for chain limits.

| Frame | Rules |
| --- | --- |
| Request | Little-endian `u32` magic, `u32` byte length, JSON, then EOF. Legacy magic is `0xfffffffe`. SDK magic is `0xfffffffd` with a checked `sdk_version`. The payload cap is 1 MiB. Reject unknown or duplicate top-level fields, bad types or encoding, truncation, and trailing bytes. |
| Legacy reply | Status `0` and a JSON string array, or bodyless status 1–11. Other statuses or error bodies fail. |
| SDK reply | Status `0` and a versioned envelope. Successful effects bind the original handle and revision. Errors carry fixed categories and, optionally, a verified source location. Transport and confinement failures use bodyless codes. |
| Parsing | Body cap of 2 MiB. Expect 1–16 legacy replacements or at most 64 edits. Cap decoded UTF-8 at 256 KiB and reject NUL, extra entries, and trailing JSON. Parser scratch allocations still count against process limits. |
| Completion | Require EOF and process exit. Deadlines cover stalled request and response pipes. Cancellation and failure kill and reap the worker. |

The parent and worker enforce semantic limits independently. Legacy replies
cannot name resources. SDK replies propose effects within the invocation's
grants. Worker-supplied diagnostics and authority assertions are discarded.

## Document editing SDK boundary

API 3 needs new per-command `document.read` and `document.edit` grants.
The host gathers inputs before launch. The worker gets at most 64 KiB of
the current eligible local document, an opaque handle, and a revision string.

| Proposal | Checks |
| --- | --- |
| Text edit | Up to 64 ordered, non-overlapping edits. Match handle and revision. Validate UTF-8, CRLF, raw and normalized sizes, and final selections. |
| Selection | Up to 16 resulting selections. Recheck live context and current grants. |
| Package import | Use bounded reviewed bytes. Helpers resolve no runtime paths. |
| Private storage | Require `storage.private`, a command request, and persistent approval. Deliver that command's scalar snapshot. The host picks the bucket by approval token. |

The host and worker validate proposals independently. A storage write rechecks
approval and the input snapshot, then atomically replaces the bucket. It rejects
conflicts. Storage updates have no text or selection effect and use no text
Undo step. Update, disable, uninstall, and revocation prune revoked buckets.
Reapproval starts empty.

Unsafe control files, permissions, ACLs, contents, or quotas fail closed.
macOS rejects extended ACL grants on both files and directories. Harmless
deny entries remain allowed. Backup, sync, and administrative access are
outside the storage boundary. See the [SDK contract](plugin-sdk.md).

## Confinement and resource limits

Lua loads text and reviewed package bytes. It exposes a bounded library
allowlist. File, network, process, dynamic-loading, and editor APIs are
unavailable. Commands get 16 MiB of Lua memory and one million instructions.
The supervisor allows two seconds for commands and 400 ms for Save hooks.

| Platform | Lua worker isolation |
| --- | --- |
| Linux | seccomp syscall allowlist, descriptor restrictions, 256 MiB address-space limit |
| macOS | Signed App Sandbox helper, exact entitlements, hardened runtime, CPU cap, zero hard `RLIMIT_NPROC`, monitored 256 MiB physical footprint |
| Windows | Capability-free AppContainer, private image ACL, explicit handles, mitigations, and a single-process job with CPU/memory limits and kill-on-close |

Windows pipes use owner-only access, identification-only client tokens, and
individual write rights. See [named-pipe impersonation](https://learn.microsoft.com/en-us/windows/win32/ipc/impersonating-a-named-pipe-client).

The sandbox is an OS process boundary. Some system and container resources
remain reachable through native code. Monitored limits can overshoot. Each
invocation has its own Lua state. OS container storage and usage across editor
processes can be shared. Kernel exploits, side channels, and permitted output
correctness remain residual risks.

### Native tool process boundary

SDK 3 approves an exact native image, command/source token, profile, and
argument recipe. Treat the image as hostile. A trusted launcher selects
the filter or service role. It rejects scripts, unsafe paths, writable or
aliased images, and excessive arguments or metadata. The tool gets a read-only
image and snapshot. The live workspace stays with the host.

| Filter resource | Bound |
| --- | --- |
| Admission | Four native processes per editor. Persistent services can occupy three. |
| Input / output | 64 KiB each for stdin and stdout. Stderr is capped at 4 KiB. The host reports fixed errors. |
| Lifetime | Ten seconds wall time and two CPU seconds. Accounting and enforcement vary by platform. |
| Memory and threads | 256 MiB memory. 32 threads, with monitoring where required. |
| Unix descriptors | 64 hard limit. |

| Platform | Isolation and cleanup |
| --- | --- |
| [Linux](../crates/hedit-plugin/src/sandbox/linux.rs) | Landlock ABI 3 restricts content access to the image, projection, and required system files. seccomp restricts sockets, process creation, tracing, metadata changes, and policy changes. Address space has a hard 256 MiB cap. The launcher sets parent-death termination. |
| [macOS](../crates/hedit-plugin/src/sandbox/macos.rs) | A fresh host process closes inherited descriptors and reads up to 16 KiB of policy. It installs Seatbelt before receiving text. The policy allows the image, snapshot, and required libraries. It denies network, writes, child creation, and other-image execution. A trusted guardian and a liveness socket enforce cleanup. |
| [Windows](../crates/hedit-plugin/src/sandbox/windows/native.rs) | Capability-free LPAC, untrusted integrity, no-write-up, write-restricted primary token, Win32k lockdown, explicit handles, and a kill-on-close single-process job. The host checks token, mitigations, and job before resuming the suspended image. |

On Linux, the exact system ELF loaders are available. A tool can use them to
interpret readable code inside its sandbox. Landlock restricts file contents
and effects. Some path metadata remains visible. See
[Landlock](https://docs.kernel.org/userspace-api/landlock.html).

On macOS, same-image execution retains Seatbelt and CPU limits. Footprint and
thread monitoring allow overshoot. The fixed policy uses a private system
interface. Setup errors stop the invocation. Native CI validates supported OS
versions. The guardian remains in the process group. Loss of either side of the
liveness connection triggers termination. The editor terminates the group
before reaping its direct child.

On Windows, the job caps committed memory at 256 MiB and CPU rate at 10%.
The host samples user and kernel CPU time every 50 ms and uses a process-specific
thread-ID snapshot. Periodic enforcement and scheduling allow overshoot.
The independent wall limit still applies. The host terminates the job and
cancels overlapped I/O before releasing pipe buffers. Argument quoting is
tested by a native executable.

Windows verifies LPAC semantics with
[AccessCheck](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-accesscheck)
and positive and negative controls. The suspended process must retain the
new, ungranted restricting SID. An integrity label alone allowed writes in
a package-SID control. See
[LPAC token queries](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ne-winnt-token_information_class),
[AppContainer launch](https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer),
and [mandatory integrity](https://learn.microsoft.com/en-us/windows/win32/secauthz/mandatory-integrity-control).
Native x64/ARM64 probes cover token identity, read-only snapshots, resource
bounds, transport, and cancellation.

The supervisor kills and reaps on cancellation, bad transport, or exceeded
limits. Keep private copies and disclosure guards through cleanup. The native
suite in [`native_tool.rs`](../crates/hedit-plugin/tests/native_tool.rs) exercises the real boundary
with hostile operations and Unicode/full-size inputs. Native platform checks
are required before enabling a profile.

## Verification and change requirements

| Layer | Regression coverage |
| --- | --- |
| Codec | `runtime/protocol/tests.rs`: every request truncation, framing/type/encoding failures, byte/count boundaries, fragmented reads and I/O errors, all status bytes, escaped output, early extra-element rejection, and 10,000 deterministic mutation cases plus arbitrary byte inputs. |
| Worker | `runtime/engine.rs` and `tests/lua_plugin_runtime.rs`: no request read after failed isolation, fresh Lua state, forbidden libraries, fixed error replies, instruction/memory limits, and real confined execution. |
| Supervision | `runtime/unix.rs` and `runtime/windows.rs`: output floods, malformed status/error payloads, unsuccessful exits, incomplete delivery, stalled pipes, deadlines, cancellation, and cleanup. Unix additionally injects identity rejection before input delivery. |
| Native policy | `sandbox/` tests: direct native file/network attempts, inherited descriptor/handle denial, child-process restrictions (including a direct macOS fork attempt), resource termination, helper identity checks, and process cleanup. These test native operations independently of the Lua allowlist. |
| Broker | Editor catalog, command, and save tests: command-specific and one-time consent, unsafe/corrupt grant storage, changed installations, lock contention, revocation/reapproval, stale revisions/selections/panes, previews after policy changes, and atomic edits/Undo. |

Run the following from the repository root. On macOS, native probes require
permission to launch locally signed sandbox helpers; plain codec tests do not.

```sh
cargo test --locked -p hedit-plugin
cargo test --locked --bin hedit app::extensions
cargo test --locked --test lua_plugin_runtime --test plugin_distribution --test save_security
```

Linux and macOS CI run the workspace suites; the dedicated Windows plugin workflow
runs native x64 and ARM64 confinement, protocol, broker, and package checks. Treat
these as release requirements. Mutation tests are repeatable regression coverage,
not a substitute for continuous coverage-guided fuzzing or an independent audit.

New host operations require a threat-model update before an API change: specify
the resource and scope, deny it by default, check grants in the broker for every
operation, define revocation/cancellation, and add adversarial tests on every OS.
Prefer host-mediated operations to broad filesystem, network, or shell access.
Do not mix plugins or permission domains in one worker to reduce startup cost.
Any worker prewarming must retain one invocation per worker and demonstrate that
no document data, authority, or mutable plugin state survives reuse.

## Provider role and presentation boundary

API 4 approvals bind package bytes, triggers, document access, and optional
edit authority. Cache is bounded scalar state for the approved provider and
active document. Stop and policy or scope changes discard jobs, cache, and
results. Each callback starts a new worker.

| Boundary | Rule |
| --- | --- |
| Launch | Trusted role selected before IPC. Ready after confinement. Two seconds for startup, 750 ms for processing, EOF and exit. 200,000 instructions plus native limits. |
| Scheduling | Four global slots, per-package/window serialization, event coalescing, work limits, backoff, idle expiry, and quarantine. Unix lowers priority. Windows uses below-normal priority and a 10% CPU rate. |
| Validation | Check frame, version, request, document handle, revision, Unicode/CRLF coordinates, result kind, edit access, and aggregate bounds. Watcher failure disables registrations. |
| Native UI | Plain strings under trusted captions. Typed edits, selections, and callbacks expire after 120 seconds or a context change. No markup or arbitrary editor commands. |
| Edit effect | Reuse transaction validation, review, and Undo. Automatic callbacks cannot commit. Suppress change/save feedback for the originating package. |
| Declarative packs | Bounded language definitions and themes. The scanner is linear. Large files fall back to plain text. Custom syntax does not activate a server. |

Disclosure uses shared approval guards. Revocation and private storage writes
take exclusive guards. An unconsumed UI receipt expires after 200 ms, and Drop
releases it immediately. Prompt and preview dwell hold no guard. Host threads
wait up to 200 ms to verify the revision around guard acquisition. The UI
treats a busy document as pending. Effect application checks the actual
revision. Policy revocation cannot retract text already delivered.

Received data remains untrusted after worker validation. Conformance covers
role, request/revision, Unicode, UI result, consent, and scheduler failures.
Native probes run on macOS, Linux x86-64/ARM64, and Windows x64/ARM64. egui probes
measure CPU frames and allocation, separate from GPU presentation and input.

## Workspace and external-service implementation

Manifest APIs 1–4 retain the contracts above. API 5 / SDK 3 commands and providers
use separate, exact-source resource review. A document grant never supplies a
workspace directory or HTTPS endpoint; commands without document.read receive an
empty neutral snapshot. The [broker contract](plugin-broker.md) specifies scoped
file, recovery, watch, HTTPS and off-thread operation limits.

Explicit workspace recovery is available through Extensions settings, including
for journals whose source has since been uninstalled. It requires fresh user
review and preserves ambiguous data. Filters and persistent LSP use separate
image selection and final resource approval. API conformance, installed
references, native adversarial checks and extracted-package acceptance cover
these routes independently.

### Native image preflight and trust marks

| Stage | Checks and limits |
| --- | --- |
| Preflight | Pin a regular native executable. Inspect up to 64 KiB of ELF, PE, or Mach-O headers from an image up to 64 MiB. Reject scripts, DLLs, non-executables, bad slices, and unsupported architectures. The native loader validates the image inside the sandbox. |
| Import | Copy into a new owner-only, single-link file inside an excluded directory. Hash it and recheck source identity, times, length, and trust marks. Preserve `com.apple.quarantine` and `Zone.Identifier` up to 64 KiB. Keep original files untouched. |
| Imported store | Up to 64 records and 256 MiB of payload. Each import reserves its length plus 128 KiB for bounded metadata. Interrupted or invalid records retain reservations and cannot run. |
| Grant | Review filename, digest, scope, and complete argument recipe. Bind image, command, exact source token, and profile. Reapproval invalidates old authority. |
| Invocation | Recheck identity, times, size, header, and digests, then make a distinct executable copy. Reserve up to four bundles and 512 MiB of payload plus bookkeeping. Hold the lease until tool cleanup. |
| Project copy | Require discover and read approval. Admit visible regular UTF-8 files. Cap at 16 MiB of text, 64 KiB of paths, and 2,000 nodes. Refuse incomplete, aliased, hidden, or unsafe content. Files are read-only. |
| Retirement | Resume known retirement entries after interruption. Remove only verified private files. Preserve changed roots, unknown data, and ambiguous reservations. |

The stable store root is excluded from workspaces. Imports do not add
per-image exclusions. Damaged tool storage leaves unrelated plugin features
available. Check the selected image at setup. Hash and copy it for each native
launch. Global admission scans run on import and removal. Work spends a
256 MiB verification/copy budget and the ten-second chain budget.

Trust marks carry OS metadata. The manager does not verify reputation or a
signature during preflight. Tools see their private paths and system allowance.
Storage totals count logical payload. Filesystem metadata, OS pressure, and
abandoned AppContainer housekeeping remain availability risks.

### Language-server framing

The [shared codec](../crates/hedit-protocol/src/lsp.rs) validates frames.
The [broker owner](../crates/hedit-broker/src/lsp/owner.rs) manages the process.

| Layer | Checks |
| --- | --- |
| Header | ASCII with CRLF, one positive byte-counted `Content-Length`, optional JSON/UTF-8 `Content-Type`. Reject duplicate headers. |
| Body | Scoped frames up to 256 KiB, 8,192 values/keys and 64 nesting levels. Built-in servers have a separate 8 MiB limit. Reject invalid UTF-8, duplicate fields, trailing data, ambiguous envelopes, and out-of-range IDs. |
| Decoder | Chunks up to 4 KiB. Scan fragmented headers once. Malformed input or truncated EOF closes the decoder. |
| Owner | Exact command, approval token, policy epoch, document, image, recipe, workspace identity, and projected name. New owners reverify and copy the image. Warm requests reuse it. |

The host selects the native-service role and drives its I/O off-thread, even
when idle. At most three services share the four-process limit. Holds and
leases survive until process cleanup. Readiness waits are at most 50 ms.
Each owner has one queued and one active request.

| Resource | Service limit |
| --- | --- |
| Cumulative CPU | 60 seconds. Hard native user CPU on Windows and hard CPU on Unix, plus monitored user and kernel time. |
| Rolling CPU | Two CPU seconds per ten seconds. Three five-second buckets retain charges for ten to fifteen seconds. |
| Wall lifetime | 30 minutes. Startup and individual requests have ten-second deadlines. |
| Input / output | 8 MiB each per ten seconds |
| Stderr | 4 KiB per ten seconds. Discard after accounting. |
| Requests | 32,768 non-reused IDs |
| Memory / threads | 256 MiB and 32 threads, with the filter policy's enforcement and overshoot |
| Restarts | Explicit provider restart after failure or exhaustion |

Sample CPU at most every 50 ms using platform counters. Counters must increase
monotonically. Accounting failure closes the connection. Windows keeps its
10% job limit. Starting a new callback does not reset the native owner's
limits.

| Protocol operation | Behavior |
| --- | --- |
| Document | Issue one private URI and increasing LSP versions for one opaque document. Overlay current unsaved text in the approved projection. A new owner is needed to refresh other files. |
| Diagnostics | Require the exact issued URI and version before retention. Drop stale, unversioned, and foreign results. Pull diagnostics reject other documents. |
| Positions | Convert UTF-8, UTF-16, or UTF-32 through an indexed snapshot. Reject splits inside UTF-8, surrogate, or CRLF boundaries. |
| Requests | Check delivery before accepting a reply. Reject foreign, duplicate, unsolicited, or error replies. Server-initiated requests receive `MethodNotFound`. Cap an exchange at 2 MiB and 64 incoming messages. |
| Typed results | Validate diagnostics, plain completion, hover, symbols, format, and current-document edit actions. Preserve the SDK snapshot and review rules. Hover is plain text. Snippet and additional completion edits, commands, resource operations, and cross-document actions are unsupported. |
| Supersession | Drop unsent work. Finish a partial frame while its guard is held. Send `$/cancelRequest` for delivered work. Keep at most 16 discarded IDs pending. Unknown or repeated replies and exhausted tombstones close the connection. |

Only a sealed broker grant starts a service. A server URI is a key, with
no file-open or edit authority. Source and scope changes close leases even
without UI polling. Closing or shelving a tab ends its lease. Reopening
creates a new one. Stop, revocation, and updates end persistent registries.
Keep policy subscriptions bounded to 32 entries. Failed starts stay recorded
until explicit scope restart.

The I/O thread acknowledges cancellation before the caller releases its guard.
Document or policy closure kills and reaps the process before bundle retirement.
The host checks both callback and owner scope before sends and results.
Startup, request, and idle traffic all spend native budgets. The shipped JSON
formatter and project LSP exercise this installed path.

## Scoped broker effects and recovery

The [broker contract](plugin-broker.md) defines the API and exact quotas for
discovery, search, watchers, batched reads, reviewed existing-file edit plans and
endpoint-scoped HTTPS. Resource grants are independent of source approval and
document grants. Physical root identity, exclusions, relative-path validation,
file identity and content versions are checked at use; a worker cannot choose an
ambient root or turn a read into an edit.

Workspace previews confer no authority. Application checks current policy and
all file preconditions, then uses a recoverable journal and per-file receipts.
Filesystems do not provide an atomic multi-file commit: partial failures require
explicit recovery, and there is no global multi-file Undo. Unknown, aliased or
corrupt recovery data is preserved rather than recursively deleted. Recovery
does not silently overwrite externally changed files.

HTTPS approval fixes the exact public URL, method, content type and upload/
response limits. No redirects, proxy settings, arbitrary headers, cookies,
compression or ambient credentials are exposed. DNS answers must all be public;
the broker pins them, verifies TLS hostname and connected peer, and applies
deadlines, rate limits and cancellation. Combining reads with network access
explicitly discloses the data that may leave the editor. Public-endpoint trust
and approved output correctness remain outside sandbox enforcement.

Coverage-guided AddressSanitizer campaigns run separately for SDK worker replies
and shared LSP/data JSON, retaining corpus, logs, hashes and source snapshots.
Native adversarial probes, real editor integration and extracted-package tests
remain required: a clean parser campaign cannot establish sandbox enforcement.
The [broker validation guide](plugin-broker.md#external-tools-and-validation)
describes reproduction commands and the scope of the measurements.
