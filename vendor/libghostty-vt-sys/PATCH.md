# Local Ghostty build adjustment

Based on `libghostty-vt-sys` from the locked Cargo dependency. Rust bindings and
the pinned Ghostty source commit are unchanged.

The native build selects the Zig version required by that source. It honors an
explicit `ZIG` executable, accepts the matching compiler on `PATH`, or calls
hedit's `scripts/setup-zig.py` to prepare an isolated compiler under `target/`.
It also watches `ZIG`, `PYTHON`, `PATH`, and the setup script for changes.
GitHub Actions uses `RUNNER_TEMP` for the compiler so Rust cache restoration
cannot prune its executable; the build also watches that CI environment.

This prevents ordinary Cargo and Make builds from accidentally using a newer,
incompatible system Zig. `GHOSTTY_SOURCE_DIR`, `GHOSTTY_ZIG_SYSTEM_DIR`, and the
optional pkg-config build path retain their upstream behavior.
