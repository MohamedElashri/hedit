//! Extra windows, each a separate hedit process with its own recovery session
//! under `<data>/windows/`. The main window's session is `<data>/session`.
use std::path::{Path, PathBuf};

const WINDOWS: &str = "windows";

/// The folder holding extra windows' sessions, for any window's session.
pub fn windows_dir(session: &Path) -> PathBuf {
    match session.parent() {
        Some(parent) if parent.file_name().is_some_and(|n| n == WINDOWS) => parent.to_owned(),
        Some(parent) => parent.join(WINDOWS),
        None => PathBuf::from(WINDOWS),
    }
}

/// Whether a session belongs to an extra window rather than the main one.
pub fn is_extra(session: &Path) -> bool {
    session
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|n| n == WINDOWS)
}

/// A new, unused session folder for another window.
pub fn new_session(session: &Path) -> PathBuf {
    let dir = windows_dir(session);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    (0..)
        .map(|n| dir.join(format!("window-{stamp}-{n}")))
        .find(|p| !p.exists())
        .expect("a free folder name exists")
}

/// Extra windows' sessions to reopen with the main window: those left from
/// the last run whose window is not already open.
pub fn to_restore(session: &Path) -> Vec<PathBuf> {
    if is_extra(session) {
        return vec![];
    }
    let Ok(entries) = std::fs::read_dir(windows_dir(session)) else {
        return vec![];
    };
    let mut sessions: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && !crate::ipc::is_running(p))
        .collect();
    sessions.sort();
    sessions.truncate(16);
    sessions
}

/// Start another hedit window on `session`, returning without waiting for it.
pub fn spawn(executable: &Path, session: &Path, new_buffer: bool) -> Result<(), String> {
    let mut command = std::process::Command::new(executable);
    command.arg("--session-dir").arg(session);
    if new_buffer {
        command.arg("--new");
    }
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Could not open a new window: {e}"))?;
    // The launcher returns once the window is up; reap it without blocking.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
