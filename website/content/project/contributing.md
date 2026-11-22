+++
title = "Build and contribute"
description = "Set up a development build, check your changes, and open a focused pull request."
weight = 10
template = "page"
+++

## Set up a build

Install Rust, a native C/C++ toolchain, Git, Make, and Python 3.11+.
Use the Rust toolchain pinned in [CI](https://github.com/MohamedElashri/hedit/blob/main/.github/workflows/ci.yml).
macOS needs Xcode command-line tools; Windows needs Visual Studio C++ tools and MSVC Rust.
Linux needs an X11 or Wayland desktop and OpenGL to run the editor.

Clone the [repository](https://github.com/MohamedElashri/hedit), then build and run on Linux or macOS:

```sh
make dev
make run ARGS='--session-dir /tmp/hedit-dev'
```

The terminal build sets up pinned Zig automatically when needed. To build without it:

```sh
cargo build --locked --workspace --no-default-features
```

On macOS, use the Make targets to prepare the signed plugin helper. After a direct
Cargo build, run `python3 scripts/macos_plugin_helper.py --editor target/debug/hedit`.
On Windows, build with `cargo build --locked --workspace`.

## Check your change

Start with focused tests for the behavior you changed, then run:

```sh
make static
make clippy
```

| Change | Validation |
| --- | --- |
| Application code on Linux | `make test` |
| Application code on macOS | `make test-macos` |
| Windows behavior | The Windows CI workflows; include a regression test |
| Website only | `make test-docs-site docs-site` with the pinned Nida release |

Use disposable documents and a separate session for manual tests. Keep filesystem,
network, and process work off the UI thread. Changes to saves, recovery, Unicode,
or plugin permissions need regression coverage that preserves those boundaries.

## Open a pull request

- Keep the change focused on one problem and explain the resulting behavior.
- Include the checks you ran and any platform you could not verify.
- Update the relevant user guide or API contract when behavior changes.
- Include screenshots for visible UI changes and retain required license notices.

## Further reference

- [Plugin development](@/plugins/_index.md) covers authoring extensions.
- [Threat model](@/reference/security.md) defines the plugin security boundary.
- [Website maintenance](https://github.com/MohamedElashri/hedit/tree/main/website) covers Nida and Pages.
- The [maintainer guide](https://github.com/MohamedElashri/hedit/blob/main/DEVELOPER_GUIDE.md)
  contains the code map, feature-specific tests, architecture, and release procedures.
