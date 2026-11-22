+++
title = "Write your first plugin"
description = "Create a document command, test it in the sandbox, and install a bundle."
weight = 10
template = "page"
+++

This tutorial adds a prefix to the current document. It uses **manifest API 3 /
SDK 1**, the document-editing contract. Later SDKs add capabilities; this contract
remains supported.

Use an installed hedit package with sandbox support and the `hedit` command on
your `PATH`. On macOS, you can also use `hedit.app/Contents/MacOS/hedit` directly.

{{< download file="downloads/first-plugin.zip" name="first-plugin.zip" text="Download the complete example" >}}

## Create the manifest

Create a folder called `first-plugin` and save this as `extension.json`:

{{< code file="examples/first-plugin/extension.json" language="json" >}}

The package ID and command ID form `first-plugin.prefix`. `script_files` reads
sibling source during authoring; packing embeds those bytes in the bundle.
The two permissions request access to the whole eligible current local document
and permission to propose an edit. Installation alone does not grant either.

## Write the command

Save `prefix.lua` beside the manifest:

{{< code file="examples/first-plugin/prefix.lua" language="lua" >}}

The host supplies a snapshot in `ctx.document.text` and collects the declared
`prefix` argument before starting the worker. `hedit.replace` returns a proposed
transaction bound to that document and revision. Changing a Lua table does not
change the editor; the host owns validation and application.

This example replaces the whole document. For smaller changes, use
[`hedit.transaction` and `hedit.edit`](@/reference/document-sdk.md#command-contract).
All offsets in that API are zero-based **UTF-8 bytes**, not characters or UTF-16 units.

## Add fixtures

Save `fixtures.json` in the same folder:

{{< code file="examples/first-plugin/fixtures.json" language="json" >}}

From that folder, validate and test:

```sh
hedit plugin check extension.json
hedit plugin test extension.json fixtures.json
```

`check` validates without executing Lua. `test` uses the real confined worker
with synthetic documents and creates no persistent editor approvals.
The fixtures exercise Unicode, CRLF line endings, and a declared argument override.

## Pack and install

```sh
hedit plugin pack extension.json first-plugin.hedit-plugin --version 1.0.0
```

Open **Settings → Extensions → Install from file…**, select the bundle, and choose
**Install**. Open **Review permissions…** for **Prefix document**. On a disposable
local note, choose **Allow once** where offered or approve the command.

Run **Prefix document** from the Command Palette. Choose a prefix, review the edit
when previews are enabled, and apply it. One **Undo** restores the original text.
Use **Recent Lua activity** to inspect the result and timing.

## Change the installed version

The installed bundle is a copy. Editing your source files does not alter it.
Rerun the fixtures, pack a new version, and use **Update…** to review the new copy.
Changed commands need fresh approval. Revoke approval or uninstall the example
when you have finished.

Next, [choose a contract](@/plugins/choose-api.md) for the capabilities you need,
or read the [document SDK reference](@/reference/document-sdk.md).
