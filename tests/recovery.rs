use hedit::{
    document::{Document, PAGE_SIZE, SMALL_LIMIT},
    session::SessionView,
    workspace::{splice, Workspace},
    Editor,
};
use std::{fs, io::Write};

fn edit(workspace: &mut Workspace, text: &str) {
    let index = workspace.active;
    let old = workspace.open[index].page.text.clone();
    workspace.open[index].page.text = text.to_owned();
    workspace.edit(index, &old).unwrap();
}

#[test]
fn untitled_and_closed_buffers_survive_restarts_and_keep_selection() {
    let directory = tempfile::tempdir().unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().into()).unwrap();
    edit(&mut ws, "notes kept for days\nαβγ\n");
    ws.open[0].view.anchor = 3;
    ws.open[0].view.head = 7;
    ws.new_buffer().unwrap();
    edit(&mut ws, "second unsaved snippet");
    ws.close(0).unwrap();
    ws.checkpoint().unwrap();
    drop(ws);
    let (mut ws, errors) = Workspace::restore(directory.path().into()).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(ws.open[0].page.text, "second unsaved snippet");
    assert!(ws.open[0].page.info.path.is_none());
    ws.reopen().unwrap();
    assert_eq!(ws.open[ws.active].page.text, "notes kept for days\nαβγ\n");
    assert_eq!(
        (ws.open[ws.active].view.anchor, ws.open[ws.active].view.head),
        (3, 7)
    );
}

#[test]
fn discarding_restored_edits_does_not_reuse_old_checkpoint_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.txt");
    fs::write(&path, "original\n").unwrap();
    let session = directory.path().join("session");
    let (mut ws, _) = Workspace::restore(session.clone()).unwrap();
    let mut doc = Document::open(ws.editor.id(), &path).unwrap();
    let page = doc.page(0).unwrap();
    ws.add(doc, page).unwrap();
    edit(&mut ws, "saved edits\n");
    let id = ws.open[ws.active].view.id;
    ws.open[ws.active].page.info = ws
        .editor
        .get(id)
        .unwrap()
        .lock()
        .unwrap()
        .save(None)
        .unwrap();
    ws.checkpoint().unwrap();
    drop(ws);

    let (mut ws, errors) = Workspace::restore(session.clone()).unwrap();
    assert!(errors.is_empty());
    // A restored document starts at revision zero with saved undo history.
    ws.checkpoint().unwrap();
    edit(&mut ws, "discard these edits\n");
    let shared = ws.editor.get(ws.open[ws.active].view.id).unwrap();
    let page = shared.lock().unwrap().discard_changes().unwrap();
    ws.open[ws.active].set_page(page);
    ws.checkpoint().unwrap();
    drop(shared);
    drop(ws);

    let (ws, errors) = Workspace::restore(session).unwrap();
    assert!(errors.is_empty());
    let shared = ws.editor.get(ws.open[ws.active].view.id).unwrap();
    let mut doc = shared.lock().unwrap();
    let revision = doc.info().revision;
    assert_eq!(
        doc.history(false, 0, revision).unwrap().text,
        "saved edits\n"
    );
    assert_eq!(fs::read_to_string(path).unwrap(), "saved edits\n");
}

#[test]
fn failed_discard_keeps_edits_and_undo_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.txt");
    fs::write(&path, "original\n").unwrap();
    let mut doc = Document::open(1, &path).unwrap();
    let info = doc
        .replace(0, doc.info().size, "edited\n".into(), 0)
        .unwrap();
    fs::remove_file(path).unwrap();
    assert!(doc.discard_changes().is_err());
    assert_eq!(doc.page(0).unwrap().text, "edited\n");
    assert_eq!(doc.info().revision, info.revision);
    assert!(doc.info().dirty);
    assert_eq!(
        doc.history(false, 0, info.revision).unwrap().text,
        "original\n"
    );
}

#[test]
fn second_process_cannot_write_the_same_session() {
    let directory = tempfile::tempdir().unwrap();
    let (ws, _) = Workspace::restore(directory.path().into()).unwrap();
    ws.checkpoint().unwrap();
    assert!(Workspace::restore(directory.path().into()).is_err());
    drop(ws);
    assert!(Workspace::restore(directory.path().into()).is_ok());
}

#[test]
fn invalid_checkpoint_leaves_previous_manifest_intact() {
    let directory = tempfile::tempdir().unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().into()).unwrap();
    edit(&mut ws, "checkpointed");
    ws.checkpoint().unwrap();
    let previous = fs::read(directory.path().join("session.json")).unwrap();
    let mut view = ws.view();
    view.open.push(view.open[0].clone());
    assert!(ws.store.save(&ws.editor, view).is_err());
    assert_eq!(
        fs::read(directory.path().join("session.json")).unwrap(),
        previous
    );
}

#[test]
fn corrupt_latest_manifest_falls_back_without_deleting_newer_buffers() {
    let directory = tempfile::tempdir().unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().into()).unwrap();
    edit(&mut ws, "previous generation");
    ws.checkpoint().unwrap();
    edit(&mut ws, "latest generation");
    ws.checkpoint().unwrap();
    fs::write(directory.path().join("session.json"), "{broken").unwrap();
    drop(ws);
    let (ws, errors) = Workspace::restore(directory.path().into()).unwrap();
    assert!(!errors.is_empty());
    assert_eq!(ws.open[0].page.text, "previous generation");
    ws.checkpoint().unwrap();
    let generations: Vec<_> = fs::read_dir(directory.path())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("buffers-"))
        .collect();
    assert!(generations
        .iter()
        .flat_map(|g| fs::read_dir(g.path()).unwrap().flatten())
        .any(|e| fs::read_to_string(e.path()).is_ok_and(|s| s == "latest generation")));
}

#[test]
fn corrupt_only_manifest_is_archived_with_its_buffers() {
    let directory = tempfile::tempdir().unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().into()).unwrap();
    edit(&mut ws, "rescue me");
    ws.checkpoint().unwrap();
    drop(ws);
    fs::write(directory.path().join("session.json"), "invalid").unwrap();
    let (_, errors) = Workspace::restore(directory.path().into()).unwrap();
    assert!(!errors.is_empty());
    let archive = fs::read_dir(directory.path())
        .unwrap()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().starts_with("preserved-"))
        .unwrap();
    assert!(archive.path().join("session.json").exists());
    assert!(fs::read_dir(archive.path())
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with("buffers-")));
}

#[test]
fn missing_recovery_buffer_is_retained_in_future_manifests() {
    let directory = tempfile::tempdir().unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().into()).unwrap();
    edit(&mut ws, "not silently forgotten");
    ws.checkpoint().unwrap();
    drop(ws);
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.path().join("session.json")).unwrap()).unwrap();
    let path = manifest["open"][0]["document"]["buffer"].as_str().unwrap();
    fs::remove_file(directory.path().join(path)).unwrap();
    let (ws, errors) = Workspace::restore(directory.path().into()).unwrap();
    assert!(!errors.is_empty());
    ws.checkpoint().unwrap();
    let updated: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.path().join("session.json")).unwrap()).unwrap();
    assert!(updated["open"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tab| tab["document"]["buffer"].as_str() == Some(path)));
}

#[test]
fn closed_history_never_evicts_unsaved_snippets() {
    let directory = tempfile::tempdir().unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().into()).unwrap();
    for n in 0..50 {
        edit(&mut ws, &format!("snippet {n}"));
        ws.close(ws.active).unwrap();
    }
    edit(&mut ws, "still open");
    assert!(ws.close(ws.active).is_err());
    assert_eq!(ws.closed.len(), 50);
    assert_eq!(ws.open[ws.active].page.text, "still open");
    ws.checkpoint().unwrap();
}

#[test]
fn small_buffer_has_backend_undo_and_redo_and_tracks_saved_state() {
    let directory = tempfile::tempdir().unwrap();
    let mut doc = Document::empty(1);
    doc.replace(0, 0, "abc".into(), 0).unwrap();
    doc.save(Some(directory.path().join("note.txt"))).unwrap();
    let rev = doc.info().revision;
    doc.replace(1, 2, "λ".into(), rev).unwrap();
    let page = doc.history(false, 0, doc.info().revision).unwrap();
    assert_eq!(page.text, "abc");
    assert!(!page.info.dirty);
    let page = doc.history(true, 0, doc.info().revision).unwrap();
    assert_eq!(page.text, "aλc");
    assert!(page.info.dirty);
}

#[test]
fn minimal_splices_do_not_split_unicode_scalars() {
    for (old, new) in [
        ("αβγ", "αδγ"),
        ("🐈 old", "🐈 new"),
        ("é", "ê"),
        ("", "hello"),
        ("delete", ""),
        ("same", "same"),
    ] {
        let (start, end, replacement) = splice(old, new);
        let mut actual = old.to_owned();
        actual.replace_range(start..end, replacement);
        assert_eq!(actual, new);
    }
}

#[test]
fn stale_edits_and_external_changes_cannot_overwrite_data() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("input.log");
    fs::write(&path, "original\n").unwrap();
    let mut doc = Document::open(1, &path).unwrap();
    doc.replace(0, 0, "notes ".into(), 0).unwrap();
    assert!(doc.replace(0, 0, "stale".into(), 0).is_err());
    fs::write(&path, "new external content\n").unwrap();
    assert!(doc.save(None).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "new external content\n");
    doc.save(Some(directory.path().join("rescued.log")))
        .unwrap();
    assert_eq!(
        fs::read_to_string(directory.path().join("rescued.log")).unwrap(),
        "notes original\n"
    );
}

#[test]
fn new_named_buffer_does_not_clobber_a_file_created_later() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("new.txt");
    let mut doc = Document::open_or_new(1, &path).unwrap();
    doc.replace(0, 0, "mine".into(), 0).unwrap();
    fs::write(&path, "theirs").unwrap();
    assert!(doc.save(None).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "theirs");
}

#[test]
fn atomic_save_preserves_bom_crlf_and_permissions() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("windows.log");
    fs::write(&path, b"\xef\xbb\xbffirst\r\nsecond\r\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    }
    let mut doc = Document::open(1, &path).unwrap();
    assert_eq!(doc.info().eol, "CRLF");
    doc.replace(0, 5, "edited".into(), 0).unwrap();
    doc.save(None).unwrap();
    assert_eq!(
        fs::read(&path).unwrap(),
        b"\xef\xbb\xbfedited\r\nsecond\r\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
}

#[test]
fn multi_gigabyte_open_uses_bounded_pages() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("huge.log");
    let mut file = fs::File::create(&path).unwrap();
    file.write_all(&vec![b'a'; 1024 * 1024]).unwrap();
    file.set_len(3 * 1024 * 1024 * 1024).unwrap();
    let mut doc = Document::open(1, &path).unwrap();
    let page = doc.page(0).unwrap();
    assert!(page.info.paged);
    assert_eq!(page.info.size, 3 * 1024 * 1024 * 1024);
    assert!(page.text.len() <= PAGE_SIZE as usize);
}

#[test]
fn paged_edits_recover_even_after_original_file_disappears() {
    let directory = tempfile::tempdir().unwrap();
    let session = directory.path().join("session");
    let path = directory.path().join("big.log");
    let content = "log record\n".repeat(SMALL_LIMIT as usize / 10 + 100);
    fs::write(&path, &content).unwrap();
    let (mut ws, _) = Workspace::restore(session.clone()).unwrap();
    let mut doc = Document::open(ws.editor.id(), &path).unwrap();
    let page = doc.page(0).unwrap();
    ws.add(doc, page).unwrap();
    let mut edited = ws.open[ws.active].page.text.clone();
    edited.insert_str(0, "kept\n");
    edit(&mut ws, &edited);
    ws.checkpoint().unwrap();
    fs::remove_file(path).unwrap();
    drop(ws);
    let (ws, errors) = Workspace::restore(session).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    assert!(ws.open[ws.active].page.text.starts_with("kept\n"));
    assert_eq!(ws.open[ws.active].page.info.size, content.len() as u64 + 5);
}

#[test]
fn rejects_binary_and_malformed_encodings() {
    let directory = tempfile::tempdir().unwrap();
    for bytes in [vec![0, 1, 2], vec![0xff, 0xfe, 1], vec![0xff, b'a']] {
        let path = directory.path().join("binary");
        fs::write(&path, bytes).unwrap();
        assert!(Document::open(1, &path).is_err());
    }
}

#[test]
fn global_search_finds_a_match_crossing_io_boundary() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("search.log");
    let mut text = "x".repeat(1024 * 1024 - 2);
    text.push_str("needle");
    text.push_str(&"x".repeat(1024 * 1024 + 10));
    fs::write(&path, text).unwrap();
    let doc = Document::open(1, &path).unwrap();
    let scan = doc.scan_search("needle", 0, 0).unwrap();
    assert_eq!(scan.count, 1);
    assert_eq!(scan.next_offset, Some(1024 * 1024 - 2));
}

#[test]
fn session_rejects_recovery_path_traversal() {
    let directory = tempfile::tempdir().unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().into()).unwrap();
    edit(&mut ws, "test");
    ws.checkpoint().unwrap();
    drop(ws);
    let path = directory.path().join("session.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["open"][0]["document"]["buffer"] = "../../secret".into();
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let (_, errors) = Workspace::restore(directory.path().into()).unwrap();
    assert!(errors
        .iter()
        .any(|e| e.contains("Invalid session buffer path")));
}

#[test]
fn save_before_restore_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let store = hedit::session::SessionStore::new(directory.path().into());
    let editor = Editor::default();
    assert!(store
        .save(
            &editor,
            SessionView {
                open: vec![],
                closed: vec![],
                active: None,
                secondary: None,
                folder: None,
                folders: vec![]
            }
        )
        .is_err());
}

#[test]
fn an_externally_growing_clean_log_does_not_block_scratch_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("growing.log");
    fs::write(&path, "record\n".repeat(SMALL_LIMIT as usize / 7 + 100)).unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let mut doc = Document::open(ws.editor.id(), &path).unwrap();
    let page = doc.page(0).unwrap();
    ws.add(doc, page).unwrap();
    ws.checkpoint().unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"new record\n")
        .unwrap();
    ws.new_buffer().unwrap();
    edit(&mut ws, "scratch text");
    ws.checkpoint().unwrap();
    drop(ws);
    let (ws, errors) = Workspace::restore(directory.path().join("session")).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(ws.open[ws.active].page.text, "scratch text");
}

#[test]
fn crash_writer_fixture() {
    let Some(directory) = std::env::var_os("HEDIT_TEST_CRASH_DIRECTORY") else {
        return;
    };
    let (mut ws, _) = Workspace::restore(directory.into()).unwrap();
    edit(&mut ws, "checkpoint survives a killed process");
    ws.checkpoint().unwrap();
    println!("checkpoint complete");
    std::io::stdout().flush().unwrap();
    loop {
        std::thread::park();
    }
}

#[test]
fn checkpoint_survives_actual_process_kill() {
    use std::{
        io::{BufRead, BufReader},
        process::{Command, Stdio},
    };
    let directory = tempfile::tempdir().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash_writer_fixture", "--nocapture"])
        .env("HEDIT_TEST_CRASH_DIRECTORY", directory.path())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let reader = BufReader::new(child.stdout.take().unwrap());
    let mut checkpointed = false;
    for line in reader.lines().take(20) {
        if line.unwrap().contains("checkpoint complete") {
            checkpointed = true;
            break;
        }
    }
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(checkpointed);
    let (ws, errors) = Workspace::restore(directory.path().into()).unwrap();
    assert!(errors.is_empty());
    assert_eq!(ws.open[0].page.text, "checkpoint survives a killed process");
}

#[cfg(target_os = "linux")]
#[test]
fn stdin_is_preserved_when_native_window_startup_fails() {
    use std::process::{Command, Stdio};
    let directory = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_hedit"))
        .args(["--session-dir", directory.path().to_str().unwrap(), "-"])
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
        .write_all(b"stdin must survive a display error\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(
        !output.stderr.is_empty(),
        "startup errors must reach the terminal"
    );
    let (ws, errors) = Workspace::restore(directory.path().into()).unwrap();
    assert!(errors.is_empty());
    assert_eq!(
        ws.open[ws.active].page.text,
        "stdin must survive a display error\n"
    );
}
