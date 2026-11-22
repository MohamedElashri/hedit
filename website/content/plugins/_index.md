+++
title = "Develop plugins"
description = "Build, test, and distribute plugins for hedit's isolated Lua extension system."
weight = 30
template = "section"
sort_by = "weight"
+++

hedit plugins contribute editing commands, snippets, file templates, language
providers, visual packs, and scoped project or service integrations.
Lua runs in a separate, confined process. The host validates results and controls
access to documents, folders, native tools, and endpoints.

You can develop plugins with a stable hedit installation and a text editor.
The built-in CLI validates manifests, runs fixtures, and creates distributable bundles.
You do not need to build hedit itself.

If you want to install a plugin rather than write one, see
[Install and manage plugins](@/guide/extensions.md).
