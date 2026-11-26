+++
title = "Design around permissions"
description = "Make data access explicit and keep your plugin useful when a grant is unavailable."
weight = 30
template = "page"
+++

## Request only the data you need

| Feature | Ask for |
| --- | --- |
| A transform over a selection | Selection-only access |
| A formatter | The current document |
| A project command | The directory and the capabilities it uses |
| An external integration | The exact approved tool image or public endpoint |
| Automatic editing on Save | Persistent command approval and a language binding |

Approval applies to the reviewed installed command and source. Review workspace,
endpoint, native-image, and Save-hook grants separately. A publisher signature
identifies the signer and verifies the bundle's integrity.

## Make effects reviewable

Return a transaction tied to the snapshot and revision. Recompute an edit if the
document changes. For a multi-file edit, require complete results before staging
a plan. The host previews and commits it separately. See
[workspace edits and recovery](@/reference/broker.md#workspace-edits-and-recovery).

## Handle refusal and cancellation

| Outcome | What the plugin should do |
| --- | --- |
| Missing, changed, denied, or revoked resource | Inspect the fixed failure category. Stop or offer an action the user can take. |
| Partial or truncated result | End the edit flow. |
| User revokes a grant | Expect future effects to fail authorization. Text already sent and edits already committed remain. |

Keep diagnostics short. Leave document contents, credentials, and full service
responses out of messages.

## Treat Save hooks as a separate feature

A bound hook can replace the whole eligible local document on Save without a
preview. Use a deterministic command that finishes within the hook deadline.
For SDK 3, use provider events for automatic work. See the
[threat model](@/reference/security.md) for platform enforcement and limits.
