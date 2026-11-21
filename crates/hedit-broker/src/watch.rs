//! Bounded native notifications; callbacks retain no paths and only coalesce wakeups.
//! Creation/draining/reconfiguration are filesystem work and belong off the UI thread.
use crate::workspace::{Budget, Identity, Right, Root};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
const MAX_DIRECTORIES: usize = 128;
const MAX_WATCHERS: usize = 16;
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
struct Permit;
impl Permit {
    fn take() -> io::Result<Self> {
        ACTIVE
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MAX_WATCHERS).then_some(n + 1)
            })
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    "Extension watcher limit reached.",
                )
            })?;
        Ok(Self)
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Ordering::AcqRel);
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Fingerprint {
    identity: Identity,
    bytes: u64,
    changed: i128,
    modified: i128,
}
struct Scan {
    files: BTreeMap<String, Fingerprint>,
    directories: BTreeSet<PathBuf>,
    truncated: bool,
}
fn scan(root: &Root, budget: &mut Budget<'_>) -> io::Result<Scan> {
    let listing = root.enumerate("", Right::Watch, budget)?;
    let mut scan = Scan {
        files: BTreeMap::new(),
        directories: BTreeSet::from([root.locator().to_owned()]),
        truncated: listing.truncated,
    };
    for entry in listing.entries {
        budget.check()?;
        if entry.directory {
            if scan.directories.len() >= MAX_DIRECTORIES {
                scan.truncated = true;
                continue;
            }
            scan.directories.insert(root.locator().join(entry.relative));
        } else {
            let stamp = match root
                .file(&entry.relative)
                .and_then(|file| crate::workspace::identity::regular(&file))
            {
                Ok(stamp) => stamp,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
                    ) =>
                {
                    continue
                }
                Err(error) => return Err(error),
            };
            scan.files.insert(
                entry.relative,
                Fingerprint {
                    identity: stamp.identity,
                    bytes: stamp.bytes,
                    changed: stamp.changed,
                    modified: stamp.modified,
                },
            );
        }
    }
    Ok(scan)
}
#[derive(Debug)]
pub struct Changes {
    pub paths: Vec<String>,
    pub truncated: bool,
}
pub struct Watch {
    native: RecommendedWatcher,
    root: Identity,
    dirty: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    last: Instant,
    files: BTreeMap<String, Fingerprint>,
    directories: BTreeSet<PathBuf>,
    truncated: bool,
    _permit: Permit,
}
impl Watch {
    #[cfg(test)]
    pub(crate) fn signal_for_test(&self) {
        self.dirty.store(true, Ordering::Release);
    }
    #[cfg(test)]
    pub(crate) fn clear_for_test(&mut self) {
        // Isolate the cancellation state-machine probe from delayed OS events;
        // the native watcher itself is exercised by the dedicated adapter test.
        self.native = notify::recommended_watcher(|_| {}).unwrap();
        self.dirty.store(false, Ordering::Release);
    }
    pub fn truncated(&self) -> bool {
        self.truncated
    }
    pub fn start(
        root: &Root,
        budget: &mut Budget<'_>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> io::Result<Self> {
        root.require(Right::Watch)?;
        let permit = Permit::take()?;
        let scan = scan(root, budget)?;
        let dirty = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let callback_dirty = dirty.clone();
        let callback_failed = failed.clone();
        let mut native =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if event.is_err() {
                    callback_failed.store(true, Ordering::Release);
                }
                if !callback_dirty.swap(true, Ordering::AcqRel) {
                    wake();
                }
            })
            .map_err(io::Error::other)?;
        for path in &scan.directories {
            native
                .watch(path, RecursiveMode::NonRecursive)
                .map_err(io::Error::other)?;
        }
        Ok(Self {
            native,
            root: root.identity(),
            dirty,
            failed,
            last: Instant::now() - Duration::from_secs(1),
            files: scan.files,
            directories: scan.directories,
            truncated: scan.truncated,
            _permit: permit,
        })
    }
    /// Host may retry after `ready_after` when a coalesced event is waiting.
    pub fn ready_after(&self) -> Option<Duration> {
        self.dirty
            .load(Ordering::Acquire)
            .then(|| Duration::from_millis(500).saturating_sub(self.last.elapsed()))
    }
    pub fn drain(&mut self, root: &Root, budget: &mut Budget<'_>) -> io::Result<Option<Changes>> {
        root.require(Right::Watch)?;
        root.current()?;
        if root.identity() != self.root || self.failed.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Workspace watcher stopped. Review its scope before restarting.",
            ));
        }
        if self
            .ready_after()
            .is_none_or(|remaining| !remaining.is_zero())
        {
            return Ok(None);
        }
        self.dirty.store(false, Ordering::Release);
        self.last = Instant::now();
        let scan = scan(root, budget)?;
        let paths: BTreeSet<_> = self
            .files
            .iter()
            .filter(|(path, stamp)| scan.files.get(*path) != Some(stamp))
            .map(|(path, _)| path.clone())
            .chain(
                scan.files
                    .iter()
                    .filter(|(path, stamp)| self.files.get(*path) != Some(stamp))
                    .map(|(path, _)| path.clone()),
            )
            .collect();
        for path in self.directories.difference(&scan.directories) {
            self.native.unwatch(path).map_err(io::Error::other)?;
        }
        for path in scan.directories.difference(&self.directories) {
            self.native
                .watch(path, RecursiveMode::NonRecursive)
                .map_err(io::Error::other)?;
        }
        self.files = scan.files;
        self.directories = scan.directories;
        self.truncated = scan.truncated;
        let truncated = self.truncated || paths.len() > 256;
        Ok(Some(Changes {
            paths: paths.into_iter().take(256).collect(),
            truncated,
        }))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{Policy, Protected};
    #[test]
    fn events_coalesce_and_watch_does_not_imply_read_or_discovery_authority() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("file.txt"), "before").unwrap();
        let root = Root::open_selected(
            dir.path(),
            Policy::new([Right::Watch]).unwrap(),
            Protected::new([]).unwrap(),
        )
        .unwrap();
        let cancel = AtomicBool::new(false);
        let mut budget = Budget::new(&cancel, Duration::from_secs(5), 4096);
        let mut watch = Watch::start(&root, &mut budget, Arc::new(|| {})).unwrap();
        assert!(root.read("file.txt", &mut budget).is_err());
        assert!(root.discover("", &mut budget).is_err());
        std::fs::write(dir.path().join("file.txt"), "changed").unwrap();
        // Deterministically exercise coalescing independent of OS notification latency.
        for _ in 0..100_000 {
            watch.dirty.store(true, Ordering::Release);
        }
        let changes = watch.drain(&root, &mut budget).unwrap().unwrap();
        assert_eq!(changes.paths, ["file.txt"]);
        watch.dirty.store(true, Ordering::Release);
        assert!(watch.drain(&root, &mut budget).unwrap().is_none());
        watch.failed.store(true, Ordering::Release);
        assert!(watch.drain(&root, &mut budget).is_err());
    }
}
