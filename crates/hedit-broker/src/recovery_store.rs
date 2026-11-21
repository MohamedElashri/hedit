//! Durable global admission for retained workspace recovery data. A failed or
//! interrupted preparation keeps its reservation until explicit host cleanup.
use crate::{
    workspace::{Identity, Protected},
    Handle,
};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, OpenOptions};
use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
    time::Duration,
};
const MAX_JOURNALS: usize = 32;
const MAX_BYTES: u64 = 64 * 1024 * 1024;
const OVERHEAD: u64 = 1024 * 1024;
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub id: Handle,
    pub root: Identity,
    pub stage_name: String,
    pub reserved: u64,
    #[serde(skip)]
    pub stage: Option<Identity>,
}
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Stage {
    id: Handle,
    identity: Identity,
}
impl Record {
    pub(crate) fn payload_limit(&self) -> u64 {
        self.reserved.saturating_sub(OVERHEAD)
    }
}
pub struct RecoveryStore {
    directory: Dir,
}
pub struct Lease {
    record: Record,
}
struct Lock(std::fs::File);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Workspace recovery index needs review.",
    )
}
fn full() -> io::Error {
    io::Error::new(io::ErrorKind::OutOfMemory,"Workspace recovery storage is full. Discard completed recovery records before applying another plan.")
}
fn file_options(create: bool) -> OpenOptions {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(create)
        .follow(FollowSymlinks::No)
        .nonblock(true);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}
fn filename(id: Handle) -> String {
    format!("{id}.lease")
}
impl RecoveryStore {
    /// Host supplies a dedicated private subdirectory, excluded from every scope.
    pub fn open(directory: Dir, protected: &Protected) -> io::Result<Self> {
        hedit_core::private_io::verify(&directory.try_clone()?.into_std_file())?;
        protected.insert(directory.try_clone()?)?;
        let file = directory.open_with("index.lock", &file_options(true))?;
        crate::workspace::identity::regular(&file)?;
        hedit_core::private_io::verify(&file.into_std())?;
        let store = Self { directory };
        let _guard = store.lock()?;
        store.records_unlocked()?;
        Ok(store)
    }
    fn lock(&self) -> io::Result<Lock> {
        let file = self
            .directory
            .open_with("index.lock", &file_options(false))?;
        crate::workspace::identity::regular(&file)?;
        let file = file.into_std();
        hedit_core::private_io::verify(&file)?;
        hedit_core::file_io::lock_with_timeout(&file, Duration::from_millis(100))
            .map_err(io::Error::other)?;
        Ok(Lock(file))
    }
    fn records_unlocked(&self) -> io::Result<Vec<Record>> {
        let mut records = Vec::new();
        let mut stages = BTreeMap::new();
        let mut count = 0;
        for entry in self.directory.entries()? {
            count += 1;
            if count > MAX_JOURNALS * 2 + 1 {
                return Err(full());
            }
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_str().ok_or_else(invalid)?;
            if name == "index.lock" {
                continue;
            }
            if !name.ends_with(".lease") && !name.ends_with(".stage") {
                return Err(invalid());
            }
            let file = self.directory.open_with(
                name,
                OpenOptions::new()
                    .read(true)
                    .follow(FollowSymlinks::No)
                    .nonblock(true),
            )?;
            crate::workspace::identity::regular(&file)?;
            let mut file = file.into_std();
            hedit_core::private_io::verify(&file)?;
            if file.metadata()?.len() > 8192 {
                return Err(invalid());
            }
            let mut bytes = Vec::new();
            (&mut file).take(8193).read_to_end(&mut bytes)?;
            if bytes.len() > 8192 {
                return Err(invalid());
            }
            if name.ends_with(".stage") {
                let stage: Stage = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
                if name != format!("{}.stage", stage.id)
                    || stages.insert(stage.id, stage.identity).is_some()
                {
                    return Err(invalid());
                }
                continue;
            }
            let record: Record = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
            if filename(record.id) != name
                || record.reserved < OVERHEAD
                || record.reserved > MAX_BYTES
                || record.stage_name != format!(".hedit-transaction-{}", record.id)
            {
                return Err(invalid());
            }
            records.push(record);
        }
        for record in &mut records {
            record.stage = stages.remove(&record.id);
        }
        if !stages.is_empty() {
            return Err(invalid());
        }
        if records.len() > MAX_JOURNALS
            || records.iter().map(|record| record.reserved).sum::<u64>() > MAX_BYTES
        {
            return Err(full());
        }
        Ok(records)
    }
    pub fn records(&self) -> io::Result<Vec<Record>> {
        let _guard = self.lock()?;
        self.records_unlocked()
    }
    pub(crate) fn reserve(&self, root: Identity, payload: u64) -> io::Result<Lease> {
        let _guard = self.lock()?;
        let records = self.records_unlocked()?;
        let reserved = payload
            .checked_add(OVERHEAD)
            .filter(|bytes| *bytes <= MAX_BYTES)
            .ok_or_else(full)?;
        if records.len() >= MAX_JOURNALS
            || records.iter().map(|record| record.reserved).sum::<u64>() + reserved > MAX_BYTES
        {
            return Err(full());
        }
        let id = Handle::fresh()?;
        let record = Record {
            id,
            root,
            stage_name: format!(".hedit-transaction-{id}"),
            reserved,
            stage: None,
        };
        let mut options = file_options(false);
        options.create_new(true);
        let mut file = self.directory.open_with(filename(id), &options)?;
        let bytes = serde_json::to_vec(&record).map_err(|_| invalid())?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        crate::journal::native::sync_dir(&self.directory)?;
        Ok(Lease { record })
    }
    /// Persist the private stage's physical identity before any payload or
    /// ledger is written. The reservation itself remains immutable.
    pub(crate) fn bind_stage(&self, lease: &mut Lease, identity: Identity) -> io::Result<()> {
        let _guard = self.lock()?;
        let records = self.records_unlocked()?;
        let current = records
            .iter()
            .find(|record| record.id == lease.record.id)
            .ok_or_else(invalid)?;
        if current.root != lease.record.root
            || current.reserved != lease.record.reserved
            || current.stage.is_some()
        {
            return Err(invalid());
        }
        let mut options = file_options(false);
        options.create_new(true);
        let mut file = self
            .directory
            .open_with(format!("{}.stage", lease.record.id), &options)?;
        file.write_all(
            &serde_json::to_vec(&Stage {
                id: lease.record.id,
                identity,
            })
            .map_err(|_| invalid())?,
        )?;
        file.sync_all()?;
        crate::journal::native::sync_dir(&self.directory)?;
        lease.record.stage = Some(identity);
        Ok(())
    }
    /// Release only after the trusted host has emptied/discarded the matching
    /// private stage. A reservation is never dropped implicitly on failure.
    pub(crate) fn release(&self, record: &Record) -> io::Result<()> {
        let _guard = self.lock()?;
        let records = self.records_unlocked()?;
        let current = records
            .iter()
            .find(|value| value.id == record.id)
            .ok_or_else(invalid)?;
        if current.root != record.root
            || current.stage_name != record.stage_name
            || current.reserved != record.reserved
            || current.stage != record.stage
        {
            return Err(invalid());
        }
        if record.stage.is_some() {
            self.directory.remove_file(format!("{}.stage", record.id))?;
        }
        self.directory.remove_file(filename(record.id))?;
        crate::journal::native::sync_dir(&self.directory)?;
        Ok(())
    }
    pub(crate) fn lease(&self, id: Handle, root: Identity) -> io::Result<Lease> {
        self.records()?
            .into_iter()
            .find(|record| record.id == id && record.root == root)
            .map(|record| Lease { record })
            .ok_or_else(invalid)
    }
}
impl Lease {
    pub(crate) fn record(&self) -> &Record {
        &self.record
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aggregate_retention_counts_quota_and_corrupt_indices_fail_closed() {
        let outer = tempfile::tempdir().unwrap();
        let parent = Dir::open_ambient_dir(outer.path(), cap_std::ambient_authority()).unwrap();
        let config = crate::journal::native::private_directory(&parent, "config").unwrap();
        let protected = Protected::persistent(&config, []).unwrap();
        let directory = crate::journal::native::private_directory(&config, "recovery").unwrap();
        let store = RecoveryStore::open(directory, &protected).unwrap();
        let identity = Identity { volume: 1, file: 2 };
        let first = store.reserve(identity, MAX_BYTES - OVERHEAD).unwrap();
        assert!(store.reserve(identity, 1).is_err());
        assert_eq!(store.records().unwrap()[0].reserved, MAX_BYTES);
        store.release(first.record()).unwrap();
        assert!(store.records().unwrap().is_empty());
        let mut leases = Vec::new();
        for _ in 0..MAX_JOURNALS {
            leases.push(store.reserve(identity, 1).unwrap());
        }
        assert!(store.reserve(identity, 1).is_err());
        for lease in &leases {
            store.release(lease.record()).unwrap();
        }
        let mut options = file_options(false);
        options.create_new(true);
        let mut bad = store.directory.open_with("broken.lease", &options).unwrap();
        bad.write_all(b"truncated").unwrap();
        assert!(store.records().is_err());
        assert!(store.reserve(identity, 1).is_err());
    }
}
