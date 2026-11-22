use hedit::{document::Document, Editor};
use std::sync::Arc;
fn clean() -> (tempfile::TempDir, std::path::PathBuf, Document) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("text.txt");
    std::fs::write(&path, "old café\r\n").unwrap();
    let mut document = Document::open(1, &path).unwrap();
    document.page(0).unwrap();
    document.replace(0, 3, "new".into(), 0).unwrap();
    document.save(None).unwrap();
    (directory, path, document)
}
#[test]
fn freezing_rejects_mutators_and_cancellation_preserves_undo() {
    let (_directory, path, mut document) = clean();
    let revision = document.info().revision;
    document.freeze_workspace(revision, 7).unwrap();
    for result in [
        document.set_encoding("windows-1252"),
        document.set_bom(true),
        document.accept_disk_version("new café\r\n"),
        document
            .take_disk_version("new café\r\n", revision)
            .map(|_| ()),
        document.discard_changes().map(|_| ()),
        document.reload(0).map(|_| ()),
        document.retarget(path.clone()).map(|_| ()),
        document.history(false, 0, revision).map(|_| ()),
        document.replace(0, 3, "bad".into(), revision).map(|_| ()),
        document
            .replace_with_line_ending(0, 3, "bad".into(), revision, false)
            .map(|_| ()),
        document.save(None).map(|_| ()),
        document
            .replace_all("new", "bad", revision, |_, _| {})
            .map(|_| ()),
        document.preserve_as_unsaved(),
    ] {
        assert!(result.unwrap_err().contains("workspace commit"));
    }
    assert_eq!(document.text(1024).unwrap(), "new café\r\n");
    assert_eq!(document.info().revision, revision);
    assert!(!document.info().dirty);
    document.thaw_workspace(8);
    assert!(document.workspace_frozen());
    document.thaw_workspace(7);
    let undo = document.history(false, 0, revision).unwrap();
    assert_eq!(undo.text, "old café\r\n");
    assert_eq!(std::fs::read_to_string(path).unwrap(), "new café\r\n");
}
#[test]
fn replacement_requires_exact_lease_and_keeps_revisions_increasing() {
    let (_directory, path, mut document) = clean();
    let revision = document.info().revision;
    let backup = document.workspace_backup(1024).unwrap();
    document.freeze_workspace(revision, 7).unwrap();
    assert!(document.accept_workspace_copy(8, backup.clone()).is_err());
    // Backup is independent even if the external writer changes the old inode.
    std::fs::write(&path, "external writer").unwrap();
    assert_eq!(backup.text(1024).unwrap(), "new café\r\n");
    document.accept_workspace_copy(7, backup).unwrap();
    assert!(document.info().dirty);
    assert_eq!(document.info().revision, revision + 1);
    assert!(document.save(None).is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "external writer");
}
#[test]
fn membership_follow_mode_and_partial_freeze_fail_without_stranding_buffers() {
    let (_directory, _path, document) = clean();
    let revision = document.info().revision;
    let editor = Arc::new(Editor::default());
    editor.insert(document).unwrap();
    let (generation, _) = editor.document_set().unwrap();
    editor.insert(Document::empty(2)).unwrap();
    assert!(editor
        .freeze_workspace(generation, 9, &[(1, revision)])
        .is_err());
    let (generation, _) = editor.document_set().unwrap();
    assert!(editor
        .freeze_workspace(generation, 9, &[(1, revision), (2, 1)])
        .is_err());
    assert!(!editor.get(1).unwrap().lock().unwrap().workspace_frozen());
    editor.set_followed(1, true).unwrap();
    assert!(editor
        .freeze_workspace(generation, 9, &[(1, revision)])
        .is_err());
    editor.set_followed(1, false).unwrap();
    let freeze = editor
        .freeze_workspace(generation, 9, &[(1, revision)])
        .unwrap();
    assert!(editor.workspace_document_frozen(1));
    assert!(!editor.workspace_document_frozen(2));
    assert!(editor.set_followed(1, true).is_err());
    assert!(editor.insert(Document::empty(3)).is_err());
    assert!(editor.remove(1).is_err());
    editor
        .get(2)
        .unwrap()
        .lock()
        .unwrap()
        .replace(0, 0, "unrelated".into(), 0)
        .unwrap();
    drop(freeze);
    editor.insert(Document::empty(3)).unwrap();
    assert!(!editor.workspace_document_frozen(1));
    editor.remove(1).unwrap();
}
