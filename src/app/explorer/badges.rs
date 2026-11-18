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
            if let Ok(directory) = git.command(&root, &["rev-parse", "--absolute-git-dir"]) {
                reply.metadata.push(directory.trim_end().to_owned());
            }
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
                            "Git decorations reached their file/path budget. Source Control provides the full review."
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
        assert_eq!(reply.metadata.len(), 1);
    }
}
