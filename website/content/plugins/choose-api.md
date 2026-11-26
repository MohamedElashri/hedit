+++
title = "Choose an API"
description = "Select the smallest supported contract that fits your plugin."
weight = 20
template = "page"
+++

## Match the feature to the contract

| You want to build | Manifest / SDK | Start here |
| --- | --- | --- |
| Snippets or file templates | API 1 | [Declarative contributions](@/reference/manifests.md#declarative-example) |
| A transform over selected strings | API 2 | [Selection-only Lua](@/reference/manifests.md#lua-api) |
| Document edits, inputs, or private command storage | API 3 / SDK 1 | [Document SDK](@/reference/document-sdk.md) |
| Diagnostics, completion, hover, symbols, formatting, or native UI | API 4 / SDK 2 | [Providers](@/reference/providers.md) |
| Project reads, watches, edit plans, native tools, or public HTTPS | API 5 / SDK 3 | [Service broker](@/reference/broker.md) |
| Language or theme packs | API 1 | [Visual packs](@/reference/providers.md#fixtures-and-declarative-packs) |

All five manifest APIs are supported. The manifest API and SDK identify contracts.
Use your bundle version for releases. Approval is specific to each installed command.

## Design for the process boundary

{{< flow "Host supplies input" "Fresh Lua worker" "Worker returns a proposal" "Host checks and applies" caption="The host checks approval before sending input and again before applying an effect." >}}

| Contract | What survives a callback |
| --- | --- |
| Document command | A transaction the host accepts |
| Provider | A typed result and optional scalar cache |
| SDK 3 continuation | Explicit cache and the broker's response. The next step starts a fresh worker. |
| Approved native language server | A separate process within a reviewed document scope and finite limits |

Package-local modules are reviewed source. Lua globals end with each worker.

## Understand the current limits

Document operations use eligible local, fully loaded buffers. Workspace edit plans
need a complete preview and a separate commit. The host owns native UI, document
access, and resource selection.

SDK 3 can reach reviewed public endpoints and exact native images. Arbitrary
shell commands, native Lua modules, credentials, webviews, and unrestricted
network access are outside the contract. See the
[threat model](@/reference/security.md#process-and-authority-boundaries).
