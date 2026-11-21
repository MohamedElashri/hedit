use super::*;
fn root(path: &Path, rights: &[Right], protected: Vec<PathBuf>) -> Root {
    Root::open_selected(
        path,
        Policy::new(rights.iter().copied()).unwrap(),
        Protected::new(protected).unwrap(),
    )
    .unwrap()
}
fn budget(cancel: &AtomicBool) -> Budget<'_> {
    Budget::new(cancel, Duration::from_secs(2), 4 * 1024 * 1024)
}
#[test]
fn separate_rights_unicode_and_cumulative_budgets_are_enforced() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("café.txt"), "café / 東京\r\n🦀").unwrap();
    let cancel = AtomicBool::new(false);
    let discover = root(directory.path(), &[Right::Discover], vec![]);
    assert_eq!(
        discover
            .discover("", &mut budget(&cancel))
            .unwrap()
            .entries
            .len(),
        1
    );
    assert!(discover.read("café.txt", &mut budget(&cancel)).is_err());
    let reader = root(directory.path(), &[Right::Read], vec![]);
    assert!(reader.discover("", &mut budget(&cancel)).is_err());
    let snapshot = reader.read("café.txt", &mut budget(&cancel)).unwrap();
    assert_eq!(snapshot.text, "café / 東京\r\n🦀");
    let mut small = Budget::new(&cancel, Duration::from_secs(2), snapshot.text.len());
    reader.read("café.txt", &mut small).unwrap();
    assert!(reader.read("café.txt", &mut small).is_err());
    cancel.store(true, Ordering::Release);
    assert!(reader.read("café.txt", &mut budget(&cancel)).is_err());
    assert!(Policy::new([Right::Edit]).is_err());
}
#[test]
fn traversal_hidden_components_device_aliases_and_binary_data_are_refused() {
    let directory = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let reader = root(directory.path(), &[Right::Read, Right::Discover], vec![]);
    for path in [
        "",
        "../outside",
        "a/../outside",
        "/outside",
        "a//b",
        ".hidden",
        "a/.hidden/file",
        "file:stream",
        "file\\other",
        "NUL",
        "con.txt",
        "COM1.log",
        "trailing.",
        "trailing ",
        "a/./b",
    ] {
        assert!(reader.read(path, &mut budget(&cancel)).is_err(), "{path}");
    }
    std::fs::write(directory.path().join("binary.txt"), [0xff, 0, 1]).unwrap();
    assert!(reader.read("binary.txt", &mut budget(&cancel)).is_err());
    std::fs::write(directory.path().join("nul.txt"), "fixture\0text").unwrap();
    assert!(reader.read("nul.txt", &mut budget(&cancel)).is_err());
    std::fs::write(
        directory.path().join("large.txt"),
        vec![b'x'; MAX_FILE_BYTES + 1],
    )
    .unwrap();
    assert!(reader.read("large.txt", &mut budget(&cancel)).is_err());
}
#[test]
fn protected_directories_cannot_be_selected_or_read_through_parent_grants() {
    let directory = tempfile::tempdir().unwrap();
    let protected = directory.path().join("editor-state");
    std::fs::create_dir(&protected).unwrap();
    std::fs::write(protected.join("grants.json"), "private grant fixture").unwrap();
    let reader = root(
        directory.path(),
        &[Right::Read, Right::Discover],
        vec![protected.clone()],
    );
    let cancel = AtomicBool::new(false);
    assert!(reader
        .read("editor-state/grants.json", &mut budget(&cancel))
        .is_err());
    assert!(reader
        .discover("", &mut budget(&cancel))
        .unwrap()
        .entries
        .is_empty());
    assert!(Root::open_selected(
        &protected,
        Policy::new([Right::Read]).unwrap(),
        Protected::new(vec![protected.clone()]).unwrap()
    )
    .is_err());
}
#[test]
fn replacing_the_named_root_never_retargets_its_pinned_authority() {
    let outer = tempfile::tempdir().unwrap();
    let selected = outer.path().join("selected");
    std::fs::create_dir(&selected).unwrap();
    std::fs::write(selected.join("file.txt"), "approved fixture").unwrap();
    let reader = root(&selected, &[Right::Read], vec![]);
    if std::fs::rename(&selected, outer.path().join("moved")).is_err() {
        // Windows directory handles deny rename while the capability is live.
        assert_eq!(
            reader
                .read("file.txt", &mut budget(&AtomicBool::new(false)))
                .unwrap()
                .text,
            "approved fixture"
        );
        return;
    }
    std::fs::create_dir(&selected).unwrap();
    std::fs::write(selected.join("file.txt"), "replacement fixture").unwrap();
    assert!(reader
        .read("file.txt", &mut budget(&AtomicBool::new(false)))
        .is_err());
}
#[cfg(unix)]
#[test]
fn links_and_fifos_cannot_disclose_outside_data_or_block_a_read() {
    use std::os::unix::fs::symlink;
    let outer = tempfile::tempdir().unwrap();
    let selected = outer.path().join("selected");
    std::fs::create_dir(&selected).unwrap();
    let outside = outer.path().join("outside.txt");
    std::fs::write(&outside, "outside canary").unwrap();
    symlink(&outside, selected.join("symbolic.txt")).unwrap();
    std::fs::hard_link(&outside, selected.join("hard.txt")).unwrap();
    symlink(outer.path(), selected.join("alias")).unwrap();
    let fifo = std::ffi::CString::new(selected.join("pipe").to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let reader = root(&selected, &[Right::Read, Right::Discover], vec![]);
    let cancel = AtomicBool::new(false);
    for name in ["symbolic.txt", "hard.txt", "alias/outside.txt", "pipe"] {
        assert!(reader.read(name, &mut budget(&cancel)).is_err(), "{name}");
    }
    assert!(reader
        .discover("", &mut budget(&cancel))
        .unwrap()
        .entries
        .is_empty());
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "outside canary");
}
#[test]
fn enumeration_depth_counts_and_deadlines_remain_bounded() {
    let outer = tempfile::tempdir().unwrap();
    for index in 0..MAX_RESULTS + 10 {
        std::fs::write(outer.path().join(format!("file-{index:04}.txt")), "fixture").unwrap();
    }
    let reader = root(outer.path(), &[Right::Discover], vec![]);
    let cancel = AtomicBool::new(false);
    let result = reader.discover("", &mut budget(&cancel)).unwrap();
    assert_eq!(result.entries.len(), MAX_RESULTS);
    assert!(result.truncated);
    assert!(reader
        .discover("", &mut Budget::new(&cancel, Duration::ZERO, 1))
        .is_err());
}
#[test]
fn a_wide_directory_tree_does_not_open_a_handle_for_every_pending_sibling() {
    let outer = tempfile::tempdir().unwrap();
    for index in 0..600 {
        let directory = outer.path().join(format!("dir-{index:04}"));
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("text.txt"), "fixture").unwrap();
    }
    let reader = root(outer.path(), &[Right::Discover], vec![]);
    let cancel = AtomicBool::new(false);
    let listing = reader
        .discover(
            "absent-name",
            &mut Budget::new(&cancel, Duration::from_secs(5), 4096),
        )
        .unwrap();
    assert!(listing.entries.is_empty());
    assert!(!listing.truncated);
}
#[test]
fn protected_file_identity_remains_excluded_after_moving_into_a_granted_directory() {
    let outer = tempfile::tempdir().unwrap();
    let selected = outer.path().join("selected");
    std::fs::create_dir(&selected).unwrap();
    let secret = outer.path().join("private-key.txt");
    std::fs::write(&secret, "synthetic private canary").unwrap();
    let reader = root(
        &selected,
        &[Right::Read, Right::Discover],
        vec![secret.clone()],
    );
    if std::fs::rename(&secret, selected.join("moved.txt")).is_err() {
        assert!(!selected.join("moved.txt").exists());
        return;
    }
    let cancel = AtomicBool::new(false);
    assert!(reader.read("moved.txt", &mut budget(&cancel)).is_err());
    assert!(reader
        .discover("", &mut budget(&cancel))
        .unwrap()
        .entries
        .is_empty());
}
#[cfg(unix)]
#[test]
fn native_files_with_windows_device_or_ambiguous_names_are_not_resources() {
    let outer = tempfile::tempdir().unwrap();
    for name in [
        "CON.txt",
        "CONIN$",
        "CONOUT$",
        "CLOCK$",
        "COM¹.txt",
        "LPT²",
        "NUL",
        "trailing.",
        "trailing ",
        "file:stream",
    ] {
        std::fs::write(outer.path().join(name), "fixture").unwrap();
    }
    let reader = root(outer.path(), &[Right::Read, Right::Discover], vec![]);
    let cancel = AtomicBool::new(false);
    assert!(reader
        .discover("", &mut budget(&cancel))
        .unwrap()
        .entries
        .is_empty());
}
#[cfg(windows)]
#[test]
fn native_reparse_points_never_redirect_workspace_authority() {
    use std::os::windows::fs::{symlink_dir, symlink_file};
    let outer = tempfile::tempdir().unwrap();
    let selected = outer.path().join("selected");
    std::fs::create_dir(&selected).unwrap();
    let outside = outer.path().join("outside.txt");
    std::fs::write(&outside, "outside canary").unwrap();
    symlink_file(&outside, selected.join("symbolic.txt"))
        .expect("Windows native reparse probe requires symlink creation support");
    symlink_dir(outer.path(), selected.join("alias")).unwrap();
    std::fs::hard_link(&outside, selected.join("hard.txt")).unwrap();
    let reader = root(&selected, &[Right::Read, Right::Discover], vec![]);
    let cancel = AtomicBool::new(false);
    for name in ["symbolic.txt", "alias/outside.txt", "hard.txt"] {
        assert!(reader.read(name, &mut budget(&cancel)).is_err());
    }
    assert!(reader
        .discover("", &mut budget(&cancel))
        .unwrap()
        .entries
        .is_empty());
}
