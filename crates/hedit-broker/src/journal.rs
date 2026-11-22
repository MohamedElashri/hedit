//! Durable per-file recovery. No claim of an atomic multi-file transaction.
use crate::{
    plan::{conflict, Plan},
    resources::Resources,
    workspace::{self, Budget, Identity},
    Handle,
};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, File};
use sha2::{Digest, Sha256};
use std::io::{self, Read, Write};
mod incomplete;
mod inspect;
pub use inspect::{Current, FileView, View};
pub(crate) mod native;
const MAX_LEDGER: usize = 128 * 1024;
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Item {
    relative: String,
    old: workspace::Stamp,
    old_hash: [u8; 32],
    new: workspace::Stamp,
    new_hash: [u8; 32],
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    version: u32,
    plan: Handle,
    root: Identity,
    stage: Identity,
    files: Vec<Item>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner: Option<Owner>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Owner {
    pub command: String,
    pub digest: String,
}
impl Owner {
    pub fn validate(&self) -> io::Result<()> {
        if !hedit_plugin::options::valid_command_id(&self.command)
            || self.digest.len() != 64
            || !self
                .digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(mismatch());
        }
        Ok(())
    }
}
/// Host-owned object: never construct a stage name or recovery authority from Lua.
pub struct Journal {
    name: String,
    directory: Dir,
    ledger: Ledger,
    lease: crate::recovery_store::Lease,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Prepared,
    Applied,
    Restored,
    Conflict,
}
#[derive(Debug)]
pub struct Receipt {
    pub relative: String,
    pub state: State,
}
fn mismatch() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Workspace recovery data does not match this scope.",
    )
}
fn digest(file: &File, budget: &mut Budget<'_>) -> io::Result<(workspace::Stamp, [u8; 32])> {
    let before = workspace::identity::regular(file)?;
    if before.bytes > workspace::MAX_FILE_BYTES as u64 {
        return Err(mismatch());
    }
    let mut reader = file;
    let mut hash = Sha256::new();
    let mut chunk = [0u8; 8192];
    let mut bytes = 0;
    loop {
        budget.check()?;
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        budget.charge(n)?;
        bytes += n;
        if bytes > workspace::MAX_FILE_BYTES {
            return Err(mismatch());
        }
        hash.update(&chunk[..n]);
    }
    if workspace::identity::regular(file)? != before || bytes as u64 != before.bytes {
        return Err(conflict());
    }
    Ok((before, hash.finalize().into()))
}
fn same(
    actual: &workspace::Stamp,
    hash: [u8; 32],
    expected: &workspace::Stamp,
    expected_hash: [u8; 32],
) -> bool {
    // Renames legitimately change ctime; identity, data and modification time must survive.
    actual.identity == expected.identity
        && actual.bytes == expected.bytes
        && actual.modified == expected.modified
        && hash == expected_hash
}
/// Compensate ordinary errors while authority is still held. Process death or a
/// competing creator can still require explicit recovery; never overwrite it.
struct Capture<'a> {
    stage: &'a Dir,
    backup: &'a str,
    parent: &'a Dir,
    name: &'a str,
    file: File,
    published: bool,
}
impl Capture<'_> {
    fn digest(&self, budget: &mut Budget<'_>) -> io::Result<(workspace::Stamp, [u8; 32])> {
        // Unix renames select a directory entry, not the opened file. A
        // racing swap may have captured a different entry; inspect that one.
        #[cfg(unix)]
        let reopened = native::writable(self.stage, self.backup)?;
        #[cfg(unix)]
        let file = &reopened;
        #[cfg(windows)]
        let file = &self.file;
        let result = digest(file, budget)?;
        file.sync_all()?;
        if workspace::identity::regular(file)? != result.0 {
            return Err(conflict());
        }
        Ok(result)
    }
}
impl Drop for Capture<'_> {
    fn drop(&mut self) {
        if !self.published {
            let _ = native::rename(
                self.stage,
                self.backup,
                Some(&self.file),
                self.parent,
                self.name,
            );
            let _ = native::sync_dir(self.parent);
            let _ = native::sync_dir(self.stage);
        }
    }
}
impl Journal {
    /// Prepare all replacements and a flushed immutable ledger before effects.
    /// The host must first freeze/reject affected buffers and hold authority.
    pub fn prepare(
        resources: &Resources,
        plan: &Plan,
        budget: &mut Budget<'_>,
        store: &crate::recovery_store::RecoveryStore,
    ) -> io::Result<Self> {
        Self::prepare_inner(resources, plan, budget, store, None)
    }
    pub fn prepare_owned(
        resources: &Resources,
        plan: &Plan,
        budget: &mut Budget<'_>,
        store: &crate::recovery_store::RecoveryStore,
        owner: &Owner,
    ) -> io::Result<Self> {
        owner.validate()?;
        Self::prepare_inner(resources, plan, budget, store, Some(owner.clone()))
    }
    fn prepare_inner(
        resources: &Resources,
        plan: &Plan,
        budget: &mut Budget<'_>,
        store: &crate::recovery_store::RecoveryStore,
        owner: Option<Owner>,
    ) -> io::Result<Self> {
        if !plan.live() {
            return Err(conflict());
        }
        let root = resources.root(plan.scope())?;
        root.require(workspace::Right::Edit)?;
        for proposed in plan.files() {
            let current = root.read(proposed.relative(), budget)?;
            let old = proposed.snapshot();
            if current.stamp != old.stamp || current.digest != old.digest {
                return Err(conflict());
            }
        }
        if !root.protected().durable() {
            return Err(mismatch());
        }
        let payload = plan
            .files()
            .iter()
            .map(|file| (file.before().len() + file.after().len()) as u64)
            .sum();
        let mut lease = store.reserve(root.identity(), payload)?;
        let name = lease.record().stage_name.clone();
        let directory = native::private_directory(root.directory(), &name)?;
        let stage = root.protected().insert(directory.try_clone()?)?;
        store.bind_stage(&mut lease, stage)?;
        let mut files = Vec::new();
        for (index, proposed) in plan.files().iter().enumerate() {
            budget.check()?;
            let (parent, name) = root.parent(proposed.relative())?;
            let original = native::writable(&parent, &name)?;
            let stamp = workspace::identity::regular(&original)?;
            if stamp != proposed.snapshot().stamp {
                return Err(conflict());
            }
            let mut new = native::new_file(&directory, &format!("{index}.new"))?;
            new.write_all(proposed.after().as_bytes())?;
            native::copy_permissions(&original, &new)?;
            new.sync_all()?;
            let new_stamp = workspace::identity::regular(&new)?;
            files.push(Item {
                relative: proposed.relative().into(),
                old: proposed.snapshot().stamp.clone(),
                old_hash: proposed.snapshot().digest,
                new: new_stamp,
                new_hash: Sha256::digest(proposed.after().as_bytes()).into(),
            });
        }
        let ledger = Ledger {
            version: if owner.is_some() { 2 } else { 1 },
            plan: plan.id(),
            root: root.identity(),
            stage,
            files,
            owner,
        };
        let bytes = serde_json::to_vec(&ledger).map_err(|_| mismatch())?;
        if bytes.len() > MAX_LEDGER {
            return Err(mismatch());
        }
        let mut file = native::new_file(&directory, "prepared.json")?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        native::sync_dir(&directory)?;
        native::sync_dir(root.directory())?;
        Ok(Self {
            name,
            directory,
            ledger,
            lease,
        })
    }
    pub(crate) fn plan_id(&self) -> Handle {
        self.ledger.plan
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Called explicitly by the host after selecting recovery, never on startup.
    /// Register private stage identities before any workspace session can read them.
    pub fn open(
        root: &workspace::Root,
        name: &str,
        store: &crate::recovery_store::RecoveryStore,
    ) -> io::Result<Self> {
        if !name.starts_with(".hedit-transaction-")
            || name.len() != 19 + 32
            || !name[19..]
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(mismatch());
        }
        let id: Handle = serde_json::from_value(serde_json::Value::String(name[19..].into()))
            .map_err(|_| mismatch())?;
        let lease = store.lease(id, root.identity())?;
        if lease.record().stage_name != name {
            return Err(mismatch());
        }
        root.current()?;
        let directory = root.directory().open_dir_nofollow(name)?;
        hedit_core::private_io::verify(&directory.try_clone()?.into_std_file())?;
        let stage = root.protected().insert(directory.try_clone()?)?;
        if lease.record().stage != Some(stage) {
            return Err(mismatch());
        }
        let file = directory.open_with(
            "prepared.json",
            cap_std::fs::OpenOptions::new()
                .read(true)
                .follow(FollowSymlinks::No)
                .nonblock(true),
        )?;
        workspace::identity::regular(&file)?;
        let bytes = hedit_core::file_io::read_bounded(file.into_std(), MAX_LEDGER as u64)?;
        let ledger: Ledger = serde_json::from_slice(&bytes).map_err(|_| mismatch())?;
        if !matches!(
            (ledger.version, ledger.owner.is_some()),
            (1, false) | (2, true)
        ) || ledger.root != root.identity()
            || ledger.stage != stage
            || ledger.files.is_empty()
            || ledger.files.len() > crate::plan::MAX_PLAN_FILES
        {
            return Err(mismatch());
        }
        if let Some(owner) = &ledger.owner {
            owner.validate()?;
        }
        let mut paths = std::collections::BTreeSet::new();
        let mut identities = std::collections::BTreeSet::new();
        let mut payload = 0u64;
        for item in &ledger.files {
            if !paths.insert(&item.relative) {
                return Err(mismatch());
            }
            for stamp in [&item.old, &item.new] {
                if stamp.bytes > workspace::MAX_FILE_BYTES as u64
                    || stamp.identity.volume != ledger.root.volume
                    || !identities.insert(stamp.identity)
                {
                    return Err(mismatch());
                }
                payload = payload.checked_add(stamp.bytes).ok_or_else(mismatch)?;
            }
        }
        if payload > lease.record().payload_limit() {
            return Err(mismatch());
        }
        // Validate every relative name without reading data or changing files.
        for item in &ledger.files {
            root.parent(&item.relative)?;
        }
        Ok(Self {
            name: name.into(),
            directory,
            ledger,
            lease,
        })
    }
    fn mark(&self, index: usize, state: &str) -> io::Result<()> {
        match native::new_file(&self.directory, &format!("{index}.{state}")) {
            Ok(mut file) => {
                file.write_all(b"1\n")?;
                file.sync_all()?;
                native::sync_dir(&self.directory)
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let file = self.directory.open_with(
                    format!("{index}.{state}"),
                    cap_std::fs::OpenOptions::new()
                        .read(true)
                        .follow(FollowSymlinks::No)
                        .nonblock(true),
                )?;
                workspace::identity::regular(&file)?;
                if hedit_core::file_io::read_bounded(file.into_std(), 2)? != b"1\n" {
                    return Err(mismatch());
                }
                Ok(())
            }
            Err(error) => Err(error),
        }
    }
    /// `authority` must return the current policy guard; it is retained through
    /// each file effect. Cancellation may leave a partial, explicitly recoverable plan.
    pub fn apply<G>(
        &self,
        root: &workspace::Root,
        budget: &mut Budget<'_>,
        mut authority: impl FnMut() -> io::Result<G>,
    ) -> io::Result<Vec<Receipt>> {
        root.require(workspace::Right::Edit)?;
        if root.identity() != self.ledger.root {
            return Err(mismatch());
        }
        let mut receipts = Vec::new();
        for (index, item) in self.ledger.files.iter().enumerate() {
            budget.check()?;
            let _guard = authority()?;
            let (parent, name) = root.parent(&item.relative)?;
            let current = root.read(&item.relative, budget);
            if let Ok(ref current) = current {
                if same(&current.stamp, current.digest, &item.new, item.new_hash) {
                    self.mark(index, "applied")?;
                    receipts.push(Receipt {
                        relative: item.relative.clone(),
                        state: State::Applied,
                    });
                    continue;
                }
            }
            let old_name = format!("{index}.old");
            let new_name = format!("{index}.new");
            let backup = match native::writable(&self.directory, &old_name) {
                Ok(backup) => Some(backup),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
            let captured = if let Some(backup) = backup {
                if !matches!(&current, Err(error) if error.kind() == io::ErrorKind::NotFound) {
                    return Err(conflict());
                }
                backup
            } else {
                let current = current?;
                if !same(&current.stamp, current.digest, &item.old, item.old_hash) {
                    return Err(conflict());
                }
                let original = native::writable(&parent, &name)?;
                if workspace::identity::regular(&original)?.identity != item.old.identity {
                    return Err(conflict());
                }
                self.mark(index, "started")?;
                native::rename(&parent, &name, Some(&original), &self.directory, &old_name)?;
                original
            };
            let mut capture = Capture {
                stage: &self.directory,
                backup: &old_name,
                parent: &parent,
                name: &name,
                file: captured,
                published: false,
            };
            native::sync_dir(&parent)?;
            native::sync_dir(&self.directory)?;
            let (stamp, hash) = capture.digest(budget)?;
            if !same(&stamp, hash, &item.old, item.old_hash) {
                return Err(conflict());
            }
            // Re-resolve the scope after capture. Never publish into a replaced parent.
            let (current_parent, _) = root.parent(&item.relative)?;
            if workspace::identity::directory(&current_parent)?
                != workspace::identity::directory(&parent)?
            {
                return Err(conflict());
            }
            let replacement = native::writable(&self.directory, &new_name)?;
            let (stamp, hash) = digest(&replacement, budget)?;
            if !same(&stamp, hash, &item.new, item.new_hash) {
                return Err(mismatch());
            }
            native::rename(
                &self.directory,
                &new_name,
                Some(&replacement),
                &parent,
                &name,
            )?;
            capture.published = true;
            native::sync_dir(&parent)?;
            // mark flushes this same stage directory after its receipt file;
            // a second stage fsync here cannot add durability.
            self.mark(index, "applied")?;
            receipts.push(Receipt {
                relative: item.relative.clone(),
                state: State::Applied,
            });
        }
        Ok(receipts)
    }
    /// Explicit host-only discard of recovery history. Validate every private
    /// entry before deletion; unknown/tampered contents retain the reservation.
    pub fn discard(
        self,
        root: &workspace::Root,
        store: &crate::recovery_store::RecoveryStore,
        budget: &mut Budget<'_>,
    ) -> io::Result<()> {
        root.require(workspace::Right::Edit)?;
        root.current()?;
        if root.identity() != self.ledger.root {
            return Err(mismatch());
        }
        let named = root.directory().open_dir_nofollow(&self.name)?;
        if workspace::identity::directory(&named)? != self.ledger.stage {
            return Err(mismatch());
        }
        drop(named);
        // A capture interrupted before publication may be the only remaining
        // original. Discard is history cleanup, never permission to delete it.
        for item in &self.ledger.files {
            let current = root.read(&item.relative, budget)?;
            if !same(&current.stamp, current.digest, &item.old, item.old_hash)
                && !same(&current.stamp, current.digest, &item.new, item.new_hash)
            {
                return Err(conflict());
            }
        }
        let mut names = Vec::new();
        for entry in self.directory.entries()? {
            budget.check()?;
            if names.len() > crate::plan::MAX_PLAN_FILES * 6 {
                return Err(mismatch());
            }
            let entry = entry?;
            let name = entry.file_name().into_string().map_err(|_| mismatch())?;
            let file = self.directory.open_with(
                &name,
                cap_std::fs::OpenOptions::new()
                    .read(true)
                    .follow(FollowSymlinks::No)
                    .nonblock(true),
            )?;
            let (stamp, hash) = digest(&file, budget)?;
            let expected = if name == "prepared.json" {
                let bytes = serde_json::to_vec(&self.ledger).map_err(|_| mismatch())?;
                stamp.bytes == bytes.len() as u64
                    && hash == <[u8; 32]>::from(Sha256::digest(&bytes))
            } else {
                let (index, suffix) = name.split_once('.').ok_or_else(mismatch)?;
                let index = index.parse::<usize>().map_err(|_| mismatch())?;
                let item = self.ledger.files.get(index).ok_or_else(mismatch)?;
                if name != format!("{index}.{suffix}") {
                    return Err(mismatch());
                }
                match suffix {
                    "old" => same(&stamp, hash, &item.old, item.old_hash),
                    "new" | "removed" => same(&stamp, hash, &item.new, item.new_hash),
                    "started" | "applied" | "restored" => {
                        stamp.bytes == 2 && hash == <[u8; 32]>::from(Sha256::digest(b"1\n"))
                    }
                    _ => false,
                }
            };
            if !expected {
                return Err(mismatch());
            }
            names.push(name);
        }
        for name in names {
            budget.check()?;
            self.directory.remove_file(name)?;
        }
        native::sync_dir(&self.directory)?;
        root.protected().release_empty(&self.directory)?;
        let Self {
            name,
            directory,
            lease,
            ..
        } = self;
        drop(directory);
        root.directory().remove_dir(name)?;
        native::sync_dir(root.directory())?;
        store.release(lease.record())
    }

    /// Restore explicitly, in reverse order, preserving competing writes. A
    /// changed file stops recovery; originals and journal remain available.
    pub fn restore<G>(
        &self,
        root: &workspace::Root,
        budget: &mut Budget<'_>,
        mut authority: impl FnMut() -> io::Result<G>,
    ) -> io::Result<Vec<Receipt>> {
        root.require(workspace::Right::Edit)?;
        if root.identity() != self.ledger.root {
            return Err(mismatch());
        }
        let mut receipts = Vec::new();
        for (index, item) in self.ledger.files.iter().enumerate().rev() {
            budget.check()?;
            let _guard = authority()?;
            let (parent, name) = root.parent(&item.relative)?;
            let current = root.read(&item.relative, budget);
            if let Ok(ref current) = current {
                if same(&current.stamp, current.digest, &item.old, item.old_hash) {
                    receipts.push(Receipt {
                        relative: item.relative.clone(),
                        state: State::Restored,
                    });
                    continue;
                }
            }
            let old_name = format!("{index}.old");
            let backup = native::writable(&self.directory, &old_name)?;
            let (stamp, hash) = digest(&backup, budget)?;
            if !same(&stamp, hash, &item.old, item.old_hash) {
                return Err(mismatch());
            }
            match current {
                Ok(snapshot) => {
                    if !same(&snapshot.stamp, snapshot.digest, &item.new, item.new_hash) {
                        return Err(conflict());
                    }
                    let current = native::writable(&parent, &name)?;
                    if workspace::identity::regular(&current)?.identity != item.new.identity {
                        return Err(conflict());
                    }
                    native::rename(
                        &parent,
                        &name,
                        Some(&current),
                        &self.directory,
                        &format!("{index}.removed"),
                    )?;
                    drop(current);
                    let captured = native::writable(&self.directory, &format!("{index}.removed"))?;
                    let (stamp, hash) = digest(&captured, budget)?;
                    if !same(&stamp, hash, &item.new, item.new_hash) {
                        let _ = native::rename(
                            &self.directory,
                            &format!("{index}.removed"),
                            Some(&captured),
                            &parent,
                            &name,
                        );
                        return Err(conflict());
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            native::rename(&self.directory, &old_name, Some(&backup), &parent, &name)?;
            native::sync_dir(&parent)?;
            native::sync_dir(&self.directory)?;
            self.mark(index, "restored")?;
            receipts.push(Receipt {
                relative: item.relative.clone(),
                state: State::Restored,
            });
        }
        Ok(receipts)
    }
}
#[cfg(test)]
mod tests;
