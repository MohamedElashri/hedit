# Recover your work

hedit keeps recovery copies of scratchpads and modified files. These copies are
separate from the files you open and are used to bring your workspace back after
closing or restarting the editor.

## Read the recovery status

| Status | Meaning |
| --- | --- |
| Recovery pending | Recent changes are waiting for a checkpoint |
| Checkpointing… | Recovery data is being written |
| Recovery up to date | A checkpoint for the current workspace has completed |
| Recovery checkpoint failed | Keep the editor open and address the displayed error |

Checkpoints normally start every two seconds while work changes. Large copies
or a slow disk can take longer. A process kill or power loss can lose edits made
after the latest completed checkpoint.

On normal window close, hedit waits for a final checkpoint. If it fails, the
window stays open. **Save as…** can preserve important text in another location
while you resolve a storage problem.

## Find a missing buffer

1. Check the open tabs and **Ctrl+P** buffer switcher.
2. Open **Browse shelf…** and filter by the buffer's name.
3. If there was a recovery warning on startup, read it before dismissing it. It
   identifies preserved data or a fallback checkpoint.
4. If you used `--session-dir` or `HEDIT_SESSION_DIR`, open that same session.
   Each session has its own buffers and shelf.

Names, bookmarks, selections, and page positions are kept with buffers. Undo
history currently lasts for the running process only.

## When a checkpoint or restore fails

Check free disk space and whether the session directory is writable. Click
**Retry checkpoint** after fixing the issue. Do not delete recovery files while
hedit is running.

If the latest session manifest cannot be read, hedit tries the previous
checkpoint and shows a warning. The newest edits may be missing from that
fallback, so all recovery generations are retained for manual rescue. If neither
manifest is usable, the previous data is archived under a `preserved-*` directory
and a fresh session starts.

A failed buffer restore keeps its recovery reference for a later attempt. It is
not silently removed by subsequent checkpoints.

## Back up or rescue session data

The normal Linux session folder is `~/.local/share/hedit/session/`, or
`$XDG_DATA_HOME/hedit/session` if that environment variable is set. A custom
`--session-dir` takes precedence.

Close hedit, then copy the **whole session directory** before making changes.
Small scratch buffers are ordinary UTF-8 files in `buffers-*` directories. The
`session.json` or `session.previous.json` manifest identifies which entries belong
to each tab. Large edited buffers can reference several source ranges; a single
source file may not be the complete document.

Keep any `preserved-*` archives until the text you need has been recovered. After
a fallback, `preserved-manifests/` disables automatic cleanup of old buffer
generations. Remove that marker directory only after backing up and rescuing the
needed text. Preserved archives are not automatically deleted.

Recovery files contain plaintext. New session directories are private on Unix,
but their contents are not encrypted. Use a regular saved file and your usual
backup system for text that needs an independent long-term copy.
