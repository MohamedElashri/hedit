//! Host-only external-process boundary. This is not a Lua subprocess API.
//! Broker grants and executable/snapshot ownership must be checked by the host.
use serde::{Deserialize, Serialize};
use std::{io, path::PathBuf, sync::atomic::AtomicBool, time::Duration};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod unix;

const HELPER: &str = "--hedit-tool-worker";
#[cfg(target_os = "macos")]
const HEADER_BYTES: usize = 16 * 1024;
#[cfg(target_os = "macos")]
const READY: &[u8; 4] = b"HT01";
pub const INPUT_BYTES: usize = 64 * 1024;
pub const OUTPUT_BYTES: usize = 64 * 1024;
pub const STDERR_BYTES: usize = 4096;

/// Trusted host configuration, never populated directly from extension messages.
/// Both paths must name private immutable copies owned by the host.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    pub executable: PathBuf,
    pub snapshot: PathBuf,
    pub arguments: Vec<String>,
}
impl Launch {
    fn validate(&self) -> io::Result<()> {
        if self.arguments.len() > 32
            || self.arguments.iter().any(|arg| arg.contains('\0'))
            || self
                .arguments
                .iter()
                .map(|arg| arg.len() + 1)
                .sum::<usize>()
                > 4096
        {
            return Err(invalid());
        }
        for path in [&self.executable, &self.snapshot] {
            if !path.is_absolute() || std::fs::canonicalize(path)? != *path {
                return Err(invalid());
            }
        }
        if !std::fs::symlink_metadata(&self.executable)?.is_file()
            || !std::fs::symlink_metadata(&self.snapshot)?.is_dir()
            || self.snapshot.parent().is_none()
        {
            return Err(invalid());
        }
        #[cfg(unix)]
        {
            use std::{io::Read, os::unix::fs::MetadataExt};
            let file = std::fs::File::open(&self.executable)?;
            let metadata = file.metadata()?;
            if metadata.uid() != unsafe { libc::geteuid() }
                || metadata.nlink() != 1
                || metadata.mode() & 0o6222 != 0
                || metadata.mode() & 0o100 == 0
                || metadata.len() > 64 * 1024 * 1024
            {
                return Err(invalid());
            }
            let mut magic = [0; 4];
            (&file).read_exact(&mut magic)?;
            #[cfg(target_os = "macos")]
            if !matches!(
                magic,
                [0xcf, 0xfa, 0xed, 0xfe]
                    | [0xfe, 0xed, 0xfa, 0xcf]
                    | [0xca, 0xfe, 0xba, 0xbe]
                    | [0xbe, 0xba, 0xfe, 0xca]
                    | [0xca, 0xfe, 0xba, 0xbf]
                    | [0xbf, 0xba, 0xfe, 0xca]
            ) {
                return Err(invalid());
            }
            #[cfg(not(target_os = "macos"))]
            let _ = magic;
        }
        Ok(())
    }
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "Invalid isolated tool launch.")
}

/// Captured output is bounded independently of native process memory limits.
#[derive(Debug)]
pub struct Output {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Run a finite filter. No shell, ambient environment, network or live project
/// directory is supplied. Other native adapters remain disabled until verified.
pub fn run(
    launch: &Launch,
    input: &[u8],
    cancel: &AtomicBool,
    timeout: Duration,
) -> io::Result<Output> {
    launch.validate()?;
    if input.len() > INPUT_BYTES || timeout.is_zero() || timeout > Duration::from_secs(10) {
        return Err(invalid());
    }
    #[cfg(target_os = "macos")]
    return unix::run(&std::env::current_exe()?, launch, input, cancel, timeout);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (input, cancel, timeout);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Tool confinement is unavailable.",
        ))
    }
}

/// Dispatch before editor initialization. Only policy metadata is read before
/// native confinement; document input is sent after the ready acknowledgement.
pub fn run_if_requested(args: &[std::ffi::OsString]) -> Option<bool> {
    if args != [HELPER] {
        return None;
    }
    #[cfg(target_os = "macos")]
    return Some(macos::bootstrap().is_ok());
    #[cfg(not(target_os = "macos"))]
    Some(false)
}
