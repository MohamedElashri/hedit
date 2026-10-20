# Contributing to hedit

Build with Rust 1.92 or newer. The application is native Rust and must remain
buildable without a web frontend or Node package manager. See the
[user installation guide](docs/installation.md) for native Linux dependencies.

## Build and check

```sh
cargo build --locked
make check
make package
python3 scripts/smoke-package.py
```

`make check` validates the native-only policy, documentation links, formatting,
clippy warnings, and tests. `make package` builds the optimized binary and creates
a versioned Linux tarball with the launcher, user guides, SVG assets, themes,
CUDA example, and an installation script.

Use `DESTDIR` with `make install` to inspect an installation without changing your
home directory:

```sh
make install DESTDIR=/tmp/hedit-stage PREFIX=/usr
/tmp/hedit-stage/usr/bin/hedit --version
```

The package script requires Python 3.11 or newer and the standard library only.
The application itself does not need Python.

## Where to look

- [Architecture](contributor/architecture.md)
- [Recovery storage and preservation](contributor/recovery.md)
- [Headless visual review](contributor/visual-review.md)
- [Product roadmap](ROADMAP.md)
- [Brand assets and generation](assets/brand.md)

## Testing changes

Changes that can lose a buffer, overwrite a file, or invalidate recovery
references need behavior tests. Cover old session files when changing persisted
metadata. Keep original-file conflict checks and failed-restore preservation.

The tests use temporary directories. A sparse 3 GiB file tests bounded opening
without allocating 3 GiB of RAM. A child process is killed after checkpointing to
exercise recovery and process-lock release. NVIDIA tool transport is tested with
a subprocess fixture; actual device debugging still needs hardware validation.

User-facing behavior belongs in `docs/`. Keep internal design, storage-format
notes, test procedures, and unimplemented work in contributor notes or the
roadmap. Document observed limits, and do not describe planned features as ready.

## Provenance

Retain upstream MIT attribution when modifying derived file-engine or recovery
code. This project is intended to start with its own Git history; the upstream
base is recorded in NOTICE rather than depending on inherited commits.
