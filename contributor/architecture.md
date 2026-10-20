# Architecture

hedit is a Linux-first native desktop application. Cargo owns the complete build.
eframe/egui draws the UI through OpenGL; no webview or browser code is shipped.

| Module | Responsibility |
| --- | --- |
| `app.rs` | Native UI, shortcuts, background I/O dispatch, checkpoint status, exit gating |
| `workspace.rs` | Open/closed buffer ownership, UTF-8-safe edit splices, session view |
| `document.rs` | Disk-backed piece table, pages, source-change guards, atomic saves, bounded undo |
| `session.rs` | Process lock, recovery snapshots, manifests, fallback and preservation |
| `debugger.rs` | CUDA-GDB MI transport, native NVIDIA tools, bounded output readers |
| `syntax.rs` | Local C/CUDA lexical highlighting with active theme colors |
| `theme.rs` | Validated RGB palettes, native styling, JSON import/export |
| `brand.rs` | Native vector mark and antialiased launcher icon |
| `cli.rs` | Native argument parsing and file locations |
| `folder.rs` | Retained upstream file utilities; not yet connected to the native UI |

Documents live behind shared mutexes. The UI sends minimal splices to the file
engine. Opening, saving, searching, and page navigation run on worker threads;
the UI disables conflicting edits until the operation completes. Checkpoint
workers clone immutable piece references and stream recovery data outside the
editing lock. A generation counter ensures a checkpoint cannot claim to include
edits made after its view was captured.

Checkpoint work is independent of foreground file work. Only one checkpoint and
one foreground I/O job are started at a time. Search cancellation is checked
between bounded scan batches. On a normal exit, the UI cancels the close request
until current work and a final checkpoint complete. An error leaves the window
open. A final exit hook also attempts recovery for framework-driven teardown.

Large files keep unchanged content on disk. Dirty paged snapshots copy their
source pieces to private recovery files. After a completed checkpoint, live
pieces and history can use these independent sources. Before that first snapshot,
an external writer can invalidate unseen source bytes; hedit cannot reconstruct
bytes it never retained. If a visible edit then fails, the edited page is rescued
to a new independent scratch buffer and an error explains what happened.

The initial renderer is a standard text widget with a bounded page. It does not
provide Sublime's custom virtualized document layout, multiple selections, or
folding. Those require editor work beyond retaining the Rust file engine.

NVIDIA tools are subprocesses, not linked dependencies. CUDA-GDB uses a
bidirectional MI pipe; stdout and stderr are drained independently with bounded
records and a bounded queue. Console output is capped. Arguments are passed
through `Command::arg`, and console text is encoded as a single MI C string.
Device inspection remains textual until a structured MI model is added.
