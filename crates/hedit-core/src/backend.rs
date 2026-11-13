//! Explicit routing for remote locations. Missing providers fail closed.
use crate::{build, git, search};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, mpsc, Arc, OnceLock, RwLock, Weak},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum GitQuery {
    Command(Vec<String>),
    Discover,
    Snapshot(usize),
    History(usize),
    Diff(git::Selection),
    CommitFiles(String),
    CommitMessage(String),
    Execute(git::Operation),
    Clone(String),
    Resolve(String),
    ScopedHistory {
        limit: usize,
        scope: git::HistoryScope,
    },
    Compare {
        base: String,
        head: String,
        merge_base: bool,
    },
    Tags,
    Reflog(usize),
    Blame {
        reference: String,
        #[serde(with = "crate::wire_path")]
        path: PathBuf,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FsQuery {
    List,
    QuickOpen { roots: Vec<PathBuf>, query: String },
    Entry,
    Create { name: String, directory: bool },
    Rename(String),
    Move(PathBuf),
    Duplicate(PathBuf),
    DuplicateSibling(String),
    Trash,
    Read { limit: u64 },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Call {
    File {
        path: PathBuf,
        query: FsQuery,
    },
    Git {
        path: PathBuf,
        query: GitQuery,
    },
    Search {
        roots: Vec<PathBuf>,
        filter: String,
        query: String,
        replacement: Option<String>,
        options: search::Options,
        open: HashMap<PathBuf, String>,
    },
    Replace(search::FilePlan),
    Restore(search::Restore),
}
impl Call {
    pub fn map_paths(
        &mut self,
        mut map: impl FnMut(&Path) -> Result<PathBuf, String>,
    ) -> Result<(), String> {
        match self {
            Self::File { path, query } => {
                *path = map(path)?;
                match query {
                    FsQuery::Move(target) | FsQuery::Duplicate(target) => *target = map(target)?,
                    FsQuery::QuickOpen { roots, .. } => {
                        for p in roots {
                            *p = map(p)?
                        }
                    }
                    _ => (),
                }
            }
            Self::Git { path, .. } => *path = map(path)?,
            Self::Search { roots, open, .. } => {
                for p in roots {
                    *p = map(p)?
                }
                *open = std::mem::take(open)
                    .into_iter()
                    .map(|(p, t)| Ok((map(&p)?, t)))
                    .collect::<Result<_, String>>()?;
            }
            Self::Replace(plan) => plan.path = map(&plan.path)?,
            Self::Restore(record) => record.path = map(&record.path)?,
        }
        Ok(())
    }
    pub fn local(self, cancel: Arc<AtomicBool>) -> Result<serde_json::Value, String> {
        fn value(v: impl Serialize) -> Result<serde_json::Value, String> {
            serde_json::to_value(v).map_err(|_| "Cannot encode workspace result.".into())
        }
        match self {
            Self::File { path, query } => match query {
                FsQuery::List => value(crate::folder::read_directory(path)?),
                FsQuery::QuickOpen { roots, query } => {
                    value(crate::folder::quick_open_roots(&roots, &query)?)
                }
                FsQuery::Entry => value(crate::folder::entry_path(&path)?),
                FsQuery::Create { name, directory } => value(if directory {
                    crate::folder::create_folder(path.to_string_lossy().into_owned(), name)?
                } else {
                    crate::folder::create_file(path.to_string_lossy().into_owned(), name)?
                }),
                FsQuery::Rename(name) => value(crate::folder::rename_path(
                    path.to_string_lossy().into_owned(),
                    name,
                )?),
                FsQuery::Move(target) => value(crate::folder::move_path(&path, &target)?),
                FsQuery::Duplicate(target) => value(crate::folder::duplicate_path(&path, &target)?),
                FsQuery::DuplicateSibling(name) => {
                    value(crate::folder::duplicate_sibling(&path, &name)?)
                }
                FsQuery::Trash => value(crate::folder::trash_path(&path)?),
                FsQuery::Read { limit } => value(
                    crate::file_io::read_bounded(
                        crate::file_io::open_regular(&path).map_err(|e| e.to_string())?,
                        limit,
                    )
                    .map_err(|e| e.to_string())?,
                ),
            },
            Self::Git { path, query } => {
                let git = git::Git { cancel };
                match query {
                    GitQuery::Command(args) => value(
                        git.command(&path, &args.iter().map(String::as_str).collect::<Vec<_>>())?,
                    ),
                    GitQuery::Discover => value(git.discover(&path)?),
                    GitQuery::Snapshot(n) => value(git.snapshot(&path, n)?),
                    GitQuery::History(n) => value(git.history(&path, n)?),
                    GitQuery::Diff(selection) => value(git.diff(&path, &selection)?),
                    GitQuery::CommitFiles(oid) => value(git.commit_files(&path, &oid)?),
                    GitQuery::CommitMessage(oid) => value(git.commit_message(&path, &oid)?),
                    GitQuery::Execute(operation) => value(git.execute(&path, &operation)?),
                    GitQuery::Clone(url) => value(git.clone_repository(&url, &path)?),
                    GitQuery::Resolve(reference) => value(git.resolve_commit(&path, &reference)?),
                    GitQuery::ScopedHistory { limit, scope } => {
                        value(git.scoped_history(&path, limit, &scope)?)
                    }
                    GitQuery::Compare {
                        base,
                        head,
                        merge_base,
                    } => value(git.compare(&path, &base, &head, merge_base)?),
                    GitQuery::Tags => value(git.tags(&path)?),
                    GitQuery::Reflog(n) => value(git.reflog(&path, n)?),
                    GitQuery::Blame {
                        reference,
                        path: entry,
                    } => value(git.blame(&path, &reference, &entry)?),
                }
            }
            Self::Search {
                roots,
                filter,
                query,
                replacement,
                options,
                open,
            } => value(search::folders(
                &roots,
                &filter,
                &query,
                replacement.as_deref(),
                &options,
                &open,
                &cancel,
            )?),
            Self::Replace(plan) => value(search::apply_recorded(&plan)?),
            Self::Restore(record) => value(search::restore(&record)?),
        }
    }
    pub fn needs_execution(&self) -> bool {
        matches!(self, Self::Git { .. })
    }
}
pub struct RemoteBuild {
    pub events: mpsc::Receiver<build::Event>,
    pub cancel: Arc<dyn Fn() + Send + Sync>,
}
pub enum PtyEvent {
    Bytes(Vec<u8>),
    Exit,
    Error(String),
}
pub type ResizeCallback = Arc<dyn Fn(u16, u16, u32, u32) -> Result<(), String> + Send + Sync>;
pub struct RemotePty {
    pub input: mpsc::SyncSender<Vec<u8>>,
    pub output: mpsc::Receiver<PtyEvent>,
    pub resize: ResizeCallback,
    pub stop: Arc<dyn Fn() + Send + Sync>,
}
pub enum DocumentChange {
    Edit {
        expected_revision: u64,
        start: u64,
        end: u64,
        text: String,
    },
    Format {
        expected_revision: u64,
        encoding: String,
        bom: bool,
    },
}
pub struct RemoteBuffer {
    pub revision: u64,
    pub text: String,
    pub dirty: bool,
    pub encoding: String,
    pub bom: bool,
}
pub trait Backend: Send + Sync {
    fn request(&self, call: Call, cancel: &AtomicBool) -> Result<serde_json::Value, String>;
    fn build(
        &self,
        dir: &Path,
        command: &str,
        variables: Option<&build::Variables>,
        wake: Box<dyn Fn() + Send>,
    ) -> Result<RemoteBuild, String>;
    fn terminal(&self, dir: &Path) -> Result<RemotePty, String>;
    fn document(&self, path: &Path, change: DocumentChange) -> Result<(), String>;
    fn save(
        &self,
        path: &Path,
        target: Option<&Path>,
        expected_revision: u64,
    ) -> Result<(), String>;
    fn retarget(&self, from: &Path, to: &Path) -> Result<(), String>;
    fn history(
        &self,
        path: &Path,
        redo: bool,
        expected_revision: u64,
    ) -> Result<RemoteBuffer, String>;
    fn reload(
        &self,
        path: &Path,
        discard: bool,
        expected: Option<&str>,
        expected_revision: u64,
    ) -> Result<RemoteBuffer, String>;
    fn changed(&self, path: &Path) -> (bool, String);
}
type Registry = RwLock<HashMap<String, Weak<dyn Backend>>>;
fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}
pub fn register(prefix: String, provider: Arc<dyn Backend>) {
    registry()
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .insert(prefix, Arc::downgrade(&provider));
}
pub fn is_remote(path: &Path) -> bool {
    path.to_string_lossy().starts_with("hedit-ssh:")
}
pub fn lookup(path: &Path) -> Result<Option<Arc<dyn Backend>>, String> {
    if !is_remote(path) {
        return Ok(None);
    }
    let path = path.to_string_lossy().replace('\\', "/");
    registry()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find_map(|(prefix, weak)| {
            (path == prefix.trim_end_matches('/') || path.starts_with(prefix))
                .then(|| weak.upgrade())
                .flatten()
        })
        .map(Some)
        .ok_or_else(|| "The remote workspace is disconnected.".into())
}
pub fn route<T: DeserializeOwned>(
    path: &Path,
    call: Call,
    cancel: &AtomicBool,
) -> Option<Result<T, String>> {
    match lookup(path) {
        Ok(None) => None,
        Err(error) => Some(Err(error)),
        Ok(Some(provider)) => {
            Some(provider.request(call, cancel).and_then(|v| {
                serde_json::from_value(v).map_err(|_| "Invalid remote result.".into())
            }))
        }
    }
}
pub fn file<T: DeserializeOwned>(path: &Path, query: FsQuery) -> Option<Result<T, String>> {
    route(
        path,
        Call::File {
            path: path.to_owned(),
            query,
        },
        &AtomicBool::new(false),
    )
}
