//! Re-exec before creating GUI threads; only the child owns the session lock.
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

const CHILD_ARGUMENT: &str = "--__hedit-detached";
const LOG_NAME: &str = "hedit.log";

#[derive(serde::Serialize, serde::Deserialize)]
enum Report {
    Ready,
    Failed(String),
}

#[derive(Default)]
pub struct Startup {
    status: Option<PathBuf>,
    stdin: Option<PathBuf>,
    ready: bool,
}

impl Startup {
    // Private re-exec arguments precede the original arguments, including `--`.
    // Paths stay as OsStrings throughout; no shell or UTF-8 conversion is used.
    pub fn take_from_args(args: &mut Vec<OsString>) -> io::Result<Self> {
        if args.first().is_none_or(|arg| arg != CHILD_ARGUMENT) {
            return Ok(Self::default());
        }
        if args.len() < 3 {
            return Err(io::Error::other("Incomplete internal launch arguments."));
        }
        let internal: Vec<_> = args.drain(..3).collect();
        Ok(Self {
            status: Some(internal[1].clone().into()),
            stdin: (!internal[2].is_empty()).then(|| internal[2].clone().into()),
            ready: false,
        })
    }

    pub fn is_child(&self) -> bool {
        self.status.is_some()
    }

    pub fn stdin_path(&self) -> Option<&Path> {
        self.stdin.as_deref()
    }

    pub fn ready(&mut self) -> io::Result<()> {
        self.report(&Report::Ready)?;
        self.ready = true;
        Ok(())
    }

    pub fn failed(&self, error: String) {
        if !self.ready {
            let _ = self.report(&Report::Failed(error));
        }
    }

    fn report(&self, report: &Report) -> io::Result<()> {
        if let Some(path) = &self.status {
            // Publish atomically so the launcher never reads a partial report.
            let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
            serde_json::to_writer(&mut file, report)?;
            file.persist(path).map_err(|error| error.error)?;
        }
        Ok(())
    }
}

pub fn launch(
    args: &[OsString],
    directory: &Path,
    stdin: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let status_directory = tempfile::Builder::new().prefix("hedit-launch-").tempdir()?;
    let status = status_directory.path().join("status.json");
    // Capture a pipe or interactive input before releasing the terminal. The
    // child consumes this private spool using the normal stdin recovery path.
    let spool = if stdin {
        let mut file = tempfile::Builder::new().prefix("hedit-stdin-").tempfile()?;
        io::copy(&mut io::stdin().lock(), &mut file)?;
        file.flush()?;
        Some(file.into_temp_path())
    } else {
        None
    };
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(directory)?;
    let log = directory.join(LOG_NAME);
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg(CHILD_ARGUMENT)
        .arg(&status)
        .arg(spool.as_deref().unwrap_or_else(|| Path::new("")))
        .args(args);
    let mut child = spawn_detached(&mut command, &log)?;
    wait_for_startup(&mut child, &status, &log)?;
    Ok(())
}

fn spawn_detached(command: &mut Command, log: &Path) -> io::Result<Child> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(OpenOptions::new().create(true).append(true).open(log)?);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe. No allocation, locks or other
        // non-signal-safe work runs between fork and exec. A separate session
        // removes the controlling terminal as well as its foreground group.
        unsafe {
            command.pre_exec(|| rustix::process::setsid().map(|_| ()).map_err(Into::into));
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS};
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    command.spawn()
}

fn wait_for_startup(child: &mut Child, status: &Path, log: &Path) -> io::Result<()> {
    loop {
        if let Some(report) = read_report(status) {
            return match report {
                Report::Ready => Ok(()),
                Report::Failed(error) => {
                    // Wait for the failed child to release its recovery lock.
                    child.wait()?;
                    Err(io::Error::other(error))
                }
            };
        }
        if let Some(exit) = child.try_wait()? {
            // The child may have published an error between the read and exit.
            let error = match read_report(status) {
                Some(Report::Failed(error)) => error,
                Some(Report::Ready) if exit.success() => return Ok(()),
                _ => format!(
                    "hedit exited during startup ({exit}). See {} for details.",
                    log.display()
                ),
            };
            return Err(io::Error::other(error));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn read_report(path: &Path) -> Option<Report> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn internal_arguments_preserve_literal_and_non_utf8_paths() {
        let mut args = vec![
            CHILD_ARGUMENT.into(),
            "/tmp/status".into(),
            "/tmp/hedit-stdin-fixture".into(),
            "--".into(),
            "--new".into(),
            "a file:42:3".into(),
        ];
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            args.push(OsString::from_vec(b"non-utf8-\xff".to_vec()));
        }
        let original = args[3..].to_vec();
        let startup = Startup::take_from_args(&mut args).unwrap();
        assert_eq!(args, original);
        assert_eq!(
            startup.stdin_path(),
            Some(Path::new("/tmp/hedit-stdin-fixture"))
        );
        let options = hedit::cli::Options::parse(args).unwrap();
        assert!(!options.new_buffer);
        assert_eq!(options.files[0], Path::new("--new"));
    }

    // Re-exec the test harness rather than requiring a graphical display.
    #[test]
    fn detached_fixture() {
        let Some(directory) = std::env::var_os("HEDIT_TEST_LAUNCH_DIRECTORY") else {
            return;
        };
        let directory = PathBuf::from(directory);
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            assert_eq!(
                rustix::process::getsid(None).unwrap(),
                rustix::process::getpid()
            );
            assert_eq!(rustix::process::getpgrp(), rustix::process::getpid());
        }
        use std::io::IsTerminal;
        assert!(!io::stdin().is_terminal());
        assert!(!io::stdout().is_terminal());
        assert!(!io::stderr().is_terminal());
        let mut startup = Startup {
            status: Some(directory.join("status.json")),
            ..Default::default()
        };
        if std::env::var_os("HEDIT_TEST_LAUNCH_FAIL").is_some() {
            startup.failed("fixture startup failed".into());
            return;
        }
        eprintln!("detached fixture diagnostic");
        startup.ready().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !directory.join("stop").exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn fixture(directory: &Path) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "launcher::tests::detached_fixture",
                "--nocapture",
            ])
            .env("HEDIT_TEST_LAUNCH_DIRECTORY", directory);
        command
    }

    #[test]
    fn returns_after_startup_while_child_runs_in_its_own_session() {
        let directory = tempfile::tempdir().unwrap();
        let log = directory.path().join(LOG_NAME);
        let mut child = spawn_detached(&mut fixture(directory.path()), &log).unwrap();
        let start = Instant::now();
        let result = wait_for_startup(&mut child, &directory.path().join("status.json"), &log);
        let running = child.try_wait().unwrap().is_none();
        fs::write(directory.path().join("stop"), "").unwrap();
        let exit = child.wait().unwrap();
        result.unwrap();
        assert!(running, "the launcher must return before the GUI exits");
        assert!(start.elapsed() < Duration::from_secs(5));
        assert!(exit.success());
        assert!(fs::read_to_string(log)
            .unwrap()
            .contains("detached fixture diagnostic"));
    }

    #[test]
    fn reports_startup_errors_and_waits_for_child_exit() {
        let directory = tempfile::tempdir().unwrap();
        let log = directory.path().join(LOG_NAME);
        let mut command = fixture(directory.path());
        command.env("HEDIT_TEST_LAUNCH_FAIL", "1");
        let mut child = spawn_detached(&mut command, &log).unwrap();
        let error =
            wait_for_startup(&mut child, &directory.path().join("status.json"), &log).unwrap_err();
        assert!(error.to_string().contains("fixture startup failed"));
        assert!(child.try_wait().unwrap().is_some());
    }
}
