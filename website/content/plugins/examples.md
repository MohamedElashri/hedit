+++
title = "Explore the examples"
description = "Use the shipped fixtures to learn document edits, providers, and scoped integrations."
weight = 50
template = "page"
+++

The repository includes complete manifests and fixtures under
[examples/extensions](https://github.com/MohamedElashri/hedit/tree/main/examples/extensions).
Installed packages also include these reference files with their documentation.
Use them as working examples of the contracts, and keep their capability limits
when adapting them.

## Document commands

| Example | What to study |
| --- | --- |
| `sdk-markdown` | Declared arguments, reviewed modules, Unicode, and full-document replacement |
| `sdk-table` | Structured edits and conversion of tab-separated text into a Markdown table |
| `sdk-trim` | Settings, line endings, and an explicitly bound Save hook |

The table example handles tab-separated text; it is not a general CSV parser.
The document SDK explains [transactions and coordinates](@/reference/document-sdk.md#coordinates-and-transactions).

## Providers and visual packs

`document-assist` demonstrates typed diagnostics, hover, navigation, and formatting
with deterministic fixtures. `demo-visuals` demonstrates declarative language and
theme contributions. Start with the
[provider contract](@/reference/providers.md#registration) before adapting callbacks.

## Scoped services

| Example | Behavior | Additional resource review |
| --- | --- | --- |
| `scoped-json` | Native JSON formatting and persistent document-bound project assistance | Exact `hedit-json-tools` image; a directory for project features |
| `scoped-rename` | Reviewed ASCII identifier replacement across existing files | Directory discovery, read, and edit access |
| `scoped-markdown-html` | Send the selected Markdown to GitHub and insert returned HTML as text | The exact declared public HTTPS endpoint |

These are bounded references. The JSON example does not provide JSON Schema
completion; identifier replacement is textual rather than language-aware symbol
renaming. The HTTPS example discloses the selection to GitHub and can fail or be
rate-limited. Returned HTML is text, never executed.

The [reference integration guide](@/reference/broker.md#reference-integrations)
links the manifests and explains native-image locations, project copies, and limits.
Use the [threat model](@/reference/security.md) to understand what the boundary enforces.
