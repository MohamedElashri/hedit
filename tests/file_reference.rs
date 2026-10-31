use hedit::{
    advanced::Selection,
    file_reference::{at, resolve},
};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn cursor(text: &str, needle: &str) -> Selection {
    Selection::caret(text[..text.find(needle).unwrap()].chars().count() + 1)
}
fn file(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "").unwrap();
    path.canonicalize().unwrap()
}

#[test]
fn file_reference_extracts_quoted_include_bare_and_unicode_paths() {
    for (text, needle, expected) in [
        ("#include \"my header.hpp\"", "header", "my header.hpp"),
        ("#include <lib/solver.h>", "solver", "lib/solver.h"),
        ("don't open \"my file.txt\"", "file", "my file.txt"),
        ("don't open 'my file.txt'", "file", "my file.txt"),
        ("load('../data/ملف.csv');", "ملف", "../data/ملف.csv"),
        ("at (src/main.cpp:12:3): error", "main", "src/main.cpp:12:3"),
        ("α = `docs/a (draft).md`", "draft", "docs/a (draft).md"),
        (
            "path = \"C:\\Users\\name\\notes.txt\"",
            "notes",
            "C:\\Users\\name\\notes.txt",
        ),
    ] {
        assert_eq!(
            at(text, cursor(text, needle)).as_deref(),
            Some(expected),
            "{text}"
        );
    }
    assert_eq!(
        at("file.txt", Selection::caret(8)).as_deref(),
        Some("file.txt")
    );
    assert_eq!(
        at("file.cpp:12:3:", Selection::caret(3)).as_deref(),
        Some("file.cpp:12:3")
    );
}

#[test]
fn file_reference_selections_take_priority_and_invalid_references_are_rejected() {
    let text = "'a file.md'\nignored";
    assert_eq!(
        at(
            text,
            Selection {
                anchor: 11,
                head: 0
            }
        )
        .as_deref(),
        Some("a file.md")
    );
    assert!(at(
        text,
        Selection {
            anchor: 0,
            head: text.chars().count()
        }
    )
    .is_none());
    for text in [
        "",
        "   ",
        "http://example.org/file",
        "file:///tmp/file",
        "bad\0name",
    ] {
        assert!(at(text, Selection::caret(0)).is_none(), "{text:?}");
    }
    assert!(at(&"x".repeat(4097), Selection::caret(20)).is_none());
}

#[test]
fn file_reference_prefers_current_directory_then_lists_project_matches() {
    let directory = tempfile::tempdir().unwrap();
    let document = file(directory.path(), "current/main.cpp");
    let nearby = file(directory.path(), "current/common.h");
    let first = file(directory.path(), "first/include/common.h");
    let second = file(directory.path(), "second/common.h");
    let roots = [
        directory.path().join("first"),
        directory.path().join("second"),
    ];
    assert_eq!(
        resolve("common.h", Some(&document), &roots),
        std::slice::from_ref(&nearby)
    );
    fs::remove_file(nearby).unwrap();
    assert_eq!(
        resolve("common.h", Some(&document), &roots),
        [first, second]
    );
}

#[test]
fn file_reference_handles_locations_absolute_paths_and_traversal_without_creation() {
    let directory = tempfile::tempdir().unwrap();
    let document = file(directory.path(), "src/main.cpp");
    let target = file(directory.path(), "data/a file.txt");
    let expected = PathBuf::from(format!("{}:12:3", target.display()));
    assert_eq!(
        resolve("../data/a file.txt:12:3", Some(&document), &[]),
        [expected]
    );
    assert_eq!(
        resolve(target.to_str().unwrap(), None, &[]),
        std::slice::from_ref(&target)
    );
    assert_eq!(
        resolve("data/a file.txt", None, &[directory.path().into()]),
        [target]
    );
    for reference in [
        "missing.txt",
        "../data",
        "https://example.org/file",
        "bad\0file",
    ] {
        assert!(resolve(reference, Some(&document), &[]).is_empty());
    }
    assert!(!directory.path().join("src/missing.txt").exists());
}

#[cfg(unix)]
#[test]
fn file_reference_honors_literal_colon_names_and_deduplicates_symlinks() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let target = file(directory.path(), "file:12");
    assert_eq!(
        resolve("file:12", None, &[directory.path().into()]),
        std::slice::from_ref(&target)
    );
    fs::create_dir(directory.path().join("include")).unwrap();
    symlink(&target, directory.path().join("include/file:12")).unwrap();
    assert_eq!(
        resolve("file:12", None, &[directory.path().into()]),
        [target]
    );
}

#[cfg(unix)]
#[test]
fn file_reference_preserves_a_literal_trailing_colon_in_a_quoted_path() {
    let directory = tempfile::tempdir().unwrap();
    let expected = file(directory.path(), "report:");
    file(directory.path(), "report");
    let reference = at("\"report:\"", Selection::caret(3)).unwrap();
    assert_eq!(reference, "report:");
    assert_eq!(
        resolve(&reference, None, &[directory.path().into()]),
        [expected]
    );
}

#[cfg(unix)]
#[test]
fn file_reference_symlink_location_does_not_open_a_different_literal_colon_file() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let expected = file(directory.path(), "real.txt");
    file(directory.path(), "real.txt:2:1");
    symlink(&expected, directory.path().join("alias.txt")).unwrap();
    let found = resolve("alias.txt:2", None, &[directory.path().into()]);
    let (actual, line, column) = hedit::cli::location(&found[0]);
    assert_eq!(actual.canonicalize().unwrap(), expected);
    assert_eq!((line, column), (2, 1));
}
