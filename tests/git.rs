use hedit::git::{DiffKind, Git, HistoryScope, Operation, Selection};
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
fn selected_lines_stage_and_unstage_independently_with_offsets_and_unicode() {
    let (directory, git) = repo();
    let root = directory.path();
    let base = (1..=40).map(|i| format!("line {i}\n")).collect::<String>();
    write(root, "lines.txt", &base);
    stage(&git, root, "lines.txt");
    commit(&git, root, "Base");
    let changed = base
        .replace("line 2\n", "العربية\nextra\n")
        .replace("line 25\n", "日本語\n")
        .replace("line 39\n", "last edit\n");
    write(root, "lines.txt", &changed);
    let selection = select("lines.txt", DiffKind::Working);
    let diff = git.diff(root, &selection).unwrap();
    let lines = diff
        .lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| {
            (l.text == "+extra" || l.text == "-line 25" || l.text == "+日本語").then_some(i)
        })
        .collect();
    git.execute(
        root,
        &Operation::Lines {
            selection,
            expected: diff.patch,
            lines,
        },
    )
    .unwrap();
    let indexed = base
        .replace("line 2\n", "line 2\nextra\n")
        .replace("line 25\n", "日本語\n");
    assert_eq!(git.command(root, &["show", ":lines.txt"]).unwrap(), indexed);
    let selection = select("lines.txt", DiffKind::Staged);
    let diff = git.diff(root, &selection).unwrap();
    let lines = diff
        .lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| matches!(l.kind, '+' | '-').then_some(i))
        .collect();
    git.execute(
        root,
        &Operation::Lines {
            selection,
            expected: diff.patch,
            lines,
        },
    )
    .unwrap();
    assert_eq!(git.command(root, &["show", ":lines.txt"]).unwrap(), base);
    assert_eq!(
        std::fs::read_to_string(root.join("lines.txt")).unwrap(),
        changed
    );
}

#[test]
fn selected_lines_preserve_crlf_and_missing_final_newline() {
    for (base, changed) in [
        ("one\r\ntwo\r\nthree\r\n", "ONE\r\nTWO\r\nthree\r\n"),
        ("one\ntwo", "ONE\nTWO"),
        ("one\n", "one\nextra\n"),
        ("one\ntwo\n", "one\n"),
    ] {
        let (directory, git) = repo();
        let root = directory.path();
        write(root, "file.txt", base);
        stage(&git, root, "file.txt");
        commit(&git, root, "Base");
        write(root, "file.txt", changed);
        for kind in [DiffKind::Working, DiffKind::Staged] {
            let selection = select("file.txt", kind);
            let diff = git.diff(root, &selection).unwrap();
            let lines = diff
                .lines
                .iter()
                .enumerate()
                .filter_map(|(i, l)| matches!(l.kind, '+' | '-').then_some(i))
                .collect();
            git.execute(
                root,
                &Operation::Lines {
                    selection,
                    expected: diff.patch,
                    lines,
                },
            )
            .unwrap();
        }
        assert_eq!(git.command(root, &["show", ":file.txt"]).unwrap(), base);
        assert_eq!(
            std::fs::read_to_string(root.join("file.txt")).unwrap(),
            changed
        );
    }
}

#[test]
fn selected_lines_reject_stale_invalid_and_whole_file_changes() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file.txt", "base\n");
    stage(&git, root, "file.txt");
    commit(&git, root, "Base");
    write(root, "file.txt", "reviewed\n");
    let selection = select("file.txt", DiffKind::Working);
    let diff = git.diff(root, &selection).unwrap();
    assert!(diff.lines_patch(&[], false).is_none());
    assert!(diff.lines_patch(&[usize::MAX], false).is_none());
    assert!(diff.lines_patch(&[0], false).is_none());
    let lines = diff
        .lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| (l.kind == '+').then_some(i))
        .collect();
    write(root, "file.txt", "newer\n");
    assert!(git
        .execute(
            root,
            &Operation::Lines {
                selection,
                expected: diff.patch,
                lines
            }
        )
        .unwrap_err()
        .contains("changed"));
    assert_eq!(git.command(root, &["show", ":file.txt"]).unwrap(), "base\n");
    write(root, "new.txt", "new\n");
    let diff = git
        .diff(root, &select("new.txt", DiffKind::Untracked))
        .unwrap();
    assert!(diff.lines_patch(&[1], false).is_none());
}

#[test]
fn every_subset_of_a_replacement_changes_only_selected_index_lines() {
    let (directory, git) = repo();
    let root = directory.path();
    let base = "one\ntwo\nunchanged\n";
    let changed = "ONE\nTWO\nunchanged\n";
    write(root, "subset.txt", base);
    stage(&git, root, "subset.txt");
    commit(&git, root, "Base");
    write(root, "subset.txt", changed);
    for reverse in [false, true] {
        for mask in 1..16 {
            git.execute(root, &Operation::Unstage(vec!["subset.txt".into()]))
                .unwrap();
            if reverse {
                stage(&git, root, "subset.txt");
            }
            let selection = select(
                "subset.txt",
                if reverse {
                    DiffKind::Staged
                } else {
                    DiffKind::Working
                },
            );
            let diff = git.diff(root, &selection).unwrap();
            let changed_lines: Vec<_> = diff
                .lines
                .iter()
                .enumerate()
                .filter_map(|(i, l)| matches!(l.kind, '+' | '-').then_some(i))
                .collect();
            assert_eq!(changed_lines.len(), 4);
            let lines = changed_lines
                .iter()
                .enumerate()
                .filter_map(|(bit, &i)| (mask & (1 << bit) != 0).then_some(i))
                .collect();
            git.execute(
                root,
                &Operation::Lines {
                    selection,
                    expected: diff.patch,
                    lines,
                },
            )
            .unwrap();
            let mut expected = String::new();
            let order = if reverse {
                [(2, "ONE\n"), (0, "one\n"), (3, "TWO\n"), (1, "two\n")]
            } else {
                [(0, "one\n"), (2, "ONE\n"), (1, "two\n"), (3, "TWO\n")]
            };
            for (bit, text) in order {
                let selected = mask & (1 << bit) != 0;
                if if reverse {
                    if bit < 2 {
                        selected
                    } else {
                        !selected
                    }
                } else if bit < 2 {
                    !selected
                } else {
                    selected
                } {
                    expected.push_str(text);
                }
            }
            expected.push_str("unchanged\n");
            assert_eq!(
                git.command(root, &["show", ":subset.txt"]).unwrap(),
                expected,
                "reverse={reverse}, mask={mask}"
            );
            assert_eq!(
                std::fs::read_to_string(root.join("subset.txt")).unwrap(),
                changed
            );
        }
    }
}

#[test]
fn character_emphasis_uses_utf8_byte_boundaries_and_ignores_context() {
    let diff = hedit::git::Diff::parse("--- a/file\n+++ b/file\n@@ -1,2 +1,2 @@\n-hello العربية world\n+hello 日本語 world\n context\n".into(), true);
    for line in &diff.lines {
        if matches!(line.kind, '+' | '-') {
            let text = &line.text[1..];
            let changed = line
                .emphasis
                .iter()
                .map(|r| &text[r.clone()])
                .collect::<String>();
            assert_eq!(
                changed,
                if line.kind == '+' {
                    "日本語"
                } else {
                    "العربية"
                }
            );
        } else {
            assert!(line.emphasis.is_empty());
        }
    }
}

#[test]
fn selected_replacement_preserves_an_adjacent_unselected_eof_line() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file.txt", "one\ntwo");
    stage(&git, root, "file.txt");
    commit(&git, root, "Base");
    write(root, "file.txt", "ONE\nTWO");
    for reverse in [false, true] {
        if reverse {
            stage(&git, root, "file.txt");
        }
        let selection = select(
            "file.txt",
            if reverse {
                DiffKind::Staged
            } else {
                DiffKind::Working
            },
        );
        let diff = git.diff(root, &selection).unwrap();
        let lines = diff
            .lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| matches!(l.text.as_str(), "-one" | "+ONE").then_some(i))
            .collect();
        git.execute(
            root,
            &Operation::Lines {
                selection,
                expected: diff.patch,
                lines,
            },
        )
        .unwrap();
        assert_eq!(
            git.command(root, &["show", ":file.txt"]).unwrap(),
            if reverse { "one\nTWO" } else { "ONE\ntwo" }
        );
    }
    assert_eq!(
        std::fs::read_to_string(root.join("file.txt")).unwrap(),
        "ONE\nTWO"
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

#[test]
fn comparisons_distinguish_branch_contributions_from_tip_contents() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "base.txt", "base\n");
    stage(&git, root, "base.txt");
    commit(&git, root, "Base");
    git.execute(root, &Operation::CreateBranch("feature".into()))
        .unwrap();
    write(root, "feature.txt", "feature\n");
    stage(&git, root, "feature.txt");
    commit(&git, root, "Feature");
    git.execute(root, &Operation::Switch("main".into()))
        .unwrap();
    write(root, "main.txt", "main\n");
    stage(&git, root, "main.txt");
    commit(&git, root, "Main only");
    let contribution = git.compare(root, "main", "feature", true).unwrap();
    assert_eq!((contribution.ahead, contribution.behind), (1, 1));
    assert_eq!(contribution.files.len(), 1);
    assert_eq!(contribution.files[0].path, Path::new("feature.txt"));
    let diff = git
        .diff(
            root,
            &Selection {
                path: "feature.txt".into(),
                kind: DiffKind::Compare {
                    base: contribution.base,
                    head: contribution.head,
                },
            },
        )
        .unwrap();
    assert!(diff.patch.contains("+feature"));
    assert!(!diff.supports_partial());
    let tips = git.compare(root, "main", "feature", false).unwrap();
    assert_eq!(tips.files.len(), 2);
    let reverse = git.compare(root, "feature", "main", true).unwrap();
    assert_eq!(reverse.files[0].path, Path::new("main.txt"));
    assert!(git
        .compare(root, "--output=/tmp/oops", "HEAD", false)
        .is_err());
    assert!(git.compare(root, "missing", "HEAD", false).is_err());
}

#[test]
fn scoped_history_and_blame_use_literal_paths_and_committed_text() {
    let (directory, git) = repo();
    let root = directory.path();
    #[cfg(unix)]
    let path = ":(glob)*.txt";
    #[cfg(not(unix))]
    let path = "[literal].txt";
    write(root, path, "first\n日本語\n");
    stage(&git, root, path);
    commit(&git, root, "Base text");
    let first = git.resolve_commit(root, "HEAD").unwrap();
    write(root, "other.txt", "other\n");
    stage(&git, root, "other.txt");
    commit(&git, root, "Unrelated");
    git.command(root, &["config", "user.name", "Second Author"])
        .unwrap();
    write(root, path, "updated\n日本語\n");
    stage(&git, root, path);
    commit(&git, root, "Change first line");
    let last = git.resolve_commit(root, "HEAD").unwrap();
    write(root, path, "unstaged\n");
    let history = git
        .scoped_history(
            root,
            20,
            &HistoryScope {
                reference: Some("main".into()),
                path: Some(path.into()),
            },
        )
        .unwrap();
    assert_eq!(
        history
            .iter()
            .map(|c| c.subject.as_str())
            .collect::<Vec<_>>(),
        ["Change first line", "Base text"]
    );
    let blame = git.blame(root, "HEAD", Path::new(path)).unwrap();
    assert_eq!(blame.len(), 2);
    assert_eq!(
        (
            &blame[0].oid,
            blame[0].number,
            blame[0].author.as_str(),
            blame[0].text.as_str()
        ),
        (&last, 1, "Second Author", "updated")
    );
    assert_eq!((&blame[1].oid, blame[1].text.as_str()), (&first, "日本語"));
    assert!(git.blame(root, "HEAD", Path::new("../outside")).is_err());
    assert!(git
        .scoped_history(
            root,
            20,
            &HistoryScope {
                reference: Some("--all".into()),
                path: None
            }
        )
        .is_err());
}

#[test]
fn annotated_tags_are_reviewable_and_delete_checks_the_reviewed_object() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file", "one\n");
    stage(&git, root, "file");
    commit(&git, root, "One");
    let oid = git.resolve_commit(root, "HEAD").unwrap();
    git.execute(
        root,
        &Operation::CreateTag {
            name: "v1.0".into(),
            oid: oid.clone(),
            message: "Release one\n\nDetails".into(),
        },
    )
    .unwrap();
    let tag = git.tags(root).unwrap().remove(0);
    assert_eq!(tag.name, "v1.0");
    assert_eq!(tag.commit, Some(oid.clone()));
    assert_ne!(tag.object, oid);
    assert!(git
        .command(root, &["cat-file", "-p", &tag.object])
        .unwrap()
        .contains("Release one\n\nDetails"));
    assert!(git
        .execute(
            root,
            &Operation::CreateTag {
                name: "v1.0".into(),
                oid: oid.clone(),
                message: "Duplicate".into()
            }
        )
        .is_err());
    git.command(root, &["tag", "-f", "v1.0", &oid]).unwrap();
    assert!(git
        .execute(
            root,
            &Operation::DeleteTag {
                name: tag.name.clone(),
                expected: tag.object
            }
        )
        .is_err());
    assert_eq!(git.tags(root).unwrap()[0].commit, Some(oid.clone()));
    git.execute(
        root,
        &Operation::DeleteTag {
            name: tag.name,
            expected: oid,
        },
    )
    .unwrap();
    assert!(git.tags(root).unwrap().is_empty());
}

#[test]
fn undo_and_reflog_recovery_preserve_the_index_worktree_and_checkout() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file", "one\n");
    stage(&git, root, "file");
    commit(&git, root, "One");
    let first = git.resolve_commit(root, "HEAD").unwrap();
    assert!(git
        .execute(
            root,
            &Operation::UndoCommit {
                expected: first.clone()
            }
        )
        .is_err());
    write(root, "file", "two\n");
    stage(&git, root, "file");
    commit(&git, root, "Two");
    let second = git.resolve_commit(root, "HEAD").unwrap();
    write(root, "file", "staged\n");
    stage(&git, root, "file");
    write(root, "file", "working\n");
    assert!(git
        .execute(
            root,
            &Operation::UndoCommit {
                expected: first.clone()
            }
        )
        .is_err());
    git.execute(
        root,
        &Operation::UndoCommit {
            expected: second.clone(),
        },
    )
    .unwrap();
    assert_eq!(git.resolve_commit(root, "HEAD").unwrap(), first);
    assert_eq!(git.resolve_commit(root, "ORIG_HEAD").unwrap(), second);
    assert_eq!(git.command(root, &["show", ":file"]).unwrap(), "staged\n");
    assert_eq!(
        std::fs::read_to_string(root.join("file")).unwrap(),
        "working\n"
    );
    assert!(git
        .reflog(root, 20)
        .unwrap()
        .iter()
        .any(|e| e.oid == second && e.subject.contains("Two")));
    git.execute(
        root,
        &Operation::CreateBranchAt {
            name: "recovered".into(),
            oid: second.clone(),
        },
    )
    .unwrap();
    assert_eq!(git.resolve_commit(root, "recovered").unwrap(), second);
    assert_eq!(git.snapshot(root, 10).unwrap().branch, "main");
    assert_eq!(
        std::fs::read_to_string(root.join("file")).unwrap(),
        "working\n"
    );
}

#[test]
fn branch_rename_upstream_and_rebase_work_without_shell_or_checkout_shortcuts() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file", "one\n");
    stage(&git, root, "file");
    commit(&git, root, "One");
    git.execute(root, &Operation::CreateBranch("topic".into()))
        .unwrap();
    write(root, "topic", "topic\n");
    stage(&git, root, "topic");
    commit(&git, root, "Topic");
    let old = git.resolve_commit(root, "HEAD").unwrap();
    git.execute(root, &Operation::Switch("main".into()))
        .unwrap();
    write(root, "main", "main\n");
    stage(&git, root, "main");
    commit(&git, root, "Main");
    git.execute(root, &Operation::Switch("topic".into()))
        .unwrap();
    git.execute(
        root,
        &Operation::RenameBranch {
            old: "topic".into(),
            new: "feature/native".into(),
        },
    )
    .unwrap();
    git.execute(
        root,
        &Operation::SetUpstream {
            branch: "feature/native".into(),
            upstream: "refs/heads/main".into(),
        },
    )
    .unwrap();
    assert_eq!(git.snapshot(root, 10).unwrap().upstream, "main");
    git.execute(root, &Operation::Rebase("refs/heads/main".into()))
        .unwrap();
    assert_ne!(git.resolve_commit(root, "HEAD").unwrap(), old);
    assert!(root.join("main").exists() && root.join("topic").exists());
    assert!(Operation::Rebase("main".into()).changes_worktree());
    assert!(!Operation::UndoCommit { expected: old }.changes_worktree());
}

#[test]
fn rebase_refuses_dirty_saved_files_and_supports_conflict_abort_and_continue() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file", "base\n");
    stage(&git, root, "file");
    commit(&git, root, "Base");
    git.execute(root, &Operation::CreateBranch("topic".into()))
        .unwrap();
    write(root, "file", "topic\n");
    stage(&git, root, "file");
    commit(&git, root, "Topic");
    let topic = git.resolve_commit(root, "HEAD").unwrap();
    git.execute(root, &Operation::Switch("main".into()))
        .unwrap();
    write(root, "file", "main\n");
    stage(&git, root, "file");
    commit(&git, root, "Main");
    git.execute(root, &Operation::Switch("topic".into()))
        .unwrap();
    git.command(root, &["config", "rebase.autoStash", "true"])
        .unwrap();
    write(root, "file", "unsaved to index\n");
    assert!(git
        .execute(root, &Operation::Rebase("main".into()))
        .is_err());
    assert_eq!(
        std::fs::read_to_string(root.join("file")).unwrap(),
        "unsaved to index\n"
    );
    assert!(git.snapshot(root, 10).unwrap().stashes.is_empty());
    write(root, "file", "topic\n");
    assert!(git
        .execute(root, &Operation::Rebase("main".into()))
        .is_err());
    let snapshot = git.snapshot(root, 10).unwrap();
    assert_eq!(snapshot.operation.as_deref(), Some("Rebase in progress"));
    assert!(snapshot.files[0].conflict);
    assert!(git
        .execute(
            root,
            &Operation::UndoCommit {
                expected: snapshot.oid
            }
        )
        .is_err());
    git.execute(root, &Operation::Abort("Rebase in progress".into()))
        .unwrap();
    assert_eq!(git.resolve_commit(root, "HEAD").unwrap(), topic);
    assert_eq!(
        std::fs::read_to_string(root.join("file")).unwrap(),
        "topic\n"
    );
    assert!(git
        .execute(root, &Operation::Rebase("main".into()))
        .is_err());
    write(root, "file", "main and topic\n");
    stage(&git, root, "file");
    git.execute(root, &Operation::Continue("Rebase in progress".into()))
        .unwrap();
    assert!(git.snapshot(root, 10).unwrap().operation.is_none());
    assert_eq!(
        std::fs::read_to_string(root.join("file")).unwrap(),
        "main and topic\n"
    );
}

#[test]
fn empty_and_cancelled_inspection_queries_do_not_misreport_history() {
    let (directory, git) = repo();
    let root = directory.path();
    assert!(git.tags(root).unwrap().is_empty());
    assert!(git.reflog(root, 20).unwrap().is_empty());
    assert!(git.compare(root, "HEAD", "HEAD", true).is_err());
    git.cancel.store(true, Ordering::Relaxed);
    assert!(git.reflog(root, 20).unwrap_err().contains("cancelled"));
    assert!(git.tags(root).is_err());
    assert!(git.blame(root, "HEAD", Path::new("file")).is_err());
}

#[test]
fn local_and_remote_branch_name_collisions_remain_switchable_and_trackable() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file", "base\n");
    stage(&git, root, "file");
    commit(&git, root, "Base");
    let oid = git.resolve_commit(root, "HEAD").unwrap();
    git.execute(
        root,
        &Operation::CreateBranchAt {
            name: "origin/main".into(),
            oid: oid.clone(),
        },
    )
    .unwrap();
    git.command(root, &["update-ref", "refs/remotes/origin/main", &oid])
        .unwrap();
    git.command(
        root,
        &[
            "config",
            "remote.origin.url",
            directory.path().to_str().unwrap(),
        ],
    )
    .unwrap();
    git.command(
        root,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
    )
    .unwrap();
    let snapshot = git.snapshot(root, 20).unwrap();
    let local = snapshot
        .branches
        .iter()
        .find(|b| b.reference == "refs/heads/origin/main")
        .unwrap();
    let remote = snapshot
        .branches
        .iter()
        .find(|b| b.reference == "refs/remotes/origin/main")
        .unwrap();
    assert_eq!(local.name, "origin/main");
    assert_eq!(remote.name, "origin/main");
    git.execute(root, &Operation::Switch(local.name.clone()))
        .unwrap();
    assert_eq!(git.snapshot(root, 20).unwrap().branch, "origin/main");
    git.execute(
        root,
        &Operation::TrackBranch {
            local: "tracking-main".into(),
            remote: remote.reference.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        git.command(root, &["rev-parse", "--symbolic-full-name", "@{upstream}"])
            .unwrap()
            .trim(),
        "refs/remotes/origin/main"
    );
}

#[test]
fn cherry_pick_and_revert_complete_or_abort_conflicts_without_losing_history() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file", "base\n");
    stage(&git, root, "file");
    commit(&git, root, "Base");
    git.execute(root, &Operation::CreateBranch("topic".into()))
        .unwrap();
    write(root, "file", "topic\n");
    stage(&git, root, "file");
    commit(&git, root, "Topic edit");
    let topic = git.resolve_commit(root, "HEAD").unwrap();
    git.execute(root, &Operation::Switch("main".into()))
        .unwrap();
    git.execute(root, &Operation::CherryPick(topic.clone()))
        .unwrap();
    let applied = git.resolve_commit(root, "HEAD").unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("file")).unwrap(),
        "topic\n"
    );
    git.execute(root, &Operation::Revert(applied.clone()))
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("file")).unwrap(),
        "base\n"
    );
    write(root, "file", "main\n");
    stage(&git, root, "file");
    commit(&git, root, "Main edit");
    let before = git.resolve_commit(root, "HEAD").unwrap();
    assert!(git
        .execute(root, &Operation::CherryPick(topic.clone()))
        .is_err());
    assert_eq!(
        git.snapshot(root, 20).unwrap().operation.as_deref(),
        Some("Cherry-pick in progress")
    );
    git.execute(root, &Operation::Abort("Cherry-pick in progress".into()))
        .unwrap();
    assert_eq!(git.resolve_commit(root, "HEAD").unwrap(), before);
    assert_eq!(
        std::fs::read_to_string(root.join("file")).unwrap(),
        "main\n"
    );
    assert!(git.execute(root, &Operation::CherryPick(topic)).is_err());
    write(root, "file", "combined\n");
    stage(&git, root, "file");
    git.execute(root, &Operation::Continue("Cherry-pick in progress".into()))
        .unwrap();
    assert!(git.snapshot(root, 20).unwrap().operation.is_none());
    assert!(git
        .execute(root, &Operation::Revert(applied.clone()))
        .is_err());
    assert_eq!(
        git.snapshot(root, 20).unwrap().operation.as_deref(),
        Some("Revert in progress")
    );
    git.execute(root, &Operation::Abort("Revert in progress".into()))
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("file")).unwrap(),
        "combined\n"
    );
    assert!(git.execute(root, &Operation::Revert(applied)).is_err());
    write(root, "file", "resolved revert\n");
    stage(&git, root, "file");
    git.execute(root, &Operation::Continue("Revert in progress".into()))
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("file")).unwrap(),
        "resolved revert\n"
    );
    assert!(git.snapshot(root, 20).unwrap().operation.is_none());
}

#[test]
fn first_parent_merge_review_and_non_destructive_branch_deletion_are_consistent() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "base", "base\n");
    stage(&git, root, "base");
    commit(&git, root, "Base");
    git.execute(root, &Operation::CreateBranch("topic".into()))
        .unwrap();
    write(root, "topic", "topic\n");
    stage(&git, root, "topic");
    commit(&git, root, "Topic");
    git.execute(root, &Operation::Switch("main".into()))
        .unwrap();
    assert!(git
        .execute(root, &Operation::DeleteBranch("topic".into()))
        .is_err());
    write(root, "main", "main\n");
    stage(&git, root, "main");
    commit(&git, root, "Main");
    git.execute(root, &Operation::Merge("topic".into()))
        .unwrap();
    let merge = git.resolve_commit(root, "HEAD").unwrap();
    let files = git.commit_files(root, &merge).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, Path::new("topic"));
    assert!(git
        .diff(root, &select("topic", DiffKind::Commit(merge.clone())))
        .unwrap()
        .patch
        .contains("+topic"));
    assert!(git
        .execute(root, &Operation::CherryPick(merge.clone()))
        .is_err());
    assert!(git.execute(root, &Operation::Revert(merge)).is_err());
    assert!(git.snapshot(root, 20).unwrap().operation.is_none());
    git.execute(root, &Operation::DeleteBranch("topic".into()))
        .unwrap();
    assert!(git.resolve_commit(root, "topic").is_err());
    assert!(git
        .execute(root, &Operation::DeleteBranch("main".into()))
        .is_err());
}

#[test]
fn stale_stash_identifiers_cannot_drop_a_newer_stash() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file", "base\n");
    stage(&git, root, "file");
    commit(&git, root, "Base");
    write(root, "file", "first stash\n");
    git.execute(root, &Operation::Stash("First".into()))
        .unwrap();
    let old = git.snapshot(root, 20).unwrap().stashes.remove(0);
    write(root, "file", "second stash\n");
    git.execute(root, &Operation::Stash("Second".into()))
        .unwrap();
    assert!(git
        .execute(
            root,
            &Operation::DropStash {
                name: old.name,
                oid: old.oid
            }
        )
        .unwrap_err()
        .contains("changed"));
    assert_eq!(git.snapshot(root, 20).unwrap().stashes.len(), 2);
}

#[test]
fn switching_a_deleted_local_branch_never_guesses_a_remote_branch() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file", "base\n");
    stage(&git, root, "file");
    commit(&git, root, "Base");
    let oid = git.resolve_commit(root, "HEAD").unwrap();
    git.execute(
        root,
        &Operation::CreateBranchAt {
            name: "topic".into(),
            oid: oid.clone(),
        },
    )
    .unwrap();
    git.command(root, &["update-ref", "refs/remotes/origin/topic", &oid])
        .unwrap();
    git.command(
        root,
        &["config", "remote.origin.url", root.to_str().unwrap()],
    )
    .unwrap();
    git.command(
        root,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
    )
    .unwrap();
    git.execute(root, &Operation::DeleteBranch("topic".into()))
        .unwrap();
    assert!(
        git.execute(root, &Operation::Switch("topic".into()))
            .is_err(),
        "A stale Switch action must not silently create a tracking branch"
    );
    assert_eq!(git.snapshot(root, 20).unwrap().branch, "main");
    assert!(!git
        .snapshot(root, 20)
        .unwrap()
        .branches
        .iter()
        .any(|b| b.reference == "refs/heads/topic"));
}

#[test]
fn inherited_repository_environment_fixture() {
    let Some(root) = std::env::var_os("HEDIT_TEST_ENV_REPOSITORY") else {
        return;
    };
    let root = Path::new(&root);
    let git = Git::default();
    stage(&git, root, "file");
    commit(&git, root, "Only change the selected repository");
}

#[test]
fn inherited_common_dir_cannot_redirect_commits_to_a_different_repository() {
    let (selected, git) = repo();
    let (other, _) = repo();
    write(selected.path(), "file", "selected\n");
    stage(&git, selected.path(), "file");
    commit(&git, selected.path(), "Selected base");
    write(other.path(), "file", "other\n");
    stage(&git, other.path(), "file");
    commit(&git, other.path(), "Other base");
    let before = git.resolve_commit(other.path(), "HEAD").unwrap();
    write(selected.path(), "file", "edited selected\n");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "inherited_repository_environment_fixture",
            "--nocapture",
        ])
        .env("HEDIT_TEST_ENV_REPOSITORY", selected.path())
        .env("GIT_COMMON_DIR", other.path().join(".git"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        git.resolve_commit(other.path(), "HEAD").unwrap(),
        before,
        "Inherited GIT_COMMON_DIR must not redirect a selected repository action"
    );
    assert_eq!(
        git.history(selected.path(), 20).unwrap()[0].subject,
        "Only change the selected repository"
    );
}

#[cfg(unix)]
#[test]
fn cancelling_a_running_hook_stops_its_process_group_and_preserves_staged_changes() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file", "base\n");
    stage(&git, root, "file");
    commit(&git, root, "Base");
    let before = git.resolve_commit(root, "HEAD").unwrap();
    write(root, "file", "staged\n");
    stage(&git, root, "file");
    let hooks = root.join("hooks");
    std::fs::create_dir(&hooks).unwrap();
    let hook = hooks.join("pre-commit");
    std::fs::write(
        &hook,
        "#!/bin/sh\nprintf ready > hook-started\nexec sleep 30\n",
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    git.command(root, &["config", "core.hooksPath", hooks.to_str().unwrap()])
        .unwrap();
    let worker_git = git.clone();
    let worker_root = root.to_owned();
    let worker = std::thread::spawn(move || {
        worker_git.execute(
            &worker_root,
            &Operation::Commit {
                message: "Cancelled".into(),
                amend: false,
            },
        )
    });
    let start = Instant::now();
    while !root.join("hook-started").exists() {
        assert!(start.elapsed() < Duration::from_secs(8));
        std::thread::sleep(Duration::from_millis(10));
    }
    git.cancel.store(true, Ordering::Relaxed);
    assert!(worker.join().unwrap().unwrap_err().contains("cancelled"));
    assert!(start.elapsed() < Duration::from_secs(10));
    git.cancel.store(false, Ordering::Relaxed);
    assert_eq!(git.resolve_commit(root, "HEAD").unwrap(), before);
    assert!(git
        .snapshot(root, 20)
        .unwrap()
        .files
        .iter()
        .any(|f| f.path == Path::new("file") && f.staged()));
    assert_eq!(
        std::fs::read_to_string(root.join("file")).unwrap(),
        "staged\n"
    );
    git.command(
        root,
        &["config", "core.hooksPath", "/nonexistent/hedit-test-hooks"],
    )
    .unwrap();
    commit(&git, root, "Retry after cancellation");
}

#[test]
fn index_lock_failures_keep_the_lock_and_all_working_text() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file", "base\n");
    stage(&git, root, "file");
    commit(&git, root, "Base");
    write(root, "file", "working\n");
    let lock = root.join(".git/index.lock");
    std::fs::write(&lock, "held by another client").unwrap();
    assert!(git
        .execute(root, &Operation::Stage(vec!["file".into()]))
        .unwrap_err()
        .contains("index.lock"));
    assert_eq!(
        std::fs::read_to_string(&lock).unwrap(),
        "held by another client"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("file")).unwrap(),
        "working\n"
    );
}

#[test]
fn oversized_git_output_is_rejected_without_losing_repository_state() {
    let (directory, git) = repo();
    let root = directory.path();
    std::fs::write(root.join("large.txt"), vec![b'x'; 25 * 1024 * 1024]).unwrap();
    stage(&git, root, "large.txt");
    commit(&git, root, "Large text");
    let before = git.resolve_commit(root, "HEAD").unwrap();
    assert!(git
        .command(root, &["show", "HEAD:large.txt"])
        .unwrap_err()
        .contains("24 MiB"));
    assert_eq!(git.resolve_commit(root, "HEAD").unwrap(), before);
    assert!(git.snapshot(root, 20).unwrap().files.is_empty());
}

#[test]
fn divergent_local_remotes_reject_pull_and_push_without_rewriting_either_side() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file", "base\n");
    stage(&git, root, "file");
    commit(&git, root, "Base");
    let remote = tempfile::tempdir().unwrap();
    git.command(remote.path(), &["init", "--bare", "-b", "main"])
        .unwrap();
    git.execute(
        root,
        &Operation::AddRemote {
            name: "origin".into(),
            url: remote.path().to_string_lossy().into_owned(),
        },
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
    let clone_parent = tempfile::tempdir().unwrap();
    let clone = git
        .clone_repository(
            remote.path().to_str().unwrap(),
            &clone_parent.path().join("other"),
        )
        .unwrap();
    for (key, value) in [
        ("user.name", "Remote Author"),
        ("user.email", "remote@example.invalid"),
        ("commit.gpgsign", "false"),
        ("core.hooksPath", "/nonexistent/hedit-test-hooks"),
    ] {
        git.command(&clone, &["config", key, value]).unwrap();
    }
    write(&clone, "remote", "remote\n");
    stage(&git, &clone, "remote");
    commit(&git, &clone, "Remote edit");
    git.execute(&clone, &Operation::Push).unwrap();
    write(root, "local", "local\n");
    stage(&git, root, "local");
    commit(&git, root, "Local edit");
    git.execute(root, &Operation::Fetch).unwrap();
    let snapshot = git.snapshot(root, 20).unwrap();
    assert_eq!((snapshot.ahead, snapshot.behind), (1, 1));
    let local = snapshot.oid;
    let remote_before = git
        .command(remote.path(), &["rev-parse", "refs/heads/main"])
        .unwrap();
    assert!(git.execute(root, &Operation::Pull).is_err());
    assert!(git.execute(root, &Operation::Push).is_err());
    assert_eq!(git.resolve_commit(root, "HEAD").unwrap(), local);
    assert_eq!(
        git.command(remote.path(), &["rev-parse", "refs/heads/main"])
            .unwrap(),
        remote_before
    );
    assert!(git.snapshot(root, 20).unwrap().files.is_empty());
    git.execute(root, &Operation::Merge("refs/remotes/origin/main".into()))
        .unwrap();
    git.execute(root, &Operation::Push).unwrap();
    assert!(root.join("local").exists() && root.join("remote").exists());
    assert_eq!(
        (
            git.snapshot(root, 20).unwrap().ahead,
            git.snapshot(root, 20).unwrap().behind
        ),
        (0, 0)
    );
}

#[cfg(unix)]
#[test]
fn advanced_queries_preserve_non_utf8_paths_with_quoting_disabled() {
    use std::os::unix::ffi::OsStringExt;
    let (directory, git) = repo();
    let root = directory.path();
    let path = std::path::PathBuf::from(std::ffi::OsString::from_vec(b"literal-\xff.txt".to_vec()));
    // Git trees can contain paths that APFS cannot check out. Test the object
    // directly so the same real-Git case also runs on macOS.
    use std::io::Write;
    let mut hash = std::process::Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    hash.stdin
        .take()
        .unwrap()
        .write_all("Unicode source: 日本語\n".as_bytes())
        .unwrap();
    let blob = hash.wait_with_output().unwrap();
    assert!(blob.status.success());
    let blob = String::from_utf8(blob.stdout).unwrap();
    let mut tree = std::process::Command::new("git")
        .args(["mktree", "-z"])
        .current_dir(root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut record = format!("100644 blob {}\t", blob.trim()).into_bytes();
    record.extend_from_slice(b"literal-\xff.txt\0");
    tree.stdin.take().unwrap().write_all(&record).unwrap();
    let tree = tree.wait_with_output().unwrap();
    assert!(tree.status.success());
    let tree = String::from_utf8(tree.stdout).unwrap();
    let oid = git
        .command(root, &["commit-tree", tree.trim(), "-m", "Non-UTF-8 path"])
        .unwrap();
    git.command(root, &["update-ref", "refs/heads/main", oid.trim()])
        .unwrap();
    git.command(root, &["config", "core.quotePath", "false"])
        .unwrap();
    assert_eq!(
        git.scoped_history(
            root,
            20,
            &HistoryScope {
                reference: None,
                path: Some(path.clone())
            }
        )
        .unwrap()
        .len(),
        1
    );
    let lines = git.blame(root, "HEAD", &path).unwrap();
    assert_eq!(lines[0].text, "Unicode source: 日本語");
}

#[test]
fn sha256_repositories_support_review_tags_partial_staging_and_undo() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let git = Git::default();
    git.command(root, &["init", "--object-format=sha256", "-b", "main"])
        .unwrap();
    for (key, value) in [
        ("user.name", "SHA256 Test"),
        ("user.email", "sha256@example.invalid"),
        ("commit.gpgsign", "false"),
        ("tag.gpgsign", "false"),
        ("core.hooksPath", "/nonexistent/hedit-test-hooks"),
    ] {
        git.command(root, &["config", key, value]).unwrap();
    }
    write(root, "file", "one\ntwo\n");
    stage(&git, root, "file");
    commit(&git, root, "Base");
    let base = git.resolve_commit(root, "HEAD").unwrap();
    assert_eq!(base.len(), 64);
    write(root, "file", "ONE\ntwo\n");
    let selection = select("file", DiffKind::Working);
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
    commit(&git, root, "Edit");
    let head = git.resolve_commit(root, "HEAD").unwrap();
    git.execute(
        root,
        &Operation::CreateTag {
            name: "sha256-tag".into(),
            oid: head.clone(),
            message: "SHA256 tag".into(),
        },
    )
    .unwrap();
    assert_eq!(git.tags(root).unwrap()[0].commit, Some(head.clone()));
    assert_eq!(
        git.compare(root, &base, &head, false).unwrap().files.len(),
        1
    );
    git.execute(root, &Operation::UndoCommit { expected: head })
        .unwrap();
    assert_eq!(git.resolve_commit(root, "HEAD").unwrap(), base);
    assert!(git.snapshot(root, 20).unwrap().files[0].staged());
}

#[test]
fn stash_apply_conflicts_keep_the_saved_stash_and_allow_manual_resolution() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file", "base\n");
    stage(&git, root, "file");
    commit(&git, root, "Base");
    write(root, "file", "stashed\n");
    git.execute(root, &Operation::Stash("Unfinished work".into()))
        .unwrap();
    let stash = git.snapshot(root, 20).unwrap().stashes[0].oid.clone();
    write(root, "file", "committed\n");
    stage(&git, root, "file");
    commit(&git, root, "Conflicting edit");
    assert!(git
        .execute(root, &Operation::ApplyStash(stash.clone()))
        .is_err());
    let snapshot = git.snapshot(root, 20).unwrap();
    assert!(snapshot.files[0].conflict);
    assert_eq!(snapshot.stashes[0].oid, stash);
    let preview = git.diff(root, &select("file", DiffKind::Conflict)).unwrap();
    assert!(preview
        .conflict
        .unwrap()
        .theirs
        .unwrap()
        .contains("stashed"));
    write(root, "file", "resolved\n");
    stage(&git, root, "file");
    commit(&git, root, "Resolve stash conflict");
    assert_eq!(git.snapshot(root, 20).unwrap().stashes[0].oid, stash);
    assert!(git.snapshot(root, 20).unwrap().files.is_empty());
}

#[test]
fn detached_undo_and_occupied_worktree_branches_preserve_other_checkouts() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file", "base\n");
    stage(&git, root, "file");
    commit(&git, root, "Base");
    let base = git.resolve_commit(root, "HEAD").unwrap();
    write(root, "file", "next\n");
    stage(&git, root, "file");
    commit(&git, root, "Next");
    let head = git.resolve_commit(root, "HEAD").unwrap();
    let linked = tempfile::tempdir().unwrap();
    let worktree = linked.path().join("linked");
    git.command(
        root,
        &[
            "worktree",
            "add",
            "-b",
            "occupied",
            worktree.to_str().unwrap(),
            "HEAD",
        ],
    )
    .unwrap();
    assert!(git
        .execute(root, &Operation::Switch("occupied".into()))
        .is_err());
    assert!(git
        .execute(root, &Operation::DeleteBranch("occupied".into()))
        .is_err());
    assert_eq!(
        std::fs::read_to_string(worktree.join("file")).unwrap(),
        "next\n"
    );
    git.command(root, &["switch", "--detach", "HEAD"]).unwrap();
    git.execute(
        root,
        &Operation::UndoCommit {
            expected: head.clone(),
        },
    )
    .unwrap();
    let snapshot = git.snapshot(root, 20).unwrap();
    assert_eq!(snapshot.branch, "(detached)");
    assert_eq!(snapshot.oid, base);
    assert_eq!(git.resolve_commit(root, "refs/heads/main").unwrap(), head);
    assert!(snapshot.files[0].staged());
}

#[test]
fn repository_initialization_refuses_existing_and_nested_repositories() {
    let directory = tempfile::tempdir().unwrap();
    let git = Git::default();
    git.execute(directory.path(), &Operation::Init).unwrap();
    assert!(git.snapshot(directory.path(), 20).unwrap().unborn);
    assert!(git.execute(directory.path(), &Operation::Init).is_err());
    let nested = directory.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    assert!(git.execute(&nested, &Operation::Init).is_err());
    assert!(!nested.join(".git").exists());
}

#[cfg(unix)]
#[test]
#[ignore = "exercises the actual 30-second Git timeout"]
fn a_running_local_operation_times_out_and_allows_a_clean_retry() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "file", "base\n");
    stage(&git, root, "file");
    commit(&git, root, "Base");
    let before = git.resolve_commit(root, "HEAD").unwrap();
    write(root, "file", "staged\n");
    stage(&git, root, "file");
    let hooks = root.join("timeout-hooks");
    std::fs::create_dir(&hooks).unwrap();
    let hook = hooks.join("pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nexec sleep 60\n").unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    git.command(root, &["config", "core.hooksPath", hooks.to_str().unwrap()])
        .unwrap();
    let start = Instant::now();
    let error = git
        .execute(
            root,
            &Operation::Commit {
                message: "Must time out".into(),
                amend: false,
            },
        )
        .unwrap_err();
    assert!(error.contains("timed out"), "{error}");
    assert!(
        start.elapsed() >= Duration::from_secs(29) && start.elapsed() < Duration::from_secs(40)
    );
    assert_eq!(git.resolve_commit(root, "HEAD").unwrap(), before);
    assert!(git
        .snapshot(root, 20)
        .unwrap()
        .files
        .iter()
        .any(|f| f.path == Path::new("file") && f.staged()));
    git.command(
        root,
        &["config", "core.hooksPath", "/nonexistent/hedit-test-hooks"],
    )
    .unwrap();
    commit(&git, root, "Retry after timeout");
}

#[test]
fn embedded_nul_in_a_path_cannot_inject_additional_stage_or_unstage_paths() {
    let (directory, git) = repo();
    let root = directory.path();
    write(root, "first", "first\n");
    write(root, "second", "second\n");
    let invalid = std::path::PathBuf::from("first\0second");
    assert!(git
        .execute(root, &Operation::Stage(vec![invalid.clone()]))
        .is_err());
    assert!(git
        .snapshot(root, 20)
        .unwrap()
        .files
        .iter()
        .all(|f| f.untracked()));
    stage(&git, root, "first");
    assert!(git
        .execute(root, &Operation::Unstage(vec![invalid]))
        .is_err());
    assert!(git
        .snapshot(root, 20)
        .unwrap()
        .files
        .iter()
        .any(|f| f.path == Path::new("first") && f.staged()));
}
