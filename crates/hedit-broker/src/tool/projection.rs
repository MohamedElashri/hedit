//! A complete bounded projection of the explicitly approved workspace surface.
use crate::{
    resources::{Reservation, Resources},
    workspace::{Budget, Snapshot},
};
use std::io;
pub(super) const MAX_PROJECT_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MAX_PATH_BYTES: usize = 64 * 1024;
pub struct Projection {
    pub(super) files: Vec<(Snapshot, Reservation)>,
    bytes: usize,
}
fn full() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "The approved workspace cannot be projected completely within native tool limits.",
    )
}
impl Projection {
    pub fn empty() -> Self {
        Self {
            files: Vec::new(),
            bytes: 0,
        }
    }
    /// Use only the normal approved workspace surface: safe visible regular
    /// UTF-8 files, no aliases, hidden paths, symlinks or protected storage.
    /// Refuse truncation, binary/oversized/unreadable files and aggregate limits;
    /// never give a language server a silently incomplete listing.
    pub fn workspace(resources: &Resources, budget: &mut Budget<'_>) -> io::Result<Self> {
        let root = resources.root(resources.scope())?;
        let listing = root.discover("", budget)?;
        if listing.truncated {
            return Err(full());
        }
        let mut projection = Self::empty();
        let mut paths = 0usize;
        for entry in listing.entries.into_iter().filter(|entry| !entry.directory) {
            budget.check()?;
            paths = paths
                .checked_add(entry.relative.len())
                .filter(|bytes| *bytes <= MAX_PATH_BYTES)
                .ok_or_else(full)?;
            // Preflight avoids allocating an oversized file even when this
            // caller supplies a larger general broker budget.
            let remaining = MAX_PROJECT_BYTES.saturating_sub(projection.bytes);
            if entry.bytes > remaining as u64 {
                return Err(full());
            }
            let snapshot = root.read(&entry.relative, budget)?;
            projection.bytes = projection
                .bytes
                .checked_add(snapshot.text.len())
                .filter(|bytes| *bytes <= MAX_PROJECT_BYTES)
                .ok_or_else(full)?;
            let memory = resources.reserve(
                snapshot.text.capacity()
                    + snapshot.relative.capacity()
                    + std::mem::size_of::<Snapshot>(),
            )?;
            projection.files.push((snapshot, memory));
        }
        root.current()?;
        Ok(projection)
    }
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    pub fn len(&self) -> usize {
        self.files.len()
    }
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{Policy, Protected, Right, Root};
    use std::{sync::atomic::AtomicBool, time::Duration};
    fn resources(path: &std::path::Path, rights: &[Right]) -> Resources {
        Resources::new(
            Root::open_selected(
                path,
                Policy::new(rights.iter().copied()).unwrap(),
                Protected::new([]).unwrap(),
            )
            .unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn projection_retains_exact_unicode_snapshots_with_explicit_omissions() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("visible.txt"), "café / 東京").unwrap();
        std::fs::write(dir.path().join(".secret"), "hidden").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("visible.txt", dir.path().join("alias.txt")).unwrap();
        let resources = resources(dir.path(), &[Right::Discover, Right::Read]);
        let cancel = AtomicBool::new(false);
        let mut budget = Budget::new(&cancel, Duration::from_secs(10), MAX_PROJECT_BYTES);
        let projection = Projection::workspace(&resources, &mut budget).unwrap();
        assert_eq!(projection.len(), 1);
        assert_eq!(projection.files[0].0.relative, "visible.txt");
        assert_eq!(projection.files[0].0.text, "café / 東京");
        std::fs::write(dir.path().join("visible.txt"), "later revision").unwrap();
        assert_eq!(projection.files[0].0.text, "café / 東京");
    }
    #[test]
    fn truncation_binary_files_and_missing_rights_do_not_create_partial_projects() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let read_only = resources(dir.path(), &[Right::Read]);
        assert!(Projection::workspace(
            &read_only,
            &mut Budget::new(&cancel, Duration::from_secs(10), MAX_PROJECT_BYTES)
        )
        .is_err());
        std::fs::write(dir.path().join("binary"), [0xff, 0]).unwrap();
        let resources = resources(dir.path(), &[Right::Discover, Right::Read]);
        assert!(Projection::workspace(
            &resources,
            &mut Budget::new(&cancel, Duration::from_secs(10), MAX_PROJECT_BYTES)
        )
        .is_err());
        std::fs::remove_file(dir.path().join("binary")).unwrap();
        for index in 0..crate::workspace::MAX_RESULTS {
            std::fs::write(dir.path().join(format!("file{index}.txt")), "x").unwrap();
        }
        assert!(Projection::workspace(
            &resources,
            &mut Budget::new(&cancel, Duration::from_secs(10), MAX_PROJECT_BYTES)
        )
        .is_err());
    }
}
