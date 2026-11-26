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

#[test]
fn unsuccessful_project_save_does_not_replace_existing_companion_data() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("work.hedit-project");
    let companion = path.with_extension("hedit-workspace");
    fs::write(&path, "{}\n").unwrap();
    fs::write(&companion, "{}\n").unwrap();
    let (ctx, mut app) = app_at(d.path());
    app.load_project(&ctx, Some(path.clone()));
    finish_app_job(&mut app, &ctx);
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    app.save_project(&ctx, false);
    drain(&mut app, &ctx);
    assert!(app.error.is_some() && path.is_dir());
    assert_eq!(fs::read_to_string(companion).unwrap(), "{}\n");
}

#[test]
fn a_project_without_personal_state_does_not_change_word_wrap() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("work.hedit-project");
    fs::write(&path, r#"{"folders":["."]}"#).unwrap();
    let (ctx, mut app) = app_at(d.path());
    app.settings.word_wrap = true;
    app.load_project(&ctx, Some(path));
    finish_app_job(&mut app, &ctx);
    assert!(app.settings.word_wrap);
}

#[test]
fn removing_an_editorconfig_rule_restores_inherited_tab_width() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("notes.txt");
    let config = d.path().join(".editorconfig");
    fs::write(&path, "unindented\n").unwrap();
    fs::write(&config, "root=true\n[*]\nindent_size=7\n").unwrap();
    let (ctx, mut app) = app_at(d.path());
    open(&mut app, &ctx, &path);
    assert_eq!(app.active().view.tab_size, Some(7));
    fs::remove_file(config).unwrap();
    app.apply_format_preferences();
    assert_eq!(app.active().view.tab_size, Some(4));
}
