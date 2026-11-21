# Plugin codec fuzzing

The native validation workflow runs a ten-minute sanitizer campaign twice weekly
on the default branch. Dispatch `macos.yml` with `protocol_security=true` to run it
on a review branch; ordinary dispatches and release callers retain universal macOS
packaging. Campaign artifacts retain source snapshots/hashes, metadata, logs, and
corpus for 14 days.

This target exercises legacy, editing, provider, and broker request decoders; legacy replacement replies;
SDK transaction preparation; and typed success/failure replies. It never evaluates Lua
or starts processes. Synthetic seeds cover framed requests, transactions, provider and broker results,
success, and failure; keep reproducing failures as regression tests.

With `cargo-fuzz` and a supported nightly Rust toolchain installed:

```sh
cargo +nightly fuzz run sdk_protocol -- -max_len=2097152 -timeout=5 -rss_limit_mb=1024 -max_total_time=300
```

The two-MiB input cap covers the new broker envelope. Legacy/editing/provider
requests retain their separate one-MiB limit. Aggregate text, arrays, and cache
limits are checked independently of framing. Native sandbox and GUI authorization tests remain separate; this target
cannot prove confinement, revocation, or whole-editor responsiveness. Do not
interpret a stable-toolchain compile or deterministic mutation run as a
coverage-guided campaign. Store campaign duration, toolchain, platform, corpus,
and any artifacts with the validation report before accepting the stage gate.

For a host Rust compiler that supports LLVM sanitizer coverage, the repository
also provides a reproducible coverage-only runner:

```sh
python3 scripts/fuzz-plugin-codec.py --seconds 300
```

This runner instruments the explicit host target to avoid instrumenting Cargo
build scripts. It does **not** enable AddressSanitizer. Its output directory keeps
raw logs, source hashes, dirty-tree status, build metadata, generated corpus, and
crash artifacts. Use the nightly
workflow for sanitizer campaigns; both workflows exercise the same target.

For an AddressSanitizer campaign with nightly Rust and a matching Clang runtime:

```sh
python3 scripts/fuzz-plugin-codec.py --sanitizer address --seconds 600 --out-dir target/plugin-fuzz-asan
```

This also instruments vendored native code through `CFLAGS`/`CXXFLAGS`. On macOS,
select compatible `CC`/`CXX` explicitly if the platform's Apple Clang has a
different sanitizer ABI from Rust. A sanitizer link failure is a failed campaign,
not a reason to suppress ABI guards or report coverage-only evidence as sanitized.
