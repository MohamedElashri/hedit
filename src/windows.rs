//! Extra windows, each a separate hedit process with its own recovery session
//! under `<data>/windows/`. The main window's session is `<data>/session`.
use std::path::{Path, PathBuf};

const WINDOWS: &str = "windows";

/// The package launcher gives each detached AppImage window its own runtime.
pub fn executable() -> std::io::Result<PathBuf> {
    let executable = std::env::current_exe()?;
    #[cfg(target_os = "linux")]
    if let (Some(directory), Some(image)) =
        (std::env::var_os("APPDIR"), std::env::var_os("APPIMAGE"))
    {
        return Ok(appimage_executable(
            &executable,
            Path::new(&directory),
            Path::new(&image),
        ));
    }
    Ok(executable)
}

#[cfg(target_os = "linux")]
fn appimage_executable(executable: &Path, directory: &Path, image: &Path) -> PathBuf {
    let launcher = directory.join("usr/bin/hedit-appimage-launch");
    if directory.is_absolute()
        && executable.starts_with(directory)
        && image.is_absolute()
        && image.is_file()
        && launcher.is_file()
    {
        launcher
    } else {
        executable.to_path_buf()
    }
}

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

#[cfg(all(test, target_os = "linux"))]
mod appimage_tests {
    use super::*;

    #[test]
    fn detached_windows_use_the_package_launcher_only_from_its_appdir() {
        let work = tempfile::tempdir().unwrap();
        let image = work.path().join("Hedit image.AppImage");
        std::fs::write(&image, "runtime").unwrap();
        let directory = work.path().join("mounted image");
        let payload = directory.join("usr/bin/hedit");
        std::fs::create_dir_all(payload.parent().unwrap()).unwrap();
        let launcher = directory.join("usr/bin/hedit-appimage-launch");
        std::fs::write(&launcher, "launcher").unwrap();
        assert_eq!(appimage_executable(&payload, &directory, &image), launcher);
        let system = Path::new("/usr/bin/hedit");
        assert_eq!(appimage_executable(system, &directory, &image), system);
        assert_eq!(
            appimage_executable(&payload, Path::new("."), &image),
            payload
        );
        assert_eq!(
            appimage_executable(&payload, &directory, &work.path().join("missing")),
            payload
        );
    }
}
