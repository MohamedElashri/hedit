+++
title = "Explore the examples"
description = "Use the shipped fixtures to learn document edits, providers, and scoped integrations."
weight = 50
template = "page"
+++

Open the [example sources](https://github.com/MohamedElashri/hedit/tree/main/examples/extensions)
or find the same files in an installed package's documentation.

## Document commands

| Example | What to study |
| --- | --- |
| `sdk-markdown` | Declared arguments, reviewed modules, Unicode, full-document replacement |
| `sdk-table` | Structured edits that turn tab-separated text into a Markdown table |
| `sdk-trim` | Settings, line endings, and a bound Save hook |

See [transactions and coordinates](@/reference/document-sdk.md#coordinates-and-transactions).

## Providers and visual packs

| Example | What to study |
| --- | --- |
| `document-assist` | Diagnostics, hover, navigation, and formatting fixtures |
| `demo-visuals` | Declarative language and theme contributions |

See [provider registration](@/reference/providers.md#registration).

## Scoped services

| Example | Behavior | Review |
| --- | --- | --- |
| `scoped-json` | JSON formatting and native project assistance | The exact `hedit-json-tools` image and a project directory |
| `scoped-rename` | Textual ASCII identifier replacement in existing files | Directory discovery, read, and edit grants |
| `scoped-markdown-html` | Sends the selection to GitHub and inserts the returned HTML as text | The declared public HTTPS endpoint |

The JSON example omits JSON Schema completion. The HTTPS example can fail or
be rate-limited. See [reference integrations](@/reference/broker.md#reference-integrations)
for manifests and native-image setup.
