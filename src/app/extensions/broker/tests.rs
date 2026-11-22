use super::*;
use crate::app::extensions::manifest::{self, ExtensionAction};
#[cfg(target_os = "macos")]
use crate::app::{
    extensions::tests::{app_with, prepare_native},
    tests::frame,
};
fn install(config: &std::path::Path, source: &str, capabilities: serde_json::Value) {
    let directory = config.join("extensions");
    std::fs::create_dir_all(&directory).unwrap();
    let manifest = serde_json::json!({
        "api_version":5,"sdk_version":3,"id":"scoped-test",
        "commands":[{"id":"run","title":"Scoped test","broker":{"capabilities":capabilities},"action":{"kind":"lua","source":"run.lua"}}],
        "scripts":{"run.lua":source}
    }).to_string();
    let path = directory.join("scoped-test.json");
    std::fs::write(&path, &manifest).unwrap();
    let digest = manifest::digest(&manifest);
    let catalog = catalog::set_lua_approved(
        config,
        &path,
        "scoped-test",
        "scoped-test.run",
        &digest,
        true,
    )
    .unwrap();
    let ExtensionAction::Lua(plugin) = catalog.contributions[0].action.clone() else {
        panic!()
    };
    let token = serde_json::from_value(plugin.grant.approval_id().into()).unwrap();
    catalog::approve_resources(
        config,
        &path,
        "scoped-test",
        &digest,
        hedit_broker::state::Approval {
            command: "scoped-test.run".into(),
            token,
            scope: None,
            endpoints: vec![],
            tools: BTreeMap::new(),
        },
    )
    .unwrap();
}
#[cfg(target_os = "macos")]
fn pump(app: &mut HeditApp, ctx: &egui::Context, ready: impl Fn(&HeditApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready(app) {
        assert!(
            Instant::now() < deadline,
            "Broker UI did not finish: {:?}",
            app.error
        );
        app.receive(ctx);
        frame(ctx, app, vec![]);
        std::thread::sleep(Duration::from_millis(3));
    }
}

#[test]
fn api_five_rejects_legacy_authority_and_requires_exact_resource_approval() {
    let config = tempfile::tempdir().unwrap();
    install(
        config.path(),
        "return function(ctx) return {intent={finish={}}} end",
        serde_json::json!(["document.read"]),
    );
    let path = config.path().join("extensions/scoped-test.json");
    let original = std::fs::read_to_string(&path).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_str(&original).unwrap();
    manifest["permissions"] = serde_json::json!(["document.read"]);
    assert!(manifest::parse_manifest(&manifest.to_string()).is_err());
    let ExtensionAction::Lua(mut plugin) = catalog::read_consistent(config.path())
        .unwrap()
        .contributions
        .remove(0)
        .action
    else {
        panic!()
    };
    let guard = catalog::broker_authorization(config.path(), &plugin).unwrap();
    plugin.broker_epoch = Some(Arc::new(
        catalog::broker_epoch(config.path(), &guard).unwrap(),
    ));
    drop(guard);
    catalog::revoke_resources(config.path(), "scoped-test.run").unwrap();
    assert!(catalog::broker_authorization(config.path(), &plugin).is_none());
    let state = State::open(config.path(), []).unwrap();
    let token = serde_json::from_value(plugin.grant.approval_id().into()).unwrap();
    assert!(state
        .resolve("scoped-test.run", token, plugin.broker.as_ref().unwrap())
        .is_err());
}
#[test]
fn native_policy_events_cancel_disclosures_before_exclusive_revocation_without_ui_frames() {
    let config = tempfile::tempdir().unwrap();
    install(
        config.path(),
        "return function(ctx) return {intent={finish={}}} end",
        serde_json::json!(["ui.present"]),
    );
    let ExtensionAction::Lua(plugin) = catalog::read_consistent(config.path())
        .unwrap()
        .contributions
        .remove(0)
        .action
    else {
        panic!()
    };
    let mut services = super::super::services::Services::default();
    let monitor = services
        .policy_monitor(config.path(), &egui::Context::default())
        .unwrap();
    let cancel = Arc::new(hedit_broker::cancel::Cancel::default());
    let sequence = monitor.register(&cancel).unwrap();
    let disclosure = catalog::broker_authorization(config.path(), &plugin).unwrap();
    let stopped = cancel.clone();
    let holder = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(4);
        while !stopped.is_cancelled() {
            assert!(
                Instant::now() < deadline,
                "The native watcher did not stop disclosure."
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        drop(disclosure);
    });
    let directory = config.path().to_owned();
    let writer =
        std::thread::spawn(move || catalog::revoke_resources(&directory, "scoped-test.run"));
    // The egui context and catalog mailbox are deliberately never pumped.
    holder.join().unwrap();
    writer.join().unwrap().unwrap();
    assert!(monitor.sequence.load(Ordering::Acquire) > sequence);
    assert!(cancel.is_cancelled());
}
#[test]
fn source_revocation_prunes_resources_and_corrupt_resource_reset_retains_only_source_consent() {
    let config = tempfile::tempdir().unwrap();
    let source = "return function(ctx) return {intent={finish={}}} end";
    install(config.path(), source, serde_json::json!(["ui.present"]));
    let path = config.path().join("extensions/scoped-test.json");
    let digest = manifest::digest(std::fs::read(&path).unwrap());
    assert_eq!(
        State::open(config.path(), [])
            .unwrap()
            .approvals()
            .unwrap()
            .len(),
        1
    );
    catalog::set_lua_approved(
        config.path(),
        &path,
        "scoped-test",
        "scoped-test.run",
        &digest,
        false,
    )
    .unwrap();
    assert!(State::open(config.path(), [])
        .unwrap()
        .approvals()
        .unwrap()
        .is_empty());
    install(config.path(), source, serde_json::json!(["ui.present"]));
    std::fs::write(
        config.path().join("extensions-broker/approvals.json"),
        "{broken",
    )
    .unwrap();
    assert!(
        catalog::read_consistent(config.path())
            .unwrap()
            .view
            .resources_unreadable
    );
    let catalog = catalog::reset_resource_approvals(config.path()).unwrap();
    assert!(!catalog.view.resources_unreadable);
    assert_eq!(catalog.contributions.len(), 1);
    assert!(State::open(config.path(), [])
        .unwrap()
        .approvals()
        .unwrap()
        .is_empty());
}
#[cfg(target_os = "macos")]
#[test]
fn native_workspace_only_command_receives_no_document_and_revocation_removes_results() {
    prepare_native();
    let config = tempfile::tempdir().unwrap();
    install(
        config.path(),
        r#"return function(ctx)
      assert(ctx.document.text == '' and ctx.document.revision == '0' and ctx.document.language == 'plain_text')
      return {intent={finish={result={presentation={status='Workspace only',output='No document data'}}}},cache={counter=7}}
    end"#,
        serde_json::json!(["ui.present"]),
    );
    let (_session, ctx, mut app) =
        app_with("private editor text / café / 東京", Some(config.path()));
    assert!(app.run_extension_by_id(&ctx, "scoped-test.run"));
    pump(&mut app, &ctx, |app| {
        app.broker_command
            .as_ref()
            .is_some_and(|command| matches!(command.phase, Phase::Ready { .. }))
            || app.error.is_some()
    });
    assert!(app.error.is_none(), "{:?}", app.error);
    let command = app.broker_command.as_ref().unwrap();
    assert!(command.context.target.is_none());
    let Phase::Ready { cache, .. } = &command.phase else {
        panic!()
    };
    let state = State::open(config.path(), []).unwrap();
    assert!(hedit_broker::state::SelectedScope::select(
        _session.path(),
        vec![hedit_broker::workspace::Right::Read],
        &state
    )
    .is_err());
    assert_eq!(cache.0["counter"], hedit::plugin_options::Value::Integer(7));
    assert_eq!(app.active().page.text, "private editor text / café / 東京");
    catalog::revoke_resources(config.path(), "scoped-test.run").unwrap();
    pump(&mut app, &ctx, |app| app.broker_command.is_none());
}
#[cfg(target_os = "macos")]
#[test]
fn native_broker_document_proposals_recheck_authority_and_undo_once() {
    prepare_native();
    let config = tempfile::tempdir().unwrap();
    install(config.path(), "return function(ctx) return {intent={finish={transaction=hedit.replace('new café / 東京')}}} end", serde_json::json!(["document.read","document.edit"]));
    let (_session, ctx, mut app) = app_with("old café / 東京", Some(config.path()));
    app.settings.extensions.preview_edits = false;
    assert!(app.run_extension_by_id(&ctx, "scoped-test.run"));
    pump(&mut app, &ctx, |app| {
        (app.broker_command.is_none() && app.lua_transform.is_none()) || app.error.is_some()
    });
    assert!(app.error.is_none(), "{:?}", app.error);
    assert_eq!(app.active().page.text, "new café / 東京");
    let shared = app.workspace.editor.get(app.active().page.info.id).unwrap();
    let mut document = shared.lock().unwrap();
    let revision = document.info().revision;
    assert_eq!(
        document.history(false, 0, revision).unwrap().text,
        "old café / 東京"
    );
}
#[cfg(target_os = "macos")]
#[test]
fn native_workspace_commits_survive_focus_changes_and_reconcile_the_originating_buffer() {
    prepare_native();
    const SOURCE: &str = r#"return function(ctx)
      local response = ctx.broker.response
      if response == nil then
        return {intent={open_many={scope=ctx.broker.scope,paths={'one.txt','two.txt'}}}}
      end
      if response.opened_many then
        local changes = {}
        for _, file in ipairs(response.opened_many.files) do
          changes[#changes+1] = {resource=file.resource.resource,version=file.resource.version,edits={hedit.edit(0,3,'new')}}
        end
        return {intent={review_edits={scope=ctx.broker.scope,changes=changes}}}
      end
      assert(response.edited and response.edited.files == 2)
      return {intent={finish={result={presentation={status='Applied'}}}}}
    end"#;
    for read_document in [false, true] {
        let config = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        for name in ["one.txt", "two.txt"] {
            std::fs::write(project.path().join(name), "old café / 東京\r\n").unwrap();
        }
        let mut capabilities = vec!["workspace.read", "workspace.edit", "ui.present"];
        if read_document {
            capabilities.push("document.read");
        }
        install(config.path(), SOURCE, serde_json::json!(capabilities));
        let catalog = catalog::read_consistent(config.path()).unwrap();
        let ExtensionAction::Lua(plugin) = &catalog.contributions[0].action else {
            panic!()
        };
        let state = State::open(config.path(), []).unwrap();
        let scope = hedit_broker::state::SelectedScope::select(
            project.path(),
            vec![
                hedit_broker::workspace::Right::Read,
                hedit_broker::workspace::Right::Edit,
            ],
            &state,
        )
        .unwrap();
        catalog::approve_resources(
            config.path(),
            &plugin.installed_path,
            "scoped-test",
            &manifest::digest(std::fs::read(&plugin.installed_path).unwrap()),
            hedit_broker::state::Approval {
                command: "scoped-test.run".into(),
                token: serde_json::from_value(plugin.grant.approval_id().into()).unwrap(),
                scope: Some(scope),
                endpoints: vec![],
                tools: Default::default(),
            },
        )
        .unwrap();
        let (_session, ctx, mut app) = app_with("unrelated", Some(config.path()));
        let id = app.workspace.editor.id();
        let mut document =
            hedit::document::Document::open(id, &project.path().join("one.txt")).unwrap();
        let page = document.page(0).unwrap();
        app.workspace.add(document, page).unwrap();
        assert!(app.run_extension_by_id(&ctx, "scoped-test.run"));
        pump(&mut app, &ctx, |app| {
            app.broker_command.as_ref().is_some_and(|command| {
                matches!(
                    &command.phase,
                    Phase::Running {
                        review: Some(_),
                        ..
                    }
                )
            }) || app.error.is_some()
        });
        assert!(app.error.is_none(), "{:?}", app.error);
        let command = app.broker_command.as_mut().unwrap();
        let Phase::Running {
            host,
            review: Some(plan),
            ..
        } = &command.phase
        else {
            panic!()
        };
        command
            .context
            .commit
            .committing
            .store(true, Ordering::Release);
        host.decide(host::Decision {
            plan: plan.id(),
            accept: true,
        })
        .unwrap();
        app.workspace.active = 0;
        app.workspace.open[0].page.text = "keep working".into();
        app.workspace.edit(0, "unrelated").unwrap();
        pump(&mut app, &ctx, |app| {
            let complete = if read_document {
                app.broker_command.is_none()
            } else {
                app.broker_command
                    .as_ref()
                    .is_some_and(|command| matches!(command.phase, Phase::Ready { .. }))
            };
            (complete
                && app
                    .workspace
                    .open
                    .iter()
                    .any(|buffer| buffer.page.info.id == id && buffer.page.info.revision == 1))
                || app.error.is_some()
        });
        assert!(app.error.is_none(), "{:?}", app.error);
        assert_eq!(app.workspace.open[0].page.text, "keep working");
        for name in ["one.txt", "two.txt"] {
            assert_eq!(
                std::fs::read_to_string(project.path().join(name)).unwrap(),
                "new café / 東京\r\n"
            );
        }
        let buffer = app
            .workspace
            .open
            .iter()
            .find(|buffer| buffer.page.info.id == id)
            .unwrap();
        assert_eq!(buffer.page.text, "new café / 東京\r\n");
        assert_eq!(buffer.page.info.revision, 1);
        assert!(!buffer.page.info.dirty);
    }
}
