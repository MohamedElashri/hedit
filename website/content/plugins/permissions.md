+++
title = "Design around permissions"
description = "Make data access explicit and keep your plugin useful when a grant is unavailable."
weight = 30
template = "page"
+++

hedit treats installed code and worker output as untrusted. Isolation limits
what a worker can do; the broker separately decides which effects are authorized.
Design the plugin's behavior around both boundaries.

## Request only the data you need

A selection transform should stay a selection transform when it does not need
the whole document. A formatter that only uses the current document should not
request a project copy. Keep optional project or endpoint features in commands
whose grants explain those features clearly.

Document approval belongs to an exact installed command and source copy.
Workspace, endpoint, native-image, and Save-hook approvals have separate scopes.
A publisher signature proves provenance and integrity; it does not make a plugin
safe or approve any capability.

## Make effects reviewable

Return a document transaction or workspace edit plan through the supported API.
Keep ranges tied to the supplied snapshot and revision. Do not retry a stale edit
against new text without recomputing it.

For multi-file work, reject incomplete discovery or truncated search results.
Stage a complete plan and let the host review it. A workspace commit is separate
from single-document Undo; retained journals support explicit recovery.
See [workspace edits and recovery](@/reference/broker.md#workspace-edits-and-recovery).

## Handle refusal and cancellation

For scoped operations, inspect the fixed failure category before consuming a
response. Missing, changed, denied, or revoked resources are ordinary outcomes.
Never reinterpret a failed or partial response as an empty successful result.

Use no-op completion where appropriate, or a bounded typed result that helps the
user understand the action they can take. Do not place document contents,
credentials, or unbounded service output in diagnostic messages.

Revocation stops future authorized effects. It cannot retract text already
disclosed to an approved worker or endpoint, and it cannot undo a committed edit.
This matters when describing a feature that sends text to an external service.

## Treat Save hooks as a separate feature

Supported save-capable commands need persistent command approval and an explicit
language binding. Hooks can replace the whole eligible local document on Save,
without an edit preview. Keep them predictable, deterministic, and comfortably
inside the hook deadline. SDK 3 commands do not declare legacy Save hooks;
automatic SDK 3 work uses provider events instead.

Read the [full threat model](@/reference/security.md) for OS-specific enforcement,
monitored limits, trust assumptions, and residual risks. Tests do not establish
security outside the boundary they actually exercise.
