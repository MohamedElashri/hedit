# Extension threat model

This document describes the current Lua extension boundary and the requirements
for changing it. The [extension reference](extensions.md) defines the author API;
the [developer guide](../DEVELOPER_GUIDE.md#lua-extension-architecture-and-testing)
describes builds and native sandbox adapters. Declarative snippets and templates
are validated data interpreted by the editor; they do not execute Lua.

## Assets and adversaries

Protect unapproved document contents, other buffers, credentials, user files,
network services, editor integrity, and the availability of the editor UI.
Treat installed extension source, manifests, bundles, and worker output as
untrusted. An extension may be malicious even when its publisher signature is
valid. Assume an attacker can construct malformed messages and, when evaluating
the parent boundary, can control the worker after a Lua/runtime compromise.

The trusted computing base includes the editor's broker, permission storage and
installation code, bounded protocol parser, worker launcher, native sandbox
adapters, their dependencies, and the operating system. The worker's Lua library
allowlist is an additional restriction, not a substitute for OS confinement.

The model does not protect against a compromised editor, kernel, or administrator,
or a process that already controls the user's configuration and editor binaries.
Same-account malware is outside this boundary. Filesystem policy checks still
reject known unsafe control-file layouts; those checks do not make configuration
storage resistant to an attacker with the user's full authority.

## Process and authority boundaries

Every invocation executes in a fresh child process. Commands, save hooks, and CLI
fixtures use the same engine and native isolation path. There is no shared host
containing multiple plugins, persistent Lua state, or in-process fallback.

The broker is currently the editor's catalog/consent code, command/save routing,
and runtime supervisor. It is not a general RPC service. Legacy workers receive
the selected strings (the whole eligible local buffer for a save hook), language
ID, declared scalar settings, and source. SDK workers instead receive a bounded
current-document snapshot, declared arguments, reviewed package resources, and
private command values only when explicitly approved. Neither contract supplies
an editor object, document path, approval file handle, or authority to choose a
different buffer. Workers return replacement strings or typed SDK proposals;
the editor owns resource selection, authorization, and committing effects.

| Boundary | Required behavior |
| --- | --- |
| Installation → executable contribution | Validate bounded manifests and source; review installed bytes; bind approvals to command, capabilities, digest, installed file identity, and a fresh token. Publisher signatures establish provenance and integrity, not permission or safety. |
| Consent → text delivery | Lua starts unapproved. Check current installation, enabled state, and command approval while holding the extension change lock. Retain the authority handle through delivery, without inheriting it into the worker. |
| Launcher → worker | Clear unnecessary environment and inherited handles/descriptors. Verify the native boundary before delivering text where supported; the worker must establish/check confinement before reading its request. Failure disables the invocation. |
| Worker → editor | Bound transport bytes and parse the response independently of worker validation. Require successful process exit, complete request delivery for success, valid framing, exact edit count, and valid replacement text. |
| Proposal → document mutation | Recheck approval and target document revision, pane, page, language, and selections. Hold authority through the atomic edit. A preview does not grant new authority. |
| Save binding → whole-buffer access | Require persistent command approval and an explicit language-specific binding to its approval token. Recheck the binding and approval before execution and application. Skip remote, read-only, paged, and oversized inputs. |

Allow-once authorization is ephemeral and claimable once, with policy revisions
checked through execution and application. Updates, disablement, removal,
revocation, and reapproval invalidate stale authority. Revocation serializes with
delivery and application; it cannot retract bytes already delivered or undo an
edit already committed. An existing worker may finish computing after revocation,
but its proposal must pass authorization again before application.

Approved code can deliberately return incorrect or destructive replacement text
within its permitted selection. Preview and Undo help users review and recover;
they do not establish semantic correctness. Explicit save-hook consent permits
automatic replacement without a preview. Failed hooks preserve the editor text
and continue the ordinary save path.

## Private worker protocol

The protocol in `crates/hedit-plugin/src/runtime/protocol.rs` is internal to a
matching editor/worker build, not a public extension API. There is one request and
one reply per process, with no callback messages. SDK replies may propose only
bounded document edits or a separately approved private-storage update.
The editing SDK uses distinct request magic `0xfffffffd` and an independently
checked `sdk_version`. Selection-transform requests retain their existing magic
and payload. No fallback translates document authority into selection authority.

- Request: little-endian `u32` magic `0xfffffffe`, little-endian `u32` JSON byte
  length, that exact payload, and EOF. Reject unknown/duplicate top-level fields,
  wrong types, invalid encodings, oversized lengths, truncation, and trailing
  bytes. The payload cap is 1 MiB; source, selections, language, and settings also
  have independent semantic limits checked by both encoder and worker decoder.
- Legacy reply: one status byte followed by a JSON array of strings on success (`0`).
  Worker errors (`1` through `11`) have no body; unknown statuses and error bodies
  are rejected. Diagnostics use fixed host messages, not worker-supplied error
  text that might contain document contents.
- SDK reply: status `0` followed by a strict versioned success/failure envelope.
  A successful transaction is bound to the original document and revision and is
  revalidated by the parent. Failures carry only fixed categories and an optional
  syntax location verified against the original script/module and line count.
  Arbitrary runtime messages and tracebacks are never displayed. Transport or
  confinement failures continue to use bodyless fixed status codes.
- The parent caps reply bodies at 2 MiB to accommodate JSON escaping. Parsing
  allocates bounded vectors (the expected 1–16 legacy replacements or up to 64
  SDK edits), rejects additional elements
  before deserializing them, and enforces the 256 KiB combined decoded UTF-8 limit
  as strings arrive. NUL, missing edits, non-string values, and trailing JSON are
  rejected. A bounded frame still permits parser/string scratch allocations
  within that frame size; the decoded text limit is not a total allocator limit.
- EOF and process exit are part of completion. A status or partial response cannot
  bypass cancellation or deadlines. A request pipe that stops accepting bytes or
  a worker that leaves output open must also terminate within the supervisor's
  budget. Cancellation and failure kill and reap the child.

Legacy responses cannot name resources or request host operations. SDK responses
can echo only their invocation's opaque target and propose typed effects within
that command's grants. Neither response grants permissions, supplies native code,
or chooses arbitrary files. Validate each effect in the broker rather than
trusting a sandboxed sender.

## Document editing SDK boundary

Manifest API 3 requires fresh command-specific `document.read` and `document.edit`
approval. It receives only the current eligible local snapshot, up to 64 KiB;
its random document handle reveals no host document ID or filesystem path. The
revision is an opaque string token. Existing selection grants cannot authorize
this snapshot. Host-owned argument forms run before worker launch.

The worker proposes at most 64 ordered non-overlapping edits and 16 resulting
selections. Parent and worker independently validate target handle/revision,
UTF-8 and CRLF boundaries, raw/normalized replacement limits, and final selection
positions. Parent application also checks live editor context and current grants.
A forged worker cannot select another document. No SDK helper exposes filesystem
paths, networking, subprocesses, or editor objects. The Lua core and native
sandbox policy are shared with legacy transforms. Verified package-local imports
and text resources use only bounded reviewed bytes; no runtime paths are resolved.
See the [SDK contract](plugin-sdk.md).

Private command storage requires `storage.private` plus an explicit per-command
request and persistent approval. Only that command's quota-limited scalar snapshot
is delivered. Its reply may propose a storage update only when storage authority
was supplied; both parent and worker check this. The host chooses the bucket from
the approval token, holds authorization through its atomic write, and compares
current state with the original snapshot to reject conflicts. Storage updates
cannot be combined with document edits or selection effects. They never enter
text Undo history. Revocation/update/disable/uninstall prune revoked buckets;
reapproval does not inherit the previous bucket. Symlink/reparse control files,
unsafe permissions/ACLs, invalid contents, and quota violations fail closed.
macOS extended ACL grants are rejected on both the storage directory and files,
even when their mode bits are owner-only; harmless deny entries remain allowed.
Administrative access and user-configured backup/sync remain outside this boundary.

## Confinement and resource limits

Lua exposes a small text-processing library allowlist, with no file, network,
process, filesystem module search, dynamic-code-loading, or editor APIs.
SDK imports load only reviewed package bytes already delivered by the host. Source is loaded
as text. Each invocation is limited to 16 MiB of Lua allocations and one million
instructions. The supervisor allows two seconds for commands and 400 ms for save
hooks, in addition to native memory and CPU restrictions.

Native policies differ. Linux uses a seccomp syscall allowlist, descriptor
restrictions, and a 256 MiB address-space limit. macOS uses a signed App Sandbox
helper with exact entitlements, hardened runtime, a CPU cap, a zero hard
`RLIMIT_NPROC` limit to deny child-process creation, and a parent-polled
256 MiB physical-footprint check. Windows uses a capability-free AppContainer,
private worker image ACL, explicit inherited handles, process mitigations, and a
single-process job with CPU/memory limits and kill-on-close cleanup. Its local
byte pipes have owner-only access and identification-only client tokens, so a
child holding stdin's server end cannot act with the editor's authority through
pipe impersonation. A native regression keeps the parent client connected while
the confined child attempts to read a private parent file through that token.
The client requests individual write rights to avoid the named-pipe instance
creation right included in generic write access. See Microsoft's
[named-pipe impersonation rules](https://learn.microsoft.com/en-us/windows/win32/ipc/impersonating-a-named-pipe-client).

These are OS process sandboxes, not virtual machines. App Sandbox and AppContainer
permit some system/container resources; a native runtime exploit is therefore
not equivalent to the restricted Lua API. Fresh processes eliminate Lua state
reuse but do not promise separate OS container storage for every invocation.
Parent-polled limits can overshoot between checks, and per-invocation limits do
not bound aggregate usage across multiple editor processes. Kernel exploits,
side channels, and correctness of permitted output remain residual risks.

### Native tool process boundary

Experimental API 5 / SDK 3 filter requests use separately imported and reviewed
native images on macOS, Linux and Windows. Source approval alone does not grant a
tool. The broker binds an approved image to the exact command/source token,
profile and argument recipe, then constructs an admitted private invocation.
Language-server requests remain unavailable until their protocol and lifetime
implementation is complete.

Treat the selected native image as hostile code. The trusted host supplies a
private read-only executable copy and private project snapshot, never a live
workspace directory. The launcher rejects noncanonical paths, symlink images,
shared or writable executable files, scripts, excessive arguments and oversized
metadata. Ownership and source/digest approval remain host responsibilities.
Workspace writers and other processes running as the user remain outside this
boundary; private-copy integrity must be checked by the broker before launch.

Before reading document input, a fresh editor process closes inherited
descriptors, reads at most 16 KiB of policy metadata and applies a fixed Seatbelt
policy. Only after its ready acknowledgement does the parent deliver text. The
image receives no ambient environment, network, filesystem writes, child-process
creation or execution of another image. It can read its own image, the snapshot
and required system libraries. Same-image exec retains the policy and CPU limit.
No shell or user-supplied policy text is interpreted.

Four finite filters can run per editor process. Input and stdout are each capped
at 64 KiB; stderr is capped at 4 KiB and is not rendered as a trusted host error.
Each run has a maximum ten-second wall deadline, two-second hard CPU limit,
64-descriptor hard limit, and monitored limits of 256 MiB physical footprint and
32 threads. Cancellation, protocol failures and resource violations kill and reap
the process. Sleeping occurs only when no transport progress is possible.

The selected macOS model uses native isolation with monitored limits. Memory and
thread use can overshoot between observations, and resource pressure can affect
the host before termination. This is not a hard memory guarantee or a VM boundary.
The fixed policy uses the system sandbox library's deprecated/private policy
interface; OS updates may make it unavailable. Initialization errors fail closed,
and native CI must validate supported macOS versions. This does not broaden the
signed Lua helper's App Sandbox entitlements.

The Linux adapter shares the Unix bootstrap, transport and supervisor. It requires
Landlock ABI 3 to deny content access outside the private snapshot/image and
system libraries, including write, rename and truncation rights. A seccomp
allowlist denies sockets, process creation, namespaces, tracing, asynchronous
kernel I/O, shared-memory APIs, metadata mutation and policy changes. Only
pthread-style clone flags are permitted, with clone3 returning ENOSYS for libc's
checked fallback. File-control commands exclude locks and signal-owner changes;
thread-directed signals are restricted to the tool's process. Address space is
hard-limited to 256 MiB. Runtimes requiring larger virtual reservations fail
unavailable instead of receiving a weaker policy.

Linux exec permits the selected image and exact system ELF loaders needed for
startup. Native code can use those loaders to interpret other readable code
inside the same sandbox, just as it can already execute code in its address
space. Initial image selection and digest remain host authority. Landlock does
not conceal arbitrary path metadata; it restricts file contents and effects.
These residuals must be disclosed in tool review rather than described as total
filesystem or code-provenance isolation. See the kernel's
[Landlock documentation](https://docs.kernel.org/userspace-api/landlock.html).
Linux sets parent-death termination before native exec. macOS arms a trusted,
event-driven guardian before confinement and text delivery. A private liveness
socket binds the guardian to the editor. EOF terminates the native process; loss
of the guardian terminates the process group from the editor. Native exec runs
as a session leader, so the OS prohibits session/group changes independently of
Seatbelt. The guardian remains in that group, anchoring its identity during
cleanup. The editor observes exit without reaping, terminates the group, then
reaps its direct child; after editor death, the system reaper collects orphaned
processes. No untrusted tool holds a guardian control descriptor. Native tests
kill the simulated editor and guardian separately and verify both processes are
gone. Broker disclosure guards remain held through native execution and cleanup.

The Windows native-tool path uses the existing launcher with a distinct role:
a capability-free less-privileged AppContainer (LPAC), an untrusted integrity
label with no-write-up policy, a separate write-restricted primary token, Win32k lockdown, child-process restrictions,
explicit inherited handles and a kill-on-close single-process job. Parent-side
checks run while the image is suspended; only after token, mitigation and job
verification does it resume. The job limits private and job-wide committed
memory to 256 MiB, user CPU time to two seconds and CPU rate to 10%. Thread count
is monitored with a bounded host-side scan. The host also samples user CPU time
every 50 ms because Windows enforces job CPU-time limits periodically; the
configured two seconds is not a promise of zero overshoot. The finite ten-second
wall deadline is independent of CPU throttling. Kernel objects, system services and
shared OS resource pressure remain residual denial-of-service risks.

Native stdin/stdout/stderr use parent-owned overlapped pipe ends. Cancellation
terminates the job and cancels pending local pipe operations while keeping their
buffers/events alive through completion. No blocked I/O thread is abandoned.
Client tokens retain identification-only quality of service and individual
access rights. MSVCRT argument quoting is tested by an actual native executable;
no shell interpolation is used. Snapshot trees are validated for regular files,
reparse points, entry count and aggregate size before ACL propagation. The host
must supply immutable private image/snapshot copies and retain their ownership.

Before resuming the suspended process, the host verifies LPAC authorization
semantics with `AccessCheck` on an identification duplicate of its token:
Everyone alone and Everyone plus ALL_APPLICATION_PACKAGES must deny read;
Everyone plus ALL_RESTRICTED_APPLICATION_PACKAGES must grant it. This uses the
[documented LPAC definition](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ne-winnt-token_information_class)
and [AccessCheck](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-accesscheck),
with both positive and negative controls. The class-46 token query is rejected
by the tested Windows kernels and is not used as an authority check.

The host creates a restricted version of its own primary token with privileges
removed and a fresh, ungranted restricting SID, then uses `CreateProcessAsUserW`
with the LPAC attributes. The restricting SID must survive in the suspended
child's token. This extra restriction is necessary: a native control demonstrated
that lowering the AppContainer token label alone did not prevent writes through
explicit package-SID grants. The integrity label is defense in depth, not the
sole enforcement of read-only snapshots or profile storage.

Native Windows validation is an activation gate. A low-integrity LPAC control
receives write access to a synthetic low-labelled file; the untrusted tool token
must fail the same write and must fail attempts to raise its integrity or disable
mandatory policy. Dedicated x64/ARM64 probes additionally check token identity,
read-only snapshots, private-file denial, network/child restrictions, memory,
output, thread/CPU limits, Unicode arguments, full-size overlapped transport and
cancellation. Native platform checks are required before enabling a tool profile.
See Microsoft's [AppContainer launch guidance](https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer)
and [mandatory integrity model](https://learn.microsoft.com/en-us/windows/win32/secauthz/mandatory-integrity-control).

`crates/hedit-plugin/tests/native_tool.rs` runs the actual native image through
the production boundary: Unicode/full-size input, literal policy parameters,
outside reads and symlink escapes, writes and metadata changes, inherited
descriptors, network attempts, fork, different-image exec, same-image reexec,
output floods, CPU/memory/thread violations, cancellation and child reaping.

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

API 4 providers add an explicit grant for current-document reads, reviewed event
triggers, and host-rendered presentation. Editing is a separate declared grant.
Selection commands never inherit these permissions. Provider source, modules,
resources, trigger definitions, and capabilities are covered by the exact-copy
approval. Private provider cache is bounded scalar data, scoped to a provider's
grant and active document. It is not a persistent shared Lua VM or a filesystem
namespace. Scope/policy changes and Stop discard cache, jobs, and results.

The native service role is selected by trusted launch arguments before IPC is
read. After isolation, it sends one fixed readiness byte. Startup has a two-second
deadline; processing/EOF/exit then has a 750 ms deadline. The callback instruction
budget is 200,000; native memory/CPU ceilings and file/network/child restrictions
remain. Rotation prevents a long-lived VM from outliving cumulative CPU limits.
Unix workers run at nice >=10; Linux denies priority changes and hard-limits
RLIMIT_NICE. Windows jobs require below-normal priority and a 10% CPU-rate cap.
The broker adds four global admission slots, per-package/window serialization,
coalesced requests, rate/work budgets, idle expiry, backoff, and quarantine.
These controls mitigate resource exhaustion; they do not make scheduling delays
or native accounting overshoot impossible. Application Sandbox residual access
and native-runtime-exploit limitations described above still apply.

The parent validates role, framing/EOF/exit, protocol version, original request,
opaque document handle, revision, coordinates, result kind, edit authority, and
aggregate UI bounds independently. Revocation is serialized with disclosure;
a fresh check guards result publication and another check guards edit effects.
A watcher cancels stale registrations/cache/results; watcher failure disables
providers. Revocation cannot retract text already delivered to a worker.

The editor renders plain strings in native widgets under trusted owner captions.
Result actions are typed edit/select/callback values with owner/request/snapshot
lifetimes and a two-minute expiry. No plugin string names an internal command,
loads markup, fetches images, or opens a URI. Editing actions reuse the SDK
transaction validator, review policy, authorization recheck, and Undo. Stale
results are dropped rather than rebased. Provider-origin changes/save events are
suppressed for that package, and automatic callbacks cannot commit text.

Declarative syntax/theme packs need no executable grants. They use the existing
native color-role parser and a bounded linear keyword scanner, without untrusted
regex execution. Large custom-syntax pages fall back to plain text. Malformed
resources, unsafe names/associations, oversized definitions, and unknown fields
are rejected during installation. A custom syntax cannot launch a language tool.

Conformance coverage includes role confusion, reply truncation/duplicates,
foreign requests/documents/revisions, Unicode/UTF-16/CRLF boundaries, malicious UI
fields, bounded arrays/cache, event storms, late completion after Stop/Resume,
failure quarantine, native confinement, and revoked/stale quick-fix effects.
The roadmap gate also requires native checks on Linux/Windows and controlled
active-provider frame/allocation measurements before claiming Stage 2 complete.

Provider disclosure guards use shared locks, while revocation and persistent
storage changes require exclusive locks. Computation and publication checks share
the four-slot admission budget. A result waiting for the UI receives a 200 ms
authorization receipt; the existing bounded authorization thread releases an
unconsumed guard even if the window stops pumping frames. Dropping a receipt
releases it immediately. The UI takes a live guard without blocking and consumes
it in the authorized operation; expired receipts produce no effect. Prompt and
preview dwell hold no authorization lock. The editing SDK's approval handoff uses
the same receipt policy. Tests cover unpumped/expired/dropped receipts and the
retention of a consumed guard through its authorized operation.

## Workspace and external-service implementation

Manifest APIs 1–4 retain the contracts above. The integration branch also accepts
experimental API 5 / SDK 3 commands and providers with separate, exact-source resource
review. A document grant never supplies a workspace directory or HTTPS endpoint;
commands without document.read receive an empty neutral snapshot.
Its [broker threat model](plugin-broker.md) describes the scoped file, recovery,
watch, HTTPS and host-thread boundaries, current limits and remaining gates.
Explicit workspace recovery is available through Extensions settings, including
for journals whose source has since been uninstalled. It requires fresh user
review and preserves ambiguous data. Native filters use separate executable
selection and final resource approval. Persistent LSP, author tooling and final
platform/performance acceptance remain implementation gates. The scoped command path is not a declaration that
Stage 3 or the complete roadmap is finished.

### Native image preflight and trust marks

Host file selection pins a regular native executable and classifies bounded ELF,
PE or Mach-O headers, including macOS universal slices. Classification reads at
most 64 KiB of headers from an image capped at 64 MiB; it does not load code,
validate signatures or run a version command. The native OS loader performs
complete executable validation only inside the tool boundary. Scripts, DLLs,
non-executable headers, unsupported architectures and inconsistent universal
slice tables are rejected.

Import streams bytes into a newly created, owner-only, single-link destination,
hashes the copy and rejects source identity, timestamp, length or trust metadata
changes. Original images are never modified. macOS `com.apple.quarantine` and
Windows `Zone.Identifier` are preserved byte for byte, capped at 64 KiB. Source
ACLs and positive sandbox grants are not copied. Destination pathname identity is
checked before named-stream operations. These primitives require a host-owned,
excluded parent and prior storage admission; they do not grant execution authority
by themselves. Execution additionally requires an exact-source resource approval.

Imported images use a durable private store with at most 64 records and 256 MiB
of reserved payload plus metadata allowance. Each image reserves its declared
length plus 128 KiB before copying; the allowance covers bounded trust marks,
receipts and bookkeeping, rather than claiming a physical filesystem-block
quota. Interrupted imports retain their reservation and cannot execute. Missing,
foreign or corrupt admission data is preserved and blocks further admission.

Image records bind the imported digest to the command, exact source approval,
profile and serialized argument recipe. Resolution rechecks identity, timestamps,
length, header, content digest and trust-mark digest. Removal requires the same
owner and uses a durable retirement name so restart can finish bounded cleanup.
Only known regular, owner-only, single-link files are removed. The stable store
root is excluded before image bytes arrive; importing and retiring images does
not append per-image entries to the permanent workspace exclusion registry.
Tool storage is opened separately from ordinary broker state, so damaged tool
storage does not disable unrelated document/workspace features. Callback setup
checks only the selected image's bounded metadata; each execution hashes and
copies it again. Global admission scans run when imports are added or removed,
rather than on every rotated callback.


Per-invocation bundles reserve disk admission separately from imported images:
at most four live or abandoned bundles and 512 MiB of reserved image/snapshot
payload plus bookkeeping allowance. A bundle uses a distinct executable copy;
Windows invocation ACLs never touch the persistent imported image. Its OS lease
stays with the host I/O owner until the native supervisor has killed/reaped the
process. Parent-death handling remains the native adapter's responsibility.
After host death, a later admission can reclaim an unlocked bundle; unknown,
aliased, oversized or changed-root data is preserved rather than recursively
deleted. Durable retirement names allow cleanup to resume after interruption.

Project snapshots contain the explicitly approved workspace surface: visible
safe regular UTF-8 files, with no hidden paths, symlinks, aliases or protected
storage. They require both discover and read approval. Projection refuses
truncation, binary/oversized/unreadable files, more than 16 MiB retained text,
64 KiB of relative path text or 2,000 file/directory nodes. It never presents a
silently truncated project to a tool. Files are immutable copies; live project
changes do not mutate a running tool's view. Native policy prohibits writes;
there is no scratch directory or live workspace mount.

These are bounded logical storage budgets with explicit entry limits, not a
filesystem block quota. OS metadata allocation and filesystem/kernel pressure
remain availability risks. Tool grants and persistent LSP activation require
the broker's exact-source policy and lifetime integration in addition to these
primitives.


The resource UI presents the selected filename, digest, data scope and complete
literal/finite-choice argument recipe before final approval. Importing an image
creates no execution grant. Revocation/reapproval changes the source token, so a
replacement approval cannot reuse the old image's authority. Unused private
imports can be explicitly removed; the original selected files are untouched.
Native output must be UTF-8 and satisfy the independent broker reply limits;
nonzero exit, malformed/flooded output or failed confinement cannot produce edits.
The session charges image verification/copy work against a 256 MiB aggregate
budget as well as the ten-second active-work budget.

Trust-mark preservation is not signature or reputation verification. Approved
native programs can observe their private image/snapshot locations and minimal
system environment; opaque document handles do not conceal those OS paths.
OS-managed objects and AppContainer profile cleanup after abnormal host exit
remain availability/housekeeping concerns, distinct from protected document
copies and kill-on-parent-exit enforcement.

### Language-server framing

The shared LSP codec accepts ASCII CRLF headers with one positive byte-counted
Content-Length and an optional JSON/UTF-8 Content-Type. It rejects duplicate
headers, duplicate JSON fields at any depth, invalid UTF-8, trailing JSON,
ambiguous JSON-RPC envelopes and out-of-range request IDs. Scoped messages are
limited to 256 KiB, 8,192 JSON values/keys and 64 nesting levels; built-in editor
servers retain their separate 8 MiB message budget. Both use the same validation.
The incremental decoder bounds incoming chunks to 4 KiB, scans fragmented
headers once and permanently fails after malformed input or truncated EOF.

This codec supplies no process, file, document or method authority. Persistent
plugin LSP activation still requires a confined native owner, request correlation,
issued-URI validation, revision checks and revocation/cleanup integration. The
built-in server launcher does not satisfy those plugin authority requirements.

The host-only duplex transport uses the existing native tool policy and private
run bundle. It reserves at most three background native owners within the shared
four-process cap. Each owner retains its image/snapshot lease until after native
termination/reaping; cancellation, deadline, closed pipes or accounting failure
permanently close the transport. The host must continuously drive the owner,
including while idle. Readiness waits are bounded to 50 ms for cancellation and
resource checks, with kernel pipe events instead of a busy loop.

The native cumulative two-second user-CPU ceiling remains in force. Duplex owners
also expire after 120 seconds and at 16 MiB in each transport direction, with a
4 KiB lifetime stderr budget. These are fixed host limits, not worker-selected
values. No implicit restart resets them. This transport alone does not activate
LSP grants or establish a persistent document/service owner.

The scoped JSON-RPC controller serializes at most 256 requests per connection
with non-reused host IDs. It rejects responses before full request delivery,
foreign/duplicate/unsolicited responses and error responses. Any cancellation,
timeout, malformed frame or protocol failure destroys the native owner, so a
late response cannot attach to a replacement request. Each exchange bounds
traffic to 2 MiB and 64 incoming messages; outgoing frames and retained raw
diagnostics have separate count/byte budgets.

Server-initiated requests receive MethodNotFound. In particular, a server cannot
invoke workspace edits, execute commands, register capabilities or request
configuration through this controller. Diagnostic notifications remain untrusted
data until document URI/version/range and publication authority are checked.
This protocol layer still does not activate installed plugin LSP profiles.

Typed LSP translation uses one exact host-issued file URI, an independent positive
LSP version and the immutable SDK document/revision. Server URIs are compared as
keys and never opened, canonicalized or passed to built-in workspace edit code.
Published diagnostics require the exact issued version; unversioned, stale or
foreign-document notifications cannot produce a result. Pull diagnostics reject
cross-document reports. UTF-8/16/32 offsets use an index of valid snapshot
boundaries; surrogate, byte and CRLF-interior splits fail without rescanning the
document for every returned item.

Diagnostics, plain completions, hover, symbols, formatting and current-document
edit actions use the shared SDK result validator before reaching Lua. Formatting
and actions retain the exact SDK document/revision and pass its edit/preview
rules. Snippet/defaulted completion edits, additional completion edits, server
commands, resource operations and cross-document actions are unsupported by this
translator and fail closed. Hover markup is plain native-rendered text. These
typed primitives still require persistent authority/lifecycle integration before
installed LSP profiles can be enabled.

The persistent native owner drives I/O on a dedicated host thread, including
while parked. Its one queued request and current request are bounded; UI Drop
only signals cancellation. The I/O thread kills/reaps before retiring the private
bundle. Scope cancellation is independent of a callback's cancellation, and both
are checked before transfers and again before returning a result. Startup and
requests cannot exceed ten seconds; transport lifetime/CPU/traffic ceilings still
apply. Failed connections never restart implicitly.

An owner accepts one opaque document ID and increasing SDK revisions. It sends
didOpen/didChange with an independent increasing LSP version, clears old
diagnostics and rejects rollback, foreign documents or a changed language.
Current unsaved text overlays an already-approved projected file, or receives a
synthetic private filename without the original editor path. The underlying
project copy stays fixed for that owner's lifetime. Refreshing other project
files requires an explicit new owner.

Unversioned/foreign diagnostic notifications are filtered before retention.
Idle message/byte accounting persists across polls, so an output flood cannot
reset its budget with every wakeup. Installed LSP activation still requires the
source/resource binding, policy subscriptions and editor scope owner; these
host-only lifetime primitives do not confer a plugin grant.

The broker now binds cached native owners to command, exact source-approval token,
policy epoch, opaque document, imported image handle, complete profile recipe,
physical workspace identity and projected document name. A different image or
scope cannot reuse a cached process, even if a caller supplies an old epoch.
New owners reverify/copy the image; warm requests do not repeatedly hash/copy it.
Failed native starts leave bounded tombstones until explicit scope restart.

Editor document leases close when a tab is shelved, forgotten or replaced, even
without UI polling. Reopening creates a distinct lease and cannot revive old
callbacks. Policy cancellation and document leases are independent inputs to the
native I/O owner. Providers keep their registry across callbacks for one document;
Stop, scope changes, source updates and revocation end it. Policy subscriptions
deduplicate persistent registries and are bounded to 32 entries alongside rotating
contexts; native process/context admission limits are unchanged.

Language-server recipes currently use literal startup arguments. Native UI
selection remains gated while cancellation/reply draining, installed references
and final author/package/performance validation are completed.
