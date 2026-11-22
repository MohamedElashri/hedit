use super::*;
use crate::app::extensions::tests::app_with;
use hedit::document::Document;
#[test]
fn pending_opens_wait_for_commit_and_reopen_the_current_disk_copy() {
    let files = tempfile::tempdir().unwrap();
    let path = files.path().join("text.txt");
    std::fs::write(&path, "before").unwrap();
    let (_session, ctx, mut app) = app_with("unrelated", None);
    let id = app.workspace.editor.id();
    let mut document = Document::open(id, &path).unwrap();
    let page = document.page(0).unwrap();
    let (generation, _) = app.workspace.editor.document_set().unwrap();
    let frozen = app
        .workspace
        .editor
        .freeze_workspace(generation, 7, &[])
        .unwrap();
    app.tx
        .send(Ok(Action::Add(Box::new(document), page, 0)))
        .unwrap();
    app.receive(&ctx);
    assert_eq!(app.broker_deferred.len(), 1);
    assert!(!app
        .workspace
        .open
        .iter()
        .any(|buffer| buffer.page.info.id == id));
    std::fs::write(&path, "after").unwrap();
    drop(frozen);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !app
        .workspace
        .open
        .iter()
        .any(|buffer| buffer.page.info.id == id)
        || app.broker_deferred_running
    {
        assert!(std::time::Instant::now() < deadline, "{:?}", app.error);
        app.receive(&ctx);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(
        app.workspace
            .open
            .iter()
            .find(|buffer| buffer.page.info.id == id)
            .unwrap()
            .page
            .text,
        "after"
    );
    assert_eq!(
        app.workspace
            .editor
            .get(id)
            .unwrap()
            .lock()
            .unwrap()
            .text(1024)
            .unwrap(),
        "after"
    );
}
#[test]
fn stale_pages_and_save_replies_cannot_roll_back_reconciled_buffers_or_leave_jobs_busy() {
    let files = tempfile::tempdir().unwrap();
    let path = files.path().join("text.txt");
    let (_session, ctx, mut app) = app_with("before", None);
    let id = app.active().page.info.id;
    let shared = app.workspace.editor.get(id).unwrap();
    let info = shared.lock().unwrap().save(Some(path.clone())).unwrap();
    app.workspace.open[app.workspace.active].page.info = info;
    let old = app.active().page.clone();
    let (generation, _) = app.workspace.editor.document_set().unwrap();
    let freeze = app
        .workspace
        .editor
        .freeze_workspace(generation, 7, &[(id, old.info.revision)])
        .unwrap();
    std::fs::write(&path, "after").unwrap();
    let mut fresh = Document::open(id, &path).unwrap();
    let mut page = fresh.page(0).unwrap();
    shared
        .lock()
        .unwrap()
        .accept_workspace_copy(7, fresh)
        .unwrap();
    page.info = shared.lock().unwrap().info();
    drop(freeze);
    app.workspace.open[app.workspace.active].page = page;
    for action in [
        Action::Page(id, old.clone(), None),
        Action::Reloaded(id, old.clone(), None),
        Action::Saved(id, old.info.clone()),
        Action::Discarded(id, Some(old.clone())),
    ] {
        app.busy = true;
        app.tx.send(Ok(action)).unwrap();
        app.receive(&ctx);
        assert!(!app.busy);
        assert_eq!(app.active().page.text, "after");
        assert_eq!(app.active().page.info.revision, old.info.revision + 1);
    }
}
