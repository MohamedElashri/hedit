use hedit::{build, git};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn git_ok(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn build_message_survives_a_missing_optional_column() {
    let found = build::locations(
        "notes.c:18: warning message",
        r"^(.+?):(\d+)(?::(\d+))?: (.*)$",
        Path::new("src"),
        0,
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].column, None);
    assert_eq!(found[0].message, "warning message");
}

#[test]
fn build_regex_keeps_spaces_in_a_captured_filename() {
    let found = build::locations(
        " notes.c :18:2: warning",
        r"^(.+?):(\d+):(\d+): (.*)$",
        Path::new("src"),
        0,
    );
    assert_eq!(found[0].file, Path::new("src").join(" notes.c "));
}

#[cfg(unix)]
#[test]
fn git_root_name_can_end_in_a_literal_carriage_return() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("repository\r");
    fs::create_dir(&root).unwrap();
    git_ok(&root, &["init", "-q"]);
    assert_eq!(
        git::Git::default().discover(&root).unwrap(),
        root.canonicalize().unwrap()
    );
}

#[test]
fn untracked_text_diff_marks_a_missing_final_newline() {
    let d = tempfile::tempdir().unwrap();
    git_ok(d.path(), &["init", "-q"]);
    fs::write(d.path().join("new.txt"), "abc").unwrap();
    let diff = git::Git::default()
        .diff(
            d.path(),
            &git::Selection {
                path: PathBuf::from("new.txt"),
                kind: git::DiffKind::Untracked,
            },
        )
        .unwrap();
    assert_eq!(diff.additions, 1);
    assert!(
        diff.patch.contains("No newline at end of file"),
        "{}",
        diff.patch
    );
}
