+++
title = "Test and distribute"
description = "Use fixtures, profile the real boundary, and publish reviewable plugin bundles."
weight = 40
template = "page"
+++

## Keep fixtures beside the source

Use an installed hedit package with sandbox support.

```sh
hedit plugin check extension.json
hedit plugin test extension.json fixtures.json
hedit plugin test extension.json fixtures.json --timings
```

| Contract | Include in fixtures |
| --- | --- |
| Document command | Empty input, Unicode, CRLF, selections, inputs, unchanged text, and limit cases. Use offsets into the exact snapshot. |
| Provider | Typed results and explicit cache. |
| SDK 3 | An ordered transcript of intents and synthetic responses. Use a native integration test for real side effects. |

See [offline author fixtures](@/reference/broker.md#offline-author-fixtures).

## Check interactive behavior

Use disposable documents and a separate configuration directory.

```sh
HEDIT_CONFIG_DIR=/tmp/hedit-plugin-config hedit
```

```powershell
$env:HEDIT_CONFIG_DIR = Join-Path $env:TEMP 'hedit-plugin-config'
hedit
```

Check edits and Undo. Change the document or revoke the grant to exercise stale
work. Try **Stop** and **Resume** for providers. Review each real directory,
native image, and endpoint the plugin requests.

## Measure complete invocations

Use `--timings` and **Recent Lua activity**. Compare wall time with worker
evaluation. Prompt and preview dwell are measured separately. Phases can overlap.
Measure realistic documents and sustained typing. Debounce automatic callbacks.
See the [performance probes](@/reference/document-sdk.md#validation-and-performance-probes).

## Build a release bundle

```sh
hedit plugin pack extension.json example.hedit-plugin --version 1.0.0
```

Packing embeds declared Lua modules and resources. Ship the bundle with its
install steps, supported API, data scopes, limits, and platforms.

For a signed release, create a publisher key and keep it outside the bundle.

```sh
hedit plugin keygen publisher.key
hedit plugin pack extension.json example.hedit-plugin --version 1.0.0 --publisher "Your name" --sign-key publisher.key
```

Share the printed fingerprint separately. Sign updates with the same key and a
higher bundle version. Users review the new bundle and grant command access
separately. See [bundles](@/reference/manifests.md#develop-and-package).
