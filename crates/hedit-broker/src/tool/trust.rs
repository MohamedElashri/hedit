//! Preserve negative execution trust marks, never sandbox grants or source ACLs.
use std::{fs::File, io, path::Path};
#[cfg(any(target_os = "macos", windows))]
const MAX_MARK_BYTES: usize = 64 * 1024;
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Marks {
    bytes: Option<Vec<u8>>,
}
impl Marks {
    pub fn bytes(&self) -> usize {
        self.bytes.as_ref().map_or(0, Vec::len)
    }
    pub fn read(file: &File, path: &Path) -> io::Result<Self> {
        #[cfg(target_os = "macos")]
        {
            use std::os::fd::AsRawFd;
            let count = unsafe {
                libc::fgetxattr(
                    file.as_raw_fd(),
                    c"com.apple.quarantine".as_ptr(),
                    std::ptr::null_mut(),
                    0,
                    0,
                    0,
                )
            };
            if count < 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::ENOATTR) {
                    return Ok(Self { bytes: None });
                }
                return Err(error);
            }
            if count as usize > MAX_MARK_BYTES {
                return Err(io::Error::other(
                    "Executable trust metadata exceeds its limit.",
                ));
            }
            let mut bytes = vec![0; count as usize];
            let copied = unsafe {
                libc::fgetxattr(
                    file.as_raw_fd(),
                    c"com.apple.quarantine".as_ptr(),
                    bytes.as_mut_ptr().cast(),
                    bytes.len(),
                    0,
                    0,
                )
            };
            if copied != count {
                return Err(io::Error::other("Executable trust metadata changed."));
            }
            let _ = path;
            Ok(Self { bytes: Some(bytes) })
        }
        #[cfg(windows)]
        {
            use std::io::Read;
            let _ = file;
            let mut stream = path.as_os_str().to_os_string();
            stream.push(":Zone.Identifier");
            let stream = match File::open(stream) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Ok(Self { bytes: None })
                }
                Err(error) => return Err(error),
            };
            if stream.metadata()?.len() > MAX_MARK_BYTES as u64 {
                return Err(io::Error::other(
                    "Executable trust metadata exceeds its limit.",
                ));
            }
            let mut bytes = Vec::new();
            stream
                .take(MAX_MARK_BYTES as u64 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > MAX_MARK_BYTES {
                return Err(io::Error::other(
                    "Executable trust metadata exceeds its limit.",
                ));
            }
            Ok(Self { bytes: Some(bytes) })
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            let _ = (file, path);
            Ok(Self { bytes: None })
        }
    }
    pub fn apply(&self, file: &File, path: &Path) -> io::Result<()> {
        let Some(bytes) = &self.bytes else {
            return Ok(());
        };
        #[cfg(target_os = "macos")]
        {
            use std::os::fd::AsRawFd;
            if unsafe {
                libc::fsetxattr(
                    file.as_raw_fd(),
                    c"com.apple.quarantine".as_ptr(),
                    bytes.as_ptr().cast(),
                    bytes.len(),
                    0,
                    0,
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            let _ = path;
        }
        #[cfg(windows)]
        {
            use std::io::Write;
            let _ = file;
            let mut name = path.as_os_str().to_os_string();
            name.push(":Zone.Identifier");
            let mut stream = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(name)?;
            stream.write_all(bytes)?;
            stream.sync_all()?;
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            let _ = (bytes, file, path);
        }
        Ok(())
    }
}

#[cfg(all(test, any(target_os = "macos", windows)))]
mod tests {
    use super::*;
    use crate::tool::{
        source::tests::{destination, fixture},
        Source,
    };
    use std::sync::atomic::AtomicBool;
    #[test]
    fn negative_execution_trust_marks_survive_import_byte_for_byte() {
        let directory = tempfile::tempdir().unwrap();
        let (path, original) = fixture(directory.path());
        #[cfg(target_os = "macos")]
        let bytes = b"0083;5f000001;hedit-test;00000000-0000-0000-0000-000000000000".to_vec();
        #[cfg(windows)]
        let bytes =
            b"[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=https://example.invalid/fixture\r\n".to_vec();
        let marks = Marks { bytes: Some(bytes) };
        let selected = File::open(&path).unwrap();
        marks.apply(&selected, &path).unwrap();
        let source = Source::select(&path).unwrap();
        assert_eq!(source.trust_bytes(), marks.bytes());
        let (mut file, target) = destination(directory.path());
        source
            .copy_to(&mut file, &target, &AtomicBool::new(false))
            .unwrap();
        assert_eq!(Marks::read(&file, &target).unwrap(), marks);
        assert_eq!(Marks::read(&selected, &path).unwrap(), marks);
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
    #[test]
    fn trust_metadata_changes_invalidate_the_selected_image() {
        let directory = tempfile::tempdir().unwrap();
        let (path, _) = fixture(directory.path());
        let source = Source::select(&path).unwrap();
        let selected = File::open(&path).unwrap();
        let changed = Marks {
            bytes: Some(b"unreviewed mark".to_vec()),
        }
        .apply(&selected, &path);
        let (mut file, target) = destination(directory.path());
        #[cfg(target_os = "macos")]
        {
            changed.unwrap();
            assert!(source
                .copy_to(&mut file, &target, &AtomicBool::new(false))
                .is_err());
            assert_eq!(file.metadata().unwrap().len(), 0);
        }
        #[cfg(windows)]
        {
            // The pinned base image denies write sharing. Windows may reject
            // the stream change outright; otherwise the import must detect it.
            if changed.is_ok() {
                assert!(source
                    .copy_to(&mut file, &target, &AtomicBool::new(false))
                    .is_err());
            }
        }
    }
}
