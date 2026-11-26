use super::*;
use crate::app::{
    extensions::{
        manifest,
        tests::{app_with, prepare_native},
    },
    tests::{click_at, click_label, frame, text_rect},
};
use hedit_broker::{
    state::{Approval, SelectedScope, State as BrokerState},
    workspace::Right,
};
use serde_json::json;
use std::path::Path;

fn install(
    config: &Path,
    source: &str,
    capabilities: serde_json::Value,
    kind: &str,
    events: &[&str],
    project: Option<&Path>,
) {
    prepare_native();
    std::fs::create_dir_all(config.join("extensions")).unwrap();
    let value = json!({"api_version":5,"sdk_version":3,"id":"scoped-provider",
        "commands":[{"id":"run","title":"Scoped provider","broker":{"capabilities":capabilities},"provider":{"kind":kind,"events":events,"may_edit":false},"action":{"kind":"lua","source":"run.lua"}}],
        "scripts":{"run.lua":source}}).to_string();
    let path = config.join("extensions/scoped-provider.json");
    std::fs::write(&path, &value).unwrap();
    let digest = manifest::digest(&value);
    let catalog = catalog::set_lua_approved(
        config,
        &path,
        "scoped-provider",
        "scoped-provider.run",
        &digest,
        true,
    )
    .unwrap();
    let ExtensionAction::Lua(plugin) = &catalog.contributions[0].action else {
        panic!("missing source")
    };
    let state = BrokerState::open(config, []).unwrap();
    let scope = project.map(|project| {
        SelectedScope::select(
            project,
            vec![Right::Discover, Right::Read, Right::Edit],
            &state,
        )
        .unwrap()
    });
    catalog::approve_resources(
        config,
        &path,
        "scoped-provider",
        &digest,
        Approval {
            command: "scoped-provider.run".into(),
            token: serde_json::from_value(plugin.grant.approval_id().into()).unwrap(),
            scope,
            endpoints: vec![],
            tools: Default::default(),
        },
    )
    .unwrap();
}
fn pump(app: &mut HeditApp, ctx: &egui::Context, ready: impl Fn(&HeditApp) -> bool) {
    let started = Instant::now();
    while !ready(app) {
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "Provider did not finish: app={:?}, service={:?}, entries={:?}",
            app.error,
            app.plugin_services.error,
            app.plugin_services
                .entries
                .iter()
                .map(|e| (&e.error, e.state.quarantined))
                .collect::<Vec<_>>()
        );
        app.receive(ctx);
        frame(ctx, app, vec![]);
        std::thread::sleep(Duration::from_millis(3));
    }
}

#[test]
fn oversized_provider_snapshot_is_rejected_before_copying_document_text() {
    let config = tempfile::tempdir().unwrap();
    install(
        config.path(),
        "return function(ctx) return {intent={finish={}}} end",
        json!(["document.read", "events.observe", "ui.present"]),
        "presentation",
        &["manual"],
        None,
    );
    let (_session, ctx, mut app) = app_with("short", Some(config.path()));
    let catalog = catalog::read_consistent(config.path()).unwrap();
    app.plugin_services.install(catalog.contributions);
    let monitor = app
        .plugin_services
        .policy_monitor(config.path(), &ctx)
        .unwrap();
    let services = std::mem::take(&mut app.plugin_services);
    app.workspace.open[app.workspace.active].page.text = "x".repeat(1024 * 1024);
    let tick = Tick {
        config: config.path(),
        monitor: &monitor,
        target: None,
        language: hedit::syntax::Language::PlainText,
        selection: false,
        manual: None,
        saved: None,
        origin: None,
        now: 0,
        allow_start: true,
    };
    let mut result = None;
    let costs = crate::app::extensions::lua::tests::measured_allocations::measure(|| {
        result = Some(Scoped::default().start(
            &services.entries[0],
            &app,
            &ctx,
            &tick,
            service::schedule::Pending {
                trigger: Trigger::Manual,
                generation: 0,
                due_ms: 0,
            },
        ));
    });
    assert!(
        matches!(result, Some(Err(StartFailure::Failed(message))) if message.contains("snapshot limit"))
    );
    assert!(
        costs.2 < plugin_runtime::MAX_SELECTION_BYTES as u64,
        "Oversized rejection allocated {} bytes",
        costs.2
    );
}
fn status(app: &HeditApp) -> Option<&str> {
    let ready = app
        .plugin_services
        .entries
        .first()?
        .scoped
        .as_ref()?
        .ready
        .as_ref()?;
    match ready.result.as_ref() {
        ResultSet::Presentation(result) => result.status.as_deref(),
        _ => None,
    }
}
fn diagnostic(app: &HeditApp) -> Option<&str> {
    let ready = app
        .plugin_services
        .entries
        .first()?
        .scoped
        .as_ref()?
        .ready
        .as_ref()?;
    match ready.result.as_ref() {
        ResultSet::Diagnostics(items) => items.first().map(|item| item.message.as_str()),
        _ => None,
    }
}
#[test]
fn native_automatic_events_rotate_workers_and_close_without_document_or_cache() {
    let config = tempfile::tempdir().unwrap();
    install(
        config.path(),
        r#"local invocations = 0
        return function(ctx)
            invocations = invocations + 1
            assert(invocations == 1)
            if ctx.request.trigger == 'close' then
                assert(ctx.document.text == '' and next(ctx.cache) == nil)
                return {intent={finish={result='closed'}}}
            end
            ctx.cache.calls = (ctx.cache.calls or 0) + 1
            return {cache=ctx.cache,intent={finish={result={diagnostics={{range={start=0,finish=0},severity='warning',message=ctx.request.trigger..':'..ctx.cache.calls}}}}}}
        end"#,
        json!(["events.observe", "ui.present", "document.read"]),
        "diagnostics",
        &["open", "change", "close"],
        None,
    );
    let (_session, ctx, mut app) = app_with("first document", Some(config.path()));
    pump(&mut app, &ctx, |app| diagnostic(app) == Some("open:1"));
    app.workspace.edit(0, "changed ").unwrap();
    pump(&mut app, &ctx, |app| diagnostic(app) == Some("change:2"));
    let first = app.active().page.info.id;
    let id = app.workspace.editor.id();
    let mut document = hedit::document::Document::empty(id);
    let page = document.page(0).unwrap();
    app.workspace.add(document, page).unwrap();
    pump(&mut app, &ctx, |app| {
        app.plugin_services.entries[0]
            .scoped
            .as_ref()
            .unwrap()
            .run
            .as_ref()
            .is_some_and(|run| run.trigger == Trigger::Close)
    });
    pump(&mut app, &ctx, |app| {
        diagnostic(app) == Some("open:1")
            && app.plugin_services.entries[0]
                .scope
                .as_ref()
                .is_some_and(|target| target.document() != first)
    });
    assert!(app.plugin_services.entries[0].error.is_none());
    assert_eq!(
        app.plugin_services.entries[0].state.cache.0["calls"],
        hedit::plugin_options::Value::Integer(1)
    );
}
#[test]
fn native_workspace_provider_never_receives_document_text_and_interactions_preserve_cache() {
    let config = tempfile::tempdir().unwrap();
    install(
        config.path(),
        r#"return function(ctx)
        assert(ctx.document.text == '' and ctx.document.revision == '0' and ctx.document.language == 'plain_text')
        if ctx.request.trigger == 'interaction' then assert(ctx.request.action == 'again' and ctx.request.choice == 7 and ctx.cache.calls == 1) end
        ctx.cache.calls = (ctx.cache.calls or 0) + 1
        return {cache=ctx.cache,intent={finish={result={presentation={status='Calls '..ctx.cache.calls,quick_list={title='Actions',items={{title='Again',action={callback={name='again',value=7}}}}}}}}}}
    end"#,
        json!(["events.observe", "ui.present"]),
        "presentation",
        &["manual"],
        None,
    );
    let (_session, ctx, mut app) = app_with("private café / 東京", Some(config.path()));
    app.workspace.open[0].follow = true;
    assert!(app.lua_target().is_none());
    assert!(app.run_extension_by_id(&ctx, "scoped-provider.run"));
    pump(&mut app, &ctx, |app| status(app) == Some("Calls 1"));
    click_label(&ctx, &mut app, "Again");
    pump(&mut app, &ctx, |app| status(app) == Some("Calls 2"));
    assert_eq!(app.active().page.text, "private café / 東京");
    click_label(&ctx, &mut app, "Stop");
    assert!(app.plugin_services.entries[0].state.stopped);
    assert!(status(&app).is_none());
    // The permission watcher can briefly remove entries while refreshing the
    // catalog. Click Resume once the stopped provider is visible again.
    let started = Instant::now();
    loop {
        let output = frame(&ctx, &mut app, vec![]);
        if let Some(rect) = output
            .shapes
            .iter()
            .find_map(|shape| text_rect(&shape.shape, "Resume"))
        {
            click_at(&ctx, &mut app, rect.center(), egui::vec2(1280.0, 820.0));
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "Stopped provider did not reappear: open={}, suspended={}, error={:?}, entries={:?}, text={:?}",
            app.plugin_services.open,
            app.plugin_services.suspended,
            app.plugin_services.error,
            app.plugin_services.entries.iter().map(|entry| (&entry.contribution.id, entry.state.stopped, &entry.error)).collect::<Vec<_>>(),
            output.shapes.iter().filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.text()),
                _ => None,
            }).collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(3));
    }
    assert!(app.run_extension_by_id(&ctx, "scoped-provider.run"));
    pump(&mut app, &ctx, |app| status(app) == Some("Calls 1"));
    catalog::revoke_resources(config.path(), "scoped-provider.run").unwrap();
    pump(&mut app, &ctx, |app| status(app).is_none());
}
#[test]
fn native_document_provider_paints_valid_unicode_ranges_and_rejects_stale_results() {
    let config = tempfile::tempdir().unwrap();
    install(config.path(), "return function(ctx) return {intent={finish={result={diagnostics={{range={start=0,finish=5},severity='warning',message='Unicode fixture'}}}}}} end", json!(["events.observe","ui.present","document.read"]), "diagnostics", &["manual"], None);
    let (_session, ctx, mut app) = app_with("café / 東京\r\n", Some(config.path()));
    assert!(app.run_extension_by_id(&ctx, "scoped-provider.run"));
    pump(&mut app, &ctx, |app| {
        app.plugin_services
            .entries
            .first()
            .and_then(|e| e.scoped.as_ref())
            .and_then(|d| d.ready.as_ref())
            .is_some()
    });
    let info = &app.active().page.info;
    let decorations: Vec<_> = app
        .plugin_services
        .decorations(info.id, info.revision)
        .collect();
    assert_eq!(decorations.len(), 1);
    assert_eq!(app.plugin_services.summary(), Some(1));
    assert_eq!((decorations[0].start, decorations[0].finish), (0, 4));
    assert!(app.run_extension_by_id(&ctx, "scoped-provider.run"));
    pump(&mut app, &ctx, |app| {
        app.plugin_services.entries[0]
            .scoped
            .as_ref()
            .unwrap()
            .running()
    });
    app.workspace.edit(0, "changed ").unwrap();
    frame(&ctx, &mut app, vec![]);
    let data = app.plugin_services.entries[0].scoped.as_ref().unwrap();
    assert!(!data.running());
    assert!(data.ready.is_none());
}
#[test]
fn native_workspace_provider_requires_explicit_preview_and_refreshes_clean_buffers() {
    let config = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    for name in ["one.txt", "two.txt"] {
        std::fs::write(project.path().join(name), "old café / 東京\r\n").unwrap();
    }
    let source = include_str!("../../../../../crates/hedit-broker/tests/fixtures/rename.lua")
        .replace("ctx.arguments.before", "'old'")
        .replace("ctx.arguments.after", "'new'")
        .replace(
            "return {intent = {finish = {}}}",
            "return {intent = {finish = {result={presentation={status='Workspace updated'}}}}}",
        );
    install(
        config.path(),
        &source,
        json!([
            "events.observe",
            "ui.present",
            "workspace.discover",
            "workspace.read",
            "workspace.edit"
        ]),
        "presentation",
        &["manual"],
        Some(project.path()),
    );
    let (_session, ctx, mut app) = app_with("unrelated unsaved work", Some(config.path()));
    let id = app.workspace.editor.id();
    let mut document =
        hedit::document::Document::open(id, &project.path().join("one.txt")).unwrap();
    let page = document.page(0).unwrap();
    app.workspace.add(document, page).unwrap();
    assert!(app.run_extension_by_id(&ctx, "scoped-provider.run"));
    pump(&mut app, &ctx, |app| {
        app.plugin_services
            .entries
            .first()
            .and_then(|entry| entry.scoped.as_ref())
            .and_then(|data| data.run.as_ref())
            .is_some_and(|run| run.preview.is_some())
    });
    assert_eq!(
        std::fs::read_to_string(project.path().join("one.txt")).unwrap(),
        "old café / 東京\r\n"
    );
    click_label(&ctx, &mut app, "Apply workspace edit");
    pump(&mut app, &ctx, |app| {
        status(app) == Some("Workspace updated")
    });
    app.receive(&ctx);
    assert_eq!(
        std::fs::read_to_string(project.path().join("two.txt")).unwrap(),
        "new café / 東京\r\n"
    );
    assert!(app
        .workspace
        .open
        .iter()
        .any(|b| b.page.info.id == id && b.page.text == "new café / 東京\r\n"));
    assert_eq!(app.workspace.open[0].page.text, "unrelated unsaved work");
}

mod references;
