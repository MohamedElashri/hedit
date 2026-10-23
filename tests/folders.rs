use hedit::{folder, session::FolderView, workspace::Workspace};
use std::fs;

#[test]
fn listing_is_lazy_sorted_and_includes_hidden_files() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("z-folder")).unwrap();
    fs::write(root.path().join("z-folder/nested.rs"), "").unwrap();
    fs::write(root.path().join("Alpha.rs"), "").unwrap();
    fs::write(root.path().join(".hidden"), "").unwrap();
    let listing = folder::read_directory(root.path().into()).unwrap();
    assert_eq!(
        listing
            .entries
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        ["z-folder", ".hidden", "Alpha.rs"]
    );
    assert!(listing.entries[0].is_directory);
    assert!(!listing.truncated);
    assert!(folder::read_directory(root.path().join("absent")).is_err());
}

#[test]
fn fuzzy_search_finds_nested_paths_and_skips_generated_trees() {
    let root = tempfile::tempdir().unwrap();
    for name in ["src", ".git", "node_modules", "target", ".venv"] {
        fs::create_dir(root.path().join(name)).unwrap();
        fs::write(root.path().join(name).join("project.rs"), "").unwrap();
    }
    let results = folder::quick_open(root.path().into(), "SRCPRJ").unwrap();
    assert_eq!(results.entries.len(), 1);
    assert_eq!(results.entries[0].name, "src/project.rs");
    let all = folder::quick_open(root.path().into(), "").unwrap();
    assert_eq!(all.entries.len(), 1);
    assert!(folder::quick_open(root.path().join("src/project.rs"), "").is_err());
}

#[test]
fn quick_open_and_directory_listing_report_limits() {
    let root = tempfile::tempdir().unwrap();
    for i in 0..2001 {
        fs::write(root.path().join(format!("file-{i:04}")), "").unwrap();
    }
    let listing = folder::read_directory(root.path().into()).unwrap();
    assert_eq!(listing.entries.len(), 2000);
    assert!(listing.truncated);
    let results = folder::quick_open(root.path().into(), "file").unwrap();
    assert_eq!(results.entries.len(), 80);
}

#[test]
fn creation_refuses_overwrites_and_path_traversal() {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().to_str().unwrap().to_owned();
    let path = folder::create_file(parent.clone(), "notes.rs".into()).unwrap();
    fs::write(&path, "keep this").unwrap();
    assert!(folder::create_file(parent.clone(), "notes.rs".into()).is_err());
    assert!(folder::create_folder(parent.clone(), "notes.rs".into()).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "keep this");
    for name in ["", ".", "..", "../outside", "sub/file", "sub\\file"] {
        assert!(folder::create_file(parent.clone(), name.into()).is_err());
        assert!(folder::create_folder(parent.clone(), name.into()).is_err());
    }
    let sub = folder::create_folder(parent, "sources".into()).unwrap();
    assert!(std::path::Path::new(&sub).is_dir());
}

#[test]
fn folder_and_expansion_survive_restart_even_if_folder_goes_missing() {
    let session = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_str().unwrap().to_owned();
    let (mut workspace, _) = Workspace::restore(session.path().into()).unwrap();
    workspace.folders = vec![FolderView {
        path: path.clone(),
        visible: false,
        expanded: vec![format!("{path}/src")],
        collapsed: false,
    }];
    workspace.open[0].page.text = "unsaved code".into();
    workspace.edit(0, "").unwrap();
    workspace.checkpoint().unwrap();
    drop(workspace);
    drop(root);
    let (workspace, errors) = Workspace::restore(session.path().into()).unwrap();
    assert!(errors.is_empty());
    assert_eq!(workspace.open[0].page.text, "unsaved code");
    let folder = workspace.folders.first().unwrap();
    assert_eq!(folder.path, path);
    assert!(!folder.visible);
    assert_eq!(folder.expanded, [format!("{path}/src")]);
    let old: FolderView = serde_json::from_str(r#"{"path":"/tmp","visible":true}"#).unwrap();
    assert!(old.expanded.is_empty());
}

#[cfg(unix)]
#[test]
fn symlink_cycles_are_listed_but_not_followed_by_search() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("main.rs"), "").unwrap();
    symlink(root.path(), root.path().join("cycle")).unwrap();
    symlink(root.path().join("missing"), root.path().join("broken")).unwrap();
    let listing = folder::read_directory(root.path().into()).unwrap();
    assert_eq!(listing.entries.len(), 2);
    assert!(listing.entries[0].is_symlink);
    assert!(listing.entries[0].is_directory);
    let results = folder::quick_open(root.path().into(), "main").unwrap();
    assert_eq!(results.entries.len(), 1);
}

#[test]
fn multiple_roots_search_all_folders_and_distinguish_duplicate_names() {
    let directory = tempfile::tempdir().unwrap();
    let roots: Vec<_> = ["client/project", "server/project"]
        .iter()
        .map(|name| directory.path().join(name))
        .collect();
    for root in &roots {
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "").unwrap();
    }
    let results = folder::quick_open_roots(&roots, "srcmain").unwrap();
    assert_eq!(results.entries.len(), 2);
    assert_ne!(results.entries[0].name, results.entries[1].name);
    assert!(results
        .entries
        .iter()
        .any(|e| e.name.contains("client/project/src/main.rs")));
    assert!(results
        .entries
        .iter()
        .any(|e| e.name.contains("server/project/src/main.rs")));
    let filtered = folder::quick_open_roots(&roots, "server").unwrap();
    assert_eq!(filtered.entries.len(), 1);
    let mut with_missing = roots.clone();
    with_missing.push(directory.path().join("missing"));
    let results = folder::quick_open_roots(&with_missing, "main").unwrap();
    assert_eq!(results.entries.len(), 2);
    assert_eq!(results.skipped_directories, 1);
}

#[test]
fn overlapping_roots_do_not_duplicate_files_and_share_a_result_limit() {
    let directory = tempfile::tempdir().unwrap();
    let child = directory.path().join("nested");
    fs::create_dir(&child).unwrap();
    for i in 0..100 {
        fs::write(child.join(format!("file-{i}.rs")), "").unwrap();
    }
    let roots = [directory.path().into(), child.clone()];
    let results = folder::quick_open_roots(&roots, "").unwrap();
    assert_eq!(results.entries.len(), 80);
    let unique: std::collections::HashSet<_> = results.entries.iter().map(|e| &e.path).collect();
    assert_eq!(unique.len(), 80);
    assert_eq!(
        folder::display_path(&roots, &child.join("file-0.rs")),
        "nested/file-0.rs"
    );
}

#[test]
fn several_roots_restore_visibility_expansions_and_collapsed_state() {
    let session = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(session.path().into()).unwrap();
    workspace.folders = ["/project/client", "/project/server"]
        .iter()
        .enumerate()
        .map(|(index, path)| FolderView {
            path: (*path).into(),
            visible: false,
            expanded: vec![format!("{path}/src")],
            collapsed: index == 1,
        })
        .collect();
    workspace.checkpoint().unwrap();
    drop(workspace);
    let (workspace, errors) = Workspace::restore(session.path().into()).unwrap();
    assert!(errors.is_empty());
    assert_eq!(workspace.folders.len(), 2);
    assert_eq!(workspace.folders[0].path, "/project/client");
    assert!(!workspace.folders[0].visible);
    assert_eq!(workspace.folders[1].expanded, ["/project/server/src"]);
    assert!(workspace.folders[1].collapsed);
}

#[test]
fn single_folder_manifests_migrate_to_multiple_roots_without_losing_buffers() {
    let session = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(session.path().into()).unwrap();
    workspace.folders.push(FolderView {
        path: "/old/project".into(),
        visible: false,
        expanded: vec!["/old/project/src".into()],
        collapsed: false,
    });
    workspace.open[0].page.text = "old unsaved notes".into();
    workspace.edit(0, "").unwrap();
    workspace.checkpoint().unwrap();
    drop(workspace);
    let path = session.path().join("session.json");
    let mut saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    saved.as_object_mut().unwrap().remove("folders");
    saved["folder"].as_object_mut().unwrap().remove("collapsed");
    fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
    let (workspace, errors) = Workspace::restore(session.path().into()).unwrap();
    assert!(errors.is_empty());
    assert_eq!(workspace.folders.len(), 1);
    assert_eq!(workspace.folders[0].path, "/old/project");
    assert!(!workspace.folders[0].collapsed);
    assert!(!workspace.folders[0].visible);
    assert_eq!(workspace.open[0].page.text, "old unsaved notes");
}

#[test]
fn exclusion_patterns_hide_folders_and_files_everywhere() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("zz-hidden-build/deep")).unwrap();
    std::fs::write(root.path().join("zz-hidden-build/deep/inner.txt"), "x").unwrap();
    std::fs::write(root.path().join("keep.txt"), "x").unwrap();
    std::fs::write(root.path().join("notes.zzlog"), "x").unwrap();
    let defaults = |extra_folder: &[&str], extra_file: &[&str]| {
        let folders: Vec<String> = folder::DEFAULT_FOLDER_EXCLUDES
            .iter()
            .chain(extra_folder)
            .map(|s| s.to_string())
            .collect();
        let files: Vec<String> = folder::DEFAULT_FILE_EXCLUDES
            .iter()
            .chain(extra_file)
            .map(|s| s.to_string())
            .collect();
        folder::set_excludes(&folders, &files).unwrap();
    };
    // Keep the defaults so concurrent tests in this binary see no change.
    defaults(&["zz-hidden-*"], &["*.zzlog"]);
    let listing = folder::read_directory(root.path().into()).unwrap();
    let names: Vec<_> = listing.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["keep.txt"]);
    let all = folder::quick_open(root.path().into(), "").unwrap();
    assert!(all.entries.iter().all(|e| e.name == "keep.txt"));
    assert!(folder::set_excludes(&["[".into()], &[])
        .unwrap_err()
        .contains("Invalid pattern"));
    defaults(&[], &[]);
    let listing = folder::read_directory(root.path().into()).unwrap();
    assert_eq!(listing.entries.len(), 3);
}
