# Plugin codec fuzzing

This target exercises both request decoders, legacy replacement replies, SDK
transaction preparation, and SDK failure/success replies. It never evaluates Lua
or starts processes. Synthetic seeds cover a framed request, transaction,
success, and failure; keep reproducing failures as regression tests.

With `cargo-fuzz` and a supported nightly Rust toolchain installed:

```sh
cargo +nightly fuzz run sdk_protocol -- -max_len=1048576 -timeout=5 -rss_limit_mb=1024 -max_total_time=300
```

The one-MiB input cap bounds decoder allocation and matches the request frame
ceiling. Native sandbox and GUI authorization tests remain separate; this target
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
raw logs, build metadata, generated corpus, and crash artifacts. Use the nightly
workflow for sanitizer campaigns; both workflows exercise the same target.
