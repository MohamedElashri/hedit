//! Bounded native watches. The UI receives a coalesced signal, never a queue of paths.
use super::*;
use notify::{event::ModifyKind, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{BTreeSet, HashSet};
use std::sync::{atomic::AtomicU64, mpsc::SyncSender, Mutex};

const MAX_DIRECTORIES: usize = 1024;
const MAX_PATH_BYTES: usize = 1024 * 1024;
const MAX_SCAN: Duration = Duration::from_secs(2);
const SETTLE: Duration = Duration::from_millis(180);
const MAX_DELAY: Duration = Duration::from_secs(1);
const RESCAN_DELAY: Duration = Duration::from_secs(1);

#[derive(Clone, Default, PartialEq, Eq)]
struct Configuration {
    roots: Vec<String>,
    files: Vec<String>,
    excludes: Vec<String>,
}
impl Configuration {
    fn matches(&self, app: &HeditApp) -> bool {
        self.excludes == app.settings.folder_exclude_patterns
            && self.roots.len() == app.workspace.folders.len()
            && self
                .roots
                .iter()
                .zip(&app.workspace.folders)
                .all(|(a, b)| *a == b.path)
            && self.files.iter().map(String::as_str).eq(app
                .workspace
                .open
                .iter()
                .filter_map(|b| b.page.info.path.as_deref()))
    }
    fn from_app(app: &HeditApp) -> Self {
        Self {
            roots: app
                .workspace
                .folders
                .iter()
                .map(|f| f.path.clone())
                .collect(),
            files: app
                .workspace
                .open
                .iter()
                .filter_map(|b| b.page.info.path.clone())
                .collect(),
            excludes: app.settings.folder_exclude_patterns.clone(),
        }
    }
    fn normalized(mut self) -> Self {
        for path in self.roots.iter_mut().chain(self.files.iter_mut()) {
            let source = std::path::Path::new(path);
            let canonical = std::fs::canonicalize(source).ok().or_else(|| {
                let parent = std::fs::canonicalize(source.parent()?).ok()?;
                Some(parent.join(source.file_name()?))
            });
            if let Some(canonical) = canonical.and_then(|p| p.to_str().map(str::to_owned)) {
                *path = canonical;
            }
        }
        self
    }
    fn relevant(&self, path: &std::path::Path) -> bool {
        self.files.iter().any(|f| path == std::path::Path::new(f))
            || self
                .roots
                .iter()
                .any(|r| path.starts_with(r) || std::path::Path::new(r).starts_with(path))
    }
}
#[derive(Default)]
struct Signals {
    epoch: AtomicU64,
    events: AtomicU64,
    topology: AtomicBool,
    catalogue: AtomicBool,
    awake: AtomicBool,
    stop: AtomicBool,
}
impl Signals {
    fn changed(&self, ctx: &egui::Context) {
        self.events.fetch_add(1, Ordering::AcqRel);
        if !self.awake.swap(true, Ordering::AcqRel) {
            ctx.request_repaint();
        }
    }
}
#[derive(Default)]
struct Mailbox {
    request: (u64, Configuration),
    status: Option<(u64, String)>,
}
struct Worker {
    tx: SyncSender<()>,
    mailbox: Arc<Mutex<Mailbox>>,
    signals: Arc<Signals>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.signals.stop.store(true, Ordering::Release);
        let _ = self.tx.try_send(());
    }
}
#[derive(Default)]
pub(super) struct State {
    configuration: Option<Configuration>,
    worker: Option<Worker>,
    epoch: u64,
    seen: u64,
    pending: Option<(Instant, Instant)>,
    pub status: String,
    catalogue_changed: bool,
}
impl State {
    pub(super) fn retry(&mut self) {
        self.configuration = None;
    }
    fn poll(&mut self, ctx: &egui::Context, now: Instant) -> bool {
        let Some(worker) = &self.worker else {
            return false;
        };
        if let Ok(mut mailbox) = worker.mailbox.try_lock() {
            if let Some((epoch, status)) = mailbox.status.take() {
                if epoch == self.epoch {
                    self.status = status;
                }
            }
        }
        let events = worker.signals.events.load(Ordering::Acquire);
        if events != self.seen {
            self.seen = events;
            match &mut self.pending {
                Some((_, last)) => *last = now,
                None => self.pending = Some((now, now)),
            }
        }
        let Some((first, last)) = self.pending else {
            return false;
        };
        let due = SETTLE
            .saturating_sub(now.saturating_duration_since(last))
            .min(MAX_DELAY.saturating_sub(now.saturating_duration_since(first)));
        if !due.is_zero() {
            ctx.request_repaint_after(due);
            return false;
        }
        self.pending = None;
        self.catalogue_changed = worker.signals.catalogue.swap(false, Ordering::AcqRel);
        worker.signals.awake.store(false, Ordering::Release);
        if worker.signals.events.load(Ordering::Acquire) != self.seen {
            ctx.request_repaint();
        }
        true
    }
    fn configure(&mut self, ctx: &egui::Context, configuration: Configuration) {
        self.epoch = self.epoch.wrapping_add(1);
        self.pending = None;
        if self.worker.is_none() {
            let signals = Arc::new(Signals::default());
            let mailbox = Arc::new(Mutex::new(Mailbox::default()));
            let (tx, rx) = mpsc::sync_channel(1);
            let (state, messages, wake, signal, callback_tx) = (
                signals.clone(),
                mailbox.clone(),
                ctx.clone(),
                signals.clone(),
                tx.clone(),
            );
            match thread::Builder::new()
                .name("hedit-workspace-watch".into())
                .spawn(move || run(state, messages, wake, rx, callback_tx))
            {
                Ok(_) => {
                    self.worker = Some(Worker {
                        tx,
                        signals: signal,
                        mailbox,
                    })
                }
                Err(error) => {
                    self.status =
                        format!("Automatic refresh unavailable: {error}. Use Refresh folder.");
                    self.configuration = Some(configuration);
                    return;
                }
            }
        }
        let worker = self.worker.as_ref().unwrap();
        worker.signals.epoch.store(self.epoch, Ordering::Release);
        worker
            .mailbox
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .request = (self.epoch, configuration.clone());
        let _ = worker.tx.try_send(());
        self.configuration = Some(configuration);
        self.status = "Starting automatic refresh…".into();
    }
}
fn topology(kind: EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(ModifyKind::Name(_))
            | EventKind::Any
            | EventKind::Other
    )
}
fn interesting(event: &Event, configuration: &Configuration) -> bool {
    !matches!(event.kind, EventKind::Access(_))
        && !matches!(
            event.kind,
            EventKind::Modify(ModifyKind::Metadata(
                notify::event::MetadataKind::AccessTime
            ))
        )
        && (event.paths.is_empty() || event.paths.iter().any(|p| configuration.relevant(p)))
}
struct Plan {
    directories: BTreeSet<PathBuf>,
    limited: bool,
}
fn plan(configuration: &Configuration, current: impl Fn() -> bool) -> Plan {
    let started = Instant::now();
    let mut result = Plan {
        directories: BTreeSet::new(),
        limited: false,
    };
    let mut bytes = 0;
    let mut add = |path: PathBuf, result: &mut Plan| {
        if result.directories.contains(&path) {
            return;
        }
        let cost = path.as_os_str().len() * 2 + 128;
        if result.directories.len() >= MAX_DIRECTORIES || bytes + cost > MAX_PATH_BYTES {
            result.limited = true;
            return;
        }
        bytes += cost;
        result.directories.insert(path);
    };
    // Parent watches retain rename/recreate events for roots and atomic file saves.
    for path in configuration.roots.iter().chain(&configuration.files) {
        if let Some(parent) = std::path::Path::new(path).parent() {
            add(parent.to_owned(), &mut result);
        }
    }
    let mut queue = VecDeque::new();
    let mut queued_bytes = 0;
    for root in &configuration.roots {
        if queue.len() >= MAX_DIRECTORIES || queued_bytes + root.len() > MAX_PATH_BYTES {
            result.limited = true;
            continue;
        }
        queued_bytes += root.len();
        queue.push_back(PathBuf::from(root));
    }
    let mut seen = HashSet::new();
    while let Some(path) = queue.pop_front() {
        queued_bytes = queued_bytes.saturating_sub(path.as_os_str().len());
        if !current() {
            break;
        }
        if started.elapsed() >= MAX_SCAN || result.directories.len() >= MAX_DIRECTORIES {
            result.limited = true;
            break;
        }
        if !seen.insert(path.clone()) {
            continue;
        }
        add(path.clone(), &mut result);
        let Ok(listing) = hedit::folder::read_directory(path) else {
            continue;
        };
        result.limited |= listing.truncated;
        for entry in listing.entries {
            if entry.is_directory && !entry.is_symlink {
                if queue.len() + seen.len() >= MAX_DIRECTORIES
                    || queued_bytes + entry.path.len() > MAX_PATH_BYTES
                {
                    result.limited = true;
                    continue;
                }
                queued_bytes += entry.path.len();
                queue.push_back(PathBuf::from(entry.path));
            }
        }
    }
    result
}
fn run(
    signals: Arc<Signals>,
    mailbox: Arc<Mutex<Mailbox>>,
    ctx: egui::Context,
    rx: Receiver<()>,
    tx: SyncSender<()>,
) {
    let mut active: Option<RecommendedWatcher> = None;
    let mut epoch = 0;
    let mut last_scan = Instant::now() - RESCAN_DELAY;
    loop {
        let wait = if signals.topology.load(Ordering::Acquire) {
            rx.recv_timeout(RESCAN_DELAY.saturating_sub(last_scan.elapsed()))
                .map(|_| ())
                .or_else(|e| {
                    if matches!(e, mpsc::RecvTimeoutError::Timeout) {
                        Ok(())
                    } else {
                        Err(e)
                    }
                })
        } else {
            rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        };
        if wait.is_err() || signals.stop.load(Ordering::Acquire) {
            break;
        }
        let (wanted, configuration) = mailbox
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .request
            .clone();
        let reconfigure = epoch != wanted;
        if !reconfigure
            && (!signals.topology.load(Ordering::Acquire) || last_scan.elapsed() < RESCAN_DELAY)
        {
            continue;
        }
        let configuration = configuration.normalized();
        epoch = wanted;
        signals.topology.store(false, Ordering::Release);
        let current = || {
            !signals.stop.load(Ordering::Acquire) && signals.epoch.load(Ordering::Acquire) == wanted
        };
        let signal = signals.clone();
        let wake = ctx.clone();
        let callback_configuration = configuration.clone();
        let sender = tx.clone();
        let mut next = match RecommendedWatcher::new(
            move |event: notify::Result<Event>| {
                if signal.epoch.load(Ordering::Acquire) != wanted
                    || signal.stop.load(Ordering::Acquire)
                {
                    return;
                }
                let structural = match event {
                    Ok(event) if interesting(&event, &callback_configuration) => {
                        topology(event.kind) || event.need_rescan()
                    }
                    Ok(_) => return,
                    Err(_) => true,
                };
                if structural {
                    signal.catalogue.store(true, Ordering::Release);
                    signal.topology.store(true, Ordering::Release);
                    let _ = sender.try_send(());
                }
                signal.changed(&wake);
            },
            notify::Config::default().with_follow_symlinks(false),
        ) {
            Ok(watcher) => watcher,
            Err(error) => {
                mailbox.lock().unwrap_or_else(|e| e.into_inner()).status = Some((wanted, format!("Automatic refresh unavailable: {error}. Refresh folder or refocus to update.")));
                ctx.request_repaint();
                continue;
            }
        };
        let plan = plan(&configuration, current);
        let mut failed = 0;
        for path in &plan.directories {
            if !current() {
                break;
            }
            if next.watch(path, RecursiveMode::NonRecursive).is_err() {
                failed += 1;
            }
        }
        if !current() {
            continue;
        }
        // Install the replacement before releasing old watches, avoiding a refresh gap.
        active.replace(next);
        last_scan = Instant::now();
        let status = if plan.limited || failed > 0 {
            format!("Automatic refresh: {} folders · limited coverage. Refocus or Refresh folder for a full refresh.", plan.directories.len().saturating_sub(failed))
        } else {
            format!("Automatic refresh: {} folders", plan.directories.len())
        };
        mailbox.lock().unwrap_or_else(|e| e.into_inner()).status = Some((wanted, status));
        signals.catalogue.store(true, Ordering::Release);
        signals.changed(&ctx);
    }
}
impl HeditApp {
    pub(super) fn poll_workspace_watch(&mut self, ctx: &egui::Context) {
        if self.workspace.remote.is_some() || !self.settings.auto_refresh_workspace {
            self.workspace_watch = State::default();
            return;
        }
        if self
            .workspace_watch
            .configuration
            .as_ref()
            .is_none_or(|c| !c.matches(self))
        {
            let configuration = Configuration::from_app(self);
            if configuration.roots.is_empty() && configuration.files.is_empty() {
                self.workspace_watch = State::default();
                return;
            }
            self.workspace_watch.configure(ctx, configuration);
        }
        let changed = self.workspace_watch.poll(ctx, Instant::now());
        let refocused = quality::refocused(ctx);
        if (changed && self.workspace_watch.catalogue_changed) || refocused {
            self.project.invalidate();
        }
        if changed || refocused {
            self.poll_at = Instant::now() - Duration::from_secs(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::finish_app_job;
    fn fixture() -> (tempfile::TempDir, egui::Context, HeditApp) {
        let dir = tempfile::tempdir().unwrap();
        settings_store::TEST_CONFIG_DIR.with(|p| *p.borrow_mut() = Some(dir.path().join("config")));
        let (workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.choose_terminal(false);
        (dir, ctx, app)
    }
    fn wait(mut ready: impl FnMut() -> bool) {
        let start = Instant::now();
        while !ready() {
            assert!(
                start.elapsed() < Duration::from_secs(15),
                "native watch did not deliver an update"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
    #[test]
    fn event_floods_are_coalesced_with_a_maximum_latency_and_no_idle_work() {
        let ctx = egui::Context::default();
        let signals = Arc::new(Signals::default());
        let (tx, _rx) = mpsc::sync_channel(1);
        let mut state = State {
            worker: Some(Worker {
                tx,
                signals: signals.clone(),
                mailbox: Arc::new(Mutex::new(Mailbox::default())),
            }),
            ..Default::default()
        };
        let start = Instant::now();
        for _ in 0..10_000 {
            signals.changed(&ctx);
        }
        assert!(!state.poll(&ctx, start));
        for step in 1..10 {
            signals.changed(&ctx);
            assert!(!state.poll(&ctx, start + Duration::from_millis(step * 100)));
        }
        signals.changed(&ctx);
        assert!(state.poll(&ctx, start + MAX_DELAY));
        assert!(!state.poll(&ctx, start + MAX_DELAY + SETTLE));
        assert!(!signals.awake.load(Ordering::Acquire));
    }
    #[test]
    fn data_events_keep_filename_caches_warm() {
        let (_dir, ctx, mut app) = fixture();
        let signals = Arc::new(Signals::default());
        let (tx, _rx) = mpsc::sync_channel(1);
        app.workspace_watch = State {
            configuration: Some(Configuration::from_app(&app)),
            worker: Some(Worker {
                tx,
                signals: signals.clone(),
                mailbox: Arc::new(Mutex::new(Mailbox::default())),
            }),
            ..Default::default()
        };
        app.project.search = Some(Ok(hedit::folder::QuickOpen {
            entries: vec![],
            truncated: false,
            skipped_directories: 0,
        }));
        signals.changed(&ctx);
        app.poll_workspace_watch(&ctx);
        let past = Instant::now() - MAX_DELAY;
        app.workspace_watch.pending = Some((past, past));
        app.poll_workspace_watch(&ctx);
        assert!(app.project.search.is_some());
        signals.catalogue.store(true, Ordering::Release);
        signals.changed(&ctx);
        app.poll_workspace_watch(&ctx);
        app.workspace_watch.pending = Some((past, past));
        app.poll_workspace_watch(&ctx);
        assert!(app.project.search.is_none());
    }
    #[test]
    fn access_events_and_unrelated_parent_entries_do_not_invalidate_the_workspace() {
        let root = std::env::temp_dir().join("hedit-watch-root");
        let configuration = Configuration {
            roots: vec![root.to_string_lossy().into_owned()],
            ..Default::default()
        };
        let create =
            |path| Event::new(EventKind::Create(notify::event::CreateKind::File)).add_path(path);
        assert!(interesting(
            &create(root.join("src/new.rs")),
            &configuration
        ));
        assert!(!interesting(
            &create(root.with_file_name("unrelated.txt")),
            &configuration
        ));
        assert!(!interesting(
            &Event::new(EventKind::Access(notify::event::AccessKind::Any)).add_path(root),
            &configuration
        ));
    }
    #[test]
    fn plans_include_root_and_file_parents_and_do_not_follow_directory_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(dir.path()).unwrap();
        let root = base.join("root");
        let child = root.join("child");
        std::fs::create_dir_all(&child).unwrap();
        let external = base.join("external");
        std::fs::create_dir(&external).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&external, root.join("linked")).unwrap();
        let configuration = Configuration {
            roots: vec![root.to_string_lossy().into_owned()],
            files: vec![external.join("open.rs").to_string_lossy().into_owned()],
            ..Default::default()
        };
        let result = plan(&configuration, || true);
        assert!(result.directories.contains(&base));
        assert!(result.directories.contains(&root));
        assert!(result.directories.contains(&child));
        assert!(result.directories.contains(&external));
        assert!(!result.directories.contains(&root.join("linked")));
        assert!(!result.limited);
    }
    #[test]
    fn native_events_refresh_cached_names_and_preserve_unsaved_content() {
        let (dir, ctx, mut app) = fixture();
        let root = dir.path().join("root");
        std::fs::create_dir(&root).unwrap();
        let path = root.join("open.rs");
        std::fs::write(&path, "old\n").unwrap();
        app.set_folder(hedit::folder::read_directory(root.clone()).unwrap());
        app.open(&ctx, Some(path.clone()));
        finish_app_job(&mut app, &ctx);
        app.poll_workspace_watch(&ctx);
        wait(|| {
            app.poll_workspace_watch(&ctx);
            app.workspace_watch.status.starts_with("Automatic refresh:")
        });
        // Drain the initial setup refresh before creating a new name.
        wait(|| {
            app.poll_workspace_watch(&ctx);
            app.workspace_watch.pending.is_none()
        });
        let roots = vec![root.clone()];
        app.project.search(&ctx, &roots, "new");
        wait(|| {
            app.project.search(&ctx, &roots, "new");
            !app.project.indexing()
        });
        assert!(app
            .project
            .search
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .entries
            .is_empty());
        app.workspace.open[app.workspace.active].page.text = "unsaved\n".into();
        app.workspace.edit(app.workspace.active, "old\n").unwrap();
        std::fs::write(root.join("new.rs"), "fn new() {}\n").unwrap();
        std::fs::write(&path, "outside\n").unwrap();
        wait(|| {
            app.poll_workspace_watch(&ctx);
            app.project.search.is_none()
        });
        app.poll_external(&ctx);
        assert!(app.active().external && app.active().page.info.dirty);
        assert_eq!(app.active().page.text, "unsaved\n");
        app.project.search(&ctx, &roots, "new");
        wait(|| {
            app.project.search(&ctx, &roots, "new");
            !app.project.indexing()
        });
        assert!(app
            .project
            .search
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .entries
            .iter()
            .any(|e| e.path.ends_with("new.rs")));
        app.settings.auto_refresh_workspace = false;
        app.poll_workspace_watch(&ctx);
        assert!(app.workspace_watch.worker.is_none());
    }
    #[test]
    fn reloads_preserve_the_cursor_of_the_current_group_in_a_shared_file() {
        let (dir, ctx, mut app) = fixture();
        let path = dir.path().join("shared.txt");
        std::fs::write(&path, "before contents\n").unwrap();
        app.open(&ctx, Some(path.clone()));
        finish_app_job(&mut app, &ctx);
        let id = app.active().view.id;
        app.split(1);
        app.focus_group(0, false);
        app.set_selections(vec![hedit::advanced::Selection::caret(2)]);
        std::fs::write(path, "after contents!\n").unwrap();
        let page = app
            .workspace
            .editor
            .get(id)
            .unwrap()
            .lock()
            .unwrap()
            .reload(0)
            .unwrap();
        app.focus_group(1, false);
        app.set_selections(vec![hedit::advanced::Selection::caret(7)]);
        let group = app.panes.focused_id();
        app.tx
            .send(Ok(Action::Reloaded(id, page, Some((2, 2)))))
            .unwrap();
        app.receive(&ctx);
        assert_eq!(app.panes.focused_id(), group);
        assert_eq!(app.active().view.head, 7);
        app.focus_group(0, false);
        assert_eq!(app.active().view.head, 2);
    }
    #[test]
    fn a_background_reload_keeps_the_selected_document_and_group() {
        let (dir, ctx, mut app) = fixture();
        let one = dir.path().join("one.txt");
        let two = dir.path().join("two.txt");
        std::fs::write(&one, "before\n").unwrap();
        std::fs::write(&two, "other\n").unwrap();
        app.open(&ctx, Some(one.clone()));
        finish_app_job(&mut app, &ctx);
        let id = app.active().view.id;
        app.open(&ctx, Some(two));
        finish_app_job(&mut app, &ctx);
        let current = app.active().view.id;
        let group = app.panes.focused_id();
        std::fs::write(one, "after\n").unwrap();
        let page = app
            .workspace
            .editor
            .get(id)
            .unwrap()
            .lock()
            .unwrap()
            .reload(0)
            .unwrap();
        app.tx
            .send(Ok(Action::Reloaded(id, page, Some((0, 0)))))
            .unwrap();
        app.receive(&ctx);
        assert_eq!(app.active().view.id, current);
        assert_eq!(app.panes.focused_id(), group);
        assert_eq!(
            app.workspace
                .open
                .iter()
                .find(|b| b.view.id == id)
                .unwrap()
                .page
                .text,
            "after\n"
        );
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    #[test]
    fn watch_plans_stop_at_the_directory_budget() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..MAX_DIRECTORIES + 20 {
            std::fs::create_dir(dir.path().join(format!("child-{n}"))).unwrap();
        }
        let config = Configuration {
            roots: vec![dir.path().to_string_lossy().into_owned()],
            ..Default::default()
        };
        let result = plan(&config, || true);
        assert!(result.limited);
        assert!(result.directories.len() <= MAX_DIRECTORIES);
        assert!(result.directories.contains(dir.path()));
    }
    #[cfg(unix)]
    #[test]
    fn native_watch_events_match_canonical_paths_through_a_root_alias() {
        let dir = tempfile::tempdir().unwrap();
        let actual = dir.path().join("actual");
        let alias = dir.path().join("alias");
        std::fs::create_dir(&actual).unwrap();
        std::os::unix::fs::symlink(&actual, &alias).unwrap();
        let ctx = egui::Context::default();
        let mut state = State::default();
        state.configure(
            &ctx,
            Configuration {
                roots: vec![alias.to_string_lossy().into_owned()],
                ..Default::default()
            },
        );
        let start = Instant::now();
        while !state.status.starts_with("Automatic refresh:") {
            state.poll(&ctx, Instant::now());
            assert!(
                start.elapsed() < Duration::from_secs(15),
                "{}",
                state.status
            );
            thread::sleep(Duration::from_millis(10));
        }
        let signals = state.worker.as_ref().unwrap().signals.clone();
        let before = signals.events.load(Ordering::Acquire);
        std::fs::write(actual.join("new.rs"), "fn new() {}\n").unwrap();
        let start = Instant::now();
        while signals.events.load(Ordering::Acquire) == before {
            assert!(start.elapsed() < Duration::from_secs(15));
            thread::sleep(Duration::from_millis(10));
        }
    }
    #[test]
    fn native_watches_survive_root_deletion_recreation_and_atomic_file_saves() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        std::fs::create_dir(&root).unwrap();
        let ctx = egui::Context::default();
        let mut state = State::default();
        state.configure(
            &ctx,
            Configuration {
                roots: vec![root.to_string_lossy().into_owned()],
                ..Default::default()
            },
        );
        let wait = |state: &mut State, expected: &str| {
            let start = Instant::now();
            loop {
                state.poll(&ctx, Instant::now());
                if state.status.starts_with(expected) {
                    break;
                }
                assert!(
                    start.elapsed() < Duration::from_secs(15),
                    "{}",
                    state.status
                );
                thread::sleep(Duration::from_millis(10));
            }
        };
        wait(&mut state, "Automatic refresh: 2 folders");
        std::fs::remove_dir(&root).unwrap();
        wait(&mut state, "Automatic refresh: 1 folders");
        std::fs::create_dir(&root).unwrap();
        wait(&mut state, "Automatic refresh: 2 folders");
        let signals = state.worker.as_ref().unwrap().signals.clone();
        let before = signals.events.load(Ordering::Acquire);
        let path = root.join("file.rs");
        std::fs::write(&path, "before").unwrap();
        std::fs::write(root.join("replacement.tmp"), "after").unwrap();
        std::fs::rename(root.join("replacement.tmp"), path).unwrap();
        let start = Instant::now();
        while signals.events.load(Ordering::Acquire) == before {
            assert!(start.elapsed() < Duration::from_secs(15));
            thread::sleep(Duration::from_millis(10));
        }
    }
}
