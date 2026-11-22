//! Owner-only privacy checks shared by extension storage and recovery journals.
use std::{fs::File, io};
#[cfg(windows)]
mod windows;
pub fn verify(file: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata()?;
        if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Private data requires owner-only access.",
            ));
        }
        crate::file_io::reject_extended_acl_grants(file)?;
    }
    #[cfg(windows)]
    windows::verify_file(file)?;
    Ok(())
}
/// Restrict only a newly created empty, host-owned directory before writing data.
/// Existing sensitive directories must be verified, never silently repaired.
pub fn protect_directory(file: &File) -> io::Result<()> {
    if !file.metadata()?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Private storage requires a directory.",
        ));
    }
    #[cfg(windows)]
    windows::protect_directory(file)?;
    verify(file)
}
