+++
title = "API reference"
description = "The supported plugin contracts, exact limits, and security boundary."
weight = 40
template = "section"
sort_by = "weight"
+++

| Contract | Use it for |
| --- | --- |
| [Manifests](@/reference/manifests.md) | Contributions, selection transforms, packages |
| [Document SDK](@/reference/document-sdk.md) | Text transactions, inputs, private storage |
| [Provider SDK](@/reference/providers.md) | Events, diagnostics, completion, native UI |
| [Service broker](@/reference/broker.md) | Scoped workspace, tools, LSP, public HTTPS |
| [Threat model](@/reference/security.md) | Consent, isolation, limits, and residual risks |

Run `hedit plugin check` to verify a manifest against the installed editor.

{{< download file="downloads/schemas/plugin-manifest.schema.json" name="plugin-manifest.schema.json" text="Download the manifest JSON schema" >}}

{{< download file="downloads/schemas/hedit-sdk.lua" name="hedit-sdk.lua" text="Download the Lua SDK annotations" >}}
