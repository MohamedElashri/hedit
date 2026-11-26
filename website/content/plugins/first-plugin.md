+++
title = "Write your first plugin"
description = "Add a prefix to the current document with a Lua command."
weight = 10
template = "page"
+++

Use **manifest API 3 / SDK 1** for this command. Install hedit with sandbox
support and put `hedit` on your `PATH`.
On macOS you can use `hedit.app/Contents/MacOS/hedit` directly.

{{< download file="downloads/first-plugin.zip" name="first-plugin.zip" text="Download the complete example" >}}

{{< flow "Write the command" "Run the fixtures" "Pack the bundle" "Install and approve" caption="Use the hedit CLI for fixtures and packing. Review permissions in Settings." >}}

## Create the manifest

Save `extension.json` in a folder called `first-plugin`.

{{< code file="examples/first-plugin/extension.json" language="json" >}}

| Field | What it does |
| --- | --- |
| Package and command IDs | Form `first-plugin.prefix`. |
| `script_files` | Reads sibling Lua source. Packing embeds it in the bundle. |
| `document.read` | Requests access to the current eligible local document. |
| `document.edit` | Requests permission to propose edits. Review both permissions after installation. |

## Write the command

Save `prefix.lua` beside the manifest.

{{< code file="examples/first-plugin/prefix.lua" language="lua" >}}

The host collects `prefix` and supplies `ctx.document.text`. `hedit.replace`
returns a proposal tied to the snapshot and revision. The host validates and
applies it. Use [transaction helpers](@/reference/document-sdk.md#command-contract)
for smaller edits. They take zero-based UTF-8 byte offsets.

## Add fixtures

Save `fixtures.json` in the same folder.

{{< code file="examples/first-plugin/fixtures.json" language="json" >}}

```sh
hedit plugin check extension.json
hedit plugin test extension.json fixtures.json
```

`check` validates the manifest. `test` runs the confined worker with sample
documents and creates no persistent approvals.

## Pack and install

```sh
hedit plugin pack extension.json first-plugin.hedit-plugin --version 1.0.0
```

1. Open **Settings → Extensions → Install from file…** and select the bundle.
2. Open **Review permissions…** for **Prefix document**. Use a disposable note
   and choose **Allow once** where available.
3. Run **Prefix document** from the Command Palette. Enter a prefix and apply
   the edit. If previews are on, review it first.
4. Use **Undo** to restore the note. See **Recent Lua activity** for the result and timing.

## Change the installed version

The installed bundle is a copy. After changing the source, rerun the fixtures,
pack a higher version, and choose **Update…**. Review and approve changed commands.
