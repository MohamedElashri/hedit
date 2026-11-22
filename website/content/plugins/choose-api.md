+++
title = "Choose an API"
description = "Select the smallest supported contract that fits your plugin."
weight = 20
template = "page"
+++

Manifest API and SDK numbers identify contracts, not plugin release versions.
Use the smallest contract with the authority your feature requires.
The manifest checker rejects unsupported API/SDK combinations before execution.

## Match the feature to the contract

| You want to build | Manifest / SDK | Start here |
| --- | --- | --- |
| Snippets or new-file templates | API 1 / no SDK | [Declarative contributions](@/reference/manifests.md#declarative-example) |
| A transform over selected strings | API 2 / no SDK | [Selection-only Lua](@/reference/manifests.md#lua-api) |
| Structured document edits, selections, inputs, or private command storage | API 3 / SDK 1 | [Document SDK](@/reference/document-sdk.md) |
| Diagnostics, completion, hover, symbols, formatting, or native presentation | API 4 / SDK 2 | [Providers](@/reference/providers.md) |
| Scoped project reads/search/watch/edit plans, reviewed filters/LSP, or public HTTPS | API 5 / SDK 3 | [Service broker](@/reference/broker.md) |
| Declarative language or theme packs | API 1 / no SDK | [Visual packs](@/reference/providers.md#fixtures-and-declarative-packs) |

All five manifest versions remain supported. A newer SDK does not inherit the
approvals of an older command. Each command declares its own authority.

## Design for the process boundary

Lua callbacks are short-lived computations over bounded input. Each callback
starts in a fresh confined process, and Lua globals do not survive it.
Package-local modules are reviewed source, not filesystem imports.

- Document commands return transactions for the host to validate.
- Providers return typed results and may return bounded scalar cache state.
- SDK 3 callbacks yield one intent. The broker performs an authorized operation,
  then starts a fresh callback with the response and explicit cache.
- Native language servers may persist within a reviewed document scope under
  finite limits; they run separately from Lua.

Do not design around an always-running plugin object, arbitrary editor RPC,
direct document pointers, or ambient filesystem access. See the
[threat model](@/reference/security.md#process-and-authority-boundaries).

## Understand the current limits

The API exposes native, typed UI rather than arbitrary webviews or custom editor
widgets. Plugins cannot run shell commands, load native Lua modules, obtain
credentials, choose arbitrary executables, or use unrestricted network access.
Scoped HTTPS requests target separately declared and reviewed public endpoints.

Plugin document operations apply to eligible local buffers. Remote and paged
documents are outside this extension contract. Workspace edit plans operate on
reviewed existing files; they require a complete preview and a separate commit flow.

Check each reference's limits before choosing an approach. For example, document
snapshots are bounded, provider results expire, and native service lifetimes are finite.
The [broker reference](@/reference/broker.md) describes the precise available operations.
