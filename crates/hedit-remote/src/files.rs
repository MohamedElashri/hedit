//! Workspace filesystem capabilities use pinned directory descriptors.
use crate::{
    host,
    protocol::{Error, Result},
};
use hedit_core::{
    backend::FsQuery,
    folder::{FolderEntry, FolderListing, QuickOpen},
    search,
};
use rustix::fs::{self as fs, AtFlags, FileType, Mode, OFlags};
use std::{
    fs::File,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

pub struct Files {
    pub root: File,
    pub path: PathBuf,
    pub state: PathBuf,
}
impl Files {
    pub fn relative(path: &Path) -> Result<Vec<u8>> {
        if path.is_absolute() {
            return Err(Error::Permission);
        }
        let mut out = Vec::new();
        for part in path.components() {
            match part {
                std::path::Component::CurDir => (),
                std::path::Component::Normal(name) => {
                    if !out.is_empty() {
                        out.push(b'/')
                    }
                    out.extend_from_slice(name.as_bytes());
                }
                _ => return Err(Error::Permission),
            }
        }
        if out.len() > 4096 || out.contains(&0) {
            return Err(Error::Permission);
        }
        Ok(out)
    }
    pub fn actual(&self, path: &Path) -> Result<PathBuf> {
        Ok(self.path.join(host::path_or_empty(&Self::relative(path)?)))
    }
    pub fn directory(&self, path: &Path) -> Result<File> {
        let bytes = Self::relative(path)?;
        if bytes.is_empty() {
            return self.root.try_clone().map_err(|_| Error::Storage);
        }
        let (parent, name) = host::parent(&self.root, &bytes)?;
        fs::openat(
            &parent,
            &name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map(File::from)
        .map_err(|_| Error::Permission)
    }
    fn listing(&self, path: &Path) -> Result<FolderListing> {
        let dir = self.directory(path)?;
        let mut reader = fs::Dir::read_from(&dir).map_err(|_| Error::Storage)?;
        let mut entries = Vec::new();
        let mut truncated = false;
        let mut unsupported = false;
        for (index, entry) in reader.by_ref().enumerate() {
            if index >= 2002 {
                truncated = true;
                break;
            }
            let entry = entry.map_err(|_| Error::Storage)?;
            let bytes = entry.file_name().to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            let Ok(name) = std::str::from_utf8(bytes) else {
                unsupported = true;
                continue;
            };
            let stat = fs::statat(&dir, entry.file_name(), AtFlags::SYMLINK_NOFOLLOW)
                .map_err(|_| Error::Storage)?;
            let kind = FileType::from_raw_mode(stat.st_mode);
            if !matches!(kind, FileType::RegularFile | FileType::Directory) {
                continue;
            }
            let directory = kind == FileType::Directory;
            if hedit_core::folder::excluded(name, directory) {
                continue;
            }
            entries.push(FolderEntry {
                name: name.into(),
                path: self
                    .actual(&path.join(name))?
                    .to_string_lossy()
                    .into_owned(),
                is_directory: directory,
                is_symlink: false,
            });
        }
        entries.sort_by_cached_key(|e| (!e.is_directory, e.name.to_lowercase()));
        let actual = self.actual(path)?;
        Ok(FolderListing {
            name: actual
                .file_name()
                .unwrap_or(actual.as_os_str())
                .to_string_lossy()
                .into_owned(),
            path: actual.to_string_lossy().into_owned(),
            entries,
            truncated,
            unsupported_names: unsupported,
        })
    }
    fn walk(&self, roots: &[PathBuf], cancel: &AtomicBool) -> Result<Vec<PathBuf>> {
        let mut queue = roots.to_vec();
        let mut files = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut visited = 0;
        while let Some(path) = queue.pop() {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Unavailable);
            }
            if visited >= 60000 || files.len() >= 30000 || Instant::now() > deadline {
                break;
            }
            visited += 1;
            let listing = match self.listing(&path) {
                Ok(v) => v,
                Err(_) => continue,
            };
            for entry in listing.entries {
                let actual = PathBuf::from(entry.path);
                let relative = actual
                    .strip_prefix(&self.path)
                    .map_err(|_| Error::Permission)?
                    .to_owned();
                if entry.is_directory {
                    queue.push(relative)
                } else {
                    files.push(relative)
                }
            }
        }
        Ok(files)
    }
    fn read(&self, path: &Path, limit: u64) -> Result<Vec<u8>> {
        let file = host::open_source(&self.root, &Self::relative(path)?)?;
        hedit_core::file_io::read_bounded(file, limit.min(32 * 1024 * 1024))
            .map_err(|_| Error::Limit)
    }
    fn create(&self, path: &Path, name: &str, directory: bool) -> Result<PathBuf> {
        if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
            return Err(Error::Permission);
        }
        let parent = self.directory(path)?;
        if directory {
            fs::mkdirat(&parent, name, Mode::from_bits_truncate(0o700))
                .map_err(|_| Error::Conflict)?;
        } else {
            let file: File = fs::openat(
                &parent,
                name,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_bits_truncate(0o600),
            )
            .map(File::from)
            .map_err(|_| Error::Conflict)?;
            file.sync_all().map_err(|_| Error::Storage)?;
        }
        parent.sync_all().map_err(|_| Error::Storage)?;
        self.actual(&path.join(name))
    }
    fn move_entry(&self, path: &Path, target: &Path) -> Result<PathBuf> {
        let from = Self::relative(path)?;
        let to = Self::relative(target)?;
        let (parent, name) = host::parent(&self.root, &from)?;
        let (next, base) = host::parent(&self.root, &to)?;
        let stat =
            fs::statat(&parent, &name, AtFlags::SYMLINK_NOFOLLOW).map_err(|_| Error::Permission)?;
        if !matches!(
            FileType::from_raw_mode(stat.st_mode),
            FileType::Directory | FileType::RegularFile
        ) {
            return Err(Error::Permission);
        }
        host::rename_noreplace(&parent, &name, &next, &base)?;
        parent.sync_all().map_err(|_| Error::Storage)?;
        next.sync_all().map_err(|_| Error::Storage)?;
        self.actual(target)
    }
    pub fn call(
        &self,
        path: &Path,
        query: FsQuery,
        cancel: &AtomicBool,
    ) -> std::result::Result<serde_json::Value, String> {
        fn value(v: impl serde::Serialize) -> std::result::Result<serde_json::Value, String> {
            serde_json::to_value(v).map_err(|_| "Cannot encode remote file metadata.".into())
        }
        let run = (|| -> Result<serde_json::Value> {
            Ok(match query {
                FsQuery::List => {
                    serde_json::to_value(self.listing(path)?).map_err(|_| Error::Protocol)?
                }
                FsQuery::QuickOpen { roots, query } => {
                    let mut matches = self
                        .walk(&roots, cancel)?
                        .into_iter()
                        .filter_map(|p| {
                            hedit_core::folder::fuzzy_score(&p.to_string_lossy(), &query)
                                .map(|score| (score, p))
                        })
                        .collect::<Vec<_>>();
                    matches.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
                    let truncated = matches.len() > 2000;
                    matches.truncate(2000);
                    let entries = matches
                        .into_iter()
                        .map(|(_, p)| {
                            Ok(FolderEntry {
                                name: p.to_string_lossy().into_owned(),
                                path: self.actual(&p)?.to_string_lossy().into_owned(),
                                is_directory: false,
                                is_symlink: false,
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    serde_json::to_value(QuickOpen {
                        entries,
                        truncated,
                        skipped_directories: 0,
                    })
                    .map_err(|_| Error::Protocol)?
                }
                FsQuery::Entry => {
                    let bytes = Self::relative(path)?;
                    if bytes.is_empty() {
                        let _ = self.directory(path)?;
                    } else {
                        let (p, n) = host::parent(&self.root, &bytes)?;
                        let stat = fs::statat(&p, &n, AtFlags::SYMLINK_NOFOLLOW)
                            .map_err(|_| Error::Permission)?;
                        if !matches!(
                            FileType::from_raw_mode(stat.st_mode),
                            FileType::RegularFile | FileType::Directory
                        ) {
                            return Err(Error::Permission);
                        }
                    }
                    serde_json::to_value(self.actual(path)?).map_err(|_| Error::Protocol)?
                }
                FsQuery::Read { limit } => {
                    serde_json::to_value(self.read(path, limit)?).map_err(|_| Error::Protocol)?
                }
                FsQuery::Create { name, directory } => {
                    serde_json::to_value(self.create(path, &name, directory)?)
                        .map_err(|_| Error::Protocol)?
                }
                FsQuery::Rename(name) => {
                    if name.contains(['/', '\\', '\0']) || name == "." || name == ".." {
                        return Err(Error::Permission);
                    }
                    serde_json::to_value(
                        self.move_entry(path, &path.parent().unwrap_or(Path::new("")).join(name))?,
                    )
                    .map_err(|_| Error::Protocol)?
                }
                FsQuery::Move(target) => serde_json::to_value(self.move_entry(path, &target)?)
                    .map_err(|_| Error::Protocol)?,
                FsQuery::Duplicate(target) => {
                    let bytes = self.read(path, 32 * 1024 * 1024)?;
                    host::atomic_save(&self.root, &Self::relative(&target)?, &bytes, None)?;
                    serde_json::Value::Null
                }
                FsQuery::DuplicateSibling(name) => {
                    if name.contains(['/', '\\', '\0']) || name == "." || name == ".." {
                        return Err(Error::Permission);
                    }
                    let target = path.parent().unwrap_or(Path::new("")).join(name);
                    let bytes = self.read(path, 32 * 1024 * 1024)?;
                    host::atomic_save(&self.root, &Self::relative(&target)?, &bytes, None)?;
                    serde_json::to_value(self.actual(&target)?).map_err(|_| Error::Protocol)?
                }
                FsQuery::Trash => {
                    let (parent, name) = host::parent(&self.root, &Self::relative(path)?)?;
                    let trash = host::private_directory(&self.state.join("trash"))?;
                    host::rename_noreplace(
                        &parent,
                        &name,
                        &trash,
                        std::ffi::OsStr::new(&host::random_id()?),
                    )?;
                    parent.sync_all().map_err(|_| Error::Storage)?;
                    trash.sync_all().map_err(|_| Error::Storage)?;
                    serde_json::Value::Null
                }
            })
        })();
        run.map_err(|e| format!("Remote filesystem operation: {e}"))
            .and_then(value)
    }
    #[allow(clippy::too_many_arguments)] // Mirrors the shared search engine API.
    pub fn search(
        &self,
        roots: &[PathBuf],
        filter: &str,
        query: &str,
        replacement: Option<&str>,
        options: &search::Options,
        open: &std::collections::HashMap<PathBuf, String>,
        cancel: &AtomicBool,
    ) -> std::result::Result<search::Results, String> {
        let regex = options.compile(query)?;
        let mut result = search::Results::default();
        let mut retained = 0;
        for path in self.walk(roots, cancel).map_err(|e| e.to_string())? {
            if !filter.is_empty() && !search::glob(filter, &path.to_string_lossy()) {
                continue;
            }
            let actual = self.actual(&path).map_err(|e| e.to_string())?;
            let loaded = host::read_encoded(
                &self.root,
                &Self::relative(&path).map_err(|e| e.to_string())?,
            );
            let (disk, encoding, bom, hash) = match loaded {
                Ok(v) => v,
                Err(_) => {
                    result.skipped += 1;
                    continue;
                }
            };
            let text = open.get(&actual).unwrap_or(&disk);
            let found = regex.find_iter(text).take(2001).collect::<Vec<_>>();
            if found.is_empty() {
                continue;
            }
            if found.len() > 2000
                || result.hits.len() + found.len() > 20000
                || retained + text.len() > 16 * 1024 * 1024
            {
                result.truncated = true;
                break;
            }
            let lines = text.lines().collect::<Vec<_>>();
            let mut contexts = std::collections::BTreeMap::new();
            for matched in &found {
                let line = text[..matched.start()]
                    .bytes()
                    .filter(|b| *b == b'\n')
                    .count();
                let offset = text[..matched.start()].rfind('\n').map_or(0, |p| p + 1);
                let column = text[offset..matched.start()].chars().count();
                result.hits.push(search::Hit {
                    path: actual.clone(),
                    line: line + 1,
                    column: column + 1,
                    text: lines.get(line).unwrap_or(&"").chars().take(1000).collect(),
                });
                for n in line.saturating_sub(1)..=(line + 1).min(lines.len().saturating_sub(1)) {
                    contexts.entry(n).or_insert(search::ContextLine {
                        line: n + 1,
                        text: lines.get(n).unwrap_or(&"").chars().take(1000).collect(),
                        column: (n == line).then_some(column + 1),
                    });
                }
            }
            retained += text.len();
            result.files.push(search::FileHits {
                path: actual.clone(),
                count: found.len(),
                lines: contexts.into_values().collect(),
            });
            if let Some(replacement) = replacement {
                let after = if options.regex {
                    regex.replace_all(text, replacement).into_owned()
                } else {
                    regex
                        .replace_all(text, regex::NoExpand(replacement))
                        .into_owned()
                };
                let hash: [u8; 32] = hash.try_into().map_err(|_| "Invalid source identity")?;
                result.plans.push(search::FilePlan {
                    path: actual.clone(),
                    open: open.contains_key(&actual),
                    before: text.clone(),
                    after,
                    hash,
                    encoding,
                    bom,
                    selected: true,
                });
            }
        }
        Ok(result)
    }
}
