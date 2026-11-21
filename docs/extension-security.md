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
single-process job with CPU/memory limits and kill-on-close cleanup.

These are OS process sandboxes, not virtual machines. App Sandbox and AppContainer
permit some system/container resources; a native runtime exploit is therefore
not equivalent to the restricted Lua API. Fresh processes eliminate Lua state
reuse but do not promise separate OS container storage for every invocation.
Parent-polled limits can overshoot between checks, and per-invocation limits do
not bound aggregate usage across multiple editor processes. Kernel exploits,
side channels, and correctness of permitted output remain residual risks.

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
