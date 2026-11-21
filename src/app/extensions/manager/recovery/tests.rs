use super::*;
use crate::app::{
    extensions::tests::app_with,
    tests::{click_label, frame},
};
use hedit_broker::{
    plan::{Change, Plan},
    resources::Resources,
};

fn finish(app: &mut HeditApp, ctx: &egui::Context) {
    let started = Instant::now();
    while app.extension_manager.pending.is_some() {
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "Recovery worker did not finish"
        );
        std::thread::sleep(Duration::from_millis(5));
        app.poll_extension_manager();
    }
    app.receive(ctx);
    frame(ctx, app, vec![]);
}
fn select(app: &mut HeditApp, scope: &SelectedScope, state: &State) {
    let root = scope.resolve(state).unwrap();
    let record = state.recovery().records().unwrap().remove(0);
    let cancel = AtomicBool::new(false);
    let mut budget = Budget::new(&cancel, Duration::from_secs(10), 64 * 1024 * 1024);
    let view = Journal::open(&root, &record.stage_name, &state.recovery())
        .unwrap()
        .inspect(&root, &mut budget)
        .unwrap();
    // Supply the same authenticated review the native directory picker delivers.
    app.extension_manager.recovery = Some(Review::Selected {
        record: Box::new(record),
        scope: scope.clone(),
        view: Some(Arc::new(view)),
        error: None,
        created: Instant::now(),
        file: 0,
        before: 0,
        after: 0,
    });
}

#[test]
fn settings_recovers_an_uninstalled_plugins_journal_and_keeps_actions_visible_for_many_lines() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config");
    let project = directory.path().join("project");
    std::fs::create_dir(&config).unwrap();
    std::fs::create_dir(&project).unwrap();
    let path = project.join("text.txt");
    let original = format!("old café / 東京{}", "\r\n".repeat(1200));
    std::fs::write(&path, &original).unwrap();
    let (_session, ctx, mut app) = app_with("unrelated unsaved text", Some(&config));
    let state = State::open(&config, [app.workspace.store.directory().to_owned()]).unwrap();
    let scope = SelectedScope::select(&project, vec![Right::Read, Right::Edit], &state).unwrap();
    let mut resources = Resources::new(scope.resolve(&state).unwrap()).unwrap();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget::new(&cancel, Duration::from_secs(10), 64 * 1024 * 1024);
    let version = resources
        .load(resources.scope(), "text.txt", &mut budget)
        .unwrap();
    let plan = Plan::prepare(
        &resources,
        resources.scope(),
        vec![Change {
            resource: version.resource,
            version: version.version,
            edits: vec![hedit_plugin::sdk::Edit {
                start: 0,
                finish: 3,
                text: "new".into(),
            }],
        }],
        &mut budget,
    )
    .unwrap();
    Journal::prepare_owned(
        &resources,
        &plan,
        &mut budget,
        &state.recovery(),
        &hedit_broker::journal::Owner {
            command: "removed-plugin.rename".into(),
            digest: "a".repeat(64),
        },
    )
    .unwrap();
    let id = app.workspace.editor.id();
    let mut document = hedit::document::Document::open(id, &path).unwrap();
    let page = document.page(0).unwrap();
    app.workspace.add(document, page).unwrap();
    app.open_preferences(PreferencesPage::Extensions);
    frame(&ctx, &mut app, vec![]);
    finish(&mut app, &ctx);
    assert!(app.extensions.is_empty());
    click_label(&ctx, &mut app, "Workspace recovery…");
    finish(&mut app, &ctx);
    assert!(
        matches!(&app.extension_manager.recovery, Some(Review::List(records)) if records.len() == 1)
    );
    select(&mut app, &scope, &state);
    let stale_view = match &app.extension_manager.recovery {
        Some(Review::Selected {
            view: Some(view), ..
        }) => view.clone(),
        _ => panic!("Missing selected recovery view"),
    };
    click_label(&ctx, &mut app, "Resume reviewed edit");
    finish(&mut app, &ctx);
    assert!(!matches!(
        app.extension_manager.feedback,
        Some(Feedback::Error(_))
    ));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        original.replacen("old", "new", 1)
    );
    assert!(app
        .workspace
        .open
        .iter()
        .any(|b| b.page.info.id == id && b.page.text.starts_with("new")));
    select(&mut app, &scope, &state);
    if let Some(Review::Selected { view, .. }) = &mut app.extension_manager.recovery {
        *view = Some(stale_view);
    }
    click_label(&ctx, &mut app, "Restore originals");
    finish(&mut app, &ctx);
    assert!(
        matches!(&app.extension_manager.feedback, Some(Feedback::Error(error)) if error.contains("changed"))
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        original.replacen("old", "new", 1)
    );
    select(&mut app, &scope, &state);
    app.extension_manager.feedback = None;
    click_label(&ctx, &mut app, "Restore originals");
    finish(&mut app, &ctx);
    assert!(!matches!(
        app.extension_manager.feedback,
        Some(Feedback::Error(_))
    ));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    assert!(app
        .workspace
        .open
        .iter()
        .any(|b| b.page.info.id == id && b.page.text.starts_with("old")));
    select(&mut app, &scope, &state);
    click_label(&ctx, &mut app, "Delete recovery history");
    finish(&mut app, &ctx);
    assert!(!matches!(
        app.extension_manager.feedback,
        Some(Feedback::Error(_))
    ));
    assert!(state.recovery().records().unwrap().is_empty());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    assert!(!config.join("extensions-lua-grants.json").exists());
}
