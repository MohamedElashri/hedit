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
fn opening_a_reference_in_an_open_file_uses_utf16_column() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("code.rs");
    fs::write(&path, "🙂 + target\n").unwrap();
    let (ctx, mut app) = app_at(d.path());
    open(&mut app, &ctx, &path);
    let b = app.active();
    let place = hedit::intelligence::references(&b.page.text, "target", 1)
        .pop()
        .unwrap();
    assert_eq!(place.column, 5);
    app.open_location(
        &ctx,
        definitions::Location {
            view: Some(b.view.id),
            path: Some(path),
            file: b.view.name.clone(),
            place,
            column16: 6,
        },
    );
    assert_eq!(app.active().view.head, 4);
}

#[test]
fn undo_remaps_a_bookmark_to_the_restored_line() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("notes.txt");
    fs::write(&path, "alpha\nbeta\n").unwrap();
    let (ctx, mut app) = app_at(d.path());
    open(&mut app, &ctx, &path);
    set_caret(&mut app, 6);
    app.workspace.toggle_bookmark(app.workspace.active).unwrap();
    assert!(app.apply_edit(
        hedit::editing::Edit {
            text: "intro\nalpha\nbeta\n".into(),
            anchor: 0,
            head: 0
        },
        None
    ));
    assert_eq!(app.active().view.bookmarks[0].offset, 12);
    app.history(&ctx, false);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().page.text, "alpha\nbeta\n");
    assert_eq!(app.active().view.bookmarks[0].offset, 6);
}

#[test]
fn select_bookmarks_does_not_create_a_caret_at_the_page_edge_for_a_distant_mark() {
    let (_d, ctx, mut app, _) = paged_app();
    let local = app.workspace.active;
    app.workspace.toggle_bookmark(local).unwrap();
    app.goto_location(&ctx, 90_000, 2);
    finish_app_job(&mut app, &ctx);
    assert!(app.active().page.start > 0);
    app.workspace.toggle_bookmark(local).unwrap();
    assert_eq!(app.active().view.bookmarks.len(), 2);
    app.goto_location(&ctx, 1, 1);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().page.start, 0);
    assert!(app.command_enabled(Command::SelectBookmarks));
    app.execute(&ctx, Command::SelectBookmarks);
    let selections = app.selections();
    assert_eq!(selections, [hedit::advanced::Selection::caret(0)]);
}

#[test]
fn copy_file_location_works_beyond_the_first_page() {
    let (_d, ctx, mut app, _) = paged_app();
    app.goto_location(&ctx, 90_000, 2);
    finish_app_job(&mut app, &ctx);
    assert!(app.active().page.start > 0);
    assert!(app.command_enabled(Command::CopyFileLocation));
    let mut output = crate::app::tests::run_pass(&ctx, egui::RawInput::default(), |ctx| {
        app.execute(ctx, Command::CopyFileLocation)
    });
    let started = Instant::now();
    while !output.platform_output.commands.iter().any(|command| matches!(command, egui::OutputCommand::CopyText(text) if text.ends_with(":90000:2"))) {
        assert!(started.elapsed() < Duration::from_secs(5), "copy did not produce the absolute position: {:?}", app.error);
        output = crate::app::tests::run_pass(&ctx, egui::RawInput::default(), |ctx| app.receive(ctx));
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn goto_file_parses_a_windows_verbatim_drive_path_with_coordinates() {
    use goto_anything::{parse_file_query, Target};
    let (path, target) = parse_file_query(r"\\?\C:\work\notes.rs:12:3");
    assert_eq!(path, r"\\?\C:\work\notes.rs");
    assert_eq!(target, Some(Target::Line(Some(12), Some(3))));
}

#[test]
fn goto_text_with_quotes_keeps_a_preceding_quoted_filename() {
    use goto_anything::{parse_file_query, Target};
    let query = format!(r#""notes draft.rs"{}println!("hello")"#, '#');
    let (path, target) = parse_file_query(&query);
    assert_eq!(path, "notes draft.rs");
    assert_eq!(target, Some(Target::Text(r#"println!("hello")"#.into())));
}

#[test]
fn failed_build_attempt_keeps_folder_search_results_as_f4_target() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("project");
    fs::create_dir(&root).unwrap();
    let path = root.join("notes.txt");
    fs::write(&path, "needle").unwrap();
    let (ctx, mut app) = app_at(d.path());
    app.set_folder(hedit::folder::read_directory(root).unwrap());
    app.quality.folder_query = "needle".into();
    app.folder_search(&ctx, false);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.quality.results.hits.len(), 1);
    app.build(&ctx);
    assert!(app
        .error
        .take()
        .unwrap()
        .contains("Save the file before building"));
    app.execute(&ctx, Command::NextResult);
    drain(&mut app, &ctx);
    assert_eq!(app.quality.result_current, Some(0));
}
