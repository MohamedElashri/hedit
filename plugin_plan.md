# Plugin system implementation plan

## Goal and scope

Build a native hedit extension platform that supports the same broad categories
of plugins as Sublime Text: advanced editing commands, language assistance,
syntax and theme packs, editor annotations, project tools, formatters, and
external integrations. Design the API around hedit's document model, native UI,
shared Lua runtime, and explicit user consent.

This document records the three-stage v0.7.0 design and its acceptance status.
The SDK references define the implemented contract; the design below explains
the goals and sequencing. The final status table distinguishes completed
capabilities, deliberate exclusions and any outstanding release gates.

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

## Starting foundation

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
those documents. Current capability coverage is recorded at the end.

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

Implementation ownership:

| Existing area | Planned responsibility |
| --- | --- |
| `crates/hedit-plugin/src/runtime/` | Shared Lua transport, worker validation, bindings and fresh invocation/callback lifecycle |
| `crates/hedit-plugin/src/sandbox/` and `src/tool/` | Native worker and distinct filter/service tool policies, transport and accounting |
| `crates/hedit-protocol/` | Shared bounded data JSON and LSP framing/codec |
| `crates/hedit-broker/` | Scoped workspace/network/tool operations, recovery, native LSP translation and lifetime ownership |
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

The implementation retains one Lua core and small native sandbox adapters.
API/SDK versions remain additive: APIs 1–2 cover legacy transforms, API 3 / SDK 1
document transactions, API 4 / SDK 2 providers, and API 5 / SDK 3 scoped broker
continuations. Existing versions are supported; none is deprecated by this work.
The manifest schema, Lua annotations and confined CLI fixtures define author
conformance. [SDK reference](docs/plugin-sdk.md),
[provider contract](docs/plugin-services.md), [broker contract](docs/plugin-broker.md).

| Stage | Implemented vertical behavior |
| --- | --- |
| 1 — Editing | Opaque document/revision snapshots, Unicode/CRLF-safe structured edits and selections, arguments, review/Undo, approved Save hooks, reviewed modules/resources, bounded private command storage, signatures, schemas, annotations and real confined fixtures. |
| 2 — Providers | Diagnostics, completion, hover, symbols, quick lists, decorations and typed actions; event coalescing, four-slot admission with foreground reserve, fresh Lua worker per callback, bounded scalar cache, Stop/revocation/quarantine, syntax/theme packs and native host rendering. |
| 3 — Scoped services | Discover/read/search/watch grants, batched reads, reviewed existing-file edits with conflict checks and recoverable journals, fixed public HTTPS endpoints, reviewed private native images, immutable project projections, finite filters and persistent source/document-bound LSP owners. |

The third stage includes six typed LSP result families: diagnostics, plain
completion, hover, symbols, formatting and current-document edit actions. The
broker owns request IDs, exact issued URI/version matching, UTF-8/16/32
conversion, cancellation/draining, resource accounting and termination. Server
requests cannot execute commands, apply edits or broaden grants. Native images
never receive a live workspace mount, network or child-process authority.

Shipped references demonstrate the public contracts:

- SDK editing: Markdown formatting, tab-separated table conversion and trim/Save.
- Providers: diagnostics/quick fixes, completion/hover, navigation and visual packs.
- Scoped services: native JSON formatting/project LSP, reviewed ASCII identifier
  replacement across existing files, and selected Markdown to HTML source through
  the public GitHub endpoint. The rename example is bounded text replacement,
  not semantic language refactoring.

The JSON native image and all companion manifests, Lua sources, fixtures and
schemas are included in platform packages and offline Help. Install/update smoke
tests check the extracted resources and actual native launch path.

### Acceptance evidence

This table records evidence for the feature stack, not a claim of independent
security audit, formal verification, physical input latency or arbitrary language
server compatibility.

| Boundary | Evidence |
| --- | --- |
| Worker and native policy | Actual confined Lua/SDK and hostile native images on macOS, Linux x86-64/ARM64 and Windows x64/ARM64; file/network/child denial, pipe impersonation, positive/negative LPAC controls, quotas, cancellation, parent/guardian death and kill/reap cleanup. |
| Broker and effects | Strict malformed/duplicate/truncated/foreign replies; grant/revision/root identity changes; source and document lease closure; recoverable partial failure; policy recheck around disclosure and effects; no implicit failure restart. |
| Author/API | Independent manifest schema validation, legacy/SDK 1–3 conformance, exact offline SDK 3 intent/response transcripts, installed source/resource grants and real JSON filter/project-LSP requests. |
| Shared codecs | Final shared-codec AddressSanitizer campaign: 2,571,791 LSP/data JSON inputs and 1,092,470 SDK inputs without a finding (run 44957425023). Source archives, hashes, logs and corpus retained. This complements native policy tests. |
| Performance | Optimized M1 macOS run at `8d0ff5b`: 3,600 frames per mode over three alternating trials, near-64-KiB Unicode/CRLF JSON and three native LSP profiles. Idle p95 1,188 → 1,223 µs (+2.9%); typing p95 1,158 → 1,188 µs (+2.6%), both below the 5% target. 178 results published, no provider failures or retained native run payloads after Stop; UI Stop ≤86 µs, cleanup ≤95 ms. Broker phase results below. |
| Distribution | Native image/import/runtime checks pass on the five supported native CI runners. The corrected macOS updater package fixture passes locally; final universal and full platform package runs are in progress. |

The earlier editing/provider baseline also passed universal macOS packaging and
native CI. Its controlled egui probe recorded at most about 2.1% idle p95
CPU-frame overhead across the measured workloads. The final scoped-service
campaign above measures the later native owner/broker path separately.
Reproduction commands and measurement limitations are in the
[validation guide](docs/plugin-broker.md#external-tools-and-validation).

### Measurement details

On this Apple M1 / 16-GiB Mac, the release workload measured 60-KiB-class JSON
with three approved native LSP profiles. Across 14,400 CPU-frame samples, native
idle p99 was 1,656 µs versus 1,936 µs disabled, and active typing p99 was 1,503 µs
versus 1,722 µs disabled. UI allocation p95 was 746 versus 718 at idle, and 760
versus 722 while typing. Requested bytes p95 were 199,001 versus 198,056 at idle,
and 205,659 versus 198,170 while typing. These are allocation traffic, not retained
heap measurements.

Host physical footprint at the end of each active typing workload was
17.7–19.4 MiB; one trial grew about 0.20 MiB, the other two decreased. The
twenty-second-class trials and Stop checks are bounded-load evidence, not a
proof of indefinite leak freedom. Host CPU and footprint exclude child usage.
Native child limits have their separate adversarial enforcement tests. No
physical input, GPU, display-latency or cross-editor benchmark claim is made.

The 25-trial optimized broker campaign used two small Unicode/CRLF files and
100 actual confined callbacks. At p95, input copies took 3 µs, reply validation
1 µs and broker I/O 640 µs, below the one-millisecond bookkeeping target. Complete
callbacks were 31.3 ms p95; the first cold callback was 383 ms. Recoverable journal
preparation was 32.2 ms p95 and application 58.1 ms p95. Those off-UI filesystem
phases include durable flushes and are reported separately from bookkeeping and
callback transport. This is not a large-workspace or network-latency benchmark.

Raw samples, numeric phase activity, source archives/hashes, compiler/build/
hardware metadata and the UI test executable hash are retained under
`target/plugin-validation/final-ui` and `target/plugin-validation/final-broker`.
The measured source was clean at `8d0ff5b`. Subsequent documentation and
install/updater-fixture corrections do not change the measured runtime path.

### Deliberate limits

- No Sublime package compatibility, editor-internal object API, arbitrary shell,
  environment, sockets, credentials or unrestricted native code loading.
- Workspace plans edit existing eligible text files only. Create/delete/rename
  operations and global multi-file Undo require separate designs.
- LSP supports one current document and a fixed bounded project copy per owner.
  No cross-file LSP edit actions, server commands, snippets/additional completion
  edits, dynamic registration, live filesystem writes or process-tree servers.
- Native services have finite wall/CPU/traffic/request budgets and zero automatic
  restarts. Heavy runtimes exceeding the native policy fail unavailable.
- Credentials are conditional future work: the reference HTTPS integration is
  public and needs none. There is no ambient or plaintext-secret fallback.
- Remote, paged, read-only and oversized documents do not acquire implicit
  authority. Native memory monitoring on macOS can overshoot between samples;
  it is not a hard memory cap or VM guarantee.

### Final validation status

Implementation and sustained performance work are complete. Final full-feature
macOS regression, documentation verification and corrected universal/full
platform package CI are being checked. The unchecked completion items above
remain open until those results are reviewed. No capability uses a weaker
sandbox or fallback to pass a gate.
