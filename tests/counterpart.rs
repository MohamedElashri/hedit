use hedit::counterpart;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

fn file(root: &Path, relative: &str) -> PathBuf {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "").unwrap();
    path.canonicalize().unwrap()
}

fn find(source: &Path, roots: &[PathBuf]) -> Vec<PathBuf> {
    counterpart::find(source, roots, &AtomicBool::new(false)).unwrap()
}

#[test]
fn counterpart_switches_both_directions_for_c_cpp_cuda_and_objective_c() {
    let directory = tempfile::tempdir().unwrap();
    for (source, header) in [
        ("main.c", "main.h"),
        ("solver.cpp", "solver.hpp"),
        ("kernel.cu", "kernel.cuh"),
        ("view.mm", "view.hh"),
        ("شيفرة.CXX", "شيفرة.HXX"),
    ] {
        let source = file(directory.path(), source);
        let header = file(directory.path(), header);
        assert_eq!(find(&source, &[]), std::slice::from_ref(&header));
        assert_eq!(find(&header, &[]), [source]);
    }
}

#[test]
fn counterpart_prefers_siblings_and_returns_all_local_choices() {
    let directory = tempfile::tempdir().unwrap();
    let source = file(directory.path(), "src/solver.cpp");
    let first = file(directory.path(), "src/solver.h");
    let second = file(directory.path(), "src/solver.hpp");
    file(directory.path(), "include/solver.h");
    file(directory.path(), "src/solver.cu");
    file(directory.path(), "src/solver_test.hpp");
    fs::create_dir(directory.path().join("src/solver.cuh")).unwrap();
    assert_eq!(find(&source, &[directory.path().into()]), [first, second]);
}

#[test]
fn counterpart_searches_only_the_nearest_containing_project_and_honors_ignores() {
    let directory = tempfile::tempdir().unwrap();
    let source = file(directory.path(), "nested/src/solver.cpp");
    let header = file(directory.path(), "nested/include/solver.hpp");
    file(directory.path(), "include/solver.h");
    file(directory.path(), "nested/target/solver.h");
    file(directory.path(), "nested/generated/solver.h");
    file(directory.path(), "other/solver.h");
    fs::write(directory.path().join("nested/.gitignore"), "generated/\n").unwrap();
    let roots = [
        directory.path().into(),
        directory.path().join("nested"),
        directory.path().join("other"),
    ];
    assert_eq!(find(&source, &roots), [header]);
    assert!(
        find(&source, &[]).is_empty(),
        "no project means siblings only"
    );
}

#[test]
fn counterpart_handles_unsupported_missing_and_cancelled_searches() {
    let directory = tempfile::tempdir().unwrap();
    let source = file(directory.path(), "main.cpp");
    assert!(find(&source, &[]).is_empty());
    assert!(!counterpart::supported(Path::new("notes.md")));
    assert!(find(&directory.path().join("notes.md"), &[]).is_empty());
    assert!(counterpart::find(&source, &[], &AtomicBool::new(true)).is_err());
    fs::remove_file(&source).unwrap();
    assert!(counterpart::find(&source, &[], &AtomicBool::new(false))
        .unwrap()
        .is_empty());
}

#[test]
fn counterpart_finds_headers_for_a_source_that_has_not_been_saved_yet() {
    let directory = tempfile::tempdir().unwrap();
    let header = file(directory.path(), "new.hpp");
    let source = directory.path().join("new.cpp");
    assert_eq!(find(&source, &[]), [header]);
}

#[cfg(unix)]
#[test]
fn counterpart_deduplicates_aliases_and_skips_broken_links_and_cycles() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let source = file(directory.path(), "main.cpp");
    let header = file(directory.path(), "main.h");
    symlink(&header, directory.path().join("main.hpp")).unwrap();
    symlink(
        directory.path().join("missing"),
        directory.path().join("main.hh"),
    )
    .unwrap();
    symlink(directory.path(), directory.path().join("main.hxx")).unwrap();
    assert_eq!(find(&source, &[]), [header]);
}
