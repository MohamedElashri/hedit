//! Windows storage privacy uses the shared handle-based policy.
use std::{fs::File, io, path::Path};
pub(super) fn verify_file(file: &File) -> io::Result<()> {
    hedit_core::private_io::verify(file)
}
pub(super) fn protect_directory(path: &Path) -> io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::*;
    let directory = std::fs::OpenOptions::new()
        .access_mode(FILE_GENERIC_READ | WRITE_DAC)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    hedit_core::private_io::protect_directory(&directory)
}
