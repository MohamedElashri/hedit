//! One coalescing worker owns the filename catalogue. No contents or names are
//! persisted. A changed query reuses metadata; changed roots cancel the walk.
use super::*;
use std::collections::VecDeque;
use std::sync::{mpsc::SyncSender, Mutex};

const MAX_ENTRIES: usize = 50_000;
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_TIME: Duration = Duration::from_secs(5);

#[derive(Clone, PartialEq, Eq)]
struct Request {
    epoch: u64,
    roots: Vec<PathBuf>,
    query: String,
    ticket: u64,
}
struct Response {
    ticket: u64,
    result: QuickOpen,
    indexing: bool,
}
#[derive(Default)]
struct Shared {
    request: Option<Request>,
    response: Option<Response>,
}
struct Worker {
    shared: Arc<Mutex<Shared>>,
    tx: SyncSender<()>,
}
#[derive(Default)]
pub(super) struct Names {
    worker: Option<Worker>,
    epoch: u64,
    ticket: u64,
    requested: Option<Request>,
    completed: Option<Instant>,
    pub indexing: bool,
}
impl Names {
    pub fn invalidate(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.requested = None;
        self.completed = None;
        self.indexing = false;
        if let Some(worker) = &self.worker {
            let mut shared = worker.shared.lock().unwrap_or_else(|e| e.into_inner());
            shared.request = None;
            shared.response = None;
            let _ = worker.tx.try_send(());
        }
    }
    /// Refresh once when reopening a stale picker, never on each keystroke.
    pub fn begin(&mut self) -> bool {
        let stale = self
            .completed
            .is_some_and(|t| t.elapsed() >= Duration::from_secs(2));
        if stale {
            self.invalidate();
        }
        stale
    }
    pub fn current(&self, roots: &[PathBuf], query: &str) -> bool {
        self.requested
            .as_ref()
            .is_some_and(|r| r.epoch == self.epoch && r.roots == roots && r.query == query)
    }
    pub fn search(
        &mut self,
        ctx: &egui::Context,
        roots: &[PathBuf],
        query: &str,
    ) -> Option<QuickOpen> {
        let changed = self
            .requested
            .as_ref()
            .is_none_or(|r| r.epoch != self.epoch || r.roots != roots || r.query != query);
        if self.worker.is_none() {
            let shared = Arc::new(Mutex::new(Shared::default()));
            let (tx, rx) = mpsc::sync_channel(1);
            let state = shared.clone();
            let ctx = ctx.clone();
            thread::spawn(move || run(state, rx, ctx, folder::read_directory));
            self.worker = Some(Worker { shared, tx });
        }
        let worker = self.worker.as_ref().unwrap();
        if changed {
            if self
                .requested
                .as_ref()
                .is_none_or(|r| r.epoch != self.epoch || r.roots != roots)
            {
                self.completed = None;
                self.indexing = true;
            }
            self.ticket = self.ticket.wrapping_add(1);
            let request = Request {
                epoch: self.epoch,
                roots: roots.to_vec(),
                query: query.into(),
                ticket: self.ticket,
            };
            let mut shared = worker.shared.lock().unwrap_or_else(|e| e.into_inner());
            shared.request = Some(request.clone());
            shared.response = None;
            self.requested = Some(request);
            let _ = worker.tx.try_send(());
        }
        let response = worker
            .shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .response
            .take();
        response.filter(|r| r.ticket == self.ticket).map(|r| {
            if self.indexing && !r.indexing {
                self.completed = Some(Instant::now());
            }
            self.indexing = r.indexing;
            r.result
        })
    }
}
impl Drop for Names {
    fn drop(&mut self) {
        self.invalidate();
    }
}

#[derive(Default)]
struct Catalogue {
    entries: Vec<folder::FolderEntry>,
    truncated: bool,
    skipped: u64,
}
impl Catalogue {
    fn filter(&self, query: &str) -> QuickOpen {
        let query = query.to_lowercase().replace('\\', "/");
        let mut best: Vec<(i64, &folder::FolderEntry)> = vec![];
        for entry in &self.entries {
            if let Some(score) = folder::fuzzy_score(&entry.name, &query) {
                let at = best.partition_point(|(prior, e)| {
                    *prior > score || (*prior == score && e.name <= entry.name)
                });
                if at < 80 {
                    best.insert(at, (score, entry));
                    best.truncate(80);
                }
            }
        }
        QuickOpen {
            entries: best.into_iter().map(|(_, e)| e.clone()).collect(),
            truncated: self.truncated,
            skipped_directories: self.skipped,
        }
    }
}
fn request(shared: &Mutex<Shared>) -> Option<Request> {
    shared
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .request
        .clone()
}
fn same_catalogue(a: &Request, b: &Request) -> bool {
    a.epoch == b.epoch && a.roots == b.roots
}
fn publish(
    shared: &Mutex<Shared>,
    ctx: &egui::Context,
    catalogue: &Catalogue,
    request: &Request,
    indexing: bool,
) {
    // Filtering runs without holding the UI mailbox lock.
    let result = catalogue.filter(&request.query);
    let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
    let accepted = state.request.as_ref() == Some(request);
    if accepted {
        state.response = Some(Response {
            ticket: request.ticket,
            result,
            indexing,
        });
    }
    drop(state);
    if accepted {
        ctx.request_repaint();
    }
}
fn run(
    shared: Arc<Mutex<Shared>>,
    rx: Receiver<()>,
    ctx: egui::Context,
    mut read: impl FnMut(PathBuf) -> Result<FolderListing, String>,
) {
    let mut cache: Option<(Request, Catalogue)> = None;
    while rx.recv().is_ok() {
        let Some(wanted) = request(&shared) else {
            cache = None;
            continue;
        };
        if cache
            .as_ref()
            .is_none_or(|(key, _)| !same_catalogue(key, &wanted))
        {
            drop(cache.take());
            let catalogue = scan(
                &wanted,
                || {
                    shared
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .request
                        .as_ref()
                        .is_some_and(|r| same_catalogue(&wanted, r))
                },
                |partial| {
                    if let Some(current) = request(&shared).filter(|r| same_catalogue(&wanted, r)) {
                        publish(&shared, &ctx, partial, &current, true);
                    }
                },
                &mut read,
            );
            if !request(&shared).is_some_and(|r| same_catalogue(&wanted, &r)) {
                cache = None;
                continue;
            }
            cache = Some((wanted.clone(), catalogue));
        }
        if let Some(current) = request(&shared).filter(|r| same_catalogue(&wanted, r)) {
            publish(&shared, &ctx, &cache.as_ref().unwrap().1, &current, false);
        }
    }
}

fn scan(
    wanted: &Request,
    current: impl Fn() -> bool,
    mut progress: impl FnMut(&Catalogue),
    mut read: impl FnMut(PathBuf) -> Result<FolderListing, String>,
) -> Catalogue {
    let mut result = Catalogue::default();
    let mut roots = wanted.roots.clone();
    // Nested roots own their labels; scanning them first avoids duplicates.
    roots.sort_by_key(|r| std::cmp::Reverse(r.as_os_str().len()));
    let mut unique_roots = HashSet::new();
    roots.retain(|root| unique_roots.insert(root.clone()));
    let mut directories: VecDeque<_> = roots
        .iter()
        .map(|r| (r.clone(), String::new(), None::<String>))
        .collect();
    let mut seen = HashSet::new();
    let mut seen_files = HashSet::new();
    let mut bytes = 0usize;
    let mut visited = 0usize;
    let started = Instant::now();
    let mut published = started;
    while let Some((path, relative, root_label)) = directories.pop_front() {
        if !current() {
            break;
        }
        if visited >= MAX_ENTRIES || bytes >= MAX_BYTES || started.elapsed() >= MAX_TIME {
            result.truncated = true;
            break;
        }
        if !seen.insert(path.clone()) {
            continue;
        }
        // Account for both the queue and visited set, in addition to filenames.
        let listing = match read(path) {
            Ok(listing) => listing,
            Err(_) => {
                result.skipped += 1;
                continue;
            }
        };
        result.truncated |= listing.truncated || listing.unsupported_names;
        let label = root_label.unwrap_or_else(|| {
            if roots.len() == 1 {
                return String::new();
            }
            let duplicates = roots
                .iter()
                .filter(|r| r.file_name() == PathBuf::from(&listing.path).file_name())
                .count();
            if duplicates > 1 {
                listing.path.clone()
            } else {
                listing.name.clone()
            }
        });
        for entry in listing.entries {
            if !current() {
                return result;
            }
            let cost = entry.path.len() * 3 + entry.name.len() + relative.len() + label.len() + 256;
            if visited >= MAX_ENTRIES
                || bytes.saturating_add(cost) > MAX_BYTES
                || started.elapsed() >= MAX_TIME
            {
                result.truncated = true;
                return result;
            }
            visited += 1;
            bytes += cost;
            if folder::excluded(&entry.name, entry.is_directory) {
                continue;
            }
            let name = if relative.is_empty() {
                entry.name.clone()
            } else {
                format!("{relative}/{}", entry.name)
            };
            if entry.is_directory {
                // Automatic walks never follow directory links outside a root.
                if !entry.is_symlink {
                    directories.push_back((entry.path.into(), name, Some(label.clone())));
                }
            } else if seen_files.insert(entry.path.clone()) {
                result.entries.push(folder::FolderEntry {
                    name: if label.is_empty() {
                        name
                    } else {
                        format!("{label}/{name}")
                    },
                    ..entry
                });
            }
        }
        if published.elapsed() >= Duration::from_millis(100) {
            progress(&result);
            published = Instant::now();
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wanted(roots: &[&str]) -> Request {
        Request {
            roots: roots.iter().map(PathBuf::from).collect(),
            epoch: 0,
            query: String::new(),
            ticket: 1,
        }
    }
    fn file(name: &str, path: &str, directory: bool) -> folder::FolderEntry {
        folder::FolderEntry {
            name: name.into(),
            path: path.into(),
            is_directory: directory,
            is_symlink: false,
        }
    }
    fn listing(path: &str, entries: Vec<folder::FolderEntry>) -> FolderListing {
        FolderListing {
            path: path.into(),
            name: PathBuf::from(path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            entries,
            truncated: false,
            unsupported_names: false,
        }
    }
    fn scan_with(
        roots: &[&str],
        read: impl FnMut(PathBuf) -> Result<FolderListing, String>,
    ) -> Catalogue {
        scan(&wanted(roots), || true, |_| {}, read)
    }
    #[test]
    fn encoded_ssh_paths_are_opaque_and_relative_labels_are_not_uri_paths() {
        let root = "hedit-ssh://session/";
        let directory = "hedit-ssh://session/src%2Fspace%20dir";
        let path = "hedit-ssh://session/src%2Fspace%20dir%2Fمرحبا.rs";
        let index = scan_with(&[root], |p| match p.to_str().unwrap() {
            p if p == root => Ok(listing(root, vec![file("src/space dir", directory, true)])),
            p if p == directory => Ok(listing(directory, vec![file("مرحبا.rs", path, false)])),
            p => panic!("fabricated URI: {p}"),
        });
        assert_eq!(index.entries[0].name, "src/space dir/مرحبا.rs");
        assert_eq!(index.filter("مرح").entries[0].path, path);
    }
    #[test]
    fn overlapping_roots_are_deduplicated_and_same_leaf_roots_are_unambiguous() {
        let index = scan_with(&["/a/app", "/a/app/src", "/b/app"], |p| {
            Ok(match p.to_str().unwrap() {
                "/a/app" => listing("/a/app", vec![file("src", "/a/app/src", true)]),
                "/a/app/src" => listing(
                    "/a/app/src",
                    vec![file("main.rs", "/a/app/src/main.rs", false)],
                ),
                "/b/app" => listing("/b/app", vec![file("main.rs", "/b/app/main.rs", false)]),
                p => panic!("unexpected walk: {p}"),
            })
        });
        assert_eq!(index.entries.len(), 2);
        assert!(index.entries.iter().any(|e| e.name == "src/main.rs"));
        assert!(index.entries.iter().any(|e| e.name == "/b/app/main.rs"));
    }
    #[test]
    fn directory_symlinks_excluded_names_and_unreadable_directories_do_not_escape_the_scan() {
        let mut link = file("outside", "/private/outside", true);
        link.is_symlink = true;
        let index = scan_with(&["/root"], |p| {
            if p == std::path::Path::new("/root") {
                Ok(listing(
                    "/root",
                    vec![
                        link.clone(),
                        file(".git", "/root/.git", true),
                        file("secret", "/root/secret", true),
                        file("ok.rs", "/root/ok.rs", false),
                    ],
                ))
            } else {
                assert_eq!(p, PathBuf::from("/root/secret"));
                Err("permission denied".into())
            }
        });
        assert_eq!(index.skipped, 1);
        assert_eq!(index.entries.len(), 1);
        assert_eq!(index.entries[0].name, "ok.rs");
    }
    #[test]
    fn incomplete_directory_results_and_memory_limits_are_visible() {
        let index = scan_with(&["/root"], |_| {
            let mut l = listing("/root", vec![]);
            l.truncated = true;
            l.unsupported_names = true;
            Ok(l)
        });
        assert!(index.filter("missing").truncated);
        let index = scan_with(&["/root"], |_| {
            Ok(listing(
                "/root",
                (0..2000)
                    .map(|i| {
                        file(
                            &format!("{i}{}", "x".repeat(4000)),
                            &format!("/root/{i}"),
                            false,
                        )
                    })
                    .collect(),
            ))
        });
        assert!(index.truncated);
        assert!(index.entries.len() < 2000);
    }
    #[test]
    fn entry_budget_and_cancellation_stop_reads() {
        let reads = std::cell::Cell::new(0);
        let index = scan_with(&["/root"], |p| {
            reads.set(reads.get() + 1);
            Ok(listing(
                p.to_str().unwrap(),
                (0..2000)
                    .map(|i| {
                        let name = format!("d{i}");
                        file(&name, &format!("{}/{name}", p.display()), true)
                    })
                    .collect(),
            ))
        });
        assert!(index.truncated);
        assert!(reads.get() <= 26);
        let index = scan(
            &wanted(&["/root"]),
            || false,
            |_| {},
            |_| panic!("cancelled scan read a directory"),
        );
        assert!(index.entries.is_empty());
    }
    fn worker(
        read: impl FnMut(PathBuf) -> Result<FolderListing, String> + Send + 'static,
    ) -> (Names, thread::JoinHandle<()>) {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (tx, rx) = mpsc::sync_channel(1);
        let state = shared.clone();
        let handle = thread::spawn(move || run(state, rx, egui::Context::default(), read));
        let mut names = Names::default();
        names.worker = Some(Worker { shared, tx });
        (names, handle)
    }
    fn wait(names: &mut Names, roots: &[PathBuf], query: &str) -> QuickOpen {
        let start = Instant::now();
        loop {
            if let Some(result) = names.search(&egui::Context::default(), roots, query) {
                if !names.indexing {
                    return result;
                }
            }
            assert!(start.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn hundreds_of_query_changes_and_refreshes_use_one_worker_and_discard_old_results() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let reads = Arc::new(Mutex::new(vec![]));
        let observed = reads.clone();
        let (mut names, handle) = worker(move |p| {
            observed.lock().unwrap().push(p.clone());
            if p == std::path::Path::new("/old") {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            }
            Ok(listing(
                p.to_str().unwrap(),
                vec![
                    file("Alpha.rs", &format!("{}/Alpha.rs", p.display()), false),
                    file("Beta.rs", &format!("{}/Beta.rs", p.display()), false),
                ],
            ))
        });
        let ctx = egui::Context::default();
        names.search(&ctx, &["/old".into()], "Alpha");
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        for _ in 0..200 {
            names.invalidate();
            names.search(&ctx, &["/new".into()], "Beta");
        }
        release_tx.send(()).unwrap();
        let result = wait(&mut names, &["/new".into()], "Beta");
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].path, "/new/Beta.rs");
        for query in ["Alpha", "Beta", "Alpha"] {
            assert_eq!(wait(&mut names, &["/new".into()], query).entries.len(), 1);
        }
        assert_eq!(
            *reads.lock().unwrap(),
            vec![PathBuf::from("/old"), PathBuf::from("/new")]
        );
        drop(names);
        handle.join().unwrap();
    }
    #[test]
    fn refresh_observes_creates_renames_and_deletes_without_persisting_a_catalogue() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("before.rs"), "contents are not read").unwrap();
        let (mut names, handle) = worker(folder::read_directory);
        let roots = [directory.path().into()];
        assert_eq!(wait(&mut names, &roots, "before").entries.len(), 1);
        std::fs::rename(
            directory.path().join("before.rs"),
            directory.path().join("after.rs"),
        )
        .unwrap();
        std::fs::write(directory.path().join("created.rs"), "").unwrap();
        names.invalidate();
        assert!(wait(&mut names, &roots, "before").entries.is_empty());
        assert_eq!(wait(&mut names, &roots, "after").entries.len(), 1);
        std::fs::remove_file(directory.path().join("after.rs")).unwrap();
        names.invalidate();
        assert!(wait(&mut names, &roots, "after").entries.is_empty());
        assert_eq!(wait(&mut names, &roots, "created").entries.len(), 1);
        drop(names);
        handle.join().unwrap();
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
    #[test]
    fn a_stale_query_response_cannot_replace_the_requested_query() {
        let (mut names, handle) = worker(|p| Ok(listing(p.to_str().unwrap(), vec![])));
        wait(&mut names, &["/root".into()], "alpha");
        wait(&mut names, &["/root".into()], "beta");
        names
            .worker
            .as_ref()
            .unwrap()
            .shared
            .lock()
            .unwrap()
            .response = Some(Response {
            ticket: names.ticket - 1,
            result: QuickOpen {
                entries: vec![file("alpha", "/root/alpha", false)],
                truncated: false,
                skipped_directories: 0,
            },
            indexing: false,
        });
        assert!(names
            .search(&egui::Context::default(), &["/root".into()], "beta")
            .is_none());
        drop(names);
        handle.join().unwrap();
    }
    #[test]
    fn reopening_refreshes_only_a_completed_stale_catalogue() {
        let mut names = Names::default();
        names.indexing = true;
        assert!(!names.begin());
        names.completed = Some(Instant::now());
        assert!(!names.begin());
        names.completed = Some(Instant::now() - Duration::from_secs(3));
        assert!(names.begin());
        assert!(names.completed.is_none());
    }
    #[test]
    #[ignore = "manual disk scan versus cached filename filter profile"]
    fn profile_filename_scan_and_index() {
        let directory = tempfile::tempdir().unwrap();
        for d in 0..10 {
            let path = directory.path().join(format!("module{d}"));
            std::fs::create_dir(&path).unwrap();
            for i in 0..1000 {
                std::fs::write(path.join(format!("source_{i:04}.rs")), "").unwrap();
            }
        }
        let roots = [directory.path().to_path_buf()];
        let request = Request {
            roots: roots.to_vec(),
            epoch: 0,
            query: String::new(),
            ticket: 1,
        };
        let start = Instant::now();
        let catalogue = scan(&request, || true, |_| {}, folder::read_directory);
        let build_ms = start.elapsed().as_secs_f64() * 1000.0;
        assert!(!catalogue.truncated);
        assert_eq!(catalogue.entries.len(), 10_000);
        let queries = [
            "source",
            "sourc",
            "src123",
            "module",
            "rs",
            "module999",
            "source",
            "",
            "s01",
            "rs",
        ];
        let start = Instant::now();
        for query in queries {
            std::hint::black_box(folder::quick_open_roots(&roots, query).unwrap());
        }
        let disk = start.elapsed().as_secs_f64() * 1000.0 / queries.len() as f64;
        let start = Instant::now();
        for query in queries {
            std::hint::black_box(catalogue.filter(query));
        }
        let cached = start.elapsed().as_secs_f64() * 1000.0 / queries.len() as f64;
        eprintln!("10000 filenames: catalogue build {build_ms:.2} ms; old directory scan {disk:.2} ms/query; warm index {cached:.2} ms/query (worker CPU/wall timing, no native input latency)");
    }

    #[test]
    #[ignore = "manual warm filename filter CPU profile"]
    fn profile_warm_filename_index() {
        let catalogue = Catalogue {
            entries: (0..10_000)
                .map(|i| {
                    file(
                        &format!("src/module_{i:05}/source_{i}.rs"),
                        &format!("/root/{i}"),
                        false,
                    )
                })
                .collect(),
            ..Default::default()
        };
        let started = Instant::now();
        for query in [
            "source",
            "sourc",
            "src123",
            "module",
            "rs",
            "module999",
            "source",
            "",
            "s01",
            "rs",
        ] {
            std::hint::black_box(catalogue.filter(query));
        }
        eprintln!("Warm filename index with 10000 files: {:.2} ms/query (10 worker CPU queries; no filesystem reads)", started.elapsed().as_secs_f64() * 1000.0 / 10.0);
    }
}
