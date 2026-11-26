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

#[test]
fn saving_project_with_workspace_extension_keeps_the_workspace_snapshot() {
    let d = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_at(d.path());
    let file = d.path().join("notes.txt");
    fs::write(&file, "notes").unwrap();
    open(&mut app, &ctx, &file);
    let expected = app.workspace.open.len();
    let path = d.path().join("work.hedit-workspace");
    app.quality.project_path = Some(path);
    app.save_project(&ctx, false);
    finish_app_job(&mut app, &ctx);
    let saved = app.quality.project_path.as_ref().unwrap();
    let (_, state) = hedit::projects::load(saved).unwrap();
    assert_eq!(state.tabs.len(), expected);
    assert!(state
        .tabs
        .iter()
        .any(|tab| tab.path.as_deref() == Some(file.canonicalize().unwrap().as_path())));
}

#[test]
fn project_round_trip_preserves_unsaved_utf8_bom_choice() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("notes.txt");
    fs::write(&path, "notes").unwrap();
    let (mut ws, _) = Workspace::restore(d.path().join("session")).unwrap();
    let mut doc = Document::open(ws.editor.id(), &path).unwrap();
    doc.set_bom(true).unwrap();
    let page = doc.page(0).unwrap();
    assert!(page.info.bom);
    ws.add(doc, page).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, ws, vec![], vec![]);
    let project = d.path().join("work.hedit-project");
    app.quality.project_path = Some(project.clone());
    app.save_project(&ctx, false);
    finish_app_job(&mut app, &ctx);
    drop(app);
    let (ctx, mut app) = app_at(d.path());
    app.load_project(&ctx, Some(project));
    finish_app_job(&mut app, &ctx);
    assert!(
        app.active().page.info.bom,
        "project reopen silently lost the selected BOM"
    );
}

#[test]
fn project_load_deduplicates_aliases_of_the_same_folder() {
    let d = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_at(d.path());
    app.set_folder(hedit::folder::read_directory(d.path().into()).unwrap());
    app.set_folder(hedit::folder::read_directory(d.path().into()).unwrap());
    assert_eq!(app.workspace.folders.len(), 1);

    let path = d.path().join("work.hedit-project");
    fs::write(&path, r#"{"folders":[".","./"]}"#).unwrap();
    app.load_project(&ctx, Some(path));
    finish_app_job(&mut app, &ctx);
    assert_eq!(
        app.workspace.folders.len(),
        1,
        "project open registered the same root twice"
    );
}
