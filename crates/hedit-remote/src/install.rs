//! User-owned, versioned helper installation. Never elevates privileges.
use crate::host;
use rustix::fs::{self as sys, AtFlags, Mode, OFlags};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

pub use crate::client::SetupInfo as Installation;

pub fn setup(workspace: &str) -> Result<Installation, String> {
    let uid = rustix::process::geteuid().as_raw();
    if uid == 0 {
        return Err("Automatic SSH setup requires a non-root SSH account.".into());
    }
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("Remote HOME is unavailable.")?);
    setup_in(
        &home,
        workspace,
        uid,
        &std::env::current_exe().map_err(|e| e.to_string())?,
    )
}

fn setup_in(home: &Path, workspace: &str, uid: u32, source: &Path) -> Result<Installation, String> {
    if uid == 0 {
        return Err("Automatic SSH setup requires a non-root SSH account.".into());
    }
    let home_dir = host::absolute_directory(home)
        .map_err(|_| "Remote home must be an absolute directory without symlinks.")?;
    let metadata = home_dir.metadata().map_err(|e| e.to_string())?;
    if uid == 0 || metadata.uid() != uid || metadata.mode() & 0o022 != 0 {
        return Err(
            "Remote home must belong to the non-root SSH user and not be writable by other users."
                .into(),
        );
    }
    let root = if workspace.is_empty() {
        home
    } else {
        Path::new(workspace)
    };
    host::absolute_directory(root)
        .map_err(|_| "Remote workspace must be an existing absolute directory without symlinks.")?;
    let version = env!("CARGO_PKG_VERSION");
    let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let cache = home
        .join(".local/share/hedit/remote")
        .join(version)
        .join(platform);
    let directory = host::private_directory_mode(&cache, false)
        .map_err(|e| format!("Cannot create a private helper directory in this account: {e}"))?;
    let lock = host::private_file(&directory, ".install.lock").map_err(|e| e.to_string())?;
    let started = std::time::Instant::now();
    loop {
        match lock.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock)
                if started.elapsed() < std::time::Duration::from_secs(20) =>
            {
                std::thread::sleep(std::time::Duration::from_millis(50))
            }
            Err(e) => {
                return Err(format!(
                    "Another helper installation is busy or inaccessible: {e}"
                ))
            }
        }
    }
    let helper = cache.join("hedit-remote");
    let source_meta = std::fs::metadata(source).map_err(|e| e.to_string())?;
    match sys::statat(&directory, "hedit-remote", AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => {
            if stat.st_uid != uid
                || stat.st_mode & libc::S_IFMT != libc::S_IFREG
                || stat.st_mode & 0o077 != 0
                || stat.st_nlink != 1
            {
                return Err(
                    "The managed helper path is not a private user-owned executable.".into(),
                );
            }
            #[allow(clippy::unnecessary_cast)] // Darwin's dev_t is narrower than Linux's.
            let same_file =
                stat.st_ino as u64 == source_meta.ino() && stat.st_dev as u64 == source_meta.dev();
            if !same_file {
                replace(&directory, source)?;
            }
        }
        Err(rustix::io::Errno::NOENT) => replace(&directory, source)?,
        Err(e) => return Err(e.to_string()),
    }
    let root = root
        .to_str()
        .ok_or("Remote paths must be UTF-8.")?
        .to_owned();
    if root.contains(['\0', '\r', '\n']) {
        return Err("Unsupported remote workspace path.".into());
    }
    let key = Sha256::digest(root.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let state = home.join(".local/state/hedit/remote").join(key);
    host::private_directory(&state).map_err(|e| {
        format!("Cannot create private durable recovery storage in this account: {e}")
    })?;
    Ok(Installation {
        version: version.into(),
        uid,
        home: home.to_str().ok_or("Remote home must be UTF-8.")?.into(),
        root,
        helper: helper
            .to_str()
            .ok_or("Remote helper path must be UTF-8.")?
            .into(),
        state: state
            .to_str()
            .ok_or("Remote recovery path must be UTF-8.")?
            .into(),
    })
}

fn replace(directory: &File, source: &Path) -> Result<(), String> {
    let name = format!(
        ".incoming-{}",
        host::random_id().map_err(|e| e.to_string())?
    );
    let result = (|| {
        let mut output = File::from(
            sys::openat(
                directory,
                name.as_str(),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_bits_truncate(0o700),
            )
            .map_err(|e| e.to_string())?,
        );
        let source = File::open(source).map_err(|e| e.to_string())?;
        let size = source.metadata().map_err(|e| e.to_string())?.len();
        if size == 0 || size > 64 * 1024 * 1024 {
            return Err("Invalid SSH helper size.".into());
        }
        let copied =
            std::io::copy(&mut source.take(size + 1), &mut output).map_err(|e| e.to_string())?;
        if copied != size {
            return Err("SSH helper changed during installation.".into());
        }
        output.flush().map_err(|e| e.to_string())?;
        output.sync_all().map_err(|e| e.to_string())?;
        sys::renameat(directory, name.as_str(), directory, "hedit-remote")
            .map_err(|e| e.to_string())?;
        directory.sync_all().map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = sys::unlinkat(directory, name.as_str(), AtFlags::empty());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn root_is_rejected_before_creating_any_directories() {
        let dir = tempfile::tempdir().unwrap();
        assert!(setup_in(dir.path(), "", 0, Path::new("/missing"))
            .unwrap_err()
            .contains("non-root"));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    #[test]
    fn helper_replacement_is_atomic_and_failed_staging_keeps_the_existing_executable() {
        let dir = tempfile::Builder::new()
            .permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .tempdir()
            .unwrap();
        let path = dir.path().canonicalize().unwrap();
        let directory = host::private_directory_mode(&path, false).unwrap();
        let source = path.join("source");
        std::fs::write(&source, b"old helper").unwrap();
        replace(&directory, &source).unwrap();
        let mut old = File::open(path.join("hedit-remote")).unwrap();
        std::fs::write(&source, b"new helper").unwrap();
        replace(&directory, &source).unwrap();
        let mut original = String::new();
        old.read_to_string(&mut original).unwrap();
        assert_eq!(original, "old helper");
        assert_eq!(
            std::fs::read(path.join("hedit-remote")).unwrap(),
            b"new helper"
        );
        std::fs::write(&source, b"").unwrap();
        assert!(replace(&directory, &source).is_err());
        assert_eq!(
            std::fs::read(path.join("hedit-remote")).unwrap(),
            b"new helper"
        );
        assert!(!std::fs::read_dir(&path).unwrap().any(|p| p
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".incoming-")));
    }
    #[test]
    fn symlinks_and_shared_homes_cannot_redirect_installation() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let uid = rustix::process::geteuid().as_raw();
        if uid == 0 {
            return;
        }
        let home = tempfile::Builder::new()
            .permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .tempdir()
            .unwrap();
        let outside = tempfile::Builder::new()
            .permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .tempdir()
            .unwrap();
        let home_path = home.path().canonicalize().unwrap();
        let outside_path = outside.path().canonicalize().unwrap();
        symlink(&outside_path, home_path.join(".local")).unwrap();
        assert!(setup_in(&home_path, "", uid, Path::new("/missing")).is_err());
        assert_eq!(std::fs::read_dir(&outside_path).unwrap().count(), 0);
        std::fs::remove_file(home_path.join(".local")).unwrap();
        std::fs::set_permissions(&home_path, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(setup_in(&home_path, "", uid, Path::new("/missing")).is_err());
        assert_eq!(std::fs::read_dir(&home_path).unwrap().count(), 0);
    }
}
