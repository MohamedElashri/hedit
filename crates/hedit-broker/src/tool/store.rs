//! Durable admission and exact-source ownership for host-selected tool images.
use super::{format::Header, Source};
use crate::{
    declaration::ToolDefinition,
    journal::native,
    workspace::{identity, Identity, Protected, Stamp},
    Handle,
};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

const MAX_IMAGES: usize = 64;
const MAX_STORE_BYTES: u64 = 256 * 1024 * 1024;
const OVERHEAD: u64 = 128 * 1024;
const MAX_RECORD_BYTES: u64 = 8192;
pub(super) const LOCK: &str = "index.lock";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    command: String,
    source: Handle,
    profile: String,
    recipe: String,
}
impl Binding {
    pub fn new(
        command: &str,
        source: Handle,
        profile: &str,
        definition: &ToolDefinition,
    ) -> io::Result<Self> {
        if !hedit_plugin::options::valid_command_id(command)
            || !hedit_plugin::options::valid_id(profile)
        {
            return Err(invalid());
        }
        definition.validate().map_err(|_| invalid())?;
        let bytes = serde_json::to_vec(definition).map_err(|_| invalid())?;
        Ok(Self {
            command: command.into(),
            source,
            profile: profile.into(),
            recipe: hex(&Sha256::digest(bytes)),
        })
    }
    fn validate(&self) -> io::Result<()> {
        if !hedit_plugin::options::valid_command_id(&self.command)
            || !hedit_plugin::options::valid_id(&self.profile)
            || !digest(&self.recipe)
        {
            return Err(invalid());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    version: u32,
    pub id: Handle,
    binding: Binding,
    pub reserved: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u32,
    id: Handle,
    binding: Binding,
    bytes: u64,
    sha256: String,
    header: Header,
    stamp: Stamp,
    mark_digest: Option<String>,
    origin: String,
}
#[derive(Clone, Debug)]
pub struct Info {
    pub origin: String,
    pub bytes: u64,
    pub sha256: String,
    pub header: Header,
}
struct Pinned {
    receipt: Receipt,
    file: File,
    path: PathBuf,
    marks: super::trust::Marks,
}
pub struct Image {
    source: Source,
    bytes: u64,
    sha256: String,
    header: Header,
}
impl Image {
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Transfer a verified descriptor, never a path or worker-supplied authority.
    pub fn into_file(self) -> File {
        self.source.into_file()
    }
    /// Copy into an admitted per-invocation bundle, preserving trust marks and
    /// rechecking the pinned immutable image before any launch.
    pub fn copy_to(
        self,
        file: &mut File,
        path: &Path,
        cancel: &AtomicBool,
    ) -> io::Result<super::Copied> {
        self.source.copy_to(file, path, cancel)
    }
}
pub struct Store {
    directory: Dir,
    path: PathBuf,
    identity: Identity,
}
struct Lock(File);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Native tool storage needs review; existing data was preserved.",
    )
}
fn full() -> io::Error {
    io::Error::new(
        io::ErrorKind::OutOfMemory,
        "Native tool image storage is full. Remove unused imports before adding another image.",
    )
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(super) fn read(directory: &Dir, name: &str) -> io::Result<File> {
    let file = directory.open_with(
        name,
        OpenOptions::new()
            .read(true)
            .follow(FollowSymlinks::No)
            .nonblock(true),
    )?;
    identity::regular(&file)?;
    let file = file.into_std();
    hedit_core::private_io::verify(&file)?;
    Ok(file)
}
pub(super) fn new_file(directory: &Dir, name: &str) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = directory.open_with(name, &options)?;
    identity::regular(&file)?;
    let file = file.into_std();
    hedit_core::private_io::verify(&file)?;
    Ok(file)
}
fn decode<T: for<'de> Deserialize<'de>>(directory: &Dir, name: &str) -> io::Result<T> {
    let file = read(directory, name)?;
    let bytes = hedit_core::file_io::read_bounded(file, MAX_RECORD_BYTES)?;
    serde_json::from_slice(&bytes).map_err(|_| invalid())
}
fn write<T: Serialize>(directory: &Dir, name: &str, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|_| invalid())?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(invalid());
    }
    let mut file = new_file(directory, name)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    native::sync_dir(directory)
}
pub(super) fn prepare_index(directory: &Dir) -> io::Result<()> {
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
    let file = directory.open_with(LOCK, &options)?;
    identity::regular(&file)?;
    hedit_core::private_io::verify(&file.into_std())?;
    native::sync_dir(directory)
}
impl Store {
    /// The host supplies a dedicated private directory and its canonical path.
    /// Exclude this stable root before writing images; per-import identity
    /// records must not consume the permanent workspace exclusion registry.
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
        prepare_index(&store.directory)?;
        // Import/list/removal validate global admission. Opening a store for
        // one bound execution must not walk every unrelated image.
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
        let file = read(&self.directory, LOCK)?;
        hedit_core::file_io::lock_with_timeout(&file, Duration::from_millis(100))
            .map_err(io::Error::other)?;
        Ok(Lock(file))
    }
    fn child(&self, id: Handle) -> io::Result<Dir> {
        let child = self.directory.open_dir_nofollow(id.to_string())?;
        hedit_core::private_io::verify(&child.try_clone()?.into_std_file())?;
        Ok(child)
    }
    fn records_unlocked(&self) -> io::Result<Vec<Record>> {
        let mut records = Vec::new();
        let mut total = 0u64;
        let mut entries = 0;
        for entry in self.directory.entries()? {
            entries += 1;
            if entries > MAX_IMAGES * 2 + 1 {
                return Err(full());
            }
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_str().ok_or_else(invalid)?;
            if name == LOCK {
                continue;
            }
            if let Some(removed) = name.strip_prefix(".removed-") {
                let id: Handle = serde_json::from_value(serde_json::Value::String(removed.into()))
                    .map_err(|_| invalid())?;
                self.cleanup_removed(name, id)?;
                continue;
            }
            let id: Handle = serde_json::from_value(serde_json::Value::String(name.into()))
                .map_err(|_| invalid())?;
            let directory = self.child(id)?;
            let record: Record = decode(&directory, "reservation.json")?;
            record.binding.validate()?;
            if record.version != 1
                || record.id != id
                || !(OVERHEAD..=super::format::MAX_IMAGE_BYTES + OVERHEAD)
                    .contains(&record.reserved)
            {
                return Err(invalid());
            }
            // An interrupted copy retains its full reservation. No missing or
            // corrupt receipt resets the budget or creates execution authority.
            let mut children = 0;
            for child in directory.entries()? {
                children += 1;
                if children > 3 {
                    return Err(invalid());
                }
                let child = child?;
                let name = child.file_name();
                if !matches!(
                    name.to_str(),
                    Some("reservation.json" | "receipt.json" | "image")
                ) {
                    return Err(invalid());
                }
                let file = read(&directory, name.to_str().ok_or_else(invalid)?)?;
                let limit = if name == "image" {
                    record.reserved - OVERHEAD
                } else {
                    MAX_RECORD_BYTES
                };
                if file.metadata()?.len() > limit {
                    return Err(invalid());
                }
            }
            total = total
                .checked_add(record.reserved)
                .filter(|bytes| *bytes <= MAX_STORE_BYTES)
                .ok_or_else(full)?;
            records.push(record);
        }
        if records.len() > MAX_IMAGES {
            return Err(full());
        }
        Ok(records)
    }
    pub fn records(&self) -> io::Result<Vec<Record>> {
        let _lock = self.lock()?;
        self.records_unlocked()
    }
    /// Caller holds the exclusive extension-policy guard and advances its epoch.
    /// Failed imports intentionally retain admission until explicit host removal.
    pub fn import(
        &self,
        source: Source,
        binding: Binding,
        cancel: &AtomicBool,
    ) -> io::Result<Handle> {
        binding.validate()?;
        let origin = source
            .selected_path()
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| {
                !name.is_empty() && name.len() <= 1024 && !name.chars().any(char::is_control)
            })
            .ok_or_else(invalid)?
            .to_owned();
        let _lock = self.lock()?;
        let records = self.records_unlocked()?;
        let reserved = source.bytes().checked_add(OVERHEAD).ok_or_else(full)?;
        if records.len() >= MAX_IMAGES
            || records.iter().map(|record| record.reserved).sum::<u64>() + reserved
                > MAX_STORE_BYTES
        {
            return Err(full());
        }
        if cancel.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Executable import cancelled.",
            ));
        }
        let id = Handle::fresh()?;
        let directory = native::private_directory(&self.directory, &id.to_string())?;
        let record = Record {
            version: 1,
            id,
            binding: binding.clone(),
            reserved,
        };
        // Persist admission before the first executable/trust byte is copied.
        write(&directory, "reservation.json", &record)?;
        native::sync_dir(&self.directory)?;
        let mut file = new_file(&directory, "image")?;
        let path = self.path.join(id.to_string()).join("image");
        let copied = source.copy_to(&mut file, &path, cancel)?;
        self.current()?;
        let receipt = Receipt {
            version: 1,
            id,
            binding,
            bytes: copied.bytes,
            sha256: copied.sha256,
            header: copied.header,
            stamp: identity::regular_descriptor(&file)?.0,
            mark_digest: super::trust::Marks::read(&file, &path)?.fingerprint(),
            origin,
        };
        write(&directory, "receipt.json", &receipt)?;
        Ok(id)
    }
    /// Revalidate bytes and ownership for each execution; a handle is no grant
    /// for a different command, source approval, profile or argument recipe.
    fn pin(&self, id: Handle, binding: &Binding) -> io::Result<Pinned> {
        binding.validate()?;
        let directory = self.child(id)?;
        let record: Record = decode(&directory, "reservation.json")?;
        record.binding.validate()?;
        if record.version != 1
            || record.id != id
            || &record.binding != binding
            || !(OVERHEAD..=super::format::MAX_IMAGE_BYTES + OVERHEAD).contains(&record.reserved)
        {
            return Err(invalid());
        }
        let receipt: Receipt = decode(&directory, "receipt.json")?;
        if receipt.version != 1
            || receipt.id != id
            || &receipt.binding != binding
            || receipt.bytes.checked_add(OVERHEAD) != Some(record.reserved)
            || !(32..=super::format::MAX_IMAGE_BYTES).contains(&receipt.bytes)
            || receipt.stamp.bytes != receipt.bytes
            || !digest(&receipt.sha256)
            || !receipt.header.supports_host()
            || receipt.origin.is_empty()
            || receipt.origin.len() > 1024
            || receipt.origin.chars().any(char::is_control)
        {
            return Err(invalid());
        }
        let file = read(&directory, "image")?;
        let path = self.path.join(id.to_string()).join("image");
        let marks = super::trust::Marks::read(&file, &path)?;
        if identity::regular_descriptor(&file)?.0 != receipt.stamp
            || marks.fingerprint() != receipt.mark_digest
        {
            return Err(invalid());
        }
        Ok(Pinned {
            receipt,
            file,
            path,
            marks,
        })
    }
    /// Metadata-only setup avoids hashing every image on every rotated callback.
    /// Executions still verify the full content digest immediately before use.
    pub fn info(&self, id: Handle, binding: &Binding) -> io::Result<Info> {
        let _lock = self.lock()?;
        let receipt = self.pin(id, binding)?.receipt;
        Ok(Info {
            origin: receipt.origin,
            bytes: receipt.bytes,
            sha256: receipt.sha256,
            header: receipt.header,
        })
    }
    pub fn resolve(&self, id: Handle, binding: &Binding, cancel: &AtomicBool) -> io::Result<Image> {
        let _lock = self.lock()?;
        let Pinned {
            receipt,
            mut file,
            path,
            marks,
        } = self.pin(id, binding)?;
        let mut hash = Sha256::new();
        let mut bytes = 0u64;
        let mut buffer = [0; 64 * 1024];
        loop {
            if cancel.load(Ordering::Acquire) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Executable verification cancelled.",
                ));
            }
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            bytes = bytes
                .checked_add(count as u64)
                .filter(|bytes| *bytes <= receipt.bytes)
                .ok_or_else(invalid)?;
            hash.update(&buffer[..count]);
        }
        if bytes != receipt.bytes
            || hex(&hash.finalize()) != receipt.sha256
            || identity::regular_descriptor(&file)?.0 != receipt.stamp
        {
            return Err(invalid());
        }
        file.seek(SeekFrom::Start(0))?;
        if super::format::inspect(&mut file, bytes)? != receipt.header {
            return Err(invalid());
        }
        file.seek(SeekFrom::Start(0))?;
        Ok(Image {
            source: Source::verified(file, path, receipt.stamp, receipt.header.clone(), marks),
            bytes,
            sha256: receipt.sha256,
            header: receipt.header,
        })
    }
    /// Explicit host cleanup of imports not referenced by any current approval.
    /// Caller holds the policy writer and has cancelled active hosts first.
    pub fn remove_unused(
        &self,
        referenced: &std::collections::BTreeSet<Handle>,
    ) -> io::Result<usize> {
        let candidates = self.records()?;
        let mut removed = 0;
        for record in candidates {
            if !referenced.contains(&record.id) {
                self.remove(record.id, &record.binding)?;
                removed += 1;
            }
        }
        Ok(removed)
    }
    /// Explicit host cleanup for this exact owner's imported image. Revoke its
    /// grants and terminate users before calling under the policy writer.
    pub fn remove(&self, id: Handle, binding: &Binding) -> io::Result<()> {
        binding.validate()?;
        let _lock = self.lock()?;
        let records = self.records_unlocked()?;
        if !records
            .iter()
            .any(|record| record.id == id && &record.binding == binding)
        {
            return Err(invalid());
        }
        let removed = format!(".removed-{id}");
        self.directory
            .rename(id.to_string(), &self.directory, &removed)?;
        native::sync_dir(&self.directory)?;
        self.cleanup_removed(&removed, id)
    }
    fn cleanup_removed(&self, name: &str, id: Handle) -> io::Result<()> {
        let directory = self.directory.open_dir_nofollow(name)?;
        hedit_core::private_io::verify(&directory.try_clone()?.into_std_file())?;
        let mut names = std::collections::BTreeSet::new();
        for entry in directory.entries()? {
            let name = entry?.file_name().into_string().map_err(|_| invalid())?;
            if !matches!(name.as_str(), "reservation.json" | "receipt.json" | "image")
                || !names.insert(name.clone())
            {
                return Err(invalid());
            }
            let file = read(&directory, &name)?;
            let limit = if name == "image" {
                super::format::MAX_IMAGE_BYTES
            } else {
                MAX_RECORD_BYTES
            };
            if file.metadata()?.len() > limit {
                return Err(invalid());
            }
        }
        if names.contains("reservation.json") {
            let record: Record = decode(&directory, "reservation.json")?;
            record.binding.validate()?;
            if record.id != id
                || record.version != 1
                || !(OVERHEAD..=super::format::MAX_IMAGE_BYTES + OVERHEAD)
                    .contains(&record.reserved)
            {
                return Err(invalid());
            }
        } else if !names.is_empty() {
            return Err(invalid());
        }
        // Keep admission until all executable/trust bytes are gone. The durable
        // tombstone allows restart after any unlink without losing quota state.
        for file in ["receipt.json", "image", "reservation.json"] {
            if names.contains(file) {
                directory.remove_file(file)?;
            }
        }
        native::sync_dir(&directory)?;
        drop(directory);
        self.directory.remove_dir(name)?;
        native::sync_dir(&self.directory)
    }
}

#[cfg(test)]
mod tests;
