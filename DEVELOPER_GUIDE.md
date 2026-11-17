# hedit developer guide

Build, test, and release hedit. Features and shortcuts live in the
[user guide](USER_GUIDE.md), which is also bundled as offline help.

## Build and install

Requirements: Rust 1.99+, C compiler, Git, GNU Make, Python 3.11+. CI pins
Rust 1.99.1 and Python 3.12. Linux needs X11/Wayland and OpenGL at runtime,
without their development headers. The UI is native Rust; no webview or Node.js.

```sh
make dev
make run ARGS='--session-dir /tmp/hedit-dev'
make build
make install                         # ~/.local; override PREFIX or DESTDIR
cargo build --locked --no-default-features  # omit terminal and Zig
cargo build --release --locked -p hedit-remote
```

Default builds include the opt-in terminal. Ghostty needs Zig 0.15.2; Cargo finds
it or installs an isolated copy under `target/ghostty-toolchain` (`RUNNER_TEMP` in
CI). Override with `ZIG` or select the setup interpreter with `PYTHON`. First setup
needs network access and pip. Offline builds can supply `GHOSTTY_SOURCE_DIR` and
`GHOSTTY_ZIG_SYSTEM_DIR`; see the [bindings build instructions](https://github.com/Uzaaft/libghostty-rs#building).

Launches detach after window readiness; later errors go to the session's
`hedit.log`. Isolate GUI tests with a fresh session and temporary XDG
config/data/cache/state roots.

## Checks

Run **`make ci`** before merging: formatting, strict Clippy, docs/native policy,
Rust/script tests, and Linux release package/install smoke checks.

| Command | Scope |
| --- | --- |
| `make static` / `make lint` | Formatting, docs, policy, script syntax; lint adds workspace Clippy |
| `make test` | Workspace, patched renderer, installer/package tests |
| `make test-renderer` | Patched egui/epaint tests |
| `make test-remote` | Headless helper and recovery tests |
| `make test-remote-live` | Disposable SSH, crashes, reconnects; requires `sshd` |
| `cargo test --locked --no-default-features` | Without terminal support |
| `make test-macos` | macOS-applicable tests |
| `make workflow-lint` | Actions checks; requires `actionlint` |
| `make help` | Targets and overrides |

Ignored tests are profiles, native checks, or preview exports; read their ignore
reasons. Keep timing runs separate from builds/heavy I/O. Headless frame timings
are not display latency. To reproduce the CPU profiles for 1/100/1,000 tabs,
40,000-line documents, long lines, selections, and split panes:

```sh
cargo test --locked --release --bin hedit profile_workspace_interaction -- --ignored --nocapture --test-threads=1
```

Record the date, hardware, OS, toolchain, and build alongside results. Report these as warm
CPU frame measurements; use the native diagnostics below for CPU/GPU traces.

### Screenshots

On Linux/macOS, install OpenSSH server/client/keygen and FFmpeg, then run:

```sh
python3 scripts/export-readme.py --work-dir /private/disk/hedit-images
```

Use a private fixture parent on a supported durable filesystem, as for the live
SSH tests below; `HEDIT_REMOTE_LIVE_DIR` also selects it. `CARGO` can select a
toolchain, and `--output-dir` previews outside `docs/images/`. The script builds
the helper and terminal-enabled editor tests, starts a loopback-only SSH server
with disposable keys, exports the gallery, and removes the SSH fixture.

Images render real app frames from an Atlas sample project. Recovery actually
drops the app, restores its session, and reopens a closed unsaved note. SSH uses
host durability receipts and ephemeral documents. Keep these assertions and
inspect all exported images, the animation, and the README layout. Keep personal
paths and workspace content out of fixtures.

### Native frame diagnostics

Set `HEDIT_NATIVE_TRACE_FILE` to an absolute private path in an isolated editor;
`scripts/summarize-native-trace.py FILE` groups the numeric samples by input type.
Tracing is opt-in, bounded, asynchronous, and excludes keys, text, paths,
clipboard, terminal output, and SSH diagnostics. GPU drawing excludes presentation
and display latency. Keep collection/file I/O off the UI thread.

## Code map

| Area | Location |
| --- | --- |
| Shared document/filesystem/search/Git/build engine | `crates/hedit-core/` |
| SSH transport, worker, journal, path isolation | `crates/hedit-remote/` |
| Sessions, settings, syntax, editing, updates | `src/` |
| Native frame loop and feature panels | `src/app.rs`, `src/app/` |
| Integration tests and SSH workspace driver | `tests/`, `tests/support/` |
| Renderer patches and Ghostty build integration | `vendor/` |
| Packaging/installers/toolchain setup | `scripts/`, `packaging/` |

The editor re-exports engine modules; the helper does not link the GUI. Commands
share `Command` in `src/app/commands.rs`: add labels, availability, execution, and
input dispatch together. Register new dialogs in `opens_window` and `Overlay`
for consistent focus/Escape. Help resolves only explicitly bundled documents.

## Rules to keep

- **Preserve text.** Test failed saves/checkpoints and recovery; keep corrupt or
  ambiguous data for inspection. Publish saved state only after durable file
  replacement and parent-directory sync.
- Keep storage/scans/process work off the UI thread. Bound queues/caches/snapshots
  and reject results after relevant document, revision, selection, pane, or
  project changes. Layout indexes stay in memory.
- Preserve Unicode, CRLF, RTL selection, wrap affinity, and pane focus. Compare
  optimized layout/cursor/mesh paths against reference behavior.
- Terminals stay opt-in; hidden tabs drain output, disabling/quitting ends shells.
  Shell input wins over editor shortcuts; RTL display preserves logical copying.
- Keep Markdown native/bounded, with guarded local media, no automatic network
  loads, source spans, accessible links, and shared selection/layout geometry.
- Document dependency patches/licenses. Ghostty's wrapper changes only a
  build-local SDK view, never installed Apple tools. Changes to compiler selection
  or wrappers require `scripts/test-zig-setup.py` and `scripts/test-run-zig.py`.

## SSH architecture and testing

[User setup and limits](USER_GUIDE.md#ssh-remote-workspaces). Each connection owns
a native window. Remote documents are memory-backed/ephemeral; local recovery
rejects them and launch metadata excludes content/tool output. Recovery, undo,
and workspace metadata belong to the host.

Keep pinned host keys, direct connections, explicit execution trust, and
reconnect trust reset. No forwarding, multiplexed connection reuse, credentials,
or workspace-provided SSH configuration. Validate paths with descriptor-relative,
no-symlink operations; remote identities never grant local filesystem access.

Writer epochs fence old clients; receipts bind session, sequence, and revision.
Replay deduplicates committed edits. Preserve pending edits across disconnects;
fail closed on conflicts/quotas. Never blindly retry uncertain saves/mutations.
UI metadata checkpoints must not wait on a document mutex held by an SSH job.

The live harness uses disposable keys, private state, pinned loopback hosts, and
`tests/support/remote_workspace_probe.rs`. Test drivers never ship. Fault helpers
use a separate target directory and refuse release builds.

```sh
make test-remote-live
HEDIT_REMOTE_LIVE_DIR=/private/disk/hedit-tests make test-remote-live
```

Default fixtures use `target/rl`. Choose a short, durable filesystem path:
ext-family/XFS/btrfs/ZFS on Linux, APFS on macOS; `/tmp` only if disk-backed.
Mutable ancestors require safe ownership/permissions; writable shared ancestors
need the sticky bit. A private child cannot fix an unsafe checkout. Do not weaken
production checks or change shared permissions to make fixtures pass.

Real-host tests use selected non-root accounts, private temporary workspaces,
pinned keys, and agent authentication. Stop only test-owned jobs; remove their
state/locks afterwards. Agent approval must complete within startup's 15 seconds.
Durable-RPC timings include host commits and network trips, not display latency.

## Workbench navigation

Sidebar selection is independent of the editor surface. A Git review is a
reusable transient tab, not a document or recovery buffer. Folder search owns
its query/options separately from in-document Find. Search result grouping is
cached by result generation, context and collapsed files; only visible rows are
painted. Hidden sidebars retain state and accept pending results, but do not
schedule Git refreshes or render their contents.

The optional activity bar uses existing commands and workspace state. Hiding it
restores the text workspace switcher. Distraction-free mode hides both without
changing their saved preferences. Shared UI metrics scale status and terminal
headers independently of document and terminal content. Refresh stored built-in
palettes only when every color matches an old default; preserve custom colors.

## Settings UI

`src/app/settings_browser.rs` contains searchable preference metadata and validated
controls. Keep every persisted user preference discoverable by its JSON key;
complex editors link to their category. Onboarding bookkeeping is excluded.
Search and Modified filters share the same catalog, and reset uses the serialized
default value so optional formatting rules retain their inherit/true/false states.
Validate edits before applying them and preserve text drafts while focused or
invalid; otherwise list separators disappear while users type. User settings and
host-side server configuration have distinct scope and save behavior.
Preference writes must fit each file's reload limit. Write only changed key
binding, snippet and macro files, and preserve unreadable files until repaired;
fallback defaults are not permission to replace corrupt data.

Keep both category navigation and content scrollable inside the viewport, with
search and the footer outside those scroll regions. Measure header controls and
category widths using the current UI font, and reserve space for a wrapped footer.
Category arrow keys belong to the focused navigation list, never a text field.
Font choices share the asynchronous installed-family inventory and validate via
the settings catalog. Keep invalid drafts and field focus when drawing flat rows.
The minimum-window and
pointer-reset tests cover geometry and interaction. The ignored
`export_settings_browser` test exports previews to `HEDIT_PREVIEW_DIR`.

## Language-server architecture and tests

`crates/hedit-core/src/language_server/` owns the server catalog, managed package
receipts and locks, bounded stdio JSON-RPC client, position conversion, and
workspace service. `src/app/language_servers.rs` connects the native manager,
Problems panel, and guarded editor results; `auto_complete.rs` merges LSP choices.
The manager is also used by the headless `--lsp` commands.

Every workspace has a bounded worker mailbox. Filesystem/process/network work
runs there; it does not use the editor's blocking job flag. Shared installation
leases prevent changes while another window owns a server. Installs verify
upstream digests, stage/extract with path and size limits, then atomically publish
an exact receipt. Keep external executable ownership separate from managed files.
Multiple servers may own the same document. Route each request by supported and
enabled feature, descending priority, then server ID. The eight-session cap first
favors focused documents, then server priority, and starts selected sessions in
that order. Keep diagnostics keyed by server/project as well as path, and clear only the affected source. Launch environment, arguments, root
markers, initialization options, and runtime settings are separate persisted
controls. Missing initialization options retain compatibility with older settings.
Catalog presets include external-only servers; an empty repository forbids a
managed install. PATH detection runs on the host worker and never executes a probe.

SSH language workers live in the helper runtime and are tied to a bound connection,
writer epoch, and execution trust. Commands/snapshots use the existing bounded
upload/download protocol. Revocation and disconnect drop workers. Validate host
paths before enqueueing, map navigation URIs explicitly, and never fall back to
client filesystem access for remote results. Update both helper/client protocol
versions and the live harness together when changing the wire contract.

Normal tests use `crates/hedit-core/tests/support/fake_language_server.py` and
synthetic archives, without network or installed language servers:

```sh
cargo test --locked --workspace language_server
cargo test --locked -p hedit-remote language_
cargo test --locked -p hedit-core live_managed_server -- --ignored --nocapture
```

The last command is an explicit network smoke test: it installs verified upstream
rust-analyzer and clangd into temporary storage, starts real stdio sessions,
requests hover information, and removes the packages. It requires curl and the
relevant toolchains. `live_cached_native_installs_start_and_serve_language_requests`
also exercises real ty, Ruff, LuaLS and Tombi archives when `HEDIT_LSP_ASSET_DIR`
contains the official archives and GitHub release JSON; it still verifies every
archive against its upstream digest. Offline release snapshots cover native
asset selection on Linux, macOS and Windows. The disposable SSH suite additionally checks language-server
execution, Unicode navigation, large snapshots, registration ownership, and trust
revocation through the production bridge. Use `HEDIT_PREVIEW_DIR` with the ignored
`export_language_server_settings` test to inspect the native manager layout.
Terminal appearance uses a separate egui font family and optional color overrides.
Load terminal fonts off the UI thread with generation checks; preserve that family
when editor/fallback fonts change. Live style changes resize the existing PTY and
preserve its VT state, including application OSC color overrides. Reapply the
configured palette when a session is restarted.

## Linux release packages

On Ubuntu 24.04, install `rpm`, `dpkg-dev`, `desktop-file-utils`, `cpio`, `file`, and the
X11/Wayland client libraries. Run `python3 scripts/setup-appimage.py`, then
`make linux-packages`. Tools and the AppImage runtime are checksum-pinned; no
FUSE is required to build. `make package smoke` still checks the tarball alone.

`make smoke-linux-packages` checks metadata, payloads, checksums, AppImage
extraction/launch, and container install/upgrade/removal on Ubuntu and Fedora
(requires Docker). Release CI runs this on both native Linux architectures before
uploading any packages. AppImages bundle display clients and their license notices;
glibc and GPU drivers come from the host. Managed packages disable in-app replacement.

## macOS builds

Install Xcode command-line tools, then:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
make test-macos
make macos
```

The universal app targets macOS 11+. Packaging verifies both slices/resources,
signs, verifies, and checksums the ZIP. Default signing is ad hoc, not notarized;
`--identity` on `scripts/package-macos.py` selects a Developer ID. CI checks the
shipped executable/signature; desktop checks cover clipboard, scaling,
accessibility, dialogs, and windows.

## Windows builds

Use x86-64 MSVC Rust and Visual Studio C++ tools for Windows 10+ with OpenGL:

```powershell
cargo build --release --locked
python scripts/package-windows.py --smoke
```

CI tests the workspace/renderers and extracted PE executable. Packages are
unsigned portable folders. Windows is an SSH client, not a helper host. Validate
dialogs, clipboard, scaling, accessibility, and window recovery on a desktop.

## Themes and branding

Themes are JSON under `themes/`; preserve schema fields and
[palette credits](assets/licenses/theme-palettes.md). Regenerate brand SVGs with
`python3 scripts/generate-brand.py`; keep `src/brand.rs` consistent. Asset
dark/light names describe their intended background.

## Packaging and releases

`make package smoke` checks the Linux archive, checksum, extraction, installation,
and executable. Ship guides, notices, assets, themes, and CUDA examples; exclude
test drivers/development scripts. Keep packagers, installers, and tests aligned.

1. Update workspace versions/lockfile and a concise dated `CHANGELOG.md` entry.
   Match README feature availability to the release downloads.
2. Run `make ci`, no-default-features/live SSH tests, native platform validation,
   and `make release-check RELEASE_TAG=vX.Y.Z`.
3. Review notes from
   `RELEASE_TAG=vX.Y.Z python3 scripts/check-release.py --notes-file /tmp/hedit-notes.md`.
4. After review, commit/push changes, then create/push the version tag.

Tags validate Linux/macOS/Windows before publishing Linux x86-64/ARM64
tarballs, DEB/RPM/AppImage packages,
a universal macOS ZIP, and a Windows x86-64 ZIP with checksums/changelog notes.
Hyphenated tags are prereleases; manual workflows build without publishing tags.

Updaters verify checksum, archive paths, candidate version, and macOS signature;
preserve rollback and checkpoint before restarting. macOS swaps the whole bundle.
The installer is served from `main`; keep published asset names compatible.

## Provenance and license

The engine derives from [rsedit](https://github.com/goodappsstore/rsedit), commit
`63a9ee4966d3fe1cfcd7d4af70e963e10aa297ff`. Preserve its [MIT notice](LICENSE) and
all bundled font/dictionary/renderer/palette notices under `assets/`, including
the math color table's LPPL license. Workbench icons derive from pinned Microsoft
Codicons SVGs; preserve their CC-BY-4.0 license and attribution in
`assets/licenses/codicons-NOTICE.txt`. Their source revision and hashes are in
`assets/icons/codicons/provenance.json`. Icons use bounded, cached native SVG
rasterization and never depend on the selected text font.

Local patches: [Markdown](vendor/egui_commonmark/PATCH.md),
[Ghostty build](vendor/libghostty-vt-sys/PATCH.md). Text-renderer patches share row
meshes, bound viewport submission, and index cursor/glyph lookups. Preserve their
normal-gate parity tests, or remove patches and corresponding tests together.

### Shared bottom panel

`app/workbench_panel.rs` owns the shared bottom region; Problems, Output and
Terminal select one content renderer. Visibility flags never own worker lifetimes.
Build and PTY polling run independently of layout, so hidden tabs drain output
and report completion. Only the selected tab performs layout. Output line ranges
and formatted language diagnostics are cached until their source changes; rows
are virtualized. Switching away clears terminal composition and shortcut prefixes
without replacing the PTY or touching editor undo.

### Group-local tabs

`app/panes.rs` stores document IDs and per-tab cursor/revision snapshots for each
stable group ID. `Workspace::open` remains the sole owner of document buffers;
groups never clone text or undo journals. Shared-tab closure detaches a view,
while the last reference uses the existing save/discard/recovery flow. Layout
reduction merges unique references. Restored legacy layouts place existing tabs
in the first group. Pending reads capture stable group identity as part of their
focus guard, and background results attach without redirecting input. Tab strips
reuse derived global title/location geometry and paint only visible tabs.

### Document navigation

`app/document_navigation.rs` derives breadcrumbs from paths without filesystem
queries and retains at most four groups of metadata. The optional Outline reuses
the revision/path-checked native symbol cache. It requests no work while hidden,
skips paged and >2 MiB snapshots, uses a worker above 64 KiB and virtualizes up to
500 result rows. It starts no language server. Outline filter focus follows the
same Escape handoff as persistent Search.

### Scoped settings and formatting sources

`settings_scope` hosts the existing project GUI/JSON draft in Preferences. Scope
switches retain the draft, and Save checks the original settings fingerprint and
dirty project-file buffers before merging unknown JSON keys. Formatting provenance
uses the same resolver as open/save, with source allocation enabled only for the
inspector. A single background worker resolves the visible file; context keys reject
stale results and an explicit refresh rereads external EditorConfig changes.

### Shortcut resolution

`shortcut_preset` resolves personal bindings before the optional common-command
preset, then suppresses defaults whose keys or chord prefixes are claimed. Its
cache rebuilds only when the preset or personal bindings change. Menus, palette,
settings, chords and terminal routing share that result. Context routing still
restricts shell and settings input; `editor_only` applies to chords as well as
single keys. Keep local-only availability in `Command::requires_local` so menus
and execution agree for SSH workspaces.
