use super::tests::finish_app_job;
use super::*;
use std::{fs, path::Path};

fn app_at(directory: &Path) -> (egui::Context, HeditApp) {
    let (workspace, _) = Workspace::restore(directory.join("session")).unwrap();
    let ctx = egui::Context::default();
    let app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    (ctx, app)
}

fn open(app: &mut HeditApp, ctx: &egui::Context, path: &Path) {
    app.open(ctx, Some(path.into()));
    finish_app_job(app, ctx);
}

fn add_folder(app: &mut HeditApp, path: &Path) {
    app.set_folder(hedit::folder::read_directory(path.into()).unwrap());
}

fn drain(app: &mut HeditApp, ctx: &egui::Context) {
    let started = Instant::now();
    while app.busy || app.opens.pending() || !app.pending_paths.is_empty() {
        assert!(started.elapsed() < Duration::from_secs(5));
        app.receive(ctx);
        thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(unix)]
#[test]
fn failed_delete_does_not_mark_a_clean_open_file_unsaved() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_at(d.path());
    let root = d.path().join("read-only");
    fs::create_dir(&root).unwrap();
    let path = root.join("notes.txt");
    fs::write(&path, "saved").unwrap();
    open(&mut app, &ctx, &path);
    assert!(!app.active().page.info.dirty);
    fs::set_permissions(&root, fs::Permissions::from_mode(0o500)).unwrap();
    app.begin_file_action(&ctx, path.clone(), quality::FileOperation::Delete);
    app.perform_file_action(&ctx);
    drain(&mut app, &ctx);
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        app.error.is_some() && path.is_file(),
        "the delete must have failed"
    );
    assert!(
        !app.active().page.info.dirty,
        "the failed delete marked a clean tab as modified"
    );
}

#[test]
fn explicit_reveal_switches_from_search_to_files() {
    let d = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_at(d.path());
    let root = d.path().join("project");
    fs::create_dir(&root).unwrap();
    let file = root.join("notes.txt");
    fs::write(&file, "needle").unwrap();
    add_folder(&mut app, &root);
    open(&mut app, &ctx, &file);
    app.show_search_sidebar();
    assert!(app.command_enabled(Command::RevealInSidebar));
    app.execute(&ctx, Command::RevealInSidebar);
    assert!(
        app.sidebar == workbench_navigation::Sidebar::Files,
        "Search stayed visible after explicit Reveal in sidebar"
    );
}

#[test]
fn compare_with_disk_honors_the_open_buffers_encoding() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("notes.txt");
    fs::write(&path, b"caf\xe9\n").unwrap();
    let (mut ws, _) = Workspace::restore(d.path().join("session")).unwrap();
    let mut doc = Document::open_with_encoding(ws.editor.id(), &path, "windows-1252").unwrap();
    let page = doc.page(0).unwrap();
    ws.add(doc, page).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, ws, vec![], vec![]);
    assert_eq!(app.active().page.text, "café\n");
    app.compare_disk(&ctx, false);
    drain(&mut app, &ctx);
    assert!(app.quality.diff_open, "comparison failed: {:?}", app.error);
}

#[test]
fn wait_for_file_survives_renaming_the_open_tab() {
    let d = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_at(d.path());
    let old = d.path().join("before.txt");
    let indicator = d.path().join("released");
    fs::write(&old, "content").unwrap();
    app.wait_for(std::slice::from_ref(&old), indicator.clone());
    open(&mut app, &ctx, &old);
    app.track_waits();
    assert!(!indicator.exists());
    app.begin_file_action(&ctx, old, quality::FileOperation::Rename);
    app.quality.file_name = "after.txt".into();
    app.perform_file_action(&ctx);
    finish_app_job(&mut app, &ctx);
    assert!(app
        .active()
        .page
        .info
        .path
        .as_ref()
        .unwrap()
        .ends_with("after.txt"));
    app.track_waits();
    assert!(
        !indicator.exists(),
        "terminal wait ended while the document was still open"
    );
}

#[test]
fn save_as_does_not_create_two_open_buffers_for_the_same_path() {
    let d = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_at(d.path());
    let first = d.path().join("one.txt");
    let second = d.path().join("two.txt");
    fs::write(&first, "one").unwrap();
    fs::write(&second, "two").unwrap();
    open(&mut app, &ctx, &first);
    open(&mut app, &ctx, &second);
    let target = first.canonicalize().unwrap();
    app.save_to_path(&ctx, app.active().view.id, Some(target.clone()));
    drain(&mut app, &ctx);
    let aliases = app
        .workspace
        .open
        .iter()
        .filter(|b| {
            b.page
                .info
                .path
                .as_deref()
                .is_some_and(|p| Path::new(p) == target)
        })
        .count();
    assert_eq!(
        aliases, 1,
        "Save As left two independent tabs bound to the same file"
    );
}
