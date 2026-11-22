//! Explicit host file selection, never an executable requested by Lua.
use super::{
    format::{self, Header, MAX_IMAGE_BYTES},
    trust::Marks,
};
use crate::workspace::{identity, Stamp};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub struct Source {
    file: File,
    path: PathBuf,
    stamp: Stamp,
    header: Header,
    marks: Marks,
}
#[derive(Clone, Debug)]
pub struct Copied {
    pub bytes: u64,
    pub sha256: String,
    pub header: Header,
}
fn changed() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "The selected executable changed; select it again.",
    )
}
impl Source {
    pub(super) fn verified(
        file: File,
        path: PathBuf,
        stamp: Stamp,
        header: Header,
        marks: Marks,
    ) -> Self {
        Self {
            file,
            path,
            stamp,
            header,
            marks,
        }
    }
    pub(super) fn into_file(self) -> File {
        self.file
    }
    /// Call off the UI thread after an explicit host file-picker selection.
    /// This reads headers/trust marks only and does not execute selected code.
    pub fn select(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(changed());
        }
        #[cfg(windows)]
        {
            use std::path::{Component, Prefix};
            if !matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
                || path
                    .file_name()
                    .is_none_or(|name| name.to_string_lossy().contains(':'))
            {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "Select a native executable on a local disk.",
                ));
            }
        }
        let path = path.canonicalize()?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK | libc::O_NOFOLLOW);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::*;
            options
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_SEQUENTIAL_SCAN)
                .share_mode(FILE_SHARE_READ);
        }
        let mut file = options.open(&path)?;
        let stamp = identity::regular_descriptor(&file)?.0;
        if stamp.bytes > MAX_IMAGE_BYTES {
            return Err(changed());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let mode = file.metadata()?.mode();
            if mode & 0o111 == 0 || mode & 0o6000 != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Select an executable without set-user/group-ID permissions.",
                ));
            }
        }
        let header = format::inspect(&mut file, stamp.bytes)?;
        if !header.supports_host() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Select a native executable for this platform and architecture.",
            ));
        }
        let marks = Marks::read(&file, &path)?;
        if identity::regular_descriptor(&file)?.0 != stamp {
            return Err(changed());
        }
        Ok(Self {
            file,
            path,
            stamp,
            header,
            marks,
        })
    }
    pub fn bytes(&self) -> u64 {
        self.stamp.bytes
    }
    pub fn header(&self) -> &Header {
        &self.header
    }
    pub fn selected_path(&self) -> &Path {
        &self.path
    }
    pub fn trust_bytes(&self) -> usize {
        self.marks.bytes()
    }

    /// The host must reserve storage and persist exclusion of the destination's
    /// private parent before this call. It supplies a newly created empty file.
    /// Original files are never chmodded, re-signed, executed or modified.
    pub fn copy_to(
        mut self,
        destination: &mut File,
        path: &Path,
        cancel: &AtomicBool,
    ) -> io::Result<Copied> {
        let (initial, links) = identity::regular_descriptor(destination)?;
        if links != 1 || initial.identity == self.stamp.identity {
            return Err(changed());
        }
        same_file(destination, path)?;
        if destination.metadata()?.len() != 0 {
            return Err(changed());
        }
        hedit_core::private_io::verify(destination)?;
        if identity::regular_descriptor(&self.file)?.0 != self.stamp {
            return Err(changed());
        }
        self.file.seek(SeekFrom::Start(0))?;
        destination.seek(SeekFrom::Start(0))?;
        let mut bytes = 0u64;
        let mut digest = Sha256::new();
        let mut chunk = [0u8; 64 * 1024];
        loop {
            if cancel.load(Ordering::Acquire) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Executable import cancelled.",
                ));
            }
            let count = self.file.read(&mut chunk)?;
            if count == 0 {
                break;
            }
            bytes = bytes
                .checked_add(count as u64)
                .filter(|bytes| *bytes <= self.stamp.bytes && *bytes <= MAX_IMAGE_BYTES)
                .ok_or_else(changed)?;
            digest.update(&chunk[..count]);
            destination.write_all(&chunk[..count])?;
        }
        if bytes != self.stamp.bytes
            || identity::regular_descriptor(&self.file)?.0 != self.stamp
            || Marks::read(&self.file, &self.path)? != self.marks
        {
            return Err(changed());
        }
        same_file(destination, path)?;
        self.marks.apply(destination, path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            destination.set_permissions(std::fs::Permissions::from_mode(0o500))?;
        }
        destination.sync_all()?;
        if identity::regular_descriptor(destination)?.1 != 1
            || Marks::read(destination, path)? != self.marks
        {
            return Err(changed());
        }
        Ok(Copied {
            bytes,
            sha256: digest
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            header: self.header,
        })
    }
}

// Windows trust marks are named streams. Bind the host pathname to the pinned
// base file before touching one; Unix uses descriptor-only xattrs.
fn same_file(file: &File, path: &Path) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(changed());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let named = options.open(path)?;
    if identity::regular_descriptor(&named)?.0.identity
        != identity::regular_descriptor(file)?.0.identity
    {
        return Err(changed());
    }
    Ok(())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use cap_std::{ambient_authority, fs::Dir};
    pub(in crate::tool) fn fixture(root: &Path) -> (PathBuf, Vec<u8>) {
        // A classified header, deliberately not a runnable program. Import
        // must never start code to discover its version or dependencies.
        let mut bytes = vec![0; 256];
        if cfg!(target_os = "macos") {
            bytes[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
            let cpu: u32 = if cfg!(target_arch = "aarch64") {
                0x0100_000c
            } else {
                0x0100_0007
            };
            bytes[4..8].copy_from_slice(&cpu.to_le_bytes());
            bytes[12..16].copy_from_slice(&2u32.to_le_bytes());
        } else if cfg!(target_os = "linux") {
            bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
            bytes[16..18].copy_from_slice(&3u16.to_le_bytes());
            let cpu: u16 = if cfg!(target_arch = "aarch64") {
                183
            } else {
                62
            };
            bytes[18..20].copy_from_slice(&cpu.to_le_bytes());
            bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
            bytes[24..32].copy_from_slice(&4096u64.to_le_bytes());
            bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
        } else {
            bytes[..2].copy_from_slice(b"MZ");
            bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
            bytes[64..68].copy_from_slice(b"PE\0\0");
            let cpu: u16 = if cfg!(target_arch = "aarch64") {
                0xaa64
            } else {
                0x8664
            };
            bytes[68..70].copy_from_slice(&cpu.to_le_bytes());
            bytes[84..86].copy_from_slice(&112u16.to_le_bytes());
            bytes[86..88].copy_from_slice(&2u16.to_le_bytes());
            bytes[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
        }
        let path = root.join("selected café 東京.exe");
        std::fs::write(&path, &bytes).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        (path.canonicalize().unwrap(), bytes)
    }
    pub(in crate::tool) fn destination(root: &Path) -> (File, PathBuf) {
        let parent = Dir::open_ambient_dir(root, ambient_authority()).unwrap();
        let private = crate::journal::native::private_directory(&parent, "private").unwrap();
        let mut options = cap_std::fs::OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = private.open_with("image", &options).unwrap().into_std();
        let path = root.join("private/image").canonicalize().unwrap();
        (file, path)
    }
    #[test]
    fn import_copies_pinned_native_bytes_without_modifying_or_executing_source() {
        let directory = tempfile::tempdir().unwrap();
        let (path, bytes) = fixture(directory.path());
        let source = Source::select(&path).unwrap();
        let before = identity::regular_descriptor(&File::open(&path).unwrap())
            .unwrap()
            .0;
        let (mut file, target) = destination(directory.path());
        file.seek(SeekFrom::Start(100)).unwrap();
        let copied = source
            .copy_to(&mut file, &target, &AtomicBool::new(false))
            .unwrap();
        assert_eq!(copied.bytes, bytes.len() as u64);
        let expected: String = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(copied.sha256, expected);
        assert!(copied.header.supports_host());
        assert_eq!(std::fs::read(&target).unwrap(), bytes);
        assert_eq!(
            identity::regular_descriptor(&File::open(&path).unwrap())
                .unwrap()
                .0,
            before
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(file.metadata().unwrap().mode() & 0o7777, 0o500);
        }
    }
    #[test]
    fn changed_sources_cancellation_aliases_and_wrong_destinations_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let (path, bytes) = fixture(directory.path());
        let (mut file, target) = destination(directory.path());
        let cancelled =
            Source::select(&path)
                .unwrap()
                .copy_to(&mut file, &target, &AtomicBool::new(true));
        assert_eq!(cancelled.unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert_eq!(file.metadata().unwrap().len(), 0);
        assert!(Source::select(&path)
            .unwrap()
            .copy_to(&mut file, &path, &AtomicBool::new(false))
            .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let alias = directory.path().join("destination-alias");
        std::fs::hard_link(&target, &alias).unwrap();
        assert!(Source::select(&path)
            .unwrap()
            .copy_to(&mut file, &target, &AtomicBool::new(false))
            .is_err());
        assert_eq!(file.metadata().unwrap().len(), 0);
        drop(file);
        std::fs::remove_file(alias).unwrap();
        #[cfg(unix)]
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&target)
            .unwrap();
        #[cfg(unix)]
        {
            let source = Source::select(&path).unwrap();
            let mut changed = bytes.clone();
            changed[128] = 42;
            std::fs::write(&path, changed).unwrap();
            assert!(source
                .copy_to(&mut file, &target, &AtomicBool::new(false))
                .is_err());
            assert_eq!(file.metadata().unwrap().len(), 0);
        }
    }
}
