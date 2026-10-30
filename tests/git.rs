use hedit::git::{DiffKind, Git, Operation, Selection};
use std::{path::Path, sync::atomic::Ordering};

fn repo() -> (tempfile::TempDir, Git) {
    let directory = tempfile::tempdir().unwrap();
    let git = Git::default();
    git.command(directory.path(), &["init", "-b", "main"])
        .unwrap();
    for (key, value) in [
        ("user.name", "Git GUI Test"),
        ("user.email", "git-gui@example.invalid"),
        ("commit.gpgsign", "false"),
        ("core.autocrlf", "false"),
        ("core.hooksPath", "/nonexistent/hedit-test-hooks"),
    ] {
        git.command(directory.path(), &["config", key, value])
            .unwrap();
    }
    (directory, git)
}
fn write(root: &Path, path: &str, text: &str) {
    std::fs::write(root.join(path), text).unwrap();
}
fn stage(git: &Git, root: &Path, path: &str) {
    git.execute(root, &Operation::Stage(vec![path.into()]))
        .unwrap();
}
fn commit(git: &Git, root: &Path, message: &str) {
    git.execute(
        root,
        &Operation::Commit {
            message: message.into(),
            amend: false,
        },
    )
    .unwrap();
}
fn select(path: &str, kind: DiffKind) -> Selection {
    Selection {
        path: path.into(),
        kind,
    }
}

#[test]
fn initial_commit_unstage_and_staged_vs_working_text_are_independent() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "note.txt", "first\n");
    let snapshot = git.snapshot(root, 200).unwrap();
    assert!(snapshot.unborn);
    assert!(snapshot.files[0].untracked());
    stage(&git, root, "note.txt");
    git.execute(root, &Operation::Unstage(vec!["note.txt".into()]))
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("note.txt")).unwrap(),
        "first\n"
    );
    assert!(git.snapshot(root, 200).unwrap().files[0].untracked());
    stage(&git, root, "note.txt");
    commit(&git, root, "Initial note");
    write(root, "note.txt", "staged\n");
    stage(&git, root, "note.txt");
    write(root, "note.txt", "working\n");
    let snapshot = git.snapshot(root, 200).unwrap();
    assert!(snapshot.files[0].staged() && snapshot.files[0].working());
    assert!(git
        .diff(root, &select("note.txt", DiffKind::Staged))
        .unwrap()
        .patch
        .contains("+staged"));
    assert!(git
        .diff(root, &select("note.txt", DiffKind::Working))
        .unwrap()
        .patch
        .contains("+working"));
    commit(&git, root, "Only staged");
    assert_eq!(
        git.command(root, &["show", "HEAD:note.txt"]).unwrap(),
        "staged\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("note.txt")).unwrap(),
        "working\n"
    );
}

#[test]
fn hunks_can_be_staged_and_unstaged_without_changing_other_text() {
    let (directory, git) = repo();
    let root = directory.path();
    let base = (1..=30).map(|i| format!("line {i}\n")).collect::<String>();
    write(root, "file.txt", &base);
    stage(&git, root, "file.txt");
    commit(&git, root, "Base");
    let changed = base
        .replace("line 2\n", "edited 2\n")
        .replace("line 25\n", "edited 25\n");
    write(root, "file.txt", &changed);
    let selection = select("file.txt", DiffKind::Working);
    let diff = git.diff(root, &selection).unwrap();
    assert_eq!(diff.hunks.len(), 2);
    git.execute(
        root,
        &Operation::Hunk {
            selection,
            expected: diff.patch,
            hunk: 0,
        },
    )
    .unwrap();
    let staged = git.command(root, &["show", ":file.txt"]).unwrap();
    assert!(staged.contains("edited 2\n") && !staged.contains("edited 25"));
    let selection = select("file.txt", DiffKind::Staged);
    let diff = git.diff(root, &selection).unwrap();
    git.execute(
        root,
        &Operation::Hunk {
            selection,
            expected: diff.patch,
            hunk: 0,
        },
    )
    .unwrap();
    assert_eq!(git.command(root, &["show", ":file.txt"]).unwrap(), base);
    assert_eq!(
        std::fs::read_to_string(root.join("file.txt")).unwrap(),
        changed
    );
}

#[test]
fn stale_hunk_and_discard_are_rejected_and_discard_keeps_a_backup() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file.txt", "base\n");
    stage(&git, root, "file.txt");
    commit(&git, root, "Base");
    write(root, "file.txt", "reviewed\n");
    let selection = select("file.txt", DiffKind::Working);
    let diff = git.diff(root, &selection).unwrap();
    write(root, "file.txt", "newer\n");
    assert!(git
        .execute(
            root,
            &Operation::Hunk {
                selection: selection.clone(),
                expected: diff.patch.clone(),
                hunk: 0
            }
        )
        .is_err());
    let backup = tempfile::tempdir().unwrap();
    assert!(git
        .execute(
            root,
            &Operation::Discard {
                selection: selection.clone(),
                expected: diff.patch,
                backup: backup.path().into()
            }
        )
        .is_err());
    let diff = git.diff(root, &selection).unwrap();
    git.execute(
        root,
        &Operation::Discard {
            selection,
            expected: diff.patch,
            backup: backup.path().into(),
        },
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("file.txt")).unwrap(),
        "base\n"
    );
    let copy = std::fs::read_dir(backup.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(std::fs::read_to_string(copy).unwrap(), "newer\n");
}

#[test]
fn filenames_are_literal_and_renames_deletions_binary_and_empty_files_work() {
    let (directory, git) = repo();
    let root = directory.path();
    // Windows permits brackets, but not an asterisk in an actual filename.
    let glob_path = if cfg!(windows) { "[a].txt" } else { "[a]*.txt" };
    for path in [
        "white space.txt",
        "é.txt",
        glob_path,
        "-option.txt",
        "empty.txt",
    ] {
        write(root, path, "");
        stage(&git, root, path);
    }
    std::fs::write(root.join("binary.bin"), [0, 1, 2]).unwrap();
    stage(&git, root, "binary.bin");
    commit(&git, root, "Unusual paths");
    git.command(root, &["mv", "white space.txt", "renamed.txt"])
        .unwrap();
    std::fs::remove_file(root.join("é.txt")).unwrap();
    let snapshot = git.snapshot(root, 100).unwrap();
    let rename = snapshot
        .files
        .iter()
        .find(|f| f.path == Path::new("renamed.txt"))
        .unwrap();
    assert_eq!(
        rename.original.as_deref(),
        Some(Path::new("white space.txt"))
    );
    git.execute(root, &Operation::Unstage(rename.paths()))
        .unwrap();
    assert!(root.join("renamed.txt").exists());
    assert!(git
        .diff(root, &select("é.txt", DiffKind::Working))
        .unwrap()
        .patch
        .contains("deleted file mode"));
    std::fs::write(root.join("binary.bin"), [0, 5, 3]).unwrap();
    assert!(git
        .diff(root, &select("binary.bin", DiffKind::Working))
        .unwrap()
        .hunks
        .is_empty());
    assert!(git
        .execute(root, &Operation::Stage(vec!["../escape".into()]))
        .is_err());
    assert!(git
        .execute(root, &Operation::Switch("--detach".into()))
        .is_err());
}

#[cfg(unix)]
#[test]
fn nul_status_preserves_filesystem_supported_paths() {
    use std::path::PathBuf;
    let (directory, git) = repo();
    let root = directory.path();
    let paths = vec![
        PathBuf::from("line\nbreak.txt"),
        PathBuf::from("unicode-λ.txt"),
    ];
    // APFS rejects non-UTF-8 file names. Its raw Git-path parser is tested
    // independently, without asking the filesystem to create an invalid name.
    #[cfg(not(target_os = "macos"))]
    let paths = {
        use std::os::unix::ffi::OsStringExt;
        let mut paths = paths;
        paths.push(PathBuf::from(std::ffi::OsString::from_vec(
            b"non-utf8-\xff.txt".to_vec(),
        )));
        paths
    };
    for path in &paths {
        std::fs::write(root.join(path), "hello\n").unwrap();
    }
    let snapshot = git.snapshot(root, 200).unwrap();
    assert!(paths
        .iter()
        .all(|p| snapshot.files.iter().any(|f| &f.path == p)));
    git.execute(root, &Operation::Stage(paths.clone())).unwrap();
    commit(&git, root, "Literal paths");
    let oid = git.command(root, &["rev-parse", "HEAD"]).unwrap();
    let files = git.commit_files(root, oid.trim()).unwrap();
    assert_eq!(files.len(), paths.len());
    assert!(paths.iter().all(|p| files.iter().any(|f| &f.path == p)));
}

#[test]
fn branches_stashes_and_history_round_trip() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file.txt", "base\n");
    stage(&git, root, "file.txt");
    commit(&git, root, "Initial\n\nFull body");
    git.execute(root, &Operation::CreateBranch("feature".into()))
        .unwrap();
    write(root, "file.txt", "work in progress\n");
    stage(&git, root, "file.txt");
    write(root, "new.txt", "new\n");
    git.execute(root, &Operation::Stash("Saved work".into()))
        .unwrap();
    let snapshot = git.snapshot(root, 200).unwrap();
    assert!(snapshot.files.is_empty(), "{snapshot:?}");
    assert_eq!(snapshot.stashes.len(), 1);
    git.execute(root, &Operation::Switch("main".into()))
        .unwrap();
    git.execute(
        root,
        &Operation::ApplyStash(snapshot.stashes[0].oid.clone()),
    )
    .unwrap();
    assert!(git
        .snapshot(root, 200)
        .unwrap()
        .files
        .iter()
        .any(|f| f.staged()));
    assert_eq!(
        std::fs::read_to_string(root.join("new.txt")).unwrap(),
        "new\n"
    );
    let stash = &snapshot.stashes[0];
    git.execute(
        root,
        &Operation::DropStash {
            name: stash.name.clone(),
            oid: stash.oid.clone(),
        },
    )
    .unwrap();
    assert!(git.snapshot(root, 200).unwrap().stashes.is_empty());
    let history = git.history(root, 200).unwrap();
    assert_eq!(history[0].subject, "Initial");
    assert!(git
        .commit_message(root, &history[0].oid)
        .unwrap()
        .contains("Full body"));
    assert_eq!(git.commit_files(root, &history[0].oid).unwrap().len(), 1);
}

#[test]
fn merge_conflicts_are_reported_and_can_be_resolved_or_aborted() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file.txt", "base\n");
    stage(&git, root, "file.txt");
    commit(&git, root, "Base");
    git.execute(root, &Operation::CreateBranch("feature".into()))
        .unwrap();
    write(root, "file.txt", "feature\n");
    stage(&git, root, "file.txt");
    commit(&git, root, "Feature");
    git.execute(root, &Operation::Switch("main".into()))
        .unwrap();
    write(root, "file.txt", "main\n");
    stage(&git, root, "file.txt");
    commit(&git, root, "Main");
    let error = git
        .execute(root, &Operation::Merge("feature".into()))
        .unwrap_err();
    assert!(error.contains("CONFLICT"), "{error}");
    let snapshot = git.snapshot(root, 200).unwrap();
    assert!(snapshot.files[0].conflict);
    assert_eq!(snapshot.operation.as_deref(), Some("Merge in progress"));
    git.execute(root, &Operation::Abort("Merge in progress".into()))
        .unwrap();
    assert!(git.snapshot(root, 200).unwrap().files.is_empty());
    assert!(git
        .execute(root, &Operation::Merge("feature".into()))
        .is_err());
    write(root, "file.txt", "resolved\n");
    stage(&git, root, "file.txt");
    git.execute(root, &Operation::Continue("Merge in progress".into()))
        .unwrap();
    let snapshot = git.snapshot(root, 200).unwrap();
    assert!(snapshot.operation.is_none());
    assert_eq!(snapshot.commits[0].parents.len(), 2);
    let oid = &snapshot.commits[0].oid;
    assert_eq!(
        git.commit_files(root, oid).unwrap().len(),
        1,
        "Merge review must include first-parent changes"
    );
    assert!(git
        .diff(root, &select("file.txt", DiffKind::Commit(oid.clone())))
        .unwrap()
        .patch
        .contains("+resolved"));
}

#[test]
fn worktrees_and_detached_head_are_discovered_without_rewriting_files() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file.txt", "base\n");
    stage(&git, root, "file.txt");
    commit(&git, root, "Base");
    let other = tempfile::tempdir().unwrap();
    let path = other.path().join("worktree");
    git.command(
        root,
        &["worktree", "add", "-b", "other", path.to_str().unwrap()],
    )
    .unwrap();
    assert_eq!(
        git.discover(&path).unwrap().canonicalize().unwrap(),
        path.canonicalize().unwrap()
    );
    assert_eq!(git.snapshot(&path, 200).unwrap().branch, "other");
    git.command(&path, &["checkout", "--detach"]).unwrap();
    assert_eq!(git.snapshot(&path, 200).unwrap().branch, "(detached)");
    assert!(git.snapshot(&path, 200).unwrap().files.is_empty());
}

#[test]
fn local_remote_publish_fetch_and_fast_forward_pull_work_without_force() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file.txt", "base\n");
    stage(&git, root, "file.txt");
    commit(&git, root, "Base");
    let remote = tempfile::tempdir().unwrap();
    git.command(remote.path(), &["init", "--bare"]).unwrap();
    git.command(
        root,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    )
    .unwrap();
    git.execute(
        root,
        &Operation::Publish {
            remote: "origin".into(),
            branch: "main".into(),
        },
    )
    .unwrap();
    git.execute(root, &Operation::Fetch).unwrap();
    git.execute(root, &Operation::Pull).unwrap();
    let snapshot = git.snapshot(root, 200).unwrap();
    assert_eq!(snapshot.upstream, "origin/main");
    assert_eq!(snapshot.ahead, 0);
    write(root, "file.txt", "next\n");
    stage(&git, root, "file.txt");
    commit(&git, root, "Next");
    assert_eq!(git.snapshot(root, 200).unwrap().ahead, 1);
    git.execute(root, &Operation::Push).unwrap();
    assert_eq!(git.snapshot(root, 200).unwrap().ahead, 0);
}

#[test]
fn cancellation_does_not_touch_the_worktree_or_index() {
    let (directory, git) = repo();
    write(directory.path(), "file.txt", "kept\n");
    git.cancel.store(true, Ordering::Relaxed);
    assert!(git
        .execute(directory.path(), &Operation::Stage(vec!["file.txt".into()]))
        .unwrap_err()
        .contains("cancelled"));
    git.cancel.store(false, Ordering::Relaxed);
    assert!(git.snapshot(directory.path(), 200).unwrap().files[0].untracked());
}

#[test]
fn diff_review_preserves_content_that_looks_like_patch_headers() {
    let diff = hedit::git::Diff::parse("diff --git a/x b/x\nindex 111..222 100644\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n--- old text\n+++ new text\n".into(),true);
    assert_eq!(diff.lines.len(), 3);
    assert_eq!(diff.lines[1].text, "--- old text");
    assert_eq!(diff.lines[2].text, "+++ new text");
    assert_eq!(
        diff.split_rows,
        vec![(Some(0), Some(0)), (Some(1), Some(2))]
    );
    assert!(diff
        .hunk_patch(0)
        .unwrap()
        .contains("--- old text\n+++ new text"));
}

#[test]
fn conflict_sides_are_reviewable_and_replacement_is_guarded_and_backed_up() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file.txt", "base\n");
    stage(&git, root, "file.txt");
    commit(&git, root, "Base");
    git.execute(root, &Operation::CreateBranch("feature".into()))
        .unwrap();
    write(root, "file.txt", "feature\n");
    stage(&git, root, "file.txt");
    commit(&git, root, "Feature");
    git.execute(root, &Operation::Switch("main".into()))
        .unwrap();
    write(root, "file.txt", "main\n");
    stage(&git, root, "file.txt");
    commit(&git, root, "Main");
    assert!(git
        .execute(root, &Operation::Merge("feature".into()))
        .is_err());
    let selection = select("file.txt", DiffKind::Conflict);
    let diff = git.diff(root, &selection).unwrap();
    let conflict = diff.conflict.unwrap();
    assert_eq!(conflict.base.as_deref(), Some("base\n"));
    assert_eq!(conflict.ours.as_deref(), Some("main\n"));
    assert_eq!(conflict.theirs.as_deref(), Some("feature\n"));
    let backups = tempfile::tempdir().unwrap();
    let operation = Operation::Resolve {
        selection: selection.clone(),
        ours: false,
        expected: conflict.expected,
        backup: backups.path().into(),
    };
    write(root, "file.txt", "manual resolution\n");
    assert!(git.execute(root, &operation).is_err());
    let expected = git
        .diff(root, &selection)
        .unwrap()
        .conflict
        .unwrap()
        .expected;
    git.execute(
        root,
        &Operation::Resolve {
            selection,
            ours: false,
            expected,
            backup: backups.path().into(),
        },
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("file.txt")).unwrap(),
        "feature\n"
    );
    assert!(
        git.snapshot(root, 200).unwrap().files[0].conflict,
        "Choosing a side must still require review/staging"
    );
    let backup = std::fs::read_dir(backups.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        std::fs::read_to_string(backup).unwrap(),
        "manual resolution\n"
    );
}

#[test]
fn replacing_a_conflict_checks_the_reviewed_index_sides_as_well_as_working_text() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file.txt", "base\n");
    stage(&git, root, "file.txt");
    commit(&git, root, "Base");
    git.execute(root, &Operation::CreateBranch("feature".into()))
        .unwrap();
    write(root, "file.txt", "feature\n");
    stage(&git, root, "file.txt");
    commit(&git, root, "Feature");
    git.execute(root, &Operation::Switch("main".into()))
        .unwrap();
    write(root, "file.txt", "main\n");
    stage(&git, root, "file.txt");
    commit(&git, root, "Main");
    assert!(git
        .execute(root, &Operation::Merge("feature".into()))
        .is_err());
    let selection = select("file.txt", DiffKind::Conflict);
    let conflict = git.diff(root, &selection).unwrap().conflict.unwrap();
    let working = std::fs::read(root.join("file.txt")).unwrap();
    // Another Git client replaces the unmerged index entries but leaves the
    // working file untouched. Previously its old hash still authorized restore.
    git.command(root, &["read-tree", "--empty"]).unwrap();
    git.command(root, &["read-tree", "-m", "HEAD^", "feature", "HEAD"])
        .unwrap();
    assert_eq!(std::fs::read(root.join("file.txt")).unwrap(), working);
    let backups = tempfile::tempdir().unwrap();
    let error = git
        .execute(
            root,
            &Operation::Resolve {
                selection,
                ours: true,
                expected: conflict.expected,
                backup: backups.path().into(),
            },
        )
        .unwrap_err();
    assert!(error.contains("changed"));
    assert_eq!(std::fs::read(root.join("file.txt")).unwrap(), working);
}

#[test]
fn clone_and_remote_management_use_new_destinations_and_keep_history_graph() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file.txt", "base\n");
    stage(&git, root, "file.txt");
    commit(&git, root, "Base");
    let other = tempfile::tempdir().unwrap();
    let destination = other.path().join("clone");
    let cloned = git
        .clone_repository(root.to_str().unwrap(), &destination)
        .unwrap();
    assert_eq!(
        cloned.canonicalize().unwrap(),
        destination.canonicalize().unwrap()
    );
    assert!(git
        .clone_repository(root.to_str().unwrap(), &destination)
        .is_err());
    git.execute(
        &destination,
        &Operation::AddRemote {
            name: "upstream".into(),
            url: root.to_string_lossy().into_owned(),
        },
    )
    .unwrap();
    assert!(git
        .snapshot(&destination, 200)
        .unwrap()
        .remotes
        .contains(&"upstream".into()));
    git.execute(&destination, &Operation::RemoveRemote("upstream".into()))
        .unwrap();
    let snapshot = git.snapshot(&destination, 200).unwrap();
    assert!(
        !snapshot
            .branches
            .iter()
            .any(|b| b.reference.ends_with("/origin/HEAD")),
        "A remote HEAD alias is not a branch to check out"
    );
    assert_eq!(snapshot.graph.len(), snapshot.commits.len());
    assert_eq!(snapshot.graph[0].width, 1);
    assert!(snapshot.graph[0].edges.is_empty());
}

#[test]
fn repository_actions_refuse_to_fall_back_to_a_parent_repository() {
    let (directory, git) = repo();
    let parent = directory.path();
    write(parent, "file.txt", "parent base\n");
    stage(&git, parent, "file.txt");
    commit(&git, parent, "Parent");
    let nested = parent.join("nested");
    std::fs::create_dir(&nested).unwrap();
    git.command(&nested, &["init", "-b", "main"]).unwrap();
    write(&nested, "file.txt", "nested note\n");
    std::fs::rename(
        nested.join(".git"),
        directory.path().join("nested-git-backup"),
    )
    .unwrap();
    let error = git
        .execute(&nested, &Operation::Stage(vec!["file.txt".into()]))
        .unwrap_err();
    assert!(error.contains("repository"));
    assert!(git
        .command(parent, &["diff", "--cached", "--name-only"])
        .unwrap()
        .is_empty());
}

#[cfg(unix)]
#[test]
fn failed_hooks_report_stdout_without_committing_or_losing_staged_text() {
    use std::os::unix::fs::PermissionsExt;
    let (directory, git) = repo();
    let root = directory.path();
    let hooks = root.join("test-hooks");
    std::fs::create_dir(&hooks).unwrap();
    git.command(root, &["config", "core.hooksPath", hooks.to_str().unwrap()])
        .unwrap();
    let hook = hooks.join("pre-commit");
    std::fs::write(
        &hook,
        "#!/bin/sh\nprintf 'formatter refused this commit\\n'\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    write(root, "file.txt", "kept\n");
    stage(&git, root, "file.txt");
    let error = git
        .execute(
            root,
            &Operation::Commit {
                message: "Rejected".into(),
                amend: false,
            },
        )
        .unwrap_err();
    assert!(error.contains("formatter refused this commit"), "{error}");
    assert_eq!(git.command(root, &["show", ":file.txt"]).unwrap(), "kept\n");
    assert!(git.snapshot(root, 200).unwrap().unborn);
}

#[test]
fn discard_can_restore_a_deleted_parent_folder_and_amend_updates_only_staged_text() {
    let (directory, git) = repo();
    let root = directory.path();
    std::fs::create_dir(root.join("sub")).unwrap();
    write(root, "sub/file.txt", "base\n");
    stage(&git, root, "sub/file.txt");
    commit(&git, root, "Base");
    std::fs::remove_file(root.join("sub/file.txt")).unwrap();
    std::fs::remove_dir(root.join("sub")).unwrap();
    let selection = select("sub/file.txt", DiffKind::Working);
    let expected = git.diff(root, &selection).unwrap().patch;
    let backup = tempfile::tempdir().unwrap();
    git.execute(
        root,
        &Operation::Discard {
            selection,
            expected,
            backup: backup.path().into(),
        },
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("sub/file.txt")).unwrap(),
        "base\n"
    );
    write(root, "sub/file.txt", "index\n");
    stage(&git, root, "sub/file.txt");
    write(root, "sub/file.txt", "working\n");
    git.execute(
        root,
        &Operation::Commit {
            message: "Amended".into(),
            amend: true,
        },
    )
    .unwrap();
    assert_eq!(git.history(root, 200).unwrap().len(), 1);
    assert_eq!(
        git.command(root, &["show", "HEAD:sub/file.txt"]).unwrap(),
        "index\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("sub/file.txt")).unwrap(),
        "working\n"
    );
}
