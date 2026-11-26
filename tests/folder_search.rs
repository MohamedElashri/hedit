use hedit::{folder, search};
use std::{collections::HashMap, fs, path::PathBuf, sync::atomic::AtomicBool};

fn search_files(
    root: PathBuf,
    filter: &str,
    open: HashMap<PathBuf, String>,
) -> Result<search::Results, String> {
    search::folders(
        &[root],
        filter,
        "needle",
        None,
        &search::Options::default(),
        &open,
        &AtomicBool::new(false),
    )
}

#[test]
fn brace_glob_filter_keeps_commas_inside_alternatives() {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("notes.rs"), "needle").unwrap();
    fs::write(d.path().join("notes.md"), "needle").unwrap();
    fs::write(d.path().join("notes.txt"), "needle").unwrap();
    let results = search_files(d.path().into(), "*.{rs,md}", HashMap::new()).unwrap();
    assert_eq!(results.hits.len(), 2);
}

#[test]
fn unsaved_named_files_honor_git_info_exclude() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    assert!(std::process::Command::new("git")
        .arg("init")
        .arg("-q")
        .arg(&root)
        .status()
        .unwrap()
        .success());
    fs::write(root.join(".git/info/exclude"), "ignored-*.txt\n").unwrap();
    fs::write(root.join("ignored-disk.txt"), "needle").unwrap();
    let control = search_files(root.clone(), "*.txt", HashMap::new()).unwrap();
    assert!(control.hits.is_empty());
    let results = search_files(
        root.clone(),
        "*.txt",
        HashMap::from([(root.join("ignored-draft.txt"), "needle".into())]),
    )
    .unwrap();
    assert!(
        results.hits.is_empty(),
        "ignored draft appeared in search: {:?}",
        results.hits
    );
}

#[test]
fn folder_named_like_workspace_state_remains_searchable() {
    let d = tempfile::tempdir().unwrap();
    let nested = d.path().join("archive.hedit-workspace");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("notes.txt"), "needle").unwrap();
    assert!(folder::read_directory(d.path().into())
        .unwrap()
        .entries
        .iter()
        .any(|e| e.name == "archive.hedit-workspace"));
    let results = search_files(d.path().into(), "*.txt", HashMap::new()).unwrap();
    assert_eq!(results.hits.len(), 1);
}

#[cfg(unix)]
#[test]
fn unreadable_subfolders_contribute_to_skipped_results() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let locked = d.path().join("restricted");
    fs::create_dir(&locked).unwrap();
    fs::write(locked.join("notes.txt"), "needle").unwrap();
    fs::write(d.path().join("ordinary.txt"), "needle").unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    assert_eq!(
        fs::read_dir(&locked).err().unwrap().kind(),
        std::io::ErrorKind::PermissionDenied
    );
    let results = search_files(d.path().into(), "*.txt", HashMap::new());
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
    let results = results.unwrap();
    assert_eq!(results.hits.len(), 1);
    assert!(
        results.skipped > 0 || results.truncated,
        "incomplete search was reported as complete"
    );
}

#[test]
fn selection_search_does_not_let_an_unselected_match_block_a_selected_match() {
    let options = search::Options::default();
    let hits = search::matches("aaa", "aa", &options, 1..3).unwrap();
    let replaced = search::replace("aaa", "aa", "x", &options, 1..3).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0], 1..3);
    assert_eq!(replaced, ("ax".into(), 1));
}
