use eframe::egui;
use hedit_remote::ssh_config::WatchScope;
use notify::{event::MetadataKind, event::ModifyKind, Event, EventKind, Watcher};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

const MAX_PATHS: usize = 512;

#[derive(Default)]
struct Paths {
    entries: BTreeSet<PathBuf>,
    children_of: BTreeSet<PathBuf>,
}

impl Paths {
    fn matches(&self, path: &Path) -> bool {
        self.entries.contains(path)
            || path
                .parent()
                .is_some_and(|parent| self.children_of.contains(parent))
    }
}

pub(super) struct HostWatch {
    watcher: notify::RecommendedWatcher,
    paths: Arc<Mutex<Paths>>,
    observed: BTreeSet<(PathBuf, WatchScope)>,
    directories: BTreeSet<PathBuf>,
    incomplete: bool,
}

impl HostWatch {
    pub(super) fn new(ctx: egui::Context, dirty: Arc<AtomicBool>) -> Option<Self> {
        let paths = Arc::new(Mutex::new(Paths::default()));
        let matching = paths.clone();
        let watcher = notify::RecommendedWatcher::new(
            move |event: notify::Result<Event>| {
                let Ok(event) = event else { return };
                if matches!(
                    event.kind,
                    EventKind::Access(_)
                        | EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime))
                ) {
                    return;
                }
                let changed = event.need_rescan() || {
                    let paths = matching.lock().unwrap();
                    event.paths.iter().any(|path| paths.matches(path))
                };
                if changed && !dirty.swap(true, Ordering::AcqRel) {
                    ctx.request_repaint();
                }
            },
            notify::Config::default().with_follow_symlinks(false),
        )
        .ok()?;
        Some(Self {
            watcher,
            paths,
            observed: BTreeSet::new(),
            directories: BTreeSet::new(),
            incomplete: false,
        })
    }

    pub(super) fn incomplete(&self) -> bool {
        self.incomplete
    }

    pub(super) fn observe(&mut self, path: &Path, scope: WatchScope) {
        if self.observed.len() >= MAX_PATHS {
            self.incomplete = true;
            return;
        }
        if !self.observed.insert((path.to_owned(), scope)) {
            return;
        }
        if scope == WatchScope::Children {
            if let Ok(directory) = fs::canonicalize(path) {
                if directory.is_dir() {
                    self.paths
                        .lock()
                        .unwrap()
                        .children_of
                        .insert(directory.clone());
                    self.watch(directory);
                }
            }
        }
        // Watch the containing directory so replacing a config atomically
        // keeps working. For a missing path, watch its first missing component.
        for ancestor in path.ancestors().skip(1) {
            let Ok(directory) = fs::canonicalize(ancestor) else {
                continue;
            };
            if !directory.is_dir() {
                continue;
            }
            if let Some(name) = path
                .strip_prefix(ancestor)
                .ok()
                .and_then(|tail| tail.iter().next())
            {
                self.paths
                    .lock()
                    .unwrap()
                    .entries
                    .insert(directory.join(name));
                self.watch(directory);
            }
            break;
        }
    }

    fn watch(&mut self, directory: PathBuf) {
        if self.directories.insert(directory.clone()) {
            self.paths.lock().unwrap().entries.insert(directory.clone());
            self.incomplete |= self
                .watcher
                .watch(&directory, notify::RecursiveMode::NonRecursive)
                .is_err();
        }
    }
}
