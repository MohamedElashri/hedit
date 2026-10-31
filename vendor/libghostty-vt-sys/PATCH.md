# Local Ghostty build adjustment

Based on `libghostty-vt-sys` from the locked Cargo dependency. Rust bindings and
the pinned Ghostty source commit are unchanged.

The native build selects the Zig version required by that source. It honors an
explicit `ZIG` executable, accepts the matching compiler on `PATH`, or calls
hedit's `scripts/setup-zig.py` to prepare an isolated compiler under `target/`.
It also watches `ZIG`, `PYTHON`, `PATH`, and the setup script for changes.
GitHub Actions uses `RUNNER_TEMP` for the compiler so Rust cache restoration
cannot prune its executable; the build also watches that CI environment.

On macOS, `scripts/run-zig.py` handles SDKs whose top-level `libSystem.tbd`
advertises ARM64e without ARM64. Zig 0.15.2 cannot resolve that reexport. The
wrapper creates a local SDK view under Cargo's output directory using Zig's own
bundled `libSystem.tbd`, while retaining the selected SDK's headers, frameworks,
and other libraries. An xcrun shim is scoped to the Zig subprocess, including
the build runner. No installed SDK or global developer-tool setting is changed.
The cache is invalidated when the SDK or bundled definitions change; `SDKROOT`
and `DEVELOPER_DIR` changes also trigger a rebuild.
The same view supplies guarded `INFINITY` and `NAN` fallbacks when an SDK math
header delegates them to the newer Clang `float.h` module protocol. All other
header contents are preserved. Offline regression tests live in
`scripts/test-run-zig.py`.

This prevents ordinary Cargo and Make builds from accidentally using a newer,
incompatible system Zig. `GHOSTTY_SOURCE_DIR`, `GHOSTTY_ZIG_SYSTEM_DIR`, and the
optional pkg-config build path retain their upstream behavior.
