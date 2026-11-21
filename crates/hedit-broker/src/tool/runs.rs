//! Per-invocation private copies with durable disk admission and live leases.
use super::{projection, store, Image, Projection};
use crate::{
    journal::native,
    workspace::{identity, Identity, Protected},
    Handle,
};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, OpenOptions};
use hedit_plugin::tool::{Launch, Output};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

const MAX_RUNS: usize = 4;
const MAX_BYTES: u64 = 512 * 1024 * 1024;
const OVERHEAD: u64 = 512 * 1024;
const META_BYTES: u64 = 256 * 1024;
const LOCK: &str = "index.lock";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u32,
    id: Handle,
    root: Identity,
    image: u64,
    #[serde(deserialize_with = "paths")]
    files: BTreeMap<String, u64>,
}
impl Record {
    fn reserved(&self) -> io::Result<u64> {
        if self.version != 1
            || !(32..=super::format::MAX_IMAGE_BYTES).contains(&self.image)
            || self.files.len() > crate::workspace::MAX_RESULTS
        {
            return Err(invalid());
        }
        let mut paths = 0;
        let mut bytes = 0u64;
        let mut nodes = std::collections::BTreeSet::new();
        for (path, length) in &self.files {
            if !safe_path(path) || *length > crate::workspace::MAX_FILE_BYTES as u64 {
                return Err(invalid());
            }
            paths += path.len();
            let parts: Vec<_> = path.split('/').collect();
            let mut prefix = String::new();
            for (index, part) in parts.iter().enumerate() {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(part);
                if index + 1 < parts.len() && self.files.contains_key(&prefix) {
                    return Err(invalid());
                }
                nodes.insert(prefix.clone());
                if nodes.len() > 2000 {
                    return Err(invalid());
                }
            }
            bytes = bytes.checked_add(*length).ok_or_else(invalid)?;
        }
        if paths > projection::MAX_PATH_BYTES || bytes > projection::MAX_PROJECT_BYTES as u64 {
            return Err(invalid());
        }
        Ok(OVERHEAD + self.image + bytes)
    }
}
fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= crate::workspace::MAX_PATH_BYTES
        && path.split('/').all(crate::workspace::component)
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Private tool run storage needs review; existing data was preserved.",
    )
}
fn full() -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        "Private tool run capacity is exhausted.",
    )
}
fn check(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Tool preparation cancelled.",
        ))
    } else {
        Ok(())
    }
}
struct Lock(File);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
pub struct Runs {
    directory: Dir,
    path: PathBuf,
    identity: Identity,
}
pub struct Run {
    store: Arc<Runs>,
    record: Record,
    lease: Option<File>,
    launch: Launch,
}
/// A service retains the private image/snapshot lease through native kill/reap.
/// Field order intentionally drops the transport before retiring its bundle.
pub struct Service {
    transport: hedit_plugin::tool::Stream,
    _run: Run,
}
impl Service {
    pub fn poll(
        &mut self,
        input: &[u8],
        output: &mut [u8; 4096],
        cancel: &AtomicBool,
        deadline: std::time::Instant,
    ) -> io::Result<hedit_plugin::tool::Progress> {
        self.transport.poll(input, output, cancel, deadline)
    }
    pub fn wait(&self, input_pending: bool) -> io::Result<()> {
        self.transport.wait(input_pending)
    }
}
impl Run {
    pub fn service(self, cancel: &AtomicBool, deadline: std::time::Instant) -> io::Result<Service> {
        let transport = hedit_plugin::tool::Stream::open(&self.launch, cancel, deadline)?;
        Ok(Service {
            transport,
            _run: self,
        })
    }
    /// This consuming method retains the private bundle through native kill and
    /// reaping, including cancellation, errors and unwind. No process/path is
    /// exposed to the Lua worker. Persistent services need a separate owner.
    pub fn filter(
        self,
        input: &[u8],
        cancel: &AtomicBool,
        timeout: Duration,
    ) -> io::Result<Output> {
        hedit_plugin::tool::run(&self.launch, input, cancel, timeout)
    }
}
impl Drop for Run {
    fn drop(&mut self) {
        // Drop never runs on the UI thread: the host I/O thread owns Run.
        // On cleanup failure preserve admission for a later bounded reap.
        if let Ok(_lock) = self.store.lock() {
            self.lease.take();
            let _ = self.store.reap_one(&self.record);
        }
    }
}
impl Runs {
    pub fn open(directory: Dir, path: &Path, protected: &Protected) -> io::Result<Self> {
        hedit_core::private_io::verify(&directory.try_clone()?.into_std_file())?;
        let physical = identity::directory(&directory)?;
        let store = Self {
            directory,
            path: path.canonicalize()?,
            identity: physical,
        };
        store.current()?;
        protected.insert(store.directory.try_clone()?)?;
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
        let file = store.directory.open_with(LOCK, &options)?;
        identity::regular(&file)?;
        hedit_core::private_io::verify(&file.into_std())?;
        native::sync_dir(&store.directory)?;
        // Admission and payload checks run in prepare/reap. Callback setup
        // should not traverse live project projections.
        Ok(store)
    }
    fn current(&self) -> io::Result<()> {
        let named = Dir::open_ambient_dir(&self.path, cap_std::ambient_authority())?;
        if identity::directory(&named)? != self.identity {
            return Err(invalid());
        }
        hedit_core::private_io::verify(&named.into_std_file())
    }
    fn lock(&self) -> io::Result<Lock> {
        self.current()?;
        let file = store::read(&self.directory, LOCK)?;
        hedit_core::file_io::lock_with_timeout(&file, Duration::from_millis(100))
            .map_err(io::Error::other)?;
        Ok(Lock(file))
    }
    fn child(&self, record: &Record) -> io::Result<Dir> {
        let directory = self.directory.open_dir_nofollow(record.id.to_string())?;
        if identity::directory(&directory)? != record.root {
            return Err(invalid());
        }
        hedit_core::private_io::verify(&directory.try_clone()?.into_std_file())?;
        Ok(directory)
    }
    fn records(&self) -> io::Result<Vec<Record>> {
        let mut records = Vec::new();
        let mut total = 0;
        for (count, entry) in self.directory.entries()?.enumerate() {
            if count > MAX_RUNS * 2 {
                return Err(full());
            }
            let entry = entry?;
            let name = entry.file_name();
            if name == LOCK {
                continue;
            }
            let name = name.to_str().ok_or_else(invalid)?;
            if name.starts_with(".removed-") {
                self.cleanup_retired(name)?;
                continue;
            }
            let id: Handle = serde_json::from_value(serde_json::Value::String(name.into()))
                .map_err(|_| invalid())?;
            let directory = self.directory.open_dir_nofollow(name)?;
            hedit_core::private_io::verify(&directory.try_clone()?.into_std_file())?;
            let file = store::read(&directory, "reservation.json")?;
            let record: Record =
                serde_json::from_slice(&hedit_core::file_io::read_bounded(file, META_BYTES)?)
                    .map_err(|_| invalid())?;
            if record.id != id || record.root != identity::directory(&directory)? {
                return Err(invalid());
            }
            self.contents(&directory, &record)?;
            total += record.reserved()?;
            if total > MAX_BYTES {
                return Err(full());
            }
            records.push(record);
        }
        if records.len() > MAX_RUNS {
            return Err(full());
        }
        Ok(records)
    }
    /// Reclaim abandoned bundles only after taking their OS lease. Native
    /// adapters kill tools when their host dies; no surviving worker retains
    /// the lease or gains authority from cleanup. Busy/error cases retain quota.
    pub fn reap(&self) -> io::Result<()> {
        let _lock = self.lock()?;
        for record in self.records()? {
            self.reap_one(&record)?;
        }
        Ok(())
    }
    pub fn prepare(
        self: &Arc<Self>,
        image: Image,
        projection: Projection,
        arguments: Vec<String>,
        cancel: &AtomicBool,
    ) -> io::Result<Run> {
        check(cancel)?;
        Launch::validate_arguments(&arguments)?;
        let _lock = self.lock()?;
        for record in self.records()? {
            self.reap_one(&record)?;
        }
        let records = self.records()?;
        let id = Handle::fresh()?;
        let mut record = Record {
            version: 1,
            id,
            root: self.identity,
            image: image.bytes(),
            files: projection
                .files
                .iter()
                .map(|(snapshot, _)| (snapshot.relative.clone(), snapshot.text.len() as u64))
                .collect(),
        };
        let reserved = record.reserved()?;
        if records.len() >= MAX_RUNS
            || records
                .iter()
                .map(Record::reserved)
                .collect::<io::Result<Vec<_>>>()?
                .iter()
                .sum::<u64>()
                + reserved
                > MAX_BYTES
        {
            return Err(full());
        }
        let directory = native::private_directory(&self.directory, &id.to_string())?;
        record.root = identity::directory(&directory)?;
        let lease = store::new_file(&directory, "lease")?;
        hedit_core::file_io::lock_with_timeout(&lease, Duration::from_millis(100))
            .map_err(io::Error::other)?;
        let bytes = serde_json::to_vec(&record).map_err(|_| invalid())?;
        if bytes.len() as u64 > META_BYTES {
            return Err(invalid());
        }
        let mut receipt = store::new_file(&directory, "reservation.json")?;
        receipt.write_all(&bytes)?;
        receipt.sync_all()?;
        drop(receipt);
        native::sync_dir(&directory)?;
        native::sync_dir(&self.directory)?;
        let root = self.path.join(id.to_string());
        let launch = Launch {
            executable: root.join("image.exe"),
            snapshot: root.join("snapshot"),
            arguments,
        };
        let run = Run {
            store: self.clone(),
            record,
            lease: Some(lease),
            launch,
        };
        // Once admitted, every failure retains a Run owner for bounded cleanup.
        // Release the global lock before Drop can attempt its own cleanup lock.
        drop(_lock);
        let mut executable = store::new_file(&directory, "image.exe")?;
        image.copy_to(&mut executable, &run.launch.executable, cancel)?;
        drop(executable);
        let snapshot_root = native::private_directory(&directory, "snapshot")?;
        for (snapshot, _memory) in projection.files {
            check(cancel)?;
            let parts: Vec<_> = snapshot.relative.split('/').collect();
            let mut parent = snapshot_root.try_clone()?;
            for part in &parts[..parts.len() - 1] {
                parent = match native::private_directory(&parent, part) {
                    Ok(dir) => dir,
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                        parent.open_dir_nofollow(part)?
                    }
                    Err(error) => return Err(error),
                };
            }
            let mut file = store::new_file(&parent, parts[parts.len() - 1])?;
            file.write_all(snapshot.text.as_bytes())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(std::fs::Permissions::from_mode(0o400))?;
            }
            file.sync_all()?;
        }
        native::sync_dir(&snapshot_root)?;
        self.current()?;
        run.launch.validate()?;
        Ok(run)
    }
    fn reap_one(&self, record: &Record) -> io::Result<()> {
        let directory = self
            .child(record)
            .map_err(|error| context("open run directory", error))?;
        let lease = match store::read(&directory, "lease") {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(invalid()),
            Err(error) => return Err(error),
        };
        match lease.try_lock() {
            Ok(()) => (),
            Err(std::fs::TryLockError::WouldBlock) => return Ok(()),
            Err(error) => return Err(context("lock run lease", io::Error::other(error))),
        }
        self.contents(&directory, record)
            .map_err(|error| context("validate retirement payload", error))?;
        // Windows refuses directory retirement with open descendants. We have
        // proved that no live Run owns this lease; release both handles while
        // retaining the global lock. Existing bundles cannot be resumed.
        drop(directory);
        drop(lease);
        let removed = format!(".removed-{}", record.id);
        self.directory
            .rename(record.id.to_string(), &self.directory, &removed)
            .map_err(|error| context("rename retired bundle", error))?;
        native::sync_dir(&self.directory)?;
        self.cleanup_retired(&removed)
            .map_err(|error| context("remove retired payload", error))
    }
    fn cleanup_retired(&self, name: &str) -> io::Result<()> {
        let id: Handle = serde_json::from_value(serde_json::Value::String(
            name.strip_prefix(".removed-").ok_or_else(invalid)?.into(),
        ))
        .map_err(|_| invalid())?;
        let directory = self.directory.open_dir_nofollow(name)?;
        hedit_core::private_io::verify(&directory.try_clone()?.into_std_file())?;
        match store::read(&directory, "reservation.json") {
            Ok(file) => {
                let record: Record =
                    serde_json::from_slice(&hedit_core::file_io::read_bounded(file, META_BYTES)?)
                        .map_err(|_| invalid())?;
                record.reserved()?;
                if record.id != id || record.root != identity::directory(&directory)? {
                    return Err(invalid());
                }
                let lease = store::read(&directory, "lease")?;
                lease.try_lock().map_err(io::Error::other)?;
                // The global lock and retired namespace prevent resurrection;
                // no open lease handle should interfere with payload deletion.
                drop(lease);
                self.cleanup(&directory, &record)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // Reservation is removed last, after every payload. A crash
                // here can leave only the private, empty lease or no entries.
                for entry in directory.entries()? {
                    let name = entry?.file_name();
                    if name != "lease" {
                        return Err(invalid());
                    }
                    let file = store::read(&directory, "lease")?;
                    if file.metadata()?.len() != 0 {
                        return Err(invalid());
                    }
                    file.try_lock().map_err(io::Error::other)?;
                    drop(file);
                    directory.remove_file("lease")?;
                }
            }
            Err(error) => return Err(error),
        }
        drop(directory);
        self.directory.remove_dir(name)?;
        native::sync_dir(&self.directory)
    }
    fn contents(&self, directory: &Dir, record: &Record) -> io::Result<Vec<(String, bool)>> {
        let mut expected = BTreeMap::new();
        expected.insert("image.exe".to_string(), Some(record.image));
        expected.insert("snapshot".to_string(), None);
        for (path, bytes) in &record.files {
            let mut prefix = "snapshot".to_string();
            let parts: Vec<_> = path.split('/').collect();
            for (index, part) in parts.iter().enumerate() {
                prefix.push('/');
                prefix.push_str(part);
                if expected
                    .insert(
                        prefix.clone(),
                        if index + 1 == parts.len() {
                            Some(*bytes)
                        } else {
                            None
                        },
                    )
                    .is_some_and(|old| old.is_some() && index + 1 != parts.len())
                {
                    return Err(invalid());
                }
            }
        }
        let mut pending = vec![(directory.try_clone()?, String::new(), 0usize)];
        let mut found = Vec::new();
        while let Some((parent, prefix, depth)) = pending.pop() {
            if depth > 18 || found.len() > 2048 {
                return Err(invalid());
            }
            for entry in parent.entries()? {
                let entry = entry?;
                let name = entry.file_name().into_string().map_err(|_| invalid())?;
                let path = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                if prefix.is_empty() && matches!(name.as_str(), "lease" | "reservation.json") {
                    continue;
                }
                let limit = expected.get(&path).ok_or_else(invalid)?;
                if let Some(limit) = limit {
                    let file = parent.open_with(
                        &name,
                        OpenOptions::new()
                            .read(true)
                            .follow(FollowSymlinks::No)
                            .nonblock(true),
                    )?;
                    let stamp = identity::regular(&file)?;
                    if stamp.bytes > *limit {
                        return Err(invalid());
                    }
                } else {
                    let child = parent.open_dir_nofollow(&name)?;
                    pending.push((child, path.clone(), depth + 1));
                }
                found.push((path, limit.is_none()));
            }
        }
        Ok(found)
    }
    fn cleanup(&self, directory: &Dir, record: &Record) -> io::Result<()> {
        let mut found = self.contents(directory, record)?;
        // All entries are validated before the first deletion. Native policies
        // deny creation/renames; the private bundle root is never writable by it.
        found.sort_by_key(|a| std::cmp::Reverse(a.0.len()));
        for (path, is_dir) in found {
            if is_dir {
                directory.remove_dir(path)?;
            } else {
                directory.remove_file(path)?;
            }
        }
        directory.remove_file("reservation.json")?;
        directory.remove_file("lease")?;
        native::sync_dir(directory)
    }
}

fn paths<'de, D: serde::Deserializer<'de>>(decoder: D) -> Result<BTreeMap<String, u64>, D::Error> {
    struct Paths;
    impl<'de> serde::de::Visitor<'de> for Paths {
        type Value = BTreeMap<String, u64>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("bounded unique snapshot paths")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut values = BTreeMap::new();
            let mut bytes = 0;
            while let Some(path) = map.next_key::<String>()? {
                bytes += path.len();
                if !safe_path(&path)
                    || bytes > projection::MAX_PATH_BYTES
                    || values.len() >= crate::workspace::MAX_RESULTS
                    || values.contains_key(&path)
                {
                    return Err(serde::de::Error::custom(
                        "invalid or duplicate snapshot path",
                    ));
                }
                values.insert(path, map.next_value()?);
            }
            Ok(values)
        }
    }
    decoder.deserialize_map(Paths)
}

#[cfg(test)]
mod tests;

fn context(stage: &str, error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("Native tool storage could not {stage}: {error}"),
    )
}
