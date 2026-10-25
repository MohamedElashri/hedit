use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Folder and file name patterns hidden from the folder tree, Go to file, and
/// Find in folders, like Sublime's `folder_exclude_patterns`.
pub const DEFAULT_FOLDER_EXCLUDES: [&str; 7] = [
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    ".venv",
    "__pycache__",
];
pub const DEFAULT_FILE_EXCLUDES: [&str; 2] = ["*.pyc", ".DS_Store"];

struct Excludes {
    folders: globset::GlobSet,
    files: globset::GlobSet,
}

fn glob_set(patterns: &[impl AsRef<str>]) -> Result<globset::GlobSet, String> {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        let pattern = pattern.as_ref();
        builder.add(
            globset::GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
                .map_err(|e| format!("Invalid pattern “{pattern}”: {e}"))?,
        );
    }
    builder.build().map_err(|e| e.to_string())
}

static EXCLUDES: RwLock<Option<Arc<Excludes>>> = RwLock::new(None);

/// Check exclusion patterns without applying them.
pub fn validate_excludes(folders: &[String], files: &[String]) -> Result<(), String> {
    glob_set(folders).and(glob_set(files)).map(|_| ())
}

/// Replace the exclusion patterns used by every folder walk in this process.
pub fn set_excludes(folders: &[String], files: &[String]) -> Result<(), String> {
    let excludes = Excludes {
        folders: glob_set(folders)?,
        files: glob_set(files)?,
    };
    *EXCLUDES.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(excludes));
    Ok(())
}

/// Whether a folder or file with this name is hidden from folder walks.
pub fn excluded(name: &str, directory: bool) -> bool {
    let current = EXCLUDES.read().unwrap_or_else(|e| e.into_inner()).clone();
    match current {
        Some(excludes) if directory => excludes.folders.is_match(name),
        Some(excludes) => excludes.files.is_match(name),
        None if directory => DEFAULT_FOLDER_EXCLUDES.contains(&name),
        None => name == ".DS_Store" || name.ends_with(".pyc"),
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderEntry {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub is_symlink: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderListing {
    pub name: String,
    pub path: String,
    pub entries: Vec<FolderEntry>,
    pub truncated: bool,
    pub unsupported_names: bool,
}

pub fn read_directory(path: PathBuf) -> Result<FolderListing, String> {
    let path = fs::canonicalize(path).map_err(|e| e.to_string())?;
    let display_path = path_string(&path)?;
    let mut entries = Vec::new();
    let mut truncated = false;
    let mut unsupported_names = false;
    for (visited, entry) in fs::read_dir(&path).map_err(|e| e.to_string())?.enumerate() {
        if visited >= 2000 {
            truncated = true;
            break;
        }
        let entry = entry.map_err(|e| e.to_string())?;
        // Folder paths are UTF-8. Never expose a lossy path that could name a
        // different file (for example a literal Unicode replacement character).
        if entry.file_name().to_str().is_none() {
            unsupported_names = true;
            continue;
        }
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        // Follow symlinks, but omit broken links and special files such as pipes.
        let (is_directory, is_file) = if kind.is_symlink() {
            match fs::metadata(entry.path()) {
                Ok(metadata) => (metadata.is_dir(), metadata.is_file()),
                Err(_) => continue,
            }
        } else {
            (kind.is_dir(), kind.is_file())
        };
        if (is_directory || is_file)
            && !excluded(&entry.file_name().to_string_lossy(), is_directory)
        {
            entries.push(FolderEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry.path().to_string_lossy().into_owned(),
                is_directory,
                is_symlink: kind.is_symlink(),
            });
        }
    }
    entries.sort_by_cached_key(|entry| (!entry.is_directory, entry.name.to_lowercase()));
    Ok(FolderListing {
        name: path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned(),
        path: display_path,
        entries,
        truncated,
        unsupported_names,
    })
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickOpen {
    pub entries: Vec<FolderEntry>,
    pub truncated: bool,
    pub skipped_directories: u64,
}

/// Where the letters of `query` appear in order in `text`, ignoring case and
/// spaces in the query, as character indices with a score (higher is better).
/// Letters at the start of words are preferred, so `tglc` matches the T, l, and
/// c of "Toggle line comment" rather than letters inside "Toggle".
pub fn fuzzy_match(text: &str, query: &str) -> Option<(i64, Vec<usize>)> {
    let chars: Vec<char> = text.chars().collect();
    let lower: Vec<char> = chars
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    let wanted: Vec<char> = query
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let boundary = |i: usize| {
        i == 0
            || !chars[i - 1].is_alphanumeric()
            || (chars[i].is_uppercase() && chars[i - 1].is_lowercase())
    };
    // Whether wanted[w..] still appears in order from `from`.
    let rest_fits = |w: usize, from: usize| {
        let mut letters = lower[from.min(lower.len())..].iter();
        wanted[w..].iter().all(|c| letters.any(|l| l == c))
    };
    let mut positions = Vec::with_capacity(wanted.len());
    let mut from = 0;
    for (w, &c) in wanted.iter().enumerate() {
        let mut at = (from..lower.len()).find(|&i| lower[i] == c)?;
        if !boundary(at) {
            if let Some(word) = (at + 1..lower.len())
                .find(|&i| lower[i] == c && boundary(i) && rest_fits(w + 1, i + 1))
            {
                at = word;
            }
        }
        positions.push(at);
        from = at + 1;
    }
    let mut score = -(chars.len() as i64) / 4;
    for (n, &at) in positions.iter().enumerate() {
        score += 10 - at as i64 / 2;
        if boundary(at) {
            score += 15;
        }
        if n > 0 && positions[n - 1] + 1 == at {
            score += 8;
        }
    }
    Some((score, positions))
}

pub fn fuzzy_score(path: &str, query: &str) -> Option<i64> {
    let path = path.to_lowercase().replace('\\', "/");
    let mut wanted = query.chars().peekable();
    let mut score = 0;
    let mut run = 0;
    let mut boundary = true;
    for (index, ch) in path.chars().enumerate() {
        if wanted.peek() == Some(&ch) {
            wanted.next();
            run += 1;
            score += 20 + run * 8 + if boundary { 30 } else { 0 } - index as i64;
        } else {
            run = 0;
        }
        boundary = matches!(ch, '/' | '\\' | '_' | '-' | '.' | ' ');
        if wanted.peek().is_none() {
            return Some(score - path.len() as i64);
        }
    }
    if wanted.peek().is_none() {
        Some(-(path.len() as i64))
    } else {
        None
    }
}

pub fn quick_open(root: PathBuf, query: &str) -> Result<QuickOpen, String> {
    let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    path_string(&root)?;
    if !root.is_dir() {
        return Err("Choose a folder to search.".into());
    }
    quick_open_roots(&[root], query)
}

/// Label paths with their root when several folders are open. Equal root names
/// use full paths so results from unrelated projects remain distinguishable.
pub fn display_path(roots: &[PathBuf], path: &Path) -> String {
    let Some(root) = roots
        .iter()
        .filter(|root| path.starts_with(root))
        .max_by_key(|root| root.components().count())
    else {
        return path.to_string_lossy().into_owned();
    };
    let relative = path.strip_prefix(root).unwrap_or(path);
    if roots.len() == 1 {
        return relative.to_string_lossy().into_owned();
    }
    let leaf = root.file_name().unwrap_or(root.as_os_str());
    let unique = roots
        .iter()
        .filter(|other| other.file_name().unwrap_or(other.as_os_str()) == leaf)
        .count()
        == 1;
    let label = if unique { leaf } else { root.as_os_str() };
    // Verbatim Windows paths only recognize backslashes as separators. Join
    // natively so a canonical root never becomes a mixed-separator label.
    Path::new(label)
        .join(relative)
        .to_string_lossy()
        .into_owned()
}

/// Search all roots with a shared entry, time and result budget. Traversal is
/// breadth-first so a large first root does not immediately starve later roots.
pub fn quick_open_roots(roots: &[PathBuf], query: &str) -> Result<QuickOpen, String> {
    let query = query.to_lowercase().replace('\\', "/");
    let mut skipped_directories = 0;
    let mut unique_roots = std::collections::HashSet::new();
    let roots: Vec<_> = roots
        .iter()
        .filter_map(|root| {
            let root = fs::canonicalize(root).unwrap_or_else(|_| root.clone());
            if root.to_str().is_none() {
                skipped_directories += 1;
                None
            } else if unique_roots.insert(root.clone()) {
                Some(root)
            } else {
                None
            }
        })
        .collect();
    let mut directories: std::collections::VecDeque<_> = roots.iter().cloned().collect();
    let mut visited_directories = std::collections::HashSet::new();
    let mut visited_files = std::collections::HashSet::new();
    let mut best: Vec<(i64, FolderEntry)> = Vec::new();
    let mut visited = 0;
    let started = std::time::Instant::now();
    let mut truncated = false;
    'scan: while let Some(directory) = directories.pop_front() {
        if !visited_directories.insert(directory.clone()) {
            continue;
        }
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(_) => {
                skipped_directories += 1;
                continue;
            }
        };
        for entry in entries {
            if visited >= 50_000 || started.elapsed().as_millis() >= 1000 {
                truncated = true;
                break 'scan;
            }
            visited += 1;
            let Ok(entry) = entry else {
                skipped_directories += 1;
                continue;
            };
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if path.to_str().is_none() {
                truncated = true;
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if kind.is_dir() {
                if excluded(&name, true) {
                    continue;
                }
                directories.push_back(path);
            } else if (kind.is_file() || (kind.is_symlink() && path.is_file()))
                && !excluded(&name, false)
            {
                if !visited_files.insert(path.clone()) {
                    continue;
                }
                let relative = display_path(&roots, &path);
                if let Some(score) = fuzzy_score(&relative, &query) {
                    let entry = FolderEntry {
                        name: relative,
                        path: path.to_string_lossy().into_owned(),
                        is_directory: false,
                        is_symlink: kind.is_symlink(),
                    };
                    let candidate = (score, entry);
                    let index = best.partition_point(|prior| {
                        prior.0 > candidate.0
                            || (prior.0 == candidate.0 && prior.1.name <= candidate.1.name)
                    });
                    if index < 80 {
                        best.insert(index, candidate);
                        best.truncate(80);
                    }
                }
            }
        }
    }
    Ok(QuickOpen {
        entries: best.into_iter().map(|(_, entry)| entry).collect(),
        truncated,
        skipped_directories,
    })
}

fn leaf_name(name: &str) -> Result<&str, String> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains('\0')
    {
        return Err(format!("Invalid file name: {name}"));
    }
    Ok(trimmed)
}

fn path_string(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| "This path is not valid UTF-8. Manage it in the system file manager.".into())
}

fn canonical_path(path: &str) -> Result<PathBuf, String> {
    fs::canonicalize(path).map_err(|e| e.to_string())
}

/// Resolve ancestors for a folder action, keeping the named symlink itself.
/// Use this path for buffer matching before renaming or removing the entry.
pub fn entry_path(path: &Path) -> Result<PathBuf, String> {
    let leaf = path
        .file_name()
        .ok_or("A filesystem root cannot be changed.")?;
    // Reject terminal `.` / `..`, which Path components otherwise normalize.
    if path.as_os_str() != path.with_file_name(leaf).as_os_str() {
        return Err("Choose a file or folder entry.".into());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    Ok(fs::canonicalize(parent)
        .map_err(|e| e.to_string())?
        .join(leaf))
}

fn require_absent(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
        Ok(_) => Err(format!(
            "A file or folder named {} already exists.",
            path.file_name().unwrap_or_default().to_string_lossy()
        )),
    }
}

/// Create an empty file inside `parent`. Fails when the target already exists.
pub fn create_file(parent: String, name: String) -> Result<String, String> {
    let leaf = leaf_name(&name)?.to_owned();
    let parent = canonical_path(&parent)?;
    if !parent.is_dir() {
        return Err("The parent folder is no longer available.".into());
    }
    let target = parent.join(leaf);
    let result = path_string(&target)?;
    require_absent(&target)?;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|e| e.to_string())?;
    Ok(result)
}

/// Create a folder inside `parent`. Fails when the target already exists.
pub fn create_folder(parent: String, name: String) -> Result<String, String> {
    let leaf = leaf_name(&name)?.to_owned();
    let parent = canonical_path(&parent)?;
    if !parent.is_dir() {
        return Err("The parent folder is no longer available.".into());
    }
    let target = parent.join(leaf);
    let result = path_string(&target)?;
    require_absent(&target)?;
    fs::create_dir(&target).map_err(|e| e.to_string())?;
    Ok(result)
}

/// Rename `from` to a sibling leaf `name`. Never overwrites an existing path.
pub fn rename_path(from: String, name: String) -> Result<String, String> {
    let leaf = leaf_name(&name)?.to_owned();
    let from = entry_path(Path::new(&from))?;
    fs::symlink_metadata(&from).map_err(|e| e.to_string())?;
    let parent = from
        .parent()
        .ok_or("This path cannot be renamed.")?
        .to_path_buf();
    let target = parent.join(leaf);
    if target == from {
        return path_string(&from);
    }
    let result = path_string(&target)?;
    require_absent(&target)?;
    rename_no_replace(&from, &target).map_err(|e| e.to_string())?;
    Ok(result)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn rename_no_replace(from: &Path, target: &Path) -> std::io::Result<()> {
    use rustix::fs::{renameat_with, RenameFlags, CWD};
    // Fail if the filesystem cannot enforce exclusivity; never fall back to rename.
    renameat_with(CWD, from, CWD, target, RenameFlags::NOREPLACE).map_err(Into::into)
}

/// Move within a filesystem without replacing an existing destination.
pub fn move_path(from: &Path, target: &Path) -> Result<PathBuf, String> {
    let from = entry_path(from)?;
    let parent = fs::canonicalize(target.parent().ok_or("Choose a destination folder.")?)
        .map_err(|e| e.to_string())?;
    let target = parent.join(target.file_name().ok_or("Choose a destination name.")?);
    require_absent(&target)?;
    if target.starts_with(&from) {
        return Err("A folder cannot be moved into itself.".into());
    }
    rename_no_replace(&from, &target).map_err(|e| e.to_string())?;
    Ok(target)
}

/// Duplicate regular files and directories; never follow symlinks or overwrite.
pub fn duplicate_path(from: &Path, target: &Path) -> Result<(), String> {
    let from = entry_path(from)?;
    let parent = fs::canonicalize(target.parent().ok_or("Choose a destination folder.")?)
        .map_err(|e| e.to_string())?;
    let target = parent.join(target.file_name().ok_or("Choose a destination name.")?);
    fn copy(from: &Path, to: &Path, depth: usize, count: &mut usize) -> Result<(), String> {
        *count += 1;
        if depth > 32 || *count > 50_000 {
            return Err("Copy exceeds folder depth/file limits.".into());
        }
        let meta = fs::symlink_metadata(from).map_err(|e| e.to_string())?;
        if meta.is_symlink() {
            return Err(
                "Duplicating symlinks is not supported; copy their targets explicitly.".into(),
            );
        }
        if meta.is_dir() {
            fs::create_dir(to).map_err(|e| e.to_string())?;
            for entry in fs::read_dir(from).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                copy(&entry.path(), &to.join(entry.file_name()), depth + 1, count)?;
            }
        } else if meta.is_file() {
            use std::io::Write;
            let mut input = fs::File::open(from).map_err(|e| e.to_string())?;
            let mut output = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(to)
                .map_err(|e| e.to_string())?;
            std::io::copy(&mut input, &mut output).map_err(|e| e.to_string())?;
            output.flush().map_err(|e| e.to_string())?;
            output
                .set_permissions(meta.permissions())
                .map_err(|e| e.to_string())?;
        } else {
            return Err("Only regular files and folders can be duplicated.".into());
        }
        Ok(())
    }
    require_absent(&target)?;
    if target.starts_with(&from) {
        return Err("A folder cannot be copied into itself.".into());
    }
    copy(&from, &target, 0, &mut 0)
}
pub fn duplicate_sibling(from: &Path, name: &str) -> Result<PathBuf, String> {
    let name = leaf_name(name)?;
    let target = from.parent().ok_or("Choose a file or folder.")?.join(name);
    duplicate_path(from, &target)?;
    Ok(target)
}
pub fn trash_path(path: &Path) -> Result<(), String> {
    let path = entry_path(path)?;
    if path.parent().is_none() {
        return Err("Cannot trash a filesystem root.".into());
    }
    trash::delete(path).map_err(|e| e.to_string())
}

#[cfg(windows)]
fn rename_no_replace(from: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: Both paths are NUL-terminated and live for the duration of the call.
    // Zero flags excludes MOVEFILE_REPLACE_EXISTING, making the rename exclusive.
    if unsafe { MoveFileExW(from.as_ptr(), target.as_ptr(), 0) } == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn rename_no_replace(_from: &Path, _target: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Renaming without overwriting is unavailable on this platform.",
    ))
}

/// Reveal `path` in Finder, Explorer, or the Linux file manager. Best effort.
pub fn reveal_in_file_manager(path: String) -> Result<(), String> {
    let path = canonical_path(&path)?;
    let is_dir = path.is_dir();
    #[cfg(target_os = "macos")]
    {
        let mut command = std::process::Command::new("open");
        if is_dir {
            command.arg(&path);
        } else {
            command.arg("-R").arg(&path);
        }
        command.spawn().map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        if is_dir {
            std::process::Command::new("explorer")
                .arg(&path)
                .spawn()
                .map_err(|e| e.to_string())?;
        } else {
            std::process::Command::new("explorer")
                .arg(format!("/select,{}", path.to_string_lossy()))
                .spawn()
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let target = if is_dir {
            path.clone()
        } else {
            path.parent()
                .ok_or("This path cannot be revealed.")?
                .to_path_buf()
        };
        std::process::Command::new("xdg-open")
            .arg(&target)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
