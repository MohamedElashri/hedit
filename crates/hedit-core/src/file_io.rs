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
    let started = std::time::Instant::now();
    loop {
        match file.try_lock() {
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
}
