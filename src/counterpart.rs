//! Find a same-stem C-family header or implementation without reading file contents.
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

const HEADERS: &[&str] = &["h", "hh", "hpp", "hxx", "h++", "cuh"];
const SOURCES: &[&str] = &["c", "cc", "cpp", "cxx", "c++", "cu", "m", "mm"];

fn extensions(path: &Path) -> &'static [&'static str] {
    let extension = path.extension().unwrap_or_default().to_ascii_lowercase();
    if HEADERS.iter().any(|e| extension == *e) {
        SOURCES
    } else if SOURCES.iter().any(|e| extension == *e) {
        HEADERS
    } else {
        &[]
    }
}

pub fn supported(path: &Path) -> bool {
    !extensions(path).is_empty()
}

/// Prefer siblings; otherwise search only the most specific containing project.
/// Folder exclusions, Git ignores, cancellation, and limits follow project search.
pub fn find(source: &Path, roots: &[PathBuf], cancel: &AtomicBool) -> Result<Vec<PathBuf>, String> {
    let targets = extensions(source);
    if targets.is_empty() {
        return Ok(vec![]);
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("Search cancelled.".into());
    }
    let source = source.canonicalize().map_err(|e| e.to_string())?;
    let parent = source.parent().ok_or("The file has no parent directory.")?;
    let matches = |path: &Path| {
        path.file_stem() == source.file_stem()
            && targets
                .iter()
                .any(|e| path.extension().unwrap_or_default().to_ascii_lowercase() == *e)
    };
    let mut found = BTreeSet::new();
    for entry in std::fs::read_dir(parent).map_err(|e| e.to_string())? {
        if cancel.load(Ordering::Relaxed) {
            return Err("Search cancelled.".into());
        }
        let path = entry.map_err(|e| e.to_string())?.path();
        if matches(&path) && path.is_file() {
            let path = path.canonicalize().map_err(|e| e.to_string())?;
            if path != source {
                found.insert(path);
            }
        }
    }
    if !found.is_empty() {
        return Ok(found.into_iter().collect());
    }
    let root = roots
        .iter()
        .filter_map(|r| r.canonicalize().ok())
        .filter(|r| source.starts_with(r))
        .max_by_key(|r| r.components().count());
    if let Some(root) = root {
        for path in crate::search::walk(&[root], "", cancel)? {
            if path != source && matches(&path) {
                found.insert(path);
            }
        }
    }
    Ok(found.into_iter().collect())
}
