use hedit::{document::Document, workspace::Workspace};
use std::fs;

fn edit(ws: &mut Workspace, text: &str) {
    let index = ws.active;
    let old = ws.open[index].page.text.clone();
    ws.open[index].page.text = text.into();
    ws.edit(index, &old).unwrap();
}

#[test]
fn scratchpad_name_survives_edits_shelf_and_restart_without_creating_a_file() {
    let directory = tempfile::tempdir().unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().into()).unwrap();
    edit(&mut ws, "first line");
    ws.name_scratchpad(0, "Investigation notes").unwrap();
    edit(&mut ws, "a completely different line");
    assert_eq!(ws.open[0].view.name, "Investigation notes");
    assert!(ws.open[0].page.info.path.is_none());
    ws.close(0).unwrap();
    ws.checkpoint().unwrap();
    drop(ws);
    let (mut ws, errors) = Workspace::restore(directory.path().into()).unwrap();
    assert!(errors.is_empty());
    assert_eq!(ws.closed[0].name, "Investigation notes");
    ws.reopen_at(0).unwrap();
    let active = ws.active;
    assert_eq!(ws.open[active].page.text, "a completely different line");
    assert_eq!(ws.open[active].view.name, "Investigation notes");
    ws.name_scratchpad(active, "").unwrap();
    assert_eq!(ws.open[active].view.name, "a completely different line");
    assert!(!directory.path().join("Investigation notes").exists());
}

#[test]
fn shelf_reopens_selected_entry_and_retains_unselected_entries() {
    let directory = tempfile::tempdir().unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().into()).unwrap();
    for name in ["first", "second", "third"] {
        let active = ws.active;
        edit(&mut ws, name);
        ws.name_scratchpad(active, name).unwrap();
        ws.close(active).unwrap();
    }
    ws.reopen_at(1).unwrap();
    assert_eq!(ws.open[ws.active].page.text, "second");
    assert_eq!(
        ws.closed
            .iter()
            .map(|v| v.name.as_str())
            .collect::<Vec<_>>(),
        ["first", "third"]
    );
    assert!(ws.reopen_at(40).is_err());
    assert_eq!(ws.closed.len(), 2);
}

#[test]
fn closing_a_clean_disk_file_keeps_a_full_shelf_of_unsaved_notes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("saved.txt");
    fs::write(&path, "saved text").unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().join("session")).unwrap();
    for n in 0..50 {
        edit(&mut ws, &format!("unsaved note {n}"));
        ws.close(ws.active).unwrap();
    }
    let notes: Vec<_> = ws.closed.iter().map(|view| view.id).collect();
    let mut doc = Document::open(ws.editor.id(), &path).unwrap();
    let id = doc.id;
    let page = doc.page(0).unwrap();
    ws.add(doc, page).unwrap();
    ws.close(ws.active).unwrap();
    assert_eq!(
        ws.closed.iter().map(|view| view.id).collect::<Vec<_>>(),
        notes
    );
    assert!(ws.editor.get(id).is_err());
    assert!(ws.active < ws.open.len());
    assert_eq!(fs::read_to_string(path).unwrap(), "saved text");
    ws.reopen_at(0).unwrap();
    assert_eq!(ws.open[ws.active].page.text, "unsaved note 0");
}

#[test]
fn failed_shelf_read_does_not_remove_the_entry() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("file.txt");
    fs::write(&path, "source").unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let mut doc = Document::open(ws.editor.id(), &path).unwrap();
    let page = doc.page(0).unwrap();
    ws.add(doc, page).unwrap();
    ws.close(ws.active).unwrap();
    // An invalid ID simulates an unavailable document; no entry is popped early.
    ws.closed[0].id = u64::MAX;
    assert!(ws.reopen_at(0).is_err());
    assert_eq!(ws.closed.len(), 1);
}

#[test]
fn scratchpad_names_are_validated_and_do_not_rename_files() {
    let directory = tempfile::tempdir().unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().join("session")).unwrap();
    assert!(ws.name_scratchpad(0, &"x".repeat(81)).is_err());
    assert!(ws.name_scratchpad(0, "line\nbreak").is_err());
    let path = directory.path().join("file.txt");
    fs::write(&path, "text").unwrap();
    let mut doc = Document::open(ws.editor.id(), &path).unwrap();
    let page = doc.page(0).unwrap();
    ws.add(doc, page).unwrap();
    assert!(ws.name_scratchpad(ws.active, "something else").is_err());
    assert!(path.exists());
    assert!(!directory.path().join("something else").exists());
}

#[test]
fn unicode_bookmarks_wrap_and_survive_shelf_and_restart() {
    let directory = tempfile::tempdir().unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().into()).unwrap();
    edit(&mut ws, "λ first\nsecond\nthird\n");
    ws.open[0].view.head = 2;
    assert!(ws.toggle_bookmark(0).unwrap());
    ws.open[0].view.head = 10;
    assert!(ws.toggle_bookmark(0).unwrap());
    assert_eq!(ws.open[0].view.bookmarks[0].offset, 3);
    assert_eq!(ws.adjacent_bookmark(0, true), Some(3));
    assert_eq!(ws.adjacent_bookmark(0, false), Some(3));
    ws.close(0).unwrap();
    ws.checkpoint().unwrap();
    drop(ws);
    let (mut ws, errors) = Workspace::restore(directory.path().into()).unwrap();
    assert!(errors.is_empty());
    ws.reopen().unwrap();
    assert_eq!(ws.open[ws.active].view.bookmarks.len(), 2);
    ws.open[ws.active].view.head = 2;
    assert!(!ws.toggle_bookmark(ws.active).unwrap());
    assert_eq!(ws.open[ws.active].view.bookmarks.len(), 1);
}

#[test]
fn bookmarks_adjust_for_insertion_and_deletion_without_splitting_unicode() {
    let directory = tempfile::tempdir().unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().into()).unwrap();
    edit(&mut ws, "abc λxyz");
    ws.open[0].view.head = 5;
    ws.toggle_bookmark(0).unwrap();
    assert_eq!(ws.open[0].view.bookmarks[0].offset, 6);
    edit(&mut ws, "++abc λxyz");
    assert_eq!(ws.open[0].view.bookmarks[0].offset, 8);
    edit(&mut ws, "++abc λz");
    assert_eq!(ws.open[0].view.bookmarks[0].offset, 8);
    edit(&mut ws, "z");
    assert_eq!(ws.open[0].view.bookmarks[0].offset, 0);
}

#[test]
fn old_session_without_new_metadata_still_restores() {
    let directory = tempfile::tempdir().unwrap();
    let (mut ws, _) = Workspace::restore(directory.path().into()).unwrap();
    edit(&mut ws, "old session");
    ws.checkpoint().unwrap();
    drop(ws);
    let path = directory.path().join("session.json");
    let mut json: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let view = json["open"][0]["view"].as_object_mut().unwrap();
    view.remove("customName");
    view.remove("bookmarks");
    fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
    let (ws, errors) = Workspace::restore(directory.path().into()).unwrap();
    assert!(errors.is_empty());
    assert_eq!(ws.open[0].page.text, "old session");
    assert!(ws.open[0].view.bookmarks.is_empty());
}

#[test]
fn cpp_and_cuda_extensions_share_the_native_lexer() {
    for name in [
        "main.c",
        "types.h",
        "code.CPP",
        "detail.hxx",
        "kernel.CU",
        "device.cuh",
    ] {
        use hedit::syntax::Language::*;
        assert!(
            matches!(
                hedit::syntax::Language::detect(name),
                C | Cpp | Cuda | ObjectiveC
            ),
            "{name}"
        );
    }
    assert_eq!(
        hedit::syntax::Language::detect("output.log"),
        hedit::syntax::Language::Log
    );
    assert_eq!(hedit::syntax::Language::detect("main.cpp").label(), "C++");
    assert_eq!(hedit::syntax::Language::detect("kernel.cu").label(), "CUDA");
}
