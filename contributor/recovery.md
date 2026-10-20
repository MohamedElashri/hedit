# Recovery behavior

A session contains:

- `session.lock`: held by the running process; a stale file without a held lock is harmless.
- `session.json`: latest committed manifest.
- `session.previous.json`: previous valid manifest.
- `buffers-*/`: recovery text or piece-source files referenced by those manifests.
- `window.ron`: window geometry and UI preferences, separate from buffer recovery.

A checkpoint writes and syncs its buffer data, syncs its directory, writes and
syncs a new manifest, retains the previous manifest, and atomically replaces the
current manifest. The session directory is synced on Unix. Generations used by
either committed manifest survive cleanup. Unchanged documents reuse their
existing snapshots.

The old manifest remains authoritative until the replacement. Failed restores
retain their entries. A failed checkpoint leaves previous recovery data in
place and displays a persistent error banner. The status bar only says recovery
is current after a successful checkpoint for the current UI generation.

Unsaved and dirty buffers are snapshotted. Clean named files reopen from their
current disk contents. A clean deleted file may disappear from the restored tab
list; dirty buffers keep their private recovery data. Changed clean log files do
not prevent other buffers from checkpointing.

If the newest manifest is corrupt, the previous manifest is tried. A warning
explains the fallback. `preserved-manifests/` stores the corrupt manifest; while
that directory exists, automatic generation cleanup is disabled so potentially
newer recovery text is retained. If neither manifest works, the original data is
moved into a `preserved-*` archive before starting a fresh session. Interrupted
archiving is resumed on the next launch.

## Manual rescue

Close hedit before inspecting or copying session storage. Copy the entire session
directory to a backup first. Small buffers are ordinary UTF-8 files inside
`buffers-*`; a manifest associates them with their tab names and paths. A paged
buffer may instead reference several ranges in source files: use its manifest
rather than assuming a single source file is the complete document.

After a fallback, retain the whole directory until the needed text is rescued.
Only then remove `preserved-manifests/` if you want normal cleanup to resume.
`preserved-*` archives are not automatically deleted.

The closed shelf has 50 entries. Clean saved entries can be evicted to make room;
unsaved entries cannot. Reopen and save an unsaved entry to make room. There is
no expiration based on age.

Undo/redo currently spans the running process, with a maximum of 256 edit steps.
Text, tabs, and selections survive restarts; the undo stack itself does not.
Checkpointing is periodic, not a per-keystroke durable journal, and cannot cover
unwritten edits after SIGKILL, sudden power loss, or hardware failure.
