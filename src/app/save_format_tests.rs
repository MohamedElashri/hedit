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

fn drain(app: &mut HeditApp, ctx: &egui::Context) {
    let start = Instant::now();
    while app.busy
        || app.opens.pending()
        || !app.pending_paths.is_empty()
        || app.quality.folder_task.is_some()
    {
        assert!(start.elapsed() < Duration::from_secs(5));
        app.receive(ctx);
        thread::sleep(Duration::from_millis(5));
    }
}

fn set_caret(app: &mut HeditApp, position: usize) {
    let view = &mut app.workspace.open[app.workspace.active].view;
    view.anchor = position;
    view.head = position;
}

fn paged_app() -> (tempfile::TempDir, egui::Context, HeditApp, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("large.txt");
    fs::write(&path, "long filler line\n".repeat(130_000)).unwrap();
    let (ctx, mut app) = app_at(d.path());
    open(&mut app, &ctx, &path);
    assert!(app.active().page.info.paged);
    (d, ctx, app, path)
}

#[test]
fn save_line_ending_conversion_keeps_the_cursor_at_the_same_character() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("notes.txt");
    fs::write(&path, "first\r\nsecond\r\nthird\r\n").unwrap();
    fs::write(
        d.path().join(".editorconfig"),
        "root=true\n[*]\nend_of_line=lf\n",
    )
    .unwrap();
    let (ctx, mut app) = app_at(d.path());
    open(&mut app, &ctx, &path);
    set_caret(&mut app, 8);
    app.save_file(&ctx, false);
    finish_app_job(&mut app, &ctx);
    assert_eq!(fs::read_to_string(path).unwrap(), "first\nsecond\nthird\n");
    assert_eq!(app.active().view.head, 7);
}

#[test]
fn failed_save_encoding_validation_leaves_text_untouched() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("notes.txt");
    let text = "π   \n";
    fs::write(&path, text).unwrap();
    fs::write(
        d.path().join(".editorconfig"),
        "root=true\n[*]\ntrim_trailing_whitespace = true\ncharset = latin1\n",
    )
    .unwrap();
    let (ctx, mut app) = app_at(d.path());
    open(&mut app, &ctx, &path);
    app.save_file(&ctx, false);
    drain(&mut app, &ctx);
    assert!(
        app.error.is_some(),
        "this character cannot be encoded as Latin-1"
    );
    assert_eq!(fs::read_to_string(path).unwrap(), text);
    assert_eq!(app.active().page.text, text);
}

#[test]
fn paged_save_does_not_silently_ignore_an_editorconfig_charset() {
    let (d, ctx, mut app, path) = paged_app();
    fs::write(
        d.path().join(".editorconfig"),
        "root=true\n[*]\ncharset=utf-16le\n",
    )
    .unwrap();
    app.save_file(&ctx, false);
    drain(&mut app, &ctx);
    if app.error.is_none() {
        assert!(fs::read(path).unwrap().starts_with(&[0xff, 0xfe]));
    }
}
