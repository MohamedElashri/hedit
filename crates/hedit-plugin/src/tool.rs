//! Host-only external-process boundary. This is not a Lua subprocess API.
//! Broker grants and executable/snapshot ownership must be checked by the host.
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::Duration,
};

#[cfg(target_os = "macos")]
mod guardian;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(
    target_os = "macos",
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
mod unix;
#[cfg(target_os = "macos")]
use macos as native;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod linux;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
use linux as native;

#[cfg(windows)]
mod windows;

static ACTIVE: AtomicUsize = AtomicUsize::new(0);
mod stream;
pub use stream::{Progress, Stream};
struct Admission;
impl Admission {
    fn acquire() -> io::Result<Self> {
        let mut active = ACTIVE.load(Ordering::Acquire);
        loop {
            if active >= 4 {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "Tool capacity is exhausted.",
                ));
            }
            match ACTIVE.compare_exchange_weak(
                active,
                active + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(Self),
                Err(current) => active = current,
            }
        }
    }
}
impl Drop for Admission {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Ordering::AcqRel);
    }
}

const HELPER: &str = "--hedit-tool-worker";
const SERVICE_HELPER: &str = "--hedit-tool-service";
/// Chosen only by trusted host launch paths, never by a Lua message/header.
#[derive(Clone, Copy)]
pub(crate) enum Role {
    Filter,
    Service,
}
impl Role {
    pub(crate) fn cpu_seconds(self) -> u64 {
        match self {
            Self::Filter => 2,
            Self::Service => 60,
        }
    }
    #[cfg(unix)]
    fn argument(self) -> &'static str {
        match self {
            Self::Filter => HELPER,
            Self::Service => SERVICE_HELPER,
        }
    }
}
#[cfg(any(
    target_os = "macos",
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
const HEADER_BYTES: usize = 16 * 1024;
#[cfg(any(
    target_os = "macos",
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
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
    pub fn validate(&self) -> io::Result<()> {
        Self::validate_arguments(&self.arguments)?;
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
            #[cfg(not(any(
                target_os = "macos",
                all(
                    target_os = "linux",
                    any(target_arch = "x86_64", target_arch = "aarch64")
                )
            )))]
            let _ = magic;
        }
        Ok(())
    }
    pub fn validate_arguments(arguments: &[String]) -> io::Result<()> {
        if arguments.len() > 32
            || arguments.iter().any(|arg| arg.contains('\0'))
            || arguments
                .iter()
                .try_fold(0usize, |bytes, arg| {
                    bytes.checked_add(arg.len())?.checked_add(1)
                })
                .is_none_or(|bytes| bytes > 4096)
        {
            return Err(invalid());
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
    let _admission = Admission::acquire()?;
    launch.validate()?;
    if input.len() > INPUT_BYTES || timeout.is_zero() || timeout > Duration::from_secs(10) {
        return Err(invalid());
    }
    #[cfg(any(
        target_os = "macos",
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    return unix::run(&std::env::current_exe()?, launch, input, cancel, timeout);
    #[cfg(windows)]
    return windows::run(launch, input, cancel, timeout);
    #[cfg(not(any(
        windows,
        target_os = "macos",
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    )))]
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
    #[cfg(target_os = "macos")]
    if args == [guardian::ARG] {
        return Some(guardian::run().unwrap_or(false));
    }
    let role = if args == [HELPER] {
        Role::Filter
    } else if args == [SERVICE_HELPER] {
        Role::Service
    } else {
        return None;
    };
    #[cfg(any(
        target_os = "macos",
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    return Some(unix::bootstrap(role).is_ok());
    #[cfg(not(any(
        target_os = "macos",
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    )))]
    {
        let _ = role;
        Some(false)
    }
}
