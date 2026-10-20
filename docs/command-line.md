# Command line

```text
hedit [--session-dir DIR] [--new] [FILE[:LINE[:COLUMN]] ...] [-]
```

| Argument | Effect |
| --- | --- |
| `FILE` | Open an existing file or start a named file buffer |
| `FILE:LINE` | Open at a one-based line |
| `FILE:LINE:COLUMN` | Open at a line and column |
| `-` | Read UTF-8 stdin until EOF into an unsaved buffer |
| `--new` | Add an empty scratch buffer to the restored session |
| `--session-dir DIR` | Use a separate session directory |
| `--` | Treat all remaining arguments as filenames |
| `-h`, `--help` | Show help and exit |
| `-V`, `--version` | Show the version and exit |

Examples:

```sh
hedit application.log
hedit kernel.cu:42:8
hedit "notes for today.txt"
printf 'keep this snippet\n' | hedit -
hedit --new
hedit --session-dir /tmp/hedit-investigation large.log
hedit -- -notes.txt
```

An existing literal filename takes precedence over location syntax. CLI columns
use UTF-16 units; out-of-range locations clamp to the end of the document or line.
A missing file opens an empty buffer if its parent directory exists. Opening it
does not create a file; **Save** does.

Only one process may use a session directory at a time. A second launch with the
same session reports that it is locked; file forwarding to the existing window
is not implemented yet. Open more files in the current window or use another
`--session-dir`.

The default session is `$XDG_DATA_HOME/hedit/session`, normally
`~/.local/share/hedit/session`. Set `HEDIT_SESSION_DIR` for a different default;
`--session-dir` takes precedence over that environment variable.

Stdin is read into a temporary disk buffer and checkpointed before window
creation. If the graphical window cannot start, the consumed text remains in
that session for the next successful launch. Input must be UTF-8 text.
