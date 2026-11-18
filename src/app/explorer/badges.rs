//! Event-driven status queries; drawing rows never starts Git or an LSP session.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
const MAX_FILES: usize = 10_000;
const INTERVAL: Duration = Duration::from_secs(1);
#[derive(Default)]
pub(in crate::app) struct State {
    roots: Vec<PathBuf>,
    enabled: Option<bool>,
    rx: Option<Receiver<Reply>>,
    cancel: Option<Arc<AtomicBool>>,
    generation: u64,
    at: Option<Instant>,
    pub dirty: bool,
    pub metadata: Vec<String>,
    pub files: Arc<BTreeMap<PathBuf, char>>,
    pub diagnostics: Arc<BTreeMap<PathBuf, (usize, u8)>>,
    pub note: String,
}
struct Reply {
    generation: u64,
    files: BTreeMap<PathBuf, char>,
    metadata: Vec<String>,
    limited: bool,
}
impl Drop for State {
    fn drop(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.store(true, Ordering::Release);
        }
    }
}
impl State {
    pub(in crate::app) fn invalidate(&mut self) {
        self.dirty = true;
    }
}
fn priority(status: char) -> u8 {
    match status {
        '!' => 5,
        'D' => 4,
        'M' => 3,
        'A' | 'R' | 'C' => 2,
        _ => 1,
    }
}
fn git_metadata_path(root: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value.trim_end());
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}
fn add_metadata_directory(reply: &mut Reply, path: &Path) {
    match path
        .canonicalize()
        .ok()
        .filter(|path| path.is_dir())
        .and_then(|path| path.to_str().map(str::to_owned))
    {
        Some(path) => reply.metadata.push(path),
        None => reply.limited = true,
    }
}
fn metadata_directories(git: &hedit::git::Git, root: &Path, reply: &mut Reply) {
    for args in [
        ["rev-parse", "--absolute-git-dir"],
        ["rev-parse", "--git-common-dir"],
    ] {
        if let Ok(value) = git.command(root, &args) {
            let path = git_metadata_path(root, &value);
            add_metadata_directory(reply, &path);
            // Git's reftable backend keeps its tables below the metadata root.
            let reftable = path.join("reftable");
            if reftable.is_dir() {
                add_metadata_directory(reply, &reftable);
            }
        } else {
            reply.limited = true;
        }
    }
    // Loose branch refs are below refs/heads, outside a non-recursive .git watch.
    // --git-path also resolves shared refs correctly for linked worktrees.
    if let Ok(reference) = git.command(root, &["symbolic-ref", "--quiet", "HEAD"]) {
        if let Ok(value) = git.command(root, &["rev-parse", "--git-path", reference.trim_end()]) {
            let path = git_metadata_path(root, &value);
            if let Some(mut directory) = path.parent() {
                // An unborn or newly selected nested branch may not have a ref yet.
                for _ in 0..32 {
                    if directory.is_dir() {
                        add_metadata_directory(reply, directory);
                        break;
                    }
                    let Some(parent) = directory.parent() else {
                        break;
                    };
                    directory = parent;
                }
            }
        } else {
            reply.limited = true;
        }
    }
}
fn collect(git: &hedit::git::Git, generation: u64, roots: Vec<PathBuf>, local: bool) -> Reply {
    let mut reply = Reply {
        generation,
        files: BTreeMap::new(),
        metadata: vec![],
        limited: false,
    };
    let mut repositories = BTreeSet::new();
    let mut count = 0usize;
    let mut bytes = 0usize;
    reply.limited = roots.len() > 64;
    for directory in roots.into_iter().take(64) {
        if git.cancel.load(Ordering::Acquire) {
            return reply;
        }
        let Ok(root) = git.discover(&directory) else {
            continue;
        };
        if !repositories.insert(root.clone()) {
            continue;
        }
        if local {
            metadata_directories(git, &root, &mut reply);
        }
        let Ok(files) = git.status_files(&root) else {
            continue;
        };
        if count.saturating_add(files.len()) > MAX_FILES {
            reply.limited = true;
        }
        let remaining = MAX_FILES.saturating_sub(count);
        for file in files.into_iter().take(remaining) {
            count += 1;
            let status = if file.conflict {
                '!'
            } else if file.untracked() {
                '?'
            } else if file.working() {
                file.worktree
            } else {
                file.index
            };
            let path = root.join(file.path);
            for path in path.ancestors().take(32) {
                if !path.starts_with(&root) {
                    break;
                }
                if !reply.files.contains_key(path) {
                    let cost = path.as_os_str().len() * 2 + 256;
                    if reply.files.len() >= 20_000 || bytes + cost > 8 * 1024 * 1024 {
                        reply.limited = true;
                        continue;
                    }
                    bytes += cost;
                }
                reply
                    .files
                    .entry(path.to_owned())
                    .and_modify(|prior| {
                        if priority(status) > priority(*prior) {
                            *prior = status
                        }
                    })
                    .or_insert(status);
            }
        }
    }
    reply.metadata.sort();
    reply.metadata.dedup();
    reply
}
impl HeditApp {
    pub(super) fn poll_explorer_badges(&mut self, ctx: &egui::Context) {
        if self.settings.explorer_diagnostic_badges {
            self.explorer.badges.diagnostics = self.explorer_diagnostic_badges();
        } else if !self.explorer.badges.diagnostics.is_empty() {
            self.explorer.badges.diagnostics = Arc::default();
        }
        let allowed = self.settings.explorer_git_badges
            && self.workspace.remote.as_ref().is_none_or(|remote| {
                let (connected, trusted, _, _) = remote.status();
                connected && trusted
            });
        let state = &mut self.explorer.badges;
        let changed = !state.roots.iter().map(PathBuf::as_path).eq(self
            .workspace
            .folders
            .iter()
            .map(|f| Path::new(&f.path)));
        if changed || state.enabled != Some(allowed) {
            state.enabled = Some(allowed);
            if let Some(cancel) = &state.cancel {
                cancel.store(true, Ordering::Release);
            }
            state.generation = state.generation.wrapping_add(1);
            state.roots = self
                .workspace
                .folders
                .iter()
                .map(|f| PathBuf::from(&f.path))
                .collect();
            state.files = Arc::default();
            state.metadata.clear();
            state.note.clear();
            state.dirty = true;
            state.at = None;
        }
        if !allowed {
            return;
        }
        if let Some(rx) = &state.rx {
            match rx.try_recv() {
                Ok(reply) => {
                    state.rx = None;
                    state.cancel = None;
                    if reply.generation == state.generation {
                        state.files = Arc::new(reply.files);
                        state.metadata = reply.metadata;
                        state.note = if reply.limited {
                            "Git decorations have limited metadata coverage or reached their file/path budget. Source Control provides the full review."
                                .into()
                        } else {
                            String::new()
                        };
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    state.rx = None;
                    state.cancel = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if state.rx.is_some() || (!state.dirty && state.at.is_some()) || state.roots.is_empty() {
            return;
        }
        if let Some(at) = state.at {
            let due = INTERVAL.saturating_sub(at.elapsed());
            if !due.is_zero() {
                ctx.request_repaint_after(due);
                return;
            }
        }
        state.dirty = false;
        state.at = Some(Instant::now());
        let roots = state.roots.clone();
        let generation = state.generation;
        let wake = ctx.clone();
        let local = self.workspace.remote.is_none();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let (tx, rx) = mpsc::channel();
        match thread::Builder::new()
            .name("hedit-explorer-status".into())
            .spawn(move || {
                let git = hedit::git::Git { cancel: stop };
                let result = collect(&git, generation, roots, local);
                let _ = tx.send(result);
                wake.request_repaint();
            }) {
            Ok(_) => {
                state.rx = Some(rx);
                state.cancel = Some(cancel);
            }
            Err(error) => {
                state.note = format!("Git decorations unavailable: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn git_badges_combine_repository_paths_and_folder_status_without_history() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args([
                    "-c",
                    "user.name=Hedit test",
                    "-c",
                    "user.email=test@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .current_dir(&root)
                .status()
                .unwrap();
            assert!(status.success());
        };
        git(&["init", "--quiet"]);
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src/tracked.rs"), "before\n").unwrap();
        git(&["add", "src/tracked.rs"]);
        git(&["commit", "--quiet", "-m", "fixture"]);
        std::fs::write(root.join("src/tracked.rs"), "after\n").unwrap();
        std::fs::write(root.join("new.rs"), "new\n").unwrap();
        let reply = collect(
            &hedit::git::Git::default(),
            3,
            vec![root.clone(), root.join("src")],
            true,
        );
        assert_eq!(reply.generation, 3);
        assert!(!reply.limited);
        assert_eq!(reply.files[&root.join("src/tracked.rs")], 'M');
        assert_eq!(reply.files[&root.join("src")], 'M');
        assert_eq!(reply.files[&root.join("new.rs")], '?');
        assert_eq!(reply.metadata.len(), 2);
    }
}

#[cfg(test)]
mod reference_watch_tests {
    use super::*;
    fn git(root: &Path, args: &[&str]) -> String {
        let result = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Hedit test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout)
            .unwrap()
            .trim_end()
            .to_owned()
    }
    fn repository() -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap().join("repository");
        std::fs::create_dir(&root).unwrap();
        git(&root, &["init", "--quiet"]);
        std::fs::write(root.join("tracked.txt"), "first\n").unwrap();
        git(&root, &["add", "tracked.txt"]);
        git(&root, &["commit", "--quiet", "-m", "first"]);
        (directory, root)
    }
    fn ref_parent(root: &Path) -> PathBuf {
        let name = git(root, &["symbolic-ref", "--quiet", "HEAD"]);
        let path = PathBuf::from(git(root, &["rev-parse", "--git-path", &name]));
        let path = if path.is_absolute() {
            path
        } else {
            root.join(path)
        };
        path.parent().unwrap().canonicalize().unwrap()
    }
    #[test]
    fn metadata_includes_the_current_branch_reference_directory() {
        let (_directory, root) = repository();
        let reply = collect(&hedit::git::Git::default(), 1, vec![root.clone()], true);
        let parent = ref_parent(&root);
        assert!(
            reply.metadata.iter().any(|p| Path::new(p) == parent),
            "branch reference directory is not watched: {:?}",
            reply.metadata
        );
    }
    #[test]
    fn linked_worktrees_watch_shared_metadata_and_their_current_branch() {
        let (directory, root) = repository();
        let linked = directory.path().canonicalize().unwrap().join("linked");
        git(
            &root,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "linked-topic",
                linked.to_str().unwrap(),
            ],
        );
        let reply = collect(&hedit::git::Git::default(), 1, vec![linked.clone()], true);
        let common = root.join(".git").canonicalize().unwrap();
        let parent = ref_parent(&linked);
        assert!(
            reply.metadata.iter().any(|p| Path::new(p) == common),
            "shared Git directory is not watched: {:?}",
            reply.metadata
        );
        assert!(reply.metadata.iter().any(|p| Path::new(p) == parent));
    }
    #[test]
    fn native_ref_updates_refresh_badges_without_editing_files_or_refocusing() {
        let (directory, root) = repository();
        let first = git(&root, &["rev-parse", "HEAD"]);
        std::fs::write(root.join("tracked.txt"), "second\n").unwrap();
        git(&root, &["add", "tracked.txt"]);
        git(&root, &["commit", "--quiet", "-m", "second"]);
        settings_store::TEST_CONFIG_DIR
            .with(|p| *p.borrow_mut() = Some(directory.path().join("config")));
        let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.choose_terminal(false);
        app.set_folder(hedit::folder::read_directory(root.clone()).unwrap());
        let started = Instant::now();
        let mut quiet = None;
        loop {
            app.poll_explorer_badges(&ctx);
            app.poll_workspace_watch(&ctx);
            app.project.poll();
            let idle = app.workspace_watch.status.starts_with("Automatic refresh:")
                && app.explorer.badges.rx.is_none()
                && !app.explorer.badges.dirty;
            if idle {
                let since = quiet.get_or_insert_with(Instant::now);
                if since.elapsed() > Duration::from_secs(2) {
                    break;
                }
            } else {
                quiet = None;
            }
            assert!(
                started.elapsed() < Duration::from_secs(15),
                "metadata watches did not settle: {}",
                app.workspace_watch.status
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert!(!app
            .explorer
            .badges
            .files
            .contains_key(&root.join("tracked.txt")));
        git(&root, &["update-ref", "HEAD", &first]);
        let started = Instant::now();
        loop {
            app.poll_workspace_watch(&ctx);
            app.poll_explorer_badges(&ctx);
            if app.explorer.badges.files.get(&root.join("tracked.txt")) == Some(&'M') {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "branch ref changes did not refresh Explorer"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            std::fs::read_to_string(root.join("tracked.txt")).unwrap(),
            "second\n"
        );
    }
}
