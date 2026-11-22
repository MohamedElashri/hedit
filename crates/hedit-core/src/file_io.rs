//! Regular-file reads that cannot wait for pipe writers or exceed a byte limit.
use std::{
    fs::{File, OpenOptions},
    io::{self, Read},
    path::Path,
};

pub fn open_regular(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Choose a regular file.",
        ));
    }
    Ok(file)
}

pub fn read_bounded(file: File, limit: u64) -> io::Result<Vec<u8>> {
    let too_large = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("File exceeds the {limit}-byte limit."),
        )
    };
    if file.metadata()?.len() > limit {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(too_large());
    }
    Ok(bytes)
}

/// Acquire an exclusive lock with a bounded wait for transient owners.
pub fn lock_with_timeout(
    file: &File,
    timeout: std::time::Duration,
) -> Result<(), std::fs::TryLockError> {
    lock_retry(file, timeout, File::try_lock)
}

/// Shared disclosure guards permit concurrent readers while policy changes stay exclusive.
pub fn lock_shared_with_timeout(
    file: &File,
    timeout: std::time::Duration,
) -> Result<(), std::fs::TryLockError> {
    lock_retry(file, timeout, File::try_lock_shared)
}
fn lock_retry(
    file: &File,
    timeout: std::time::Duration,
    attempt: impl Fn(&File) -> Result<(), std::fs::TryLockError>,
) -> Result<(), std::fs::TryLockError> {
    let started = std::time::Instant::now();
    loop {
        match attempt(file) {
            Ok(()) => return Ok(()),
            Err(std::fs::TryLockError::WouldBlock) => {
                let remaining = timeout.saturating_sub(started.elapsed());
                if remaining.is_zero() {
                    return Err(std::fs::TryLockError::WouldBlock);
                }
                std::thread::sleep(remaining.min(std::time::Duration::from_millis(5)));
            }
            Err(error) => return Err(error),
        }
    }
}

/// Reject macOS extended ACL grants that can bypass private Unix mode bits.
/// Deny entries are harmless. Other platforms must separately enforce their
/// mode/ACL policy; this check alone never establishes ownership or privacy.
pub fn reject_extended_acl_grants(file: &File) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::{ffi::c_void, os::fd::AsRawFd};
        unsafe extern "C" {
            fn acl_get_fd(fd: std::ffi::c_int) -> *mut c_void;
            fn acl_get_entry(
                acl: *mut c_void,
                index: std::ffi::c_int,
                entry: *mut *mut c_void,
            ) -> std::ffi::c_int;
            fn acl_get_tag_type(entry: *mut c_void, tag: *mut std::ffi::c_int) -> std::ffi::c_int;
            fn acl_free(acl: *mut c_void) -> std::ffi::c_int;
        }
        struct Acl(*mut c_void);
        impl Drop for Acl {
            fn drop(&mut self) {
                /* SAFETY: allocated by acl_get_fd, freed exactly once. */
                unsafe {
                    acl_free(self.0);
                }
            }
        }
        // SAFETY: the file descriptor is live; the returned ACL is independently owned.
        let raw = unsafe { acl_get_fd(file.as_raw_fd()) };
        if raw.is_null() {
            return if std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "Private files cannot have extended ACL grants.",
                ))
            };
        }
        let acl = Acl(raw);
        let mut index = 0; // ACL_FIRST_ENTRY, then ACL_NEXT_ENTRY (-1).
        loop {
            let mut entry = std::ptr::null_mut();
            // SAFETY: the ACL and output pointer remain valid during this call.
            let found = unsafe { acl_get_entry(acl.0, index, &mut entry) };
            if found == -1 {
                break;
            }
            if found != 0 || entry.is_null() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "Private files cannot have extended ACL grants.",
                ));
            }
            let mut tag = 0;
            // SAFETY: entry belongs to the live ACL; tag points to writable storage.
            if unsafe { acl_get_tag_type(entry, &mut tag) } != 0 || tag != 2 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "Private files cannot have extended ACL grants.",
                ));
            } // ACL_EXTENDED_DENY.
            index = -1;
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = file;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn lock_waits_for_a_transient_owner() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.lock");
        let held = File::create(&path).unwrap();
        held.lock().unwrap();
        let next = File::options().read(true).write(true).open(&path).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(25));
            drop(held);
        });
        lock_with_timeout(&next, Duration::from_secs(2)).unwrap();
        release.join().unwrap();
    }

    #[test]
    fn timeout_preserves_the_existing_owner_and_allows_a_later_retry() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.lock");
        let held = File::create(&path).unwrap();
        held.lock().unwrap();
        let next = File::options().read(true).write(true).open(&path).unwrap();
        assert!(matches!(
            lock_with_timeout(&next, Duration::from_millis(25)),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        assert!(matches!(
            next.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        drop(held);
        lock_with_timeout(&next, Duration::ZERO).unwrap();
    }
    #[test]
    #[cfg(target_os = "macos")]
    fn private_acl_rejects_grants_even_with_private_modes_and_allows_denies() {
        use std::os::unix::fs::PermissionsExt;
        let file = tempfile::NamedTempFile::new().unwrap();
        assert!(reject_extended_acl_grants(file.as_file()).is_ok());
        for (entry, permitted) in [
            ("everyone deny write", true),
            (
                "everyone allow read,readattr,readextattr,readsecurity",
                false,
            ),
        ] {
            assert!(std::process::Command::new("/bin/chmod")
                .args(["+a", entry])
                .arg(file.path())
                .status()
                .unwrap()
                .success());
            assert_eq!(
                file.as_file().metadata().unwrap().permissions().mode() & 0o077,
                0
            );
            assert_eq!(
                reject_extended_acl_grants(file.as_file()).is_ok(),
                permitted
            );
        }
    }
}
