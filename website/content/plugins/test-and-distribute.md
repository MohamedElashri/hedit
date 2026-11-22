+++
title = "Test and distribute"
description = "Use fixtures, profile the real boundary, and publish reviewable plugin bundles."
weight = 40
template = "page"
+++

## Keep fixtures beside the source

Run the CLI from an installed hedit package with working native sandbox support:

```sh
hedit plugin check extension.json
hedit plugin test extension.json fixtures.json
hedit plugin test extension.json fixtures.json --timings
```

Cover empty input, Unicode, CRLF, multiple selections, declared input overrides,
and unchanged text where they apply. Ranges use byte offsets into the exact
snapshot. Include boundary cases near your output and item limits, and test that
invalid input produces a safe result.

Provider fixtures compare typed results and explicit cache. SDK 3 fixtures also
declare an ordered transcript of expected intents and synthetic responses.
These transcripts test callback behavior without filesystem effects, HTTPS
requests, or native tool launches. They do not replace native integration tests.
See [offline author fixtures](@/reference/broker.md#offline-author-fixtures).

## Check interactive behavior

Use disposable local documents and a test configuration directory:

```sh
HEDIT_CONFIG_DIR=/tmp/hedit-plugin-config hedit
```

On Windows, set the environment variable in PowerShell before launching:

```powershell
$env:HEDIT_CONFIG_DIR = Join-Path $env:TEMP 'hedit-plugin-config'
hedit
```

Install the bundle, review its grants, and test edits and Undo. Check that changing
the document or revoking a grant invalidates stale work. Exercise Stop/Resume
for providers. Separately review native images, directories, and endpoints when
your plugin requests them; fixture execution does not create those grants.

## Measure complete invocations

Use `--timings` and **Recent Lua activity** to distinguish launch, transport,
validation, and application from evaluation. Prompt and preview dwell are
separate from active worker time. Do not report Lua evaluation time as end-to-end
latency or add overlapping phases as though they partition wall time.

Measure with realistic document sizes and sustained typing. Debounce automatic
providers and avoid rediscovering the same project data on every event. Prefer
bounded work and explicit continuation state over repeated full-document scans.
The [validation and performance reference](@/reference/document-sdk.md#validation-and-performance-probes)
explains the host probes and their limitations.

## Build a release bundle

```sh
hedit plugin pack extension.json example.hedit-plugin --version 1.0.0
```

Packing embeds declared sibling Lua modules and resources. Distribute the bundle
and installation instructions; include your plugin's purpose, data scopes,
supported contract, known limits, and tested platforms.

For signed releases, create a publisher key once, outside the files you distribute:

```sh
hedit plugin keygen publisher.key
hedit plugin pack extension.json example.hedit-plugin --version 1.0.0 --publisher "Your name" --sign-key publisher.key
```

Keep the private key private. Publish its printed fingerprint through a separate
channel. Signed updates must use the installed publisher key and increase the
bundle version. Users review updates and grant command access separately.
See [bundle behavior](@/reference/manifests.md#develop-and-package).
