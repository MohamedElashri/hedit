use hedit::{document::Document, intelligence, projects, search, workspace::Workspace};
use std::{path::Path, sync::atomic::AtomicBool};

#[test]
fn unicode_regex_search_scopes_captures_and_literal_replacements() {
    let text = "α_name alpha ALPHA alphabet\nβ_name";
    let mut options = search::Options {
        regex: false,
        case_sensitive: false,
        whole_word: true,
    };
    assert_eq!(
        search::matches(text, "alpha", &options, 0..text.len())
            .unwrap()
            .len(),
        2
    );
    options.regex = true;
    options.case_sensitive = true;
    options.whole_word = false;
    let (changed, count) =
        search::replace(text, r"(\pL)_name", "${1}_value", &options, 0..text.len()).unwrap();
    assert_eq!(count, 2);
    assert!(changed.starts_with("α_value"));
    assert!(changed.ends_with("β_value"));
    assert_eq!(
        search::matches(text, "name", &options, 1..text.len()).unwrap_err(),
        "Invalid search selection."
    );
    options.regex = false;
    assert_eq!(
        search::replace("a a", "a", "$1", &options, 0..1).unwrap().0,
        "$1 a"
    );
    assert!(search::Options {
        regex: true,
        ..Default::default()
    }
    .compile("[")
    .is_err());
}

#[test]
fn folder_search_respects_ignore_filters_roots_and_stale_previews() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join(".gitignore"), "ignored.rs\n").unwrap();
    std::fs::write(root.join("ignored.rs"), "needle").unwrap();
    std::fs::write(root.join("src/main.rs"), "needle α\nneedle β\n").unwrap();
    std::fs::write(root.join("notes.txt"), "needle").unwrap();
    let roots = [root.to_path_buf(), root.join("src")];
    let cancel = AtomicBool::new(false);
    let result = search::folders(
        &roots,
        "*.rs",
        "needle",
        Some("found"),
        &Default::default(),
        &cancel,
    )
    .unwrap();
    assert_eq!(result.hits.len(), 2);
    assert_eq!(result.plans.len(), 1);
    assert_eq!(result.hits[1].line, 2);
    assert!(search::diff(
        &result.plans[0].before,
        &result.plans[0].after,
        Path::new("main.rs")
    )
    .contains("+found α"));
    std::fs::write(root.join("src/main.rs"), "changed outside\n").unwrap();
    assert!(search::apply(&result.plans[0])
        .unwrap_err()
        .contains("changed since preview"));
    let result = search::folders(
        &roots,
        "src/*.rs",
        "changed",
        Some("updated"),
        &Default::default(),
        &cancel,
    )
    .unwrap();
    search::apply(&result.plans[0]).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("src/main.rs")).unwrap(),
        "updated outside\n"
    );
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(search::folders(&roots, "", "updated", None, &Default::default(), &cancel).is_err());
}

#[test]
fn utf16_and_legacy_encodings_save_and_recover_without_loss() {
    for label in ["UTF-16LE", "UTF-16BE", "windows-1252"] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("encoded.txt");
        let source = "café €\r\n";
        let bytes = hedit::text_encoding::encode(source, label, true).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let mut doc = if label == "windows-1252" {
            Document::open_with_encoding(2, &path, label).unwrap()
        } else {
            Document::open_or_new(2, &path).unwrap()
        };
        assert_eq!(doc.page(0).unwrap().text, source);
        assert_eq!(doc.info().encoding, label);
        doc.replace(0, 0, "hello ".into(), doc.info().revision)
            .unwrap();
        doc.save(None).unwrap();
        assert_eq!(
            hedit::text_encoding::decode(&std::fs::read(&path).unwrap(), label)
                .unwrap()
                .0,
            format!("hello {source}")
        );
        let (mut ws, _) = Workspace::restore(directory.path().join("session")).unwrap();
        let page = doc.page(0).unwrap();
        ws.add(doc, page).unwrap();
        ws.checkpoint().unwrap();
        drop(ws);
        let (ws, errors) = Workspace::restore(directory.path().join("session")).unwrap();
        assert!(errors.is_empty());
        let shared = ws.editor.get(ws.open[ws.active].view.id).unwrap();
        let mut doc = shared.lock().unwrap();
        assert_eq!(doc.info().encoding, label);
        let revision = doc.info().revision;
        assert_eq!(doc.history(false, 0, revision).unwrap().text, source);
    }
    assert!(hedit::text_encoding::encode("λ", "windows-1252", false).is_err());
    assert!(hedit::text_encoding::decode(&[0xff, 0xfe, 0x00], "UTF-16LE").is_err());
}

#[test]
fn undo_redo_and_format_history_survive_multiple_checkpoints_and_restarts() {
    let directory = tempfile::tempdir().unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().into()).unwrap();
    let id = ws.open[0].view.id;
    ws.open[0].page.text = "first λ\n".into();
    ws.edit(0, "").unwrap();
    let old = ws.open[0].page.text.clone();
    ws.open[0].page.text = "second β\r\n".into();
    ws.edit_with_line_ending(0, &old, Some(true)).unwrap();
    ws.checkpoint().unwrap();
    let shared = ws.editor.get(id).unwrap();
    let mut doc = shared.lock().unwrap();
    let rev = doc.info().revision;
    let page = doc.history(false, 0, rev).unwrap();
    assert_eq!(page.text, "first λ\n");
    drop(doc);
    drop(shared);
    ws.open[0].set_page(page);
    ws.checkpoint().unwrap();
    drop(ws);
    let (ws, errors) = Workspace::restore(directory.path().into()).unwrap();
    assert!(errors.is_empty());
    let shared = ws.editor.get(ws.open[0].view.id).unwrap();
    let mut doc = shared.lock().unwrap();
    let rev = doc.info().revision;
    let page = doc.history(true, 0, rev).unwrap();
    assert_eq!(page.text, "second β\r\n");
    assert_eq!(page.info.eol, "CRLF");
    let rev = doc.info().revision;
    assert_eq!(doc.history(false, 0, rev).unwrap().text, "first λ\n");
}

#[test]
fn disk_conflict_resolution_is_guarded_and_undoable() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.txt");
    std::fs::write(&path, "original").unwrap();
    let mut doc = Document::open(1, &path).unwrap();
    doc.replace(0, 8, "my version".into(), 0).unwrap();
    std::fs::write(&path, "disk version").unwrap();
    assert!(doc.save(None).is_err());
    assert!(doc.accept_disk_version("outdated comparison").is_err());
    let revision = doc.info().revision;
    let page = doc.take_disk_version("disk version", revision).unwrap();
    assert_eq!(page.text, "disk version");
    assert!(!page.info.dirty);
    let revision = doc.info().revision;
    let page = doc.history(false, 0, revision).unwrap();
    assert_eq!(page.text, "my version");
    assert!(page.info.dirty);
    doc.accept_disk_version("disk version").unwrap();
    doc.save(None).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "my version");
}

#[test]
fn editorconfig_cascade_root_globs_and_project_language_defaults() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join(".editorconfig"),"root = true\n[*]\nindent_style = space\nindent_size = 2\ntrim_trailing_whitespace = true\n[*.{rs,cu}]\ninsert_final_newline = true\n").unwrap();
    std::fs::write(
        root.join("src/.editorconfig"),
        "[*.rs]\nindent_size = 8\nend_of_line = crlf\n",
    )
    .unwrap();
    let mut defaults = projects::Defaults::default();
    defaults.formatting.indent_size = Some(4);
    let mut project = projects::Defaults::default();
    project.languages.insert(
        "rust".into(),
        projects::Formatting {
            indent_style: Some("tabs".into()),
            ..Default::default()
        },
    );
    let format =
        projects::formatting(&root.join("src/main.rs"), "rust", &defaults, Some(&project)).unwrap();
    assert_eq!(format.indent_size, Some(8));
    assert_eq!(format.indent_style.as_deref(), Some("spaces"));
    assert_eq!(format.end_of_line.as_deref(), Some("crlf"));
    assert_eq!(projects::clean("a  \r\nβ\t", &format, "CRLF"), "a\r\nβ\r\n");
    let other = projects::formatting(&root.join("file.cu"), "cuda", &defaults, None).unwrap();
    assert_eq!(other.insert_final_newline, Some(true));
}

#[test]
fn project_definitions_resolve_relative_roots_and_preserve_personal_state() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("src")).unwrap();
    let path = directory.path().join("demo.hedit-project");
    projects::atomic_json(
        &path,
        &projects::Project {
            folders: vec!["src".into()],
            ..Default::default()
        },
    )
    .unwrap();
    projects::atomic_json(
        &path.with_extension("hedit-workspace"),
        &projects::State {
            pane_mode: 2,
            wrap: true,
            font_size: 18.0,
            ..Default::default()
        },
    )
    .unwrap();
    let (project, state) = projects::load(&path).unwrap();
    // Project folders are resolved, like /var → /private/var on macOS.
    assert_eq!(
        project.folders,
        [std::fs::canonicalize(directory.path().join("src")).unwrap()]
    );
    assert_eq!(state.pane_mode, 2);
    assert!(state.wrap);
    assert!(projects::read_json::<projects::Project>(&path, 1).is_err());
}

#[test]
fn symbols_completion_snippets_and_git_hunks_keep_unicode_positions() {
    let symbols=intelligence::symbols("pub fn λ() {}\nclass Thing:\n    async def method(self):\nint kernel(float* x) {\n// fn ignored\n",Path::new("source"));
    assert_eq!(
        symbols.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["λ", "Thing", "method", "kernel"]
    );
    let text = "λambda λatest λ";
    let (range, words) = intelligence::completions(text, text.chars().count());
    assert_eq!(&text[range], "λ");
    assert!(words.contains(&"λambda".into()));
    let (text, cursor) = intelligence::expand("before ${SELECTION} $0 after", "λ $0");
    assert_eq!(text, "before λ $0  after");
    assert_eq!(cursor, "before λ $0 ".chars().count());
    let diff = search::diff(
        "one\ntwo\nthree\n",
        "one\nchanged\nthree\n",
        Path::new("file"),
    );
    assert_eq!(
        intelligence::changed_lines(&diff)
            .into_iter()
            .collect::<Vec<_>>(),
        [2]
    );
    assert!(intelligence::hunk_at(&diff, 2).contains("+changed"));
}

#[test]
fn file_operations_refuse_overwrites_and_recursive_self_copies() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("src");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("file"), "keep").unwrap();
    let copy = directory.path().join("copy");
    hedit::folder::duplicate_path(&source, &copy).unwrap();
    assert_eq!(std::fs::read_to_string(copy.join("file")).unwrap(), "keep");
    assert!(hedit::folder::duplicate_path(&source, &copy).is_err());
    assert!(hedit::folder::duplicate_path(&source, &source.join("nested")).is_err());
    let moved = directory.path().join("moved");
    hedit::folder::move_path(&copy, &moved).unwrap();
    assert!(!copy.exists());
    assert!(moved.join("file").exists());
    assert!(hedit::folder::move_path(&source, &moved).is_err());
    assert_eq!(
        std::fs::read_to_string(source.join("file")).unwrap(),
        "keep"
    );
}

#[test]
fn existing_instance_cli_forwards_paths_and_stdin_and_returns_without_a_display() {
    use std::{
        io::Write,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let directory = tempfile::tempdir().unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().into()).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_hedit"))
        .arg("--session-dir")
        .arg(directory.path())
        .args(["file with spaces:4:2", "-"])
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"forwarded snippet\n")
        .unwrap();
    let start = Instant::now();
    let incoming = loop {
        if let Some(request) = hedit::ipc::receive(directory.path()) {
            break request.unwrap();
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(incoming.request.files[0]
        .path()
        .ends_with("file with spaces:4:2"));
    let spool = incoming.request.stdin.as_ref().unwrap().path();
    let mut doc = Document::from_stdin(ws.editor.id(), &spool).unwrap();
    let page = doc.page(0).unwrap();
    ws.add(doc, page).unwrap();
    ws.checkpoint().unwrap();
    incoming.finish(Ok(())).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(ws.open[ws.active].page.text, "forwarded snippet\n");
    assert!(!spool.exists());
}

#[test]
fn long_typing_history_is_coalesced_bounded_and_recoverable() {
    let directory = tempfile::tempdir().unwrap();
    let (ws, _) = Workspace::restore(directory.path().into()).unwrap();
    let shared = ws.editor.get(ws.open[0].view.id).unwrap();
    {
        let mut doc = shared.lock().unwrap();
        for n in 0..320 {
            let rev = doc.info().revision;
            doc.replace(n, n, "x".into(), rev).unwrap();
        }
    }
    ws.checkpoint().unwrap();
    drop(shared);
    drop(ws);
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.path().join("session.json")).unwrap())
            .unwrap();
    let history = manifest["open"][0]["document"]["history"]["undo"]
        .as_array()
        .unwrap();
    assert_eq!(history.len(), 64);
    assert!(history
        .iter()
        .all(|state| state["pieces"].as_array().unwrap().len() <= 1));
    let (ws, errors) = Workspace::restore(directory.path().into()).unwrap();
    assert!(errors.is_empty());
    let shared = ws.editor.get(ws.open[0].view.id).unwrap();
    let mut doc = shared.lock().unwrap();
    for remaining in (256..320).rev() {
        let rev = doc.info().revision;
        assert_eq!(doc.history(false, 0, rev).unwrap().text.len(), remaining);
    }
    let rev = doc.info().revision;
    assert_eq!(doc.history(false, 0, rev).unwrap().text.len(), 256);
}

#[test]
fn preserving_a_clean_source_before_removal_retains_its_text_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("remove.txt");
    std::fs::write(&path, "keep this clean file").unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let mut doc = Document::open(ws.editor.id(), &path).unwrap();
    doc.preserve_as_unsaved();
    let page = doc.page(0).unwrap();
    ws.add(doc, page).unwrap();
    ws.checkpoint().unwrap();
    std::fs::remove_file(path).unwrap();
    drop(ws);
    let (ws, errors) = Workspace::restore(directory.path().join("session")).unwrap();
    assert!(errors.is_empty());
    assert_eq!(ws.open[ws.active].page.text, "keep this clean file");
    assert!(ws.open[ws.active].page.info.dirty);
}

#[test]
fn git_comparison_reads_head_without_changing_index_or_worktree() {
    use std::process::Command;
    let directory = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(directory.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(&["init", "-q"]);
    std::fs::write(directory.path().join("source.txt"), "before\n").unwrap();
    run(&["add", "source.txt"]);
    run(&[
        "-c",
        "user.name=hedit test",
        "-c",
        "user.email=hedit@example.invalid",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-qm",
        "fixture",
    ]);
    let path = directory.path().join("source.txt");
    let before_index = std::fs::read(directory.path().join(".git/index")).unwrap();
    let diff = intelligence::git_diff(&path, "unsaved editor version\n").unwrap();
    assert!(diff.contains("-before"));
    assert!(diff.contains("+unsaved editor version"));
    assert_eq!(std::fs::read_to_string(path).unwrap(), "before\n");
    assert_eq!(
        std::fs::read(directory.path().join(".git/index")).unwrap(),
        before_index
    );
}

#[cfg(unix)]
#[test]
fn ipc_path_transport_preserves_non_utf8_bytes() {
    use std::os::unix::ffi::OsStringExt;
    let path =
        std::path::PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/filename-\xff".to_vec()));
    let wire = hedit::ipc::WirePath::new(&path);
    let bytes = serde_json::to_vec(&wire).unwrap();
    let restored: hedit::ipc::WirePath = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(restored.path(), path);
}

#[test]
fn snippet_fields_expand_in_order_with_mirrors_and_escapes() {
    let e = intelligence::expand_fields("for (${1:i} = 0; $1 < ${2:n}; $1++) {\n\t$0\n}", "");
    assert_eq!(e.text, "for (i = 0; i < n; i++) {\n\t\n}");
    assert_eq!(
        e.fields,
        vec![
            vec![(5, 6), (12, 13), (19, 20)],
            vec![(16, 17)],
            vec![(27, 27)]
        ]
    );
    // A mirror before the field with the text, nested fields, and no $0.
    let e = intelligence::expand_fields("$1 ${1:a ${2:b}}!", "");
    assert_eq!(e.text, "a b a b!");
    assert_eq!(
        e.fields,
        vec![vec![(0, 3), (4, 7)], vec![(6, 7)], vec![(8, 8)]]
    );
    // Escapes and plain dollars stay literal; the selection is not parsed.
    let e = intelligence::expand_fields("\\$1 $HOME ${x} \\} ${SELECTION}$0", "$2");
    assert_eq!(e.text, "$1 $HOME ${x} } $2");
    assert_eq!(e.fields, vec![vec![(18, 18)]]);
    // Fields are visited by number, not position.
    let e = intelligence::expand_fields("${2:b}${1}", "");
    assert_eq!(e.fields, vec![vec![(1, 1)], vec![(0, 1)], vec![(1, 1)]]);
}

#[test]
fn definitions_and_references_find_whole_words() {
    let text = "#define LIMIT 10\nint limit_of(int x) {\n    return x + LIMIT;\n}\nstatic const int other = LIMIT;\ntypedef struct node Node;\nconst value = 1;\nLIMITED = 2\n";
    let path = Path::new("a.c");
    let lines = |places: Vec<intelligence::Place>| -> Vec<(usize, usize)> {
        places.iter().map(|p| (p.line, p.column)).collect()
    };
    assert_eq!(
        lines(intelligence::definitions(text, "LIMIT", path)),
        [(1, 9)]
    );
    assert_eq!(
        lines(intelligence::definitions(text, "limit_of", path)),
        [(2, 5)]
    );
    assert_eq!(
        lines(intelligence::definitions(text, "Node", path)),
        [(6, 21)]
    );
    assert_eq!(
        lines(intelligence::definitions(text, "value", path)),
        [(7, 7)]
    );
    assert_eq!(
        lines(intelligence::definitions(text, "other", path)),
        [(5, 18)]
    );
    assert_eq!(
        lines(intelligence::definitions(text, "LIMITED", path)),
        [(8, 1)]
    );
    let refs = intelligence::references(text, "LIMIT", 100);
    assert_eq!(lines(refs.clone()), [(1, 9), (3, 16), (5, 26)]);
    assert_eq!(refs[1].text, "return x + LIMIT;");
    assert_eq!(intelligence::references(text, "LIMIT", 1).len(), 1);
    // Python definitions come from the symbol list.
    let py = "class Thing:\n    def method(self):\n        return Thing()\n";
    assert_eq!(
        lines(intelligence::definitions(py, "method", Path::new("a.py"))),
        [(2, 9)]
    );
}

#[test]
fn declarations_are_definitions_but_returns_are_not() {
    let text = "int count = 0;\nreturn count;\nchar *name, other;\nfloat grid[4];\n";
    let path = Path::new("a.c");
    let lines = |name: &str| -> Vec<usize> {
        intelligence::definitions(text, name, path)
            .iter()
            .map(|p| p.line)
            .collect()
    };
    assert_eq!(lines("count"), [1]);
    assert_eq!(lines("name"), [3]);
    assert_eq!(lines("grid"), [4]);
}
