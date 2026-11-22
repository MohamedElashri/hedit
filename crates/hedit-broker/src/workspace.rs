//! Scoped files use pinned directories, component walks, and immutable receipts.
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, File, OpenOptions},
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    io::{self, Read},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
pub(crate) mod identity;
pub use identity::{Identity, Stamp};
/// Host preflight uses physical identities to find aliases among open buffers.
/// These Rust helpers confer no worker authority or resource handles.
pub fn local_file_identity(file: &std::fs::File) -> io::Result<Identity> {
    identity::regular(&File::from_std(file.try_clone()?)).map(|stamp| stamp.identity)
}
pub fn local_path_identity(path: &Path) -> io::Result<Identity> {
    local_file_identity(&std::fs::File::open(path)?)
}
pub const MAX_FILE_BYTES: usize = 1024 * 1024;
pub const MAX_SCAN_ENTRIES: usize = 20_000;
pub const MAX_RESULTS: usize = 512;
pub const MAX_PATH_BYTES: usize = 4096;
fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Workspace resource is unavailable or outside its grant.",
    )
}
fn limit() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Workspace operation exceeds its budget.",
    )
}
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize, serde::Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Right {
    Discover,
    Read,
    Edit,
    Watch,
}
#[derive(Clone)]
pub struct Policy {
    rights: BTreeSet<Right>,
}
impl Policy {
    pub fn new(rights: impl IntoIterator<Item = Right>) -> io::Result<Self> {
        let rights: BTreeSet<_> = rights.into_iter().collect();
        if rights.is_empty() || rights.contains(&Right::Edit) && !rights.contains(&Right::Read) {
            return Err(denied());
        }
        Ok(Self { rights })
    }
    pub fn permits(&self, right: Right) -> bool {
        self.rights.contains(&right)
    }
    fn require(&self, right: Right) -> io::Result<()> {
        if self.permits(right) {
            Ok(())
        } else {
            Err(denied())
        }
    }
}
/// Host-only budget shared by every chunk/operation of a logical request chain.
pub struct Budget<'a> {
    cancelled: &'a AtomicBool,
    deadline: Instant,
    bytes: usize,
    entries: usize,
}
impl<'a> Budget<'a> {
    pub fn new(cancelled: &'a AtomicBool, duration: Duration, bytes: usize) -> Self {
        Self {
            cancelled,
            deadline: Instant::now() + duration.min(Duration::from_secs(10)),
            bytes: bytes.min(64 * 1024 * 1024),
            entries: MAX_SCAN_ENTRIES,
        }
    }
    pub(crate) fn remaining_bytes(&self) -> usize {
        self.bytes
    }
    pub fn check(&self) -> io::Result<()> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Workspace operation cancelled.",
            ));
        }
        if Instant::now() >= self.deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Workspace operation timed out.",
            ));
        }
        Ok(())
    }
    pub(crate) fn charge(&mut self, bytes: usize) -> io::Result<()> {
        self.check()?;
        if bytes > self.bytes {
            self.bytes = 0;
            return Err(limit());
        }
        self.bytes -= bytes;
        Ok(())
    }
    fn entry(&mut self) -> io::Result<()> {
        self.check()?;
        self.entries = self.entries.checked_sub(1).ok_or_else(limit)?;
        Ok(())
    }
}
mod protected;
pub use protected::Protected;
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub relative: String,
    pub stamp: Stamp,
    pub digest: [u8; 32],
    pub text: String,
}
#[derive(Clone, Debug)]
pub struct Entry {
    pub relative: String,
    pub directory: bool,
    pub bytes: u64,
}
#[derive(Debug)]
pub struct Listing {
    pub entries: Vec<Entry>,
    pub truncated: bool,
}
/// The selected object and canonical locator stay private to the trusted host.
pub struct Root {
    directory: Dir,
    locator: PathBuf,
    identity: Identity,
    policy: Policy,
    protected: Protected,
}
fn component(name: &str) -> bool {
    let base = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    !name.is_empty()
        && name.len() <= 255
        && !name.starts_with('.')
        && !name.ends_with([' ', '.'])
        && !name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '\0'))
        && !matches!(
            base.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" | "CLOCK$"
        )
        && !["COM", "LPT"].iter().any(|prefix| {
            base.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        })
}
fn relative(path: &str) -> io::Result<Vec<&str>> {
    if path.len() > MAX_PATH_BYTES || path.is_empty() {
        return Err(denied());
    }
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() > 32 || !parts.iter().all(|part| component(part)) {
        return Err(denied());
    }
    Ok(parts)
}
impl Root {
    pub fn open_selected(path: &Path, policy: Policy, protected: Protected) -> io::Result<Self> {
        let locator = path.canonicalize()?;
        let mut components = locator.components();
        let first = components.next().ok_or_else(denied)?;
        let mut anchor = PathBuf::new();
        anchor.push(first.as_os_str());
        if matches!(first, Component::Prefix(_)) {
            let root = components.next().ok_or_else(denied)?;
            if root != Component::RootDir {
                return Err(denied());
            }
            anchor.push(root.as_os_str());
        } else if first != Component::RootDir {
            return Err(denied());
        }
        let mut directory = Dir::open_ambient_dir(anchor, ambient_authority())?;
        if protected.contains(&identity::directory(&directory)?) {
            return Err(denied());
        }
        for part in components {
            let Component::Normal(name) = part else {
                return Err(denied());
            };
            directory = directory.open_dir_nofollow(name)?;
            if protected.contains(&identity::directory(&directory)?) {
                return Err(denied());
            }
        }
        let identity = identity::directory(&directory)?;
        Ok(Self {
            directory,
            locator,
            identity,
            policy,
            protected,
        })
    }
    pub fn identity(&self) -> Identity {
        self.identity
    }
    pub(crate) fn protected(&self) -> &Protected {
        &self.protected
    }
    pub(crate) fn directory(&self) -> &Dir {
        &self.directory
    }
    pub fn locator(&self) -> &Path {
        &self.locator
    }
    pub(crate) fn require(&self, right: Right) -> io::Result<()> {
        self.policy.require(right)
    }
    pub(crate) fn current(&self) -> io::Result<()> {
        if self.protected.contains(&self.identity) {
            return Err(denied());
        }
        let current = Dir::open_ambient_dir(&self.locator, ambient_authority())?;
        if identity::directory(&current)? != self.identity {
            return Err(denied());
        }
        Ok(())
    }
    pub(crate) fn parent(&self, path: &str) -> io::Result<(Dir, String)> {
        self.current()?;
        let parts = relative(path)?;
        let mut directory = self.directory.try_clone()?;
        for part in &parts[..parts.len() - 1] {
            directory = directory.open_dir_nofollow(part)?;
            if self.protected.contains(&identity::directory(&directory)?) {
                return Err(denied());
            }
        }
        Ok((directory, parts.last().unwrap().to_string()))
    }
    pub(crate) fn file(&self, path: &str) -> io::Result<File> {
        let (parent, name) = self.parent(path)?;
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No).nonblock(true);
        let file = parent.open_with(name, &options)?;
        let stamp = identity::regular(&file)?;
        if self.protected.contains(&stamp.identity) {
            return Err(denied());
        }
        Ok(file)
    }
    pub fn read(&self, path: &str, budget: &mut Budget<'_>) -> io::Result<Snapshot> {
        self.policy.require(Right::Read)?;
        budget.check()?;
        let file = self.file(path)?;
        let before = identity::regular(&file)?;
        if before.bytes > MAX_FILE_BYTES as u64 {
            return Err(limit());
        }
        if before.bytes > budget.bytes as u64 {
            budget.bytes = 0;
            return Err(limit());
        }
        let mut bytes = Vec::with_capacity(before.bytes as usize);
        let mut reader =
            (&file).take((MAX_FILE_BYTES + 1).min(budget.bytes.saturating_add(1)) as u64);
        let mut chunk = [0u8; 8192];
        loop {
            budget.check()?;
            let read = reader.read(&mut chunk)?;
            if read == 0 {
                break;
            }
            budget.charge(read)?;
            if bytes.len().saturating_add(read) > before.bytes as usize {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Workspace file changed while reading.",
                ));
            }
            bytes.extend_from_slice(&chunk[..read]);
        }
        budget.check()?;
        let after = identity::regular(&file)?;
        if before != after
            || identity::regular(&self.file(path)?)? != before
            || bytes.len() != before.bytes as usize
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Workspace file changed while reading.",
            ));
        }
        let digest = Sha256::digest(&bytes).into();
        let text = String::from_utf8(bytes).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "Workspace text must be UTF-8.")
        })?;
        if text.contains('\0') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Workspace text contains NUL bytes.",
            ));
        }
        Ok(Snapshot {
            relative: path.into(),
            stamp: before,
            digest,
            text,
        })
    }
    pub fn discover(&self, query: &str, budget: &mut Budget<'_>) -> io::Result<Listing> {
        self.enumerate(query, Right::Discover, budget)
    }
    pub(crate) fn enumerate(
        &self,
        query: &str,
        right: Right,
        budget: &mut Budget<'_>,
    ) -> io::Result<Listing> {
        self.policy.require(right)?;
        self.current()?;
        if query.len() > 128 || query.chars().any(char::is_control) {
            return Err(denied());
        }
        // Keep one iterator per active ancestor instead of opening every
        // queued sibling directory. Wide trees must not exhaust the editor's FDs.
        let mut pending = vec![(
            String::new(),
            self.directory.entries()?,
            self.identity,
            0usize,
        )];
        let mut entries: Vec<(Entry, Identity)> = Vec::new();
        let mut truncated = false;
        while let Some((prefix, iterator, parent_id, depth)) = pending.last_mut() {
            budget.check()?;
            let Some(entry) = iterator.next() else {
                pending.pop();
                continue;
            };
            match budget.entry() {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                    truncated = true;
                    break;
                }
                Err(error) => return Err(error),
            }
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str().filter(|name| component(name)) else {
                continue;
            };
            let path = if prefix.is_empty() {
                name.into()
            } else {
                format!("{prefix}/{name}")
            };
            if path.len() > MAX_PATH_BYTES {
                continue;
            }
            let parent_id = *parent_id;
            let depth = *depth;
            let (parent, leaf) = self.parent(&path)?;
            if identity::directory(&parent)? != parent_id {
                return Err(denied());
            }
            let metadata = parent.symlink_metadata(&leaf)?;
            if metadata.is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                let Ok(child) = parent.open_dir_nofollow(&leaf) else {
                    continue;
                };
                let child_id = identity::directory(&child)?;
                if self.protected.contains(&child_id) {
                    continue;
                }
                if path.contains(query) {
                    entries.push((
                        Entry {
                            relative: path.clone(),
                            directory: true,
                            bytes: 0,
                        },
                        parent_id,
                    ));
                }
                if depth >= 16 {
                    truncated = true;
                } else {
                    pending.push((path, child.entries()?, child_id, depth + 1));
                }
            } else if metadata.is_file() {
                let Ok(file) = self.file(&path) else {
                    continue;
                };
                let stamp = identity::regular(&file)?;
                if path.contains(query) {
                    entries.push((
                        Entry {
                            relative: path,
                            directory: false,
                            bytes: stamp.bytes,
                        },
                        parent_id,
                    ));
                }
            }
            if entries.len() >= MAX_RESULTS {
                truncated = true;
                break;
            }
        }
        // Do not publish names collected through a directory moved out of scope.
        // The final walk is bounded by the result limit and never reads contents.
        for (entry, expected_parent) in &entries {
            budget.check()?;
            let (parent, leaf) = self.parent(&entry.relative)?;
            if identity::directory(&parent)? != *expected_parent {
                return Err(denied());
            }
            if entry.directory {
                let child = parent.open_dir_nofollow(leaf)?;
                if self.protected.contains(&identity::directory(&child)?) {
                    return Err(denied());
                }
            } else {
                self.file(&entry.relative)?;
            }
        }
        entries.sort_by(|a, b| a.0.relative.cmp(&b.0.relative));
        Ok(Listing {
            entries: entries.into_iter().map(|(entry, _)| entry).collect(),
            truncated,
        })
    }
}

#[cfg(test)]
mod tests;
