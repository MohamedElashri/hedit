//! Descriptor-relative filesystem access for the initial strict no-symlink mode.
use crate::protocol::{Error, Result, MAX_DOCUMENT};
use rustix::fs::{self as sys, Mode, OFlags};
use std::{
    ffi::{OsStr, OsString},
    fs::{File, Metadata},
    io::{Read, Seek},
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::MetadataExt,
        net::UnixStream,
    },
    path::{Component, Path, PathBuf},
};

fn components(path: &Path, absolute: bool) -> Result<Vec<OsString>> {
    if path.is_absolute() != absolute || path.as_os_str().as_bytes().len() > 4096 {
        return Err(Error::Permission);
    }
    let mut parts = Vec::new();
    for c in path.components() {
        match c {
            Component::RootDir if absolute => (),
            Component::Normal(x) if !x.as_bytes().contains(&0) => parts.push(x.to_owned()),
            _ => return Err(Error::Permission),
        }
    }
    if parts.is_empty() {
        return Err(Error::Permission);
    }
    Ok(parts)
}

fn open_dir(parent: &File, name: &OsStr) -> Result<File> {
    sys::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(|_| Error::Permission)
}

pub fn absolute_directory(path: &Path) -> Result<File> {
    let mut dir = File::open("/").map_err(|_| Error::Permission)?;
    for part in components(path, true)? {
        dir = open_dir(&dir, &part)?;
    }
    Ok(dir)
}

// macOS extended ACL grants can bypass Unix mode bits. Deny entries are
// harmless; reject grants on private storage and its mutable ancestors.
fn private_acl(file: &File) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::{ffi::c_void, os::fd::AsRawFd};
        unsafe extern "C" {
            fn acl_get_fd(fd: libc::c_int) -> *mut c_void;
            fn acl_get_entry(
                acl: *mut c_void,
                index: libc::c_int,
                entry: *mut *mut c_void,
            ) -> libc::c_int;
            fn acl_get_tag_type(entry: *mut c_void, tag: *mut libc::c_int) -> libc::c_int;
            fn acl_free(acl: *mut c_void) -> libc::c_int;
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
                Err(Error::Permission)
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
                return Err(Error::Permission);
            }
            let mut tag = 0;
            // SAFETY: entry belongs to the live ACL; tag points to writable storage.
            if unsafe { acl_get_tag_type(entry, &mut tag) } != 0 || tag != 2 {
                return Err(Error::Permission);
            } // ACL_EXTENDED_DENY.
            index = -1;
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = file;
    Ok(())
}
pub fn private_directory(path: &Path) -> Result<File> {
    private_directory_mode(path, true)
}
pub fn private_directory_mode(path: &Path, durable: bool) -> Result<File> {
    let mut dir = File::open("/").map_err(|_| Error::Permission)?;
    for part in components(path, true)? {
        let meta = dir.metadata().map_err(|_| Error::Permission)?;
        if meta.uid() != 0 && meta.uid() != rustix::process::geteuid().as_raw() {
            return Err(Error::Permission);
        }
        if meta.mode() & 0o022 != 0 && meta.mode() & 0o1000 == 0 {
            return Err(Error::Permission);
        }
        private_acl(&dir)?;
        match sys::mkdirat(&dir, &part, Mode::from_bits_truncate(0o700)) {
            Ok(()) => dir.sync_all().map_err(|_| Error::Storage)?,
            Err(rustix::io::Errno::EXIST) => (),
            Err(_) => return Err(Error::Permission),
        }
        dir = open_dir(&dir, &part)?;
    }
    let meta = dir.metadata().map_err(|_| Error::Permission)?;
    if meta.uid() != rustix::process::geteuid().as_raw() || meta.mode() & 0o077 != 0 {
        return Err(Error::Permission);
    }
    private_acl(&dir)?;
    // The journal cannot promise reboot durability on RAM or network storage.
    #[cfg(target_os = "linux")]
    {
        let stat = sys::fstatfs(&dir).map_err(|_| Error::Storage)?;
        // ext2/3/4, XFS, btrfs, ZFS. Other filesystems need explicit validation.
        if durable
            && ![0xef53_u64, 0x58465342, 0x9123683e, 0x2fc12fc1].contains(&(stat.f_type as u64))
        {
            return Err(Error::Unsupported);
        }
    }
    #[cfg(target_os = "macos")]
    {
        let stat = sys::fstatfs(&dir).map_err(|_| Error::Storage)?;
        let name: Vec<u8> = stat
            .f_fstypename
            .iter()
            .take_while(|x| **x != 0)
            .map(|x| *x as u8)
            .collect();
        if durable && name != b"apfs" {
            return Err(Error::Unsupported);
        }
    }
    Ok(dir)
}
pub fn random_id() -> Result<String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| Error::Unavailable)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
pub fn workspace_lock(root: &File) -> Result<File> {
    use sha2::{Digest, Sha256};
    let runtime = std::env::temp_dir()
        .canonicalize()
        .map_err(|_| Error::Storage)?
        .join(format!(
            "hedit-remote-{}",
            rustix::process::geteuid().as_raw()
        ));
    let directory = private_directory_mode(&runtime, false)?;
    let m = root.metadata().map_err(|_| Error::Storage)?;
    let key = Sha256::digest([m.dev().to_be_bytes(), m.ino().to_be_bytes()].concat());
    let name = format!(
        "{}.lock",
        key.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    let lock = private_file(&directory, &name)?;
    lock.try_lock().map_err(|_| Error::Busy)?;
    Ok(lock)
}
pub fn parent(root: &File, bytes: &[u8]) -> Result<(File, OsString)> {
    let parts = components(&path(bytes)?, false)?;
    let mut directory = root.try_clone().map_err(|_| Error::Storage)?;
    for part in &parts[..parts.len() - 1] {
        directory = open_dir(&directory, part)?;
    }
    Ok((directory, parts.last().ok_or(Error::Permission)?.clone()))
}
pub fn open_source(root: &File, bytes: &[u8]) -> Result<File> {
    let (parent, name) = parent(root, bytes)?;
    let file: File = sys::openat(
        &parent,
        &name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOCTTY,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(|_| Error::Permission)?;
    if !file.metadata().map_err(|_| Error::Storage)?.is_file() {
        return Err(Error::Permission);
    }
    Ok(file)
}
pub fn read_encoded(root: &File, bytes: &[u8]) -> Result<(String, String, bool, Vec<u8>)> {
    use sha2::{Digest, Sha256};
    let mut file = open_source(root, bytes)?;
    let before = file.metadata().map_err(|_| Error::Storage)?;
    let limit = crate::protocol::MAX_DOCUMENT as u64 * 4;
    if before.len() > limit {
        return Err(Error::Limit);
    }
    let raw =
        hedit_core::file_io::read_bounded(file.try_clone().map_err(|_| Error::Storage)?, limit)
            .map_err(|_| Error::Storage)?;
    file.rewind().map_err(|_| Error::Storage)?;
    let again =
        hedit_core::file_io::read_bounded(file.try_clone().map_err(|_| Error::Storage)?, limit)
            .map_err(|_| Error::Storage)?;
    if stamp(&before) != stamp(&file.metadata().map_err(|_| Error::Storage)?) || raw != again {
        return Err(Error::Conflict);
    }
    let encoding = hedit_core::text_encoding::detect(&raw).to_string();
    let (text, bom) =
        hedit_core::text_encoding::decode(&raw, &encoding).map_err(|_| Error::Unsupported)?;
    if text.len() > crate::protocol::MAX_DOCUMENT {
        return Err(Error::Limit);
    }
    Ok((text, encoding, bom, Sha256::digest(&raw).to_vec()))
}
pub fn atomic_save(root: &File, path: &[u8], bytes: &[u8], expected: Option<&[u8]>) -> Result<()> {
    use sha2::{Digest, Sha256};
    use std::io::Write;
    let (directory, name) = parent(root, path)?;
    let old = match sys::openat(
        &directory,
        &name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::NOCTTY | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => Some(File::from(fd)),
        Err(rustix::io::Errno::NOENT) => None,
        Err(_) => return Err(Error::Permission),
    };
    match (&old, expected) {
        (Some(file), Some(digest)) => {
            let metadata = file.metadata().map_err(|_| Error::Storage)?;
            if !metadata.is_file() || metadata.permissions().readonly() {
                return Err(Error::Permission);
            }
            let raw = hedit_core::file_io::read_bounded(
                file.try_clone().map_err(|_| Error::Storage)?,
                crate::protocol::MAX_DOCUMENT as u64 * 4,
            )
            .map_err(|_| Error::Storage)?;
            if Sha256::digest(&raw).as_slice() != digest {
                return Err(Error::Conflict);
            }
        }
        (None, None) => (),
        _ => return Err(Error::Conflict),
    }
    let temporary = OsString::from(format!(".hedit-{}", random_id()?));
    let mut staged: File = sys::openat(
        &directory,
        &temporary,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_bits_truncate(0o600),
    )
    .map(File::from)
    .map_err(|_| Error::Storage)?;
    let result = (|| {
        if let Some(source) = &old {
            hedit_core::document::copy_permissions(source, &staged)
                .map_err(|_| Error::Permission)?;
        }
        staged.write_all(bytes).map_err(|_| Error::Storage)?;
        staged.sync_all().map_err(|_| Error::Storage)?;
        if let Some(source) = &old {
            let mut fresh = open_source(root, path)?;
            if stamp(&source.metadata().map_err(|_| Error::Storage)?)
                != stamp(&fresh.metadata().map_err(|_| Error::Storage)?)
            {
                return Err(Error::Conflict);
            }
            fresh.rewind().map_err(|_| Error::Storage)?;
            let raw =
                hedit_core::file_io::read_bounded(fresh, crate::protocol::MAX_DOCUMENT as u64 * 4)
                    .map_err(|_| Error::Storage)?;
            if expected != Some(Sha256::digest(&raw).as_slice()) {
                return Err(Error::Conflict);
            }
            hedit_core::document::copy_permissions(source, &staged)
                .map_err(|_| Error::Permission)?;
            staged.sync_all().map_err(|_| Error::Storage)?;
            sys::renameat(&directory, &temporary, &directory, &name).map_err(|_| Error::Storage)?;
        } else {
            rename_noreplace(&directory, &temporary, &directory, &name)?;
        }
        directory.sync_all().map_err(|_| Error::Storage)
    })();
    if result.is_err() {
        let _ = sys::unlinkat(&directory, &temporary, sys::AtFlags::empty());
    }
    result
}
pub fn rename_noreplace(from: &File, name: &OsStr, to: &File, target: &OsStr) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        sys::renameat_with(from, name, to, target, sys::RenameFlags::NOREPLACE)
            .map_err(|_| Error::Conflict)
    }
    #[cfg(target_os = "macos")]
    {
        use std::ffi::CString;
        use std::os::fd::AsRawFd;
        let a = CString::new(name.as_bytes()).map_err(|_| Error::Permission)?;
        let b = CString::new(target.as_bytes()).map_err(|_| Error::Permission)?;
        // SAFETY: directory descriptors and NUL-terminated names remain live.
        if unsafe {
            libc::renameatx_np(
                from.as_raw_fd(),
                a.as_ptr(),
                to.as_raw_fd(),
                b.as_ptr(),
                libc::RENAME_EXCL,
            )
        } == 0
        {
            Ok(())
        } else {
            Err(Error::Conflict)
        }
    }
}

pub fn private_file(dir: &File, name: &str) -> Result<File> {
    let file: File = sys::openat(
        dir,
        name,
        OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::from_bits_truncate(0o600),
    )
    .map(File::from)
    .map_err(|_| Error::Permission)?;
    let m = file.metadata().map_err(|_| Error::Permission)?;
    if !m.is_file()
        || m.uid() != rustix::process::geteuid().as_raw()
        || m.mode() & 0o077 != 0
        || m.nlink() != 1
    {
        return Err(Error::Permission);
    }
    private_acl(&file)?;
    Ok(file)
}

pub fn path(bytes: &[u8]) -> Result<PathBuf> {
    if bytes.is_empty() || bytes.len() > 4096 || bytes.contains(&0) {
        return Err(Error::Permission);
    }
    Ok(PathBuf::from(OsString::from_vec(bytes.to_vec())))
}
pub fn path_or_empty(bytes: &[u8]) -> PathBuf {
    PathBuf::from(OsString::from_vec(bytes.to_vec()))
}

pub(crate) type Stamp = (u64, u64, u64, i64, i64, i64, i64);
pub(crate) fn stamp(m: &Metadata) -> Stamp {
    (
        m.dev(),
        m.ino(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    )
}

pub fn read_source(root: &File, bytes: &[u8]) -> Result<String> {
    let parts = components(&path(bytes)?, false)?;
    let mut dir = root.try_clone().map_err(|_| Error::Storage)?;
    for p in &parts[..parts.len() - 1] {
        dir = open_dir(&dir, p)?;
    }
    let mut file: File = sys::openat(
        &dir,
        parts.last().ok_or(Error::Permission)?,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(|_| Error::Permission)?;
    let initial = file.metadata().map_err(|_| Error::Storage)?;
    if !initial.is_file() {
        return Err(Error::Permission);
    }
    if initial.len() > MAX_DOCUMENT as u64 {
        return Err(Error::Limit);
    }
    let mut first = Vec::new();
    (&mut file)
        .take(MAX_DOCUMENT as u64 + 1)
        .read_to_end(&mut first)
        .map_err(|_| Error::Storage)?;
    if first.len() > MAX_DOCUMENT {
        return Err(Error::Limit);
    }
    file.rewind().map_err(|_| Error::Storage)?;
    let mut second = Vec::new();
    (&mut file)
        .take(MAX_DOCUMENT as u64 + 1)
        .read_to_end(&mut second)
        .map_err(|_| Error::Storage)?;
    if stamp(&initial) != stamp(&file.metadata().map_err(|_| Error::Storage)?) || first != second {
        return Err(Error::Conflict);
    }
    String::from_utf8(first).map_err(|_| Error::Unsupported)
}

pub fn verify_peer(stream: &UnixStream) -> Result<()> {
    #[cfg(target_os = "linux")]
    let uid = rustix::net::sockopt::socket_peercred(stream)
        .map_err(|_| Error::Permission)?
        .uid
        .as_raw();
    #[cfg(target_os = "macos")]
    let uid = {
        use std::os::fd::AsRawFd;
        let (mut uid, mut gid) = (0, 0);
        // SAFETY: valid socket descriptor and writable uid/gid storage.
        if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0 {
            return Err(Error::Permission);
        }
        uid
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    return Err(Error::Unsupported);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if uid != rustix::process::geteuid().as_raw() {
        return Err(Error::Permission);
    }
    Ok(())
}
