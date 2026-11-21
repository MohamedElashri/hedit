//! Durable identity exclusions shared across sessions and editor processes.
use super::*;
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Seek, SeekFrom, Write},
    sync::{Arc, Mutex},
};
const MAGIC: &[u8; 8] = b"HEDITPR1";
const RECORD: usize = 56;
const MAX_IDENTITIES: usize = 4096;
struct Store {
    file: fs::File,
    read: u64,
}
struct State {
    directories: BTreeMap<Identity, Option<fs::File>>,
    store: Option<Store>,
}
#[derive(Clone)]
pub struct Protected {
    state: Arc<Mutex<State>>,
}
impl State {
    fn refresh(&mut self) -> io::Result<()> {
        let Some(store) = &mut self.store else {
            return Ok(());
        };
        hedit_core::private_io::verify(&store.file)?;
        let bytes = store.file.metadata()?.len();
        if bytes < store.read
            || bytes > 8 + (MAX_IDENTITIES * RECORD) as u64
            || (bytes - 8) % RECORD as u64 != 0
        {
            return Err(denied());
        }
        if bytes == store.read {
            return Ok(());
        }
        store.file.seek(SeekFrom::Start(store.read))?;
        while store.read < bytes {
            let mut record = [0u8; RECORD];
            store.file.read_exact(&mut record)?;
            if Sha256::digest(&record[..24]).as_slice() != &record[24..] {
                return Err(denied());
            }
            let identity = Identity {
                volume: u64::from_le_bytes(record[..8].try_into().unwrap()),
                file: u128::from_le_bytes(record[8..24].try_into().unwrap()),
            };
            self.directories.entry(identity).or_insert(None);
            store.read += RECORD as u64;
        }
        Ok(())
    }
}
impl Protected {
    /// In-memory exclusions for tests or non-persistent read-only use. A host
    /// enabling workspace edits must use `persistent` before opening sessions.
    pub fn new(paths: impl IntoIterator<Item = PathBuf>) -> io::Result<Self> {
        let protected = Self {
            state: Arc::new(Mutex::new(State {
                directories: BTreeMap::new(),
                store: None,
            })),
        };
        for path in paths {
            let path = path.canonicalize()?;
            protected.insert_path(&path)?;
        }
        Ok(protected)
    }
    /// The host supplies its already private configuration directory. This file
    /// is never replaced: other processes refresh additions through the same inode.
    pub fn persistent(
        directory: &Dir,
        paths: impl IntoIterator<Item = PathBuf>,
    ) -> io::Result<Self> {
        hedit_core::private_io::verify(&directory.try_clone()?.into_std_file())?;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create(true)
            .follow(FollowSymlinks::No)
            .nonblock(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = directory.open_with("plugin-protected-identities", &options)?;
        identity::regular(&file)?;
        let mut file = file.into_std();
        hedit_core::private_io::verify(&file)?;
        hedit_core::file_io::lock_with_timeout(&file, Duration::from_millis(100))
            .map_err(io::Error::other)?;
        let result = (|| {
            if file.metadata()?.len() == 0 {
                file.write_all(MAGIC)?;
                file.sync_all()?;
            }
            file.seek(SeekFrom::Start(0))?;
            let mut magic = [0; 8];
            file.read_exact(&mut magic)?;
            if &magic != MAGIC {
                return Err(denied());
            }
            Ok(())
        })();
        let unlocked = file.unlock();
        result?;
        unlocked?;
        crate::journal::native::sync_dir(directory)?;
        let protected = Self {
            state: Arc::new(Mutex::new(State {
                directories: BTreeMap::new(),
                store: Some(Store { file, read: 8 }),
            })),
        };
        protected.refresh()?;
        // Exclude the store itself even if callers forget to list its ancestor.
        protected.insert(directory.try_clone()?)?;
        for path in paths {
            let path = path.canonicalize()?;
            protected.insert_path(&path)?;
        }
        Ok(protected)
    }
    fn refresh(&self) -> io::Result<()> {
        let mut state = self.state.lock().map_err(|_| denied())?;
        let guard = state
            .store
            .as_ref()
            .map(|store| store.file.try_clone())
            .transpose()?;
        if let Some(file) = &guard {
            hedit_core::file_io::lock_shared_with_timeout(file, Duration::from_millis(100))
                .map_err(io::Error::other)?;
        }
        let result = state.refresh();
        let unlocked = guard.as_ref().map(fs::File::unlock).transpose();
        result?;
        unlocked?;
        Ok(())
    }
    fn insert_path(&self, path: &Path) -> io::Result<Identity> {
        if path.is_dir() {
            self.insert(Dir::open_ambient_dir(path, ambient_authority())?)
        } else {
            let file = File::from_std(hedit_core::file_io::open_regular(path)?);
            let identity = identity::regular(&file)?.identity;
            self.insert_pin(identity, file.into_std())
        }
    }
    pub(crate) fn insert(&self, directory: Dir) -> io::Result<Identity> {
        let identity = identity::directory(&directory)?;
        self.insert_pin(identity, directory.into_std_file())
    }
    fn insert_pin(&self, identity: Identity, pin: fs::File) -> io::Result<Identity> {
        let mut state = self.state.lock().map_err(|_| denied())?;
        let guard = state
            .store
            .as_ref()
            .map(|store| store.file.try_clone())
            .transpose()?;
        if let Some(file) = &guard {
            hedit_core::file_io::lock_with_timeout(file, Duration::from_millis(100))
                .map_err(io::Error::other)?;
        }
        let result = (|| {
            state.refresh()?;
            if !state.directories.contains_key(&identity) {
                if state.directories.len() >= MAX_IDENTITIES {
                    return Err(limit());
                }
                if let Some(store) = &mut state.store {
                    let mut record = Vec::with_capacity(RECORD);
                    record.extend(identity.volume.to_le_bytes());
                    record.extend(identity.file.to_le_bytes());
                    let hash = Sha256::digest(&record);
                    record.extend_from_slice(&hash);
                    store.file.seek(SeekFrom::End(0))?;
                    store.file.write_all(&record)?;
                    store.file.sync_all()?;
                    store.read += RECORD as u64;
                }
            }
            state.directories.insert(identity, Some(pin));
            Ok(identity)
        })();
        let unlocked = guard.as_ref().map(fs::File::unlock).transpose();
        result.and_then(|id| unlocked.map(|_| id))
    }
    /// Drop a no-longer-needed pin only after all private contents are gone.
    /// Its durable exclusion remains, conservatively protecting identity reuse.
    pub(crate) fn release_empty(&self, directory: &Dir) -> io::Result<()> {
        if directory.entries()?.next().is_some() {
            return Err(denied());
        }
        let identity = identity::directory(directory)?;
        let mut state = self.state.lock().map_err(|_| denied())?;
        *state.directories.get_mut(&identity).ok_or_else(denied)? = None;
        Ok(())
    }
    pub(crate) fn durable(&self) -> bool {
        self.state.lock().is_ok_and(|state| state.store.is_some())
    }
    pub(super) fn contains(&self, identity: &Identity) -> bool {
        if self.refresh().is_err() {
            return true;
        }
        self.state
            .lock()
            .map(|state| state.directories.contains_key(identity))
            .unwrap_or(true)
    }
    pub fn len(&self) -> usize {
        if self.refresh().is_err() {
            return usize::MAX;
        }
        self.state
            .lock()
            .map(|state| state.directories.len())
            .unwrap_or(usize::MAX)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persisted_ids_refresh_across_instances_and_survive_stage_renames() {
        let outer = tempfile::tempdir().unwrap();
        let private = outer.path().join("config");
        std::fs::create_dir(&private).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let directory = Dir::open_ambient_dir(&private, ambient_authority()).unwrap();
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::*;
            let handle = std::fs::OpenOptions::new()
                .access_mode(FILE_GENERIC_READ | WRITE_DAC)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&private)
                .unwrap();
            hedit_core::private_io::protect_directory(&handle).unwrap();
        }
        {
            let a = Protected::persistent(&directory, []).unwrap();
            let b = Protected::persistent(&directory, []).unwrap();
            let stage = outer.path().join("stage");
            std::fs::create_dir(&stage).unwrap();
            let stage = Dir::open_ambient_dir(&stage, ambient_authority()).unwrap();
            let id = a.insert(stage).unwrap();
            assert!(b.contains(&id));
            drop(a);
            drop(b);
            let c = Protected::persistent(&directory, []).unwrap();
            assert!(c.contains(&id));
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(private.join("plugin-protected-identities"))
                .unwrap();
            file.write_all(b"partial").unwrap();
            assert!(c.contains(&Identity { volume: 0, file: 0 }));
            assert!(Protected::persistent(&directory, []).is_err());
        }
    }
}
