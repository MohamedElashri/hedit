# Roadmap

The priority is a Linux editor that keeps long-lived scratch buffers dependable,
opens large logs with bounded memory, and offers focused CUDA tools.

## Implemented

- [x] Native Rust UI and Cargo-only build.
- [x] Background recovery, final-close gating, previous-checkpoint fallback,
  corruption preservation, and process locking.
- [x] Untitled buffers, persistent scratchpad names, and a searchable closed-buffer
  shelf that can reopen any entry.
- [x] Atomic file saves, source-change guards, and UTF-8 validation.
- [x] Bounded large-file pages, literal whole-file search, log following, and
  persistent byte-position bookmarks.
- [x] Single-selection editing, bounded process-local undo/redo, wrap, and font controls.
- [x] Four palettes, custom interface/syntax colors, JSON theme import/export,
  and an independent SVG identity.
- [x] CUDA and C/C++ lexical highlighting.
- [x] CUDA-GDB controls and console, Compute Sanitizer, and GPU inventory.
- [x] Linux install target, versioned portable package, checksum, and packaged user manual.
- [x] User guides under `docs/`, with contributor material kept separately.
- [x] Recovery, killed-process, file-safety, Unicode, large-file, theme, and transport tests.

## Next: reliability and daily use

- Durable edit journal and recovery checksums.
- Power-cut/filesystem fault injection and long-running soak tests.
- Persistent undo, and bookmark anchors that follow undo and external replacements.
- Explicit line-ending conversion and broader encoding support.
- Single-instance CLI file forwarding.
- Folder tree and fuzzy file finder, using the retained bounded folder utilities.
- Continuous large-file scrolling, global line indexes, and log filtering.
- Desktop accessibility and visual validation on X11 and Wayland.
- Installable DEB/RPM packages and published release verification.

## Larger editor features

- Regex search, case-insensitive search, and replace-all controls.
- Multiple cursors, rectangular selections, folding, and split views.
- More source-language lexers and incremental multiline state across pages.

## CUDA workbench

- NVIDIA hardware and driver testing.
- Source-position synchronization and source-gutter breakpoints.
- Structured variables, watches, and GPU execution views.
- Remote setup workflows and optional additional GPU tool integrations.

The [user manual](docs/README.md) describes current behavior. Contributor and
implementation notes are linked from [CONTRIBUTING](CONTRIBUTING.md).
