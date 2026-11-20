//! OS isolation for a fresh Lua helper process. Enter before reading its IPC.

#[cfg(not(any(target_os = "macos", windows)))]
use std::io;

pub(crate) const SUPPORTED: bool = cfg!(all(
    any(target_os = "linux", target_os = "macos", windows),
    any(target_arch = "x86_64", target_arch = "aarch64"),
    target_endian = "little",
    target_pointer_width = "64",
));

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) use macos::{enter, over_memory_limit, verify_child, worker_executable};

#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn worker_executable() -> io::Result<std::path::PathBuf> {
    #[cfg(target_os = "linux")]
    return Ok(std::path::PathBuf::from("/proc/self/exe"));
    #[cfg(not(target_os = "linux"))]
    std::env::current_exe()
}
#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) fn verify_child(_: &std::process::Child) -> io::Result<()> {
    Ok(())
}
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn over_memory_limit(_: &std::process::Child) -> io::Result<bool> {
    Ok(false)
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64"),
    target_endian = "little",
    target_pointer_width = "64",
))]
pub(crate) use linux::enter;

#[cfg(not(all(
    any(target_os = "linux", target_os = "macos", windows),
    any(target_arch = "x86_64", target_arch = "aarch64"),
    target_endian = "little",
    target_pointer_width = "64",
)))]
pub(crate) fn enter() -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Lua plugin process isolation is unavailable on this platform",
    ))
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64"),
    target_endian = "little",
    target_pointer_width = "64",
))]
mod linux;

#[cfg(windows)]
pub(crate) mod windows;
#[cfg(windows)]
pub(crate) use windows::{enter, worker_executable};
