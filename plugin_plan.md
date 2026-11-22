# Plugin system implementation plan

## Goal and scope

Build a native hedit extension platform that supports the same broad categories
of plugins as Sublime Text: advanced editing commands, language assistance,
syntax and theme packs, editor annotations, project tools, formatters, and
external integrations. Design the API around hedit's document model, native UI,
shared Lua runtime, and explicit user consent.

This is a forward-looking implementation plan against the current v0.7.0
extension work. Proposed API names, module boundaries, and new limits below are
not shipped contracts. Record their final definitions in the SDK reference
before implementing dependent features.

Success means authors can build useful plugins in these categories with a
stable, documented SDK, and users can understand and restrict their authority.
Every supported platform must enforce the same API semantics, with native
sandbox adapters supplying the OS-specific boundary.

### Explicit non-goals

- Running Sublime Text packages or implementing its Python API, package format,
  object model, or compatibility shims.
- Adding Python, Node.js, browser scripting, arbitrary native libraries, or a new
  general-purpose runtime as a prerequisite for this roadmap.
- Giving plugins unrestricted filesystem, network, subprocess, environment,
  clipboard, credential, or editor-internal access.
- Maintaining independent plugin APIs or Lua engines for three operating systems.
- Replacing existing editor subsystems solely to expose extension interfaces.
- Building a public marketplace, automatic dependency downloader, or cloud account
  system before the local platform is stable.
- Executing plugin code on SSH hosts or granting access to remote buffers
  implicitly. Remote extension placement and data access need a separate design.

## Current foundation

The existing system already provides declarative snippets and file templates,
Lua selection transforms, typed scalar settings, command conditions, previews,
Undo, explicit language-specific save bindings, signed bundles, and CLI fixtures.

The shared `hedit-plugin` crate owns the Lua engine and bounded worker protocol.
The editor owns installation, consent, routing, and applying edits. Each Lua
invocation uses a fresh worker. Linux, macOS, and Windows adapters establish
native confinement before plugin source is evaluated. The current implementation
also validates replies independently, requires Unix output EOF, and prohibits
native child-process creation in the macOS helper.

Current Lua limits include 16 ranges, 64 KiB combined input, 256 KiB combined
output, 16 KiB source, 16 MiB Lua allocations, one million instructions, and a
two-second command deadline or 400 ms save-hook deadline. Keep these contracts
for existing extensions. New SDK functionality must have separately documented
limits and consent; it must not silently expand old grants.

The authoritative current contracts are `docs/extensions.md`,
`docs/extension-security.md`, and the Lua architecture section of
`DEVELOPER_GUIDE.md` in the v0.7.0 implementation branch. This plan does not replace
those documents or claim its proposed capabilities already exist.

## Architecture and security rules

### Shared components and responsibility

Use this logical structure throughout the three stages:

```text
Extension package and declared contributions
                  |
          Host installation review
                  |
       Command / provider / event routing
                  |
       Capability broker and document services
                  |
      Bounded, versioned private IPC transport
                  |
        Isolated Lua worker or plugin service

The host renders UI, owns documents, and commits authorized effects.
External tools run in separately supervised processes with their own policy.
```

Keep transport, protocol validation, Lua bindings, capability semantics, and
scheduling shared. Limit OS adapters to launching and identifying processes,
confinement, resource accounting, handle transfer, and termination. A sandbox
failure disables the affected capability; there is no unsandboxed fallback.

Suggested implementation ownership:

| Existing area | Planned responsibility |
| --- | --- |
| `crates/hedit-plugin/src/runtime/` | Shared transport, codecs, scheduling, Lua bindings, and invocation/service lifecycle |
| `crates/hedit-plugin/src/sandbox/` | Native policy adapters for plugin workers and, later, distinct tool roles |
| `src/app/extensions/` | Broker integration, contribution registry, consent UI, activation, and host-rendered extension UI |
| Existing document/editing modules | Immutable snapshots, coordinate conversion, edit validation, Undo, and recovery |
| Existing language services | Provider adapters and reusable host functionality, after authority and threading review |
| Existing manifest/distribution modules | Versioned contributions, package resources, signatures, updates, and SDK compatibility |

Do not expose application structs, raw pointers, OS handles, or unrestricted
internal commands to Lua. Introduce narrow typed interfaces at existing subsystem
boundaries. Extract a separate shared broker module/crate only when its actual
dependencies and test boundaries justify it.

### Authority and consent

- Deny undeclared or unapproved operations. Bind authority to the authenticated
  worker session, installed package digest, capability, resource scope, and policy
  generation. Never trust plugin-supplied owner IDs or permission assertions.
- Use opaque, session-scoped handles for documents, resources, and UI actions.
  Validate ownership and lifetime on every use; close, reload, update, and revoke
  invalidate stale handles.
- Separate current-selection, current-document, workspace, storage, tool, and
  network access. A document read grant does not authorize writing, and a tool
  grant does not authorize arbitrary executable substitution or shell commands.
- Maintain command-level approval. Background/provider grants must identify their
  activation triggers and scope explicitly; ordinary command approval must not
  turn into an automatic background grant.
- Collect consent through trusted host UI. Untrusted workspace configuration may
  narrow behavior but cannot create grants, approve publishers, or select a new
  executable silently.
- Serialize revocation with sensitive delivery and effects. Recheck authority
  immediately before committing an edit or starting an external operation.
  Stop subsequent events and requests and terminate revoked services.
- Explain that revocation cannot retract already disclosed data, undo completed
  network requests, or reverse committed effects automatically.
- Treat publisher signatures as provenance and integrity checks. Updated source
  or capabilities require renewed review under the documented approval policy.

Every new capability needs a threat-model entry covering assets, adversaries,
resource scope, broker checks, native enforcement, revocation, residual risks,
and corresponding tests before it is enabled by default.

### Protocol, versions, and resource bounds

Retain the existing one-shot transform protocol for existing extensions. Introduce
an explicitly negotiated service protocol when request/response operations are
needed; do not infer message formats or silently reinterpret existing packets.

Define typed envelopes with protocol version, request ID, message kind, operation,
and bounded payload. Session identity and authority come from the host's launched
connection, not fields chosen by the worker. Specify handshake, supported
features, cancellation, end-of-response, connection loss, and process exit rules.

Validate both transport and semantic limits before effects. Reject unknown message
kinds, ambiguous/duplicate fields, invalid types, stale or foreign handles, invalid
UTF-8, excessive nesting, wrong response IDs, and unsolicited responses. Bound
individual messages, total bytes per operation/session, outstanding requests,
queues, provider results, logs, storage, and work performed while decoding.
Chunking must consume an aggregate budget; it must not bypass existing limits.

Version the public SDK, manifest schema, bundle format, and private IPC protocol
separately. Document supported SDK ranges, feature negotiation, deprecation, and
migration. Installation must reject incompatible packages before activation.
Unknown capabilities are rejected. Existing API versions continue to use their
existing semantics and isolation policy.

## Stage 1 — Editing SDK

### Goal

Let authors implement substantial editing commands and small generators with
structured document context, safe edits, command arguments, and reusable modules.
Keep the default execution model as one fresh worker per command.

### Implementation work

1. **Define the SDK and contribution schema.** Specify commands, argument types,
   language/filename/selection conditions, capabilities, package resources, and
   supported SDK versions. Publish a machine-readable manifest schema, Lua
   annotations, and reference fixtures. All new names remain provisional until
   reviewed as one coherent contract.

2. **Add scoped immutable snapshots.** Give an authorized invocation an opaque
   document handle, revision, language, permitted text range, and selections.
   Supply document identity without automatically revealing its filesystem path.
   Whole-document access requires its own grant and bounded snapshot policy.
   Keep remote and paged documents unavailable until their semantics are designed.

3. **Specify coordinates precisely.** Use UTF-8 byte offsets into the exact
   immutable snapshot and half-open ranges for the proposed SDK. Provide host
   conversion helpers for line/column and editor character positions. Document
   CRLF handling and reject boundaries inside code points or line-break pairs.
   Language protocols that use UTF-16 receive an explicit adapter in Stage 2.

4. **Return structured edit proposals.** An edit transaction names the authorized
   document handle and expected revision and contains ordered ranges/replacements
   plus optional resulting selections. Edit coordinates refer to the original
   snapshot; resulting selections refer to the final text after line-ending
   handling. Validate count, size, encoding, overlap, bounds, scope, revision,
   and normalized output before mutating anything. Apply one accepted
   single-document transaction as one Undo step. Reject stale results; do not
   automatically rebase them in this stage.

5. **Add declared command inputs.** Render simple host-owned input panels and
   static choice lists before launching the command worker. Validate arguments
   against the manifest. User interaction and preview dwell must not consume the
   worker's computation deadline. Recheck context and consent after interaction.
   Worker-driven interactive sessions and dynamic lists belong to Stage 2.

6. **Support modular, verified packages.** Add a package-local module/resource
   loader that reads only installed, reviewed resources identified by the package
   digest. Validate resource paths, duplicates, total bytes, module count, import
   depth, and cycles. Exclude external paths, filesystem `require`, downloaded
   modules, native libraries, and Lua bytecode. Set new package/module limits from
   fixtures and memory measurements; preserve old single-script limits.

7. **Add private plugin settings/storage.** Keep typed user settings separate from
   plugin-owned data. Broker a quota-limited, namespaced key/value store with
   atomic updates and explicit uninstall/reset behavior. Do not expose arbitrary
   configuration files or a host filesystem path. Separate user-wide data from
   any future workspace-scoped data.

8. **Improve developer feedback.** Provide structured error categories and safe
   source locations, richer fixtures for multiple selections and structured
   edits, CLI compatibility checks, and traces for authorization, startup,
   execution, proposal, and commit. Preserve fixed public error messages and
   redact document data. Rich runtime error text requires an explicit local
   debugging/export opt-in.

### Reference plugins

- A configurable Markdown command pack using modules and declared inputs.
- A selection-to-table/data converter with multiple edit ranges.
- A current-document text formatter implemented entirely in Lua, within the
  documented snapshot and execution budgets.

These examples must use only documented SDK interfaces. They must not receive
special host privileges that ordinary authors cannot obtain.

### Tests and acceptance gate

- Independent coordinate/edit oracles cover Unicode, emoji, combining marks,
  CRLF, empty ranges, reversed selections, adjacent edits, and rejected overlaps.
- Foreign handles, changed revisions, closed documents, changed panes, revoked
  grants, and replaced packages never read or edit an unintended resource.
- Invalid transactions produce no partial document mutation and add no Undo
  entries. Preview cancellation preserves text and selections.
- Package loaders reject traversal, aliasing, malformed modules, excessive imports,
  and resources changed after approval. Storage quotas and cross-plugin isolation
  have adversarial tests.
- Old extensions and executable fixtures still pass unchanged on every OS.
- CLI fixtures, SDK/schema examples, and offline Help navigation work from extracted
  packages. The reference plugins install, run, preview, and Undo correctly.
- Record baseline startup latency, broker overhead, allocation/copy behavior, and
  UI responsiveness before enabling the new SDK by default.

### Stage 1 excludes

Background listeners, persistent plugin hosts, language providers, generic file
access, subprocesses, networking, arbitrary custom UI, and multi-file changes.

## Stage 2 — Language, events, and presentation SDK

### Goal

Support responsive language assistance and contextual editor UI while keeping
plugin work asynchronous, bounded, isolated, and cancellable.

### Implementation work

1. **Add a provider registry.** Define typed registrations for diagnostics,
   completions, hover, symbols, formatting, and code actions. Specify language
   selectors, provider priority, result merging/deduplication, timeouts, and
   unsupported-document behavior. Adapt existing host services through these
   interfaces after checking their authority and thread ownership.

2. **Add activation and event subscriptions.** Allow explicit, reviewed triggers
   for document open/close/change, selection changes, and save. Deliver only events
   whose resource scopes are granted. Coalesce changes, debounce expensive work,
   cap event frequency and backlog, and cancel superseded requests. Tag edit
   origins and prevent recursive save/change feedback loops.

3. **Design the service lifecycle.** Prototype a lazily started sandboxed service
   per extension for features requiring state or subscriptions. Keep simple
   commands on fresh workers. Never put different plugins in a shared Lua state
   or service process. Define activation, idle shutdown, explicit disable,
   update/reload, crash restart, backoff, and repeated-failure quarantine. Do not
   eagerly start one process for every installed extension.

4. **Give services their own resource policy.** Do not reuse the one-shot sandbox
   configuration unchanged: CPU caps such as the current two-second native limit
   are cumulative process limits, not per-callback budgets. Specify a distinct
   service role with per-callback instructions/deadlines, sampled native CPU
   accounting, memory/cache quotas, bounded idle lifetime, and aggregate editor
   limits. Keep file/network/child-process restrictions. Workers cannot select
   weaker policies through protocol messages. Validate the new role natively on
   each OS before shipping it; failure leaves service features unavailable.

5. **Serialize plugin callbacks.** Start with one Lua execution context per plugin
   service and bounded asynchronous requests. Define reentrancy and cancellation
   explicitly. If callbacks can yield while awaiting host operations, preserve
   their instruction and total-work accounting across yields. Never block the UI
   thread waiting for Lua, IPC, a provider, or a user response.

6. **Track provider results against revisions.** Results name the originating
   request and snapshot. Drop stale diagnostics/completions and cancel requests
   after document closure or grant changes. Apply formatter/code-action edits
   through the Stage 1 transaction validator. Define byte/line/UTF-16 conversion
   at the host adapter boundary rather than inside each plugin.

7. **Add host-rendered contributions.** Implement bounded quick lists, interactive
   inputs, output panels, status messages, gutter/underline decorations, hover
   content, and inline annotations. Validate counts, text/markup, links, and
   action IDs. Use native widgets or a restricted presentation schema; no browser
   engine, arbitrary script execution, or implicit image/network/file loading.
   Every clickable action resolves to an authorized host action.

8. **Add declarative language and visual resources.** Support installed syntax
   definitions, language metadata, snippets, and theme resources through bounded
   manifest contributions. Reuse existing syntax/theme infrastructure where
   suitable. Validate parser/regex work, resource sizes, scope references,
   cancellation, and malformed inputs; declarative data is still untrusted.

9. **Integrate language tools without inheriting authority.** Existing built-in
   language services may be wrapped only if their trust model remains explicit.
   A plugin must not gain execution privileges by calling an existing internal
   LSP/tool launcher. Design the typed tool/provider boundary here; new
   third-party external executables ship behind Stage 3's tool-broker gate.
   Stage 2's initial reference providers run in Lua or use already authorized
   host services without broadening extension grants.

### Reference plugins

- A lightweight diagnostics provider with gutter decorations and quick fixes.
- A document-local completion/hover provider using cached, granted snapshots.
- A navigation command with a dynamic quick list and versioned result handles.
- A language/theme package installed without executable privileges.

### Tests and acceptance gate

- Test late, duplicate, unsolicited, oversized, malformed, and out-of-order
  messages, cancellation races, wrong result IDs, and reconnect/restart behavior.
- Stress rapid typing, document switches, many open documents, change storms,
  slow consumers, blocked pipes, and plugins that continually emit results.
  Work queues and retained snapshots remain bounded.
- Revoke while a service is idle, computing, awaiting a host response, or updating
  UI. Stop future disclosure/effects and remove its provider registrations and
  decorations. Updates cannot retain old grants, handles, or cached document data.
- Crash or kill one service without losing editor text or another plugin's work.
  Repeated crashes must not create an automatic restart loop.
- Test malicious UI content, forged actions, resource exhaustion, and attempts to
  access another plugin's state. No markup may fetch local or remote resources.
- Native probes verify the distinct service policy, including cumulative CPU,
  callback deadlines, memory, child-process restrictions, cleanup, and idle costs.
- Profile event dispatch, result application, typing/frame latency, cache growth,
  and multiple active services before choosing production defaults.

### Stage 2 excludes

General workspace mutation, arbitrary external commands, broad network access,
and transparent remote-workspace access. External language-tool support remains
unreleased until the Stage 3 broker can enforce its required resource policy.

## Stage 3 — Workspace and external integration SDK

### Goal

Enable project tools, cross-file operations, external formatters/language servers,
and explicitly authorized network integrations through supervised host services.

### Implementation work

1. **Add scoped workspace handles.** A host-selected workspace grants only declared
   operations on identified roots/resources. Separate discovery, read, edit,
   create, delete, and watch authority. Apply exclusions and result limits. Path
   checks must resist traversal, symlinks/reparse points, root replacement,
   case/normalization aliases, and check/use races. Prefer descriptor/handle-based
   access over string-prefix checks. Do not grant access outside the selected
   scope merely because another plugin or built-in subsystem can read it.

2. **Add bounded search and watchers.** Broker file enumeration and content search
   with cancellation, depth/result/byte/time budgets, and backpressure. Cap
   watcher counts and coalesce notifications. Bound retention of snapshots and
   search results. Full-file reads and chunked reads consume explicit cumulative
   budgets. Grant changes invalidate active searches and watchers.

3. **Add reviewed workspace edit plans.** Collect per-document revisions or on-disk
   identities/hashes, validate every path and edit, show a complete preview, and
   recheck approval/conflicts before effects. Reuse atomic single-file save and
   recovery machinery. Ordinary filesystems do not provide an atomic multi-file
   commit: document the transaction scope and implement a recoverable journal,
   per-file receipts, interruption handling, and explicit partial-failure recovery.
   Do not promise global rollback or global Undo before it is actually supported.
   Start with edits to existing text files; add create/rename/delete separately.

4. **Build a separate external-tool broker.** First prove that each native adapter
   can enforce the proposed filesystem and process-tree policy with an adversarial
   prototype; do not design the public contract around unverified OS assumptions.
   Register a specific reviewed tool
   profile with executable identity/version, argument schema, working directory,
   allowed environment, input/output protocol, filesystem scope, and network
   policy. Launch directly without a shell. A plugin cannot substitute a different
   binary, inject arbitrary environment variables, or reuse a grant after the tool
   changes. Bound output, process trees, CPU, memory, runtime, and restart behavior.

5. **Keep plugin and tool confinement distinct.** Plugin workers retain their
   restrictive policy, including child-process denial. The trusted host launches
   approved tools in a separately reviewed native sandbox/job. A language server
   may need project reads that a selection transform does not; declare that
   authority to the user. Tool discovery or installation cannot execute downloaded
   code silently. If an OS cannot enforce a required tool profile, report that
   capability unavailable rather than running the tool unrestricted.

6. **Add narrow networking through the host.** Begin with bounded HTTPS requests
   under explicit endpoint, method, upload, response-size, and time limits. Check
   redirects and resolved destinations; distinguish internet access from loopback,
   private-network, metadata-service, and local-service access. Do not expose raw
   sockets, arbitrary proxy settings, or a host credential store. Combining
   document/workspace reads with network access must clearly disclose the data
   scope that can leave the editor.

7. **Add credential references only when required.** Keep secrets in host-managed
   storage and apply narrowly scoped credentials to approved requests. Avoid
   returning raw secrets to Lua. Define account selection, origin binding,
   revocation, log redaction, and user-visible failures. Separate credentials and
   grants between workspaces/plugins; never inherit editor environment secrets.

8. **Extend capability review and lifecycle controls.** Show resource scopes,
   background triggers, executable identities, endpoints, storage use, and recent
   operations in the manager. Provide revocation and an immediate stop action.
   Restart, disable, update, uninstall, workspace closure, and editor exit clean up
   tools, watchers, pending requests, registrations, and policy-scoped caches.

### Reference plugins

- A reviewed project-wide rename/refactoring tool with conflict detection and
  recoverable multi-file application.
- An external formatter using an explicitly selected executable and bounded
  stdin/stdout transport.
- A language-server integration with a declared project-read/tool profile.
- An opt-in translation or issue-service command using one approved HTTPS endpoint
  and explicit selected-text disclosure.

### Tests and acceptance gate

- Attack filesystem boundaries with traversal, symlink/reparse swaps, hard links,
  case aliases, replaced roots, special files, concurrent modifications, and
  grant changes during I/O. Test all supported native filesystems/OS adapters.
- Inject failures before validation, between individual file commits, during
  recovery journal updates, and after process crashes. Preserve competing writes
  and demonstrate recovery from partial application without data loss.
- Test executable replacement, malformed arguments, shell metacharacters,
  environment/handle leakage, descendant processes, blocked tool I/O, output
  floods, cancellation, native memory/CPU limits, and complete cleanup.
- Test redirects, DNS changes/rebinding, IPv4/IPv6/local addresses, oversized and
  compressed responses, upload limits, timeouts, TLS failures, credential origin
  confusion, and revocation before dispatch and during transfer.
- Verify that unapproved tools/endpoints fail before any launch or disclosure.
  A child/plugin request cannot claim a broader identity or capability.
- Run reference integrations from extracted packages on Linux, universal macOS,
  and native Windows x64/ARM64. Treat capability differences as explicit failures
  or documented unavailability, never permission bypasses.

## Performance and availability requirements

Set operation-specific limits from measurements and reference workloads. Every
capability needs explicit production defaults for bytes, counts, duration,
concurrency, retained state, and aggregate cost before release. Different worker
roles require different policies; raising all global limits is not a strategy.

Use the following workload matrix:

- Small and near-limit selections, 16 selections near the end of a document,
  ASCII and mixed Unicode, CRLF/LF, and long lines.
- Documents around 64 KiB, 1 MiB, and the existing paging boundary. Large/paged
  cases measure host behavior or rejection until the relevant access is supported.
- Cold and warm invocations, repeated activation, multiple plugins/windows,
  slow providers, event floods, cancellation, and prolonged idle operation.
- Large workspaces with excluded directories, pathological search results,
  oversized tool output, and rate-limited or failing services in Stage 3.

Record p50/p95/p99 authorization, startup, execution, IPC, result validation,
commit, cancellation, and end-to-end latency separately. Exclude human prompt or
preview dwell from compute measurements and report it separately. Record peak
and steady-state memory, retained snapshots, allocations/copies, CPU, idle
wakeups, queue depth, and frame/input latency. Isolated helper timings are not
whole-editor performance claims.

Initial acceptance targets, to calibrate and lock on documented reference machines:

| Area | Acceptance target |
| --- | --- |
| UI thread | No blocking plugin IPC, filesystem, network, or process waits; cancellation and stale-result rejection never wait for a worker response. |
| Broker bookkeeping | Target p95 under 1 ms for normal dispatch/result bookkeeping on the reference workload; separately measure payload copying and edit application. |
| Editor responsiveness | Target no more than 5% p95 frame/input-latency regression against the same build with plugins disabled on controlled workloads; investigate tail spikes individually. |
| Work scaling | No repeated full-document scans per selection; no work proportional to all installed plugins on every keystroke; process only subscribed/affected documents. |
| Resource bounds | Queues, memory, storage, messages, provider results, active services, and external processes have explicit per-plugin and aggregate ceilings, tested at and beyond each boundary. |
| Idle behavior | No active plugin worker for an unused command; idle services stop or wait without a periodic UI polling requirement attributable to the plugin. |
| Failure behavior | Every operation has a cancellation/deadline policy; repeated failures trigger bounded backoff or disablement instead of retry storms. |

Do not claim these numeric targets are current measurements or cross-editor
benchmarks. If calibration changes a target, record the workload, evidence, and
reason before accepting the feature. Keep benchmarks reproducible and store raw
results with environment/toolchain/build metadata.

## Verification strategy

Keep behavior, authority, and availability tests separate enough to diagnose a
failure, but run them together at stage gates:

1. Pure protocol and transaction tests with independent expected results,
   boundary values, deterministic mutations, and coverage-guided fuzz targets.
   Seed fuzzing with existing malformed-frame and worker-status regressions.
2. Broker state-machine tests with synthetic documents and deterministic control
   of delivery, policy changes, cancellation, and result arrival.
3. Lua SDK conformance fixtures for public examples, compatibility, imports,
   structured results, providers, and failure handling.
4. Native probes that attempt forbidden operations directly, independent of Lua's
   library allowlist. Run worker, service, and tool roles separately.
5. Real editor integration tests for previews, Undo, recovery, UI actions, document
   changes, save hooks, and lifecycle cleanup.
6. Extracted-package tests for shipped helpers/resources, signatures, SDK examples,
   and executable/tool identity checks.
7. Reproducible performance/load tests plus manual desktop smoke tests for focus,
   clipboard, dialogs, presentation, and accessibility where relevant.

Required platform coverage is Linux x86-64/ARM64 where supported, universal macOS
with native Apple Silicon execution, and native Windows x64/ARM64 CI. Verify both
architectures are present in universal macOS artifacts; Intel-only development
hardware is not a prerequisite. Record which architectures ran natively rather
than presenting cross-compilation as runtime validation.

Security-critical broker, codec, and native-policy checks gate release. Add
scheduled coverage-guided fuzzing and investigate every reproducible crash,
hang, unbounded allocation, unauthorized effect, or information disclosure. A
new role or capability also requires an updated threat model and a deliberate
review of its trusted code/dependency surface. Test success is evidence about
these boundaries, not a claim of formal isolation or complete editor parity.

## Delivery sequence and review units

Use focused PRs with complete vertical behavior and their relevant tests/docs.
Keep platform-independent policy and all affected native enforcement changes in
one reviewable feature; do not publish a capability that works securely on only
one platform while silently bypassing checks on another.

| Order | Review unit | Depends on |
| --- | --- | --- |
| 1 | SDK/schema/version policy and capability vocabulary | Existing threat model and current API inventory |
| 2 | Snapshot handles, coordinate contract, structured single-document transactions | 1 |
| 3 | Command arguments, declared input panels, and preview integration | 2 |
| 4 | Verified modules/resources, private storage, CLI fixtures, and diagnostics | 1–3 |
| 5 | Stage 1 reference plugins and performance/security gate | 2–4 |
| 6 | Provider contracts, event routing, and result lifetimes | Stage 1 gate |
| 7 | Service role, scheduler, native limits, restart/revocation behavior | 6 and updated native threat model |
| 8 | Host-rendered contributions and declarative language/theme resources | 6–7 |
| 9 | Stage 2 reference plugins and sustained-load/security gate | 6–8 |
| 10 | Workspace scopes, bounded discovery/search/watch services | Stage 2 gate |
| 11 | Recoverable workspace edit plans and conflict handling | 10 |
| 12 | External-tool profiles and native tool confinement | 10 and separate tool threat model |
| 13 | Endpoint-scoped HTTPS and optional credential references | Capability model and separate network threat model |
| 14 | Stage 3 reference integrations and package/recovery/load gate | 10–13 |

Before starting each stage, decide the exact public surface and limits in a small
API review using the reference plugins. Close unresolved authority, lifecycle,
and failure semantics before parallel feature implementation. A failed security
or performance gate blocks the affected capability; it does not justify widening
permissions, disabling validation, or weakening the sandbox.

Do not assign calendar promises without sizing the accepted scope against actual
implementation capacity. Report progress by accepted capabilities and passed
stage gates, with explicit remaining limitations.

## Completion checklist

- [x] Stage 1: documented editing SDK, reference plugins, compatible legacy API,
  native/package validation, and recorded performance baseline.
- [x] Stage 2: language/event/presentation interfaces, safe service lifecycle,
  bounded sustained workloads, and native service-policy validation.
- [ ] Stage 3: scoped workspace/tool/network services, recoverable multi-file
  behavior, reference integrations, and native enforcement across supported OSes.
- [ ] Public SDK/version/deprecation policy and author tooling are maintained.
- [ ] Capability review, revocation, stop controls, and activity reporting are clear.
- [ ] Threat model, conformance tests, fuzzing, performance evidence, and package
  smoke tests cover every shipped capability and worker role.
- [ ] Remaining gaps are documented by capability; there is no claim of Sublime
  package compatibility or unrestricted access.

## Implementation status

The first Stage 1 increment is implemented on `v0.7.0/plugin-sdk`:

- Manifest API 3 / SDK 1 with separate document capabilities, exact-copy
  command approvals, schema, Lua annotations, and a public SDK reference.
- Opaque snapshots, UTF-8 coordinates, independently validated structured edits,
  final selections, preview, and one-step Undo; legacy APIs remain available.
- Declared scalar inputs, reviewed package-local Lua imports and text resources,
  and bounded private command storage with explicit persistent approval.
- Versioned typed worker results, fixed errors, verified syntax locations, richer
  confined fixtures, three reference packages, and extracted-package resources.
- Adversarial broker/codec/storage tests, independent Unicode edit checks,
  a reproducible coverage-guided codec fuzz target, and a synthetic latency probe.
- Compact preview text blocks with indexed rows, formatted only when visible;
  no-change detection does not construct a full preview.

Local macOS validation includes the full workspace regression suite, native
confinement tests, SDK fixtures, strict all-feature lint, and offline package
checks. The latest 61-second LLVM coverage campaign completed 713,703 inputs
without a crash; it did not enable AddressSanitizer. On this arm64 development
build, synthetic host transaction preparation measured p95 of 1–10 microseconds;
complete CLI fixture invocations measured p95 of 46–62 milliseconds. CLI timings
include editor process startup, confinement, transport, Lua, and exit. These are
local synthetic measurements, not a GUI responsiveness or release-build claim.
The probe records every sample and toolchain/build metadata for reproduction.

Stage 1's native Windows/Linux, universal macOS, sanitizer, numeric tracing, and
controlled egui CPU-frame/allocation gates have passed on the implementation
stack. The measurements below state their practical scope. Stage 2 is implemented
and validated as recorded in the follow-ups; Stage 3 remains unavailable to installed plugins; its broker foundation is
under implementation. Do not widen isolation to bypass a gate.

### Stage 1 validation follow-up

Linux and native Windows x64/ARM64 validation and universal macOS packaging passed
for the editing SDK baseline. The phase/measurement follow-up adds fixed numeric
atomic counters, CLI `--timings`, and a test-only per-thread allocator probe.
Prompt/preview dwell is separate from computation. Worker-reported evaluation
time is not used for authority or deadline enforcement.

A 601-second LLVM/AddressSanitizer campaign completed 2,435,033 inputs without a
finding. The controlled egui typing probe alternated disabled/installed-idle/
stale-result cases over three trials; idle p95 CPU-frame overhead stayed below
5% (largest observed increase about 2.1%). It measures CPU input/layout work and
allocation traffic, not OS delivery, GPU presentation, or retained process heap.
Raw samples, corpus/logs, and environment metadata are retained locally under
`target/plugin-validation`; reproduction commands are in the SDK reference.
The instrumentation follow-up and Windows UTF-8 package-test correction passed
CI. Universal macOS validation of the provider stack also includes this SDK.

### Stage 2 service policy decision

Use broker-managed sessions with **worker rotation per callback**. The host
serializes callbacks per package, coalesces one pending event per provider, and
retains only explicitly returned scalar cache data scoped to that provider's
grant and active document. Each callback gets a fresh native sandbox and Lua VM;
no global Lua variables survive. Clear cache/results on document scope changes,
revocation, disable, update, and crash quarantine. Idle expiry clears the service
cache; independently validated host UI results have a separate 120-second lifetime
and retain no duplicate copy of that cache.

This deliberately avoids a resident untrusted VM and its cumulative CPU-limit
problem. Keep the native one-shot ceiling and apply stricter service callback
instruction/deadline limits plus host aggregate concurrency, rate, cache, idle,
and restart budgets. A worker cannot choose a weaker role or request another
provider's cache. Validate this rotating service policy natively on each OS.
A resident-VM optimization remains a separate policy change, not a prerequisite
for provider semantics and not an excuse to remove the native CPU ceiling.

### Stage 2 implementation follow-up

Implemented on `v0.7.0/plugin-services` (review/validation pending): manifest API
4 / SDK 2 with separate provider grants; diagnostics, completions, hover,
symbols, formatting, code actions and presentation; a coalescing/rate-limited
scheduler; rotated workers with distinct trusted launch role, bounded readiness
handshake, lower priority and native limits; explicit per-provider/document cache;
revision/selection-bound results and expiring typed actions; native results,
quick lists, inputs, output/status, diagnostic gutter/underlines and annotations;
and non-executable bounded language/theme packs. Formatting/quick fixes reuse the
editing SDK validator, approval recheck, review policy and Undo. The manager
cancels providers before policy mutations; policy watcher failures disable them.

The `document-assist` and `demo-visuals` references use the ordinary public API.
The macOS workspace suite passed 1,298 tests (62 intentionally ignored), and
strict all-feature Clippy passed. Native macOS probes include service priority
and unchanged file/network/descriptor/child restrictions. Reply/role confusion,
request identity, truncation/duplicate fields, UTF-16, event storms, stale
completion, cache invalidation, quarantine, revoked quick fixes and Undo have
regressions. The native service-policy checks on Linux/Windows, controlled active
provider CPU-frame/allocation evidence, sanitizer follow-up, extracted package
smokes, and universal macOS validation remain required before closing Stage 2.
Stage 3 is not implemented or enabled by this change.

### Stage 2 security/performance follow-up

The initial provider feature passed Linux, macOS, Windows x64/ARM64, and remote
regressions in PR #112. A 601-second AddressSanitizer codec campaign completed
1,867,776 inputs without a finding, including added service request/result seeds.
Follow-up fixes bound native result rendering to visible rows for one selected
provider, paginate long output, cache/deduplicate decorations, and avoid cloning
transactions until clicked. Returned cache transfers into session state without
retaining a second copy in UI results.

Provider authorization allows concurrent read/disclosure guards while keeping
revocation and storage commits exclusive. Publication checks share global worker
admission. Unconsumed UI authorization receipts expire after 200 ms on the bounded
authorization thread, covering inactive windows; consumed guards remain held for
the authorized operation. The same handoff protects ordinary editing commands.
The updated macOS workspace suite passed 1,302 tests (63 intentionally ignored),
and strict all-feature Clippy passed. These follow-up changes require fresh CI.

The controlled four-provider egui probe uses three alternating trials and 600
samples per mode. With no concurrent builds/fuzzing, pooled typing-frame p95 was
3,428 µs disabled, 3,448 µs installed/idle (+0.6%), and 3,879 µs while repeatedly
dispatching/cancelling real native callbacks. Individual trials are noisy; this
is a local CPU-frame/allocation probe, not a physical input/GPU latency guarantee.
Raw exploratory and final samples are retained under `target/plugin-validation`.
Final follow-up `9dd7f7c` passed Linux, macOS, Windows x64/ARM64, remote regressions,
and universal macOS packaging/extracted-app validation (run `44859211258`). The
Stage 2 implementation gate is closed; PR #112 remains a draft for review. This
accepts the documented bounded provider API and measurement scope, not a claim of
unrestricted editor compatibility. Stage 3 remains required to finish this plan.


### Stage 3 broker checkpoint — implementation gate remains open

The shared SDK 3 continuation protocol and host-only `hedit-broker` crate now
cover scoped discovery/read/search, versioned chunks and batched reads, native
watch continuations, reviewed existing-file plans, recoverable journals and
retention quotas, exact-endpoint HTTPS, strict JSON helpers, and bounded
background host execution. These are internal development interfaces. Installed
API 5 ordinary commands are integrated on this branch with separate source and
resource review. SDK 3 providers remain rejected until their host routing is
complete; native tool requests report unavailable.

The host validates declarations, source-independent operation envelopes, opaque
session/version handles, callback steps, UTF-8/CRLF edits, aggregate resource
limits and current policy guards. Recovery tests cover interruption, competing
writes, capture races, malformed receipts/ledgers, full-width Windows file IDs,
replaced/moved private stages, conservative cleanup and retained storage quotas.
Ordinary capture failures attempt a no-clobber restore. The capture/publication
visibility gap remains explicit; this is not atomic single-file replacement or
global multi-file Undo. The command integration verifies/freezes/reconciles this window's matching clean
buffers, while explicit recovery UI remains required. Other windows/processes
retain the existing external-change/conflict handling.

Local macOS evidence: 1,359 workspace tests passed (63 intentionally ignored),
plus the custom native continuation test. The broker suite has 47 regressions;
real confined-worker fixtures cover Unicode/CRLF project edits, synthetic HTTPS
JSON, asynchronous apply, cancellation and stale preview decisions. Strict
all-feature workspace Clippy, static/offline-Help checks and Linux/macOS/Windows
packaging-script tests passed. This does not count script mocks as native
Windows/Linux execution or validate a new universal release artifact.

A final codec-only AddressSanitizer campaign completed 936,553 inputs in 601
seconds without a finding. Its archived source hashes still match the codec.
Scheduled campaigns now retain source, metadata, corpus, logs and failure
artifacts. They do not execute Lua or replace native confinement tests.

The release probe runs 25 two-file fixture trials (100 real confined callbacks),
with immediate synthetic preview acceptance and separate host/worker timings.
Input-copy p95 was 3 µs; reply-validation p95 1 µs; broker I/O p95 800 µs. Native callback p95 was 37.927 ms, with the first launch 276.914 ms. Journal preparation/application p95 were 40.870/54.985 ms.
These measurements cover small local files, not whole-editor input/frame latency,
large-workspace behavior, allocations or retained heap. Raw samples, archived
source and environment metadata live under `target/plugin-validation`; reproduce
with `scripts/profile-plugin-broker.py` after stopping other builds/fuzzing.

Required next work:

- Repeat native platform and universal macOS gates after editor integration.
- Finish SDK 3 provider routing and explicit recovery/cleanup UI.
- Validate command grant/manager, complete previews and buffer reconciliation on
  Linux/Windows and in universal macOS packages, with sustained UI/retention probes.
- Finish separately confined external-tool and typed LSP backends using the
  selected native macOS sandbox with monitored memory
  limits and documented overshoot. A native prototype is not a production tool
  policy. Tool/LSP intents currently report unavailable; no fallback runs them.
- Finish the author CLI, annotations/schema, installed reference integrations,
  extracted-package tests, sustained responsiveness/retention measurements, and
  cross-platform security/release gates. Credentials remain conditional on a
  reference integration actually requiring them; no ambient/plaintext fallback.

The Stage 3 gate remains open until the editor integration and supported-platform
checks pass. Local backend checks do not establish completion of the whole plan.

The broker foundation passed Linux, macOS, Windows and remote CI at `a83dd1b`
(run `44900256772`) and the dedicated Windows x64/ARM64 sandbox/broker/package
workflow (run `44900256769`). That workflow passed after retrying a save-hook
deadline failure. Universal macOS packaging, extracted-helper checks and the
protocol-security workflow passed at `74bb601` (run `44899794017`), before the
Windows-only rename correction. Native CI found and fixed Linux O_PATH directory
flushes and Windows handle-relative no-clobber recovery renames. These close the
foundation's native test gap; the editor integration and tool/LSP gates stay open.


### Scoped command integration

Experimental manifest API 5 / SDK 3 ordinary commands now use the scoped host.
Exact-source approval is separate from physical directory rights and exact HTTPS
endpoint selection. Source changes prune resource grants; corrupt resource
approval reset preserves a private backup. No document.read grant means a neutral
empty snapshot. Installed SDK 3 providers are rejected explicitly while their
routing is unfinished; tool/LSP requests remain unavailable.

Revocation publishes a cancellation generation before waiting for the policy
writer lock. Native notifications directly stop disclosures without UI frames;
orphan repair and competing writers are tested. Preview pages are indexed off
the UI thread. A generation-checked lease freezes matching clean local buffers,
keeps independent bounded backups, reconciles partial failures, preserves Undo
when disk is unchanged and advances revisions after replacement. Deferred opens
reread disk off-thread; stale page/save replies cannot overwrite newer buffers.
Unrelated buffers remain editable during an accepted workspace commit.

Local default-feature workspace validation passed 1,386 tests (63 intentionally
ignored). Native command tests cover empty-document disclosure, exact grants,
revocation without UI frames, single-document Undo, multi-file application while
switching focus, and source/resource reset. Broker backend validation passed 55
tests plus the real confined-worker continuation fixture. Final command/UI
performance, fresh platform CI, provider/recovery integration, tool/LSP backends,
author tooling and extracted-package references remain open Stage 3 gates.


### Explicit workspace recovery

Extensions settings now lists retained journals and lets the user select the
original physical directory for a bounded complete preview. New journals record
the approved command/source digest in version 2; version 1 remains readable.
Resume, restore and history deletion compare fresh authenticated state, serialize
against plugin policy changes, freeze/reconcile this window's affected buffers,
and preserve competing writes. Incomplete-preparation cleanup has its own strict
replacement-only validation. No startup effects or plugin execution are involved.
Native UI acceptance and cross-platform validation of this follow-up remain open.
The selected macOS external-tool profile uses native confinement with monitored
memory limits and documented overshoot; tool/LSP implementation is still pending.

Recovery follow-up validation on macOS covered 1,376 tests with 62 intentional
ignores across 50 distinct suites. The root/workspace run passed the editor,
rendered settings and package checks; after making an existing zero-wait lock
regression release its owner explicitly, the core, broker, plugin and remote
suites passed in a separate native run. Strict workspace/all-target Clippy and
static documentation/script checks passed with default features disabled.
The settings test exercises resume, stale-preview rejection, restore, history
cleanup and open-buffer refresh without any installed source approval. Preview
panes now scroll within a fixed height so newline-heavy output cannot displace
recovery actions. The preceding command integration passed full Linux/macOS/
Windows CI (`44911549599`) and x64/ARM64 sandbox CI (`44911549544`, with a retry
of the existing Windows Lua deadline fixture).


### Scoped provider integration

API 5 / SDK 3 providers now share exact-source authority, resource resolution,
native cancellation and workspace commit checks with ordinary commands. The
installed provider scheduler supports typed results, coalesced document events,
manual document-free presentation, callbacks/cache, Stop/Resume/quarantine and
explicit workspace previews. Automatic document events require document.read;
manual workspace providers receive an empty snapshot even without a Lua-eligible
active buffer. Provider approval shows its exact kind, triggers and capabilities.

Background hosts/sessions are capped at 12 of 16 contexts and three of four
active broker operations. Shared SDK 2/3 native service admission preserves one
foreground worker slot. Cancellable waits and retries preserve the current
callback input, never restarting a chain after host effects. Native wakeups
replace periodic SDK 3 UI polling; settled work accounting excludes human preview
and parked-watch dwell. Workspace refreshes carry package origin to suppress
self-triggered document events.

The initial native macOS workspace sweep passed 1,382 tests with 62 intentional
ignores. Additional foreground admission and queue regressions were added after
that sweep and are being validated separately. Native provider fixtures cover
neutral snapshots, callback cache, Stop/Resume/revoke, Unicode diagnostics, stale
results, explicit two-file review and clean-buffer reconciliation. Windows/Linux
provider-specific editor checks, final responsiveness/retention evidence, native
tool/LSP backends, author/reference tooling and universal packaging remain open.

Final provider follow-up checks passed 108 editor extension tests (two intentional
ignores), including automatic open/change/close lifecycle and partial-apply
reporting, plus the complete native broker/plugin suites after the priority
changes. All-feature/all-target workspace Clippy passed. The recovery PR's final
signed head `9ba28a0` passed Linux/macOS/Windows/remote CI (`44915873512`) and both
Windows sandbox architectures (`44915873326`). Its Windows cleanup fixture now
releases per-operation protected-directory pins, matching the real editor flow.

### Native tool boundary

The scoped-provider head `dde0954` passed all platform checks, including Windows
x64 and ARM64. The subsequent Windows pipe hardening uses identification-only
client tokens and a live-client native impersonation regression; both Windows
sandbox architectures passed at `0b76d57`.

The selected macOS external-tool model is native confinement with monitored
memory/thread limits, documented overshoot and no VM prerequisite. A host-only
finite-filter primitive now applies a fixed deny-by-default policy before text
delivery and exec. Native probes cover filesystem/network/descriptor isolation,
reexec, full-size transport, resource limits and cleanup. The Lua helper's policy
is unchanged. The broker tool/LSP intents remain unavailable pending private
image import, source-bound grants, validated snapshot construction and verified
Linux/Windows adapters. Persistent LSP, final author/reference tooling, sustained
performance evidence and universal-package acceptance remain open plan gates.
