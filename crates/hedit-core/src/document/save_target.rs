//! Publish an existing-file save while keeping the exact entry it replaced.
use super::{err, Stamp};
use std::{
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};
use tempfile::{NamedTempFile, TempDir, TempPath};

const CONFLICT: &str = "The destination changed while saving. Your edits are still available.";

pub(super) fn replace(
    staged: NamedTempFile,
    directory: TempDir,
    target: &Path,
    expected: &Stamp,
) -> Result<File, String> {
    let (file, mut staged_path) = staged.into_parts();
    // This name may hold another writer's file after a successful exchange.
    // Only the staging directory may clean it up after we check its identity.
    staged_path.disable_cleanup(true);
    let backup = directory.path().join("previous");
    let written = Stamp::from_metadata(&file.metadata().map_err(err)?)?;
    let displaced = match atomic_replace(&staged_path, target, &backup) {
        Ok(Some(displaced)) => displaced,
        Ok(None) => {
            replace_with_backup(&staged_path, target, &backup, expected, directory)?;
            return Ok(file);
        }
        Err(error)
            if !fs::symlink_metadata(&backup)
                .is_err_and(|error| error.kind() == io::ErrorKind::NotFound) =>
        {
            return Err(preserve(directory, &err(error)));
        }
        Err(error) => return Err(err(error)),
    };

    if !matches_entry(&displaced, expected) {
        // Exchange back so the competing write stays at the public path.
        // If a third writer changed that path, this exchange captures its
        // entry as well. Keep it rather than deleting it with our staging.
        match atomic_replace(&displaced, target, &directory.path().join("intervening")) {
            Ok(Some(captured)) if matches_entry(&captured, &written) => {
                sync_rename(target, directory.path())
                    .map_err(|error| preserve(directory, &err(error)))?;
                return Err(CONFLICT.into());
            }
            _ => return Err(preserve(directory, CONFLICT)),
        }
    }
    sync_rename(target, directory.path()).map_err(|error| preserve(directory, &err(error)))?;
    Ok(file)
}

fn matches_entry(path: &Path, expected: &Stamp) -> bool {
    fs::symlink_metadata(path)
        .ok()
        .filter(|metadata| metadata.is_file())
        .and_then(|metadata| Stamp::from_metadata(&metadata).ok())
        .is_some_and(|actual| expected.matches(&actual))
}

fn preserve(directory: TempDir, message: &str) -> String {
    let path = directory.keep();
    format!("{message} Save copies are available in {}.", path.display())
}

fn sync_rename(target: &Path, staging: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(staging)?.sync_all()?;
        File::open(target.parent().expect("save target has a parent"))?.sync_all()?;
    }
    #[cfg(not(unix))]
    let _ = (target, staging);
    Ok(())
}

fn move_without_replacing(source: &Path, target: &Path) -> io::Result<()> {
    let mut source = TempPath::try_from_path(source)?;
    source.disable_cleanup(true);
    source
        .persist_noclobber(target)
        .map_err(|failure| failure.error)
}

// Filesystems that cannot exchange entries still get a protected prior copy
// and no-clobber publication. The staging directory is on the same volume.
fn replace_with_backup(
    staged: &Path,
    target: &Path,
    backup: &Path,
    expected: &Stamp,
    directory: TempDir,
) -> Result<(), String> {
    fs::rename(target, backup).map_err(err)?;
    if !matches_entry(backup, expected) {
        return match move_without_replacing(backup, target) {
            Ok(()) => Err(CONFLICT.into()),
            Err(_) => Err(preserve(directory, CONFLICT)),
        };
    }
    if let Err(error) = sync_rename(target, directory.path()) {
        return Err(preserve(directory, &err(error)));
    }
    if let Err(error) = move_without_replacing(staged, target) {
        return match move_without_replacing(backup, target) {
            Ok(()) => Err(err(error)),
            Err(_) => Err(preserve(directory, CONFLICT)),
        };
    }
    sync_rename(target, directory.path()).map_err(|error| preserve(directory, &err(error)))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn atomic_replace(staged: &Path, target: &Path, _: &Path) -> io::Result<Option<PathBuf>> {
    use rustix::fs::{renameat_with, RenameFlags, CWD};
    match renameat_with(CWD, staged, CWD, target, RenameFlags::EXCHANGE) {
        Ok(()) => Ok(Some(staged.to_owned())),
        Err(rustix::io::Errno::INVAL | rustix::io::Errno::NOSYS | rustix::io::Errno::NOTSUP) => {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(windows)]
fn atomic_replace(staged: &Path, target: &Path, backup: &Path) -> io::Result<Option<PathBuf>> {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;
    let wide = |path: &Path| {
        path.as_os_str()
            .encode_wide()
            .chain([0])
            .collect::<Vec<_>>()
    };
    let (staged, target, backup_name) = (wide(staged), wide(target), wide(backup));
    // SAFETY: All path buffers are NUL-terminated and live until the call
    // returns. The two reserved arguments must be null.
    let ok = unsafe {
        ReplaceFileW(
            target.as_ptr(),
            staged.as_ptr(),
            backup_name.as_ptr(),
            0,
            ptr::null(),
            ptr::null(),
        )
    };
    if ok != 0 {
        return Ok(Some(backup.to_owned()));
    }
    let error = io::Error::last_os_error();
    let unchanged_names =
        fs::symlink_metadata(backup).is_err_and(|error| error.kind() == io::ErrorKind::NotFound);
    // ReplaceFileW requires generic read access, even for a write-only
    // destination whose metadata and ACL the editor already copied.
    if unchanged_names
        && matches!(
            error.kind(),
            io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
        )
    {
        Ok(None)
    } else {
        Err(error)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn atomic_replace(_: &Path, _: &Path, _: &Path) -> io::Result<Option<PathBuf>> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn publishing_preserves_a_write_after_the_final_stamp_check() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "original").unwrap();
        let expected = Stamp::read(&path).unwrap();
        let transaction = tempfile::tempdir_in(root.path()).unwrap();
        let mut file = NamedTempFile::new_in(transaction.path()).unwrap();
        file.write_all(b"editor copy").unwrap();
        fs::write(&path, "a competing revision").unwrap();
        let error = replace(file, transaction, &path, &expected).unwrap_err();
        assert!(error.contains(CONFLICT), "{error}");
        assert_eq!(fs::read_to_string(path).unwrap(), "a competing revision");
    }

    #[test]
    fn fallback_restores_a_conflict_without_replacing_it() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("file.txt");
        fs::write(&target, "original").unwrap();
        let expected = Stamp::read(&target).unwrap();
        let transaction = tempfile::tempdir_in(root.path()).unwrap();
        let staged = transaction.path().join("incoming");
        let backup = transaction.path().join("backup");
        fs::write(&staged, "editor copy").unwrap();
        fs::write(&target, "a competing revision").unwrap();
        assert!(replace_with_backup(&staged, &target, &backup, &expected, transaction).is_err());
        assert_eq!(fs::read_to_string(target).unwrap(), "a competing revision");
    }
}
