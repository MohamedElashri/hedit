use super::*;
use crate::app::extensions::{catalog::ToolSelection, distribution};
use std::sync::atomic::AtomicBool;

#[test]
fn installed_json_reference_formats_and_keeps_lsp_alive_until_native_revocation() {
    prepare_native();
    let config = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let initial = "{\"東京\":\"café\",\"active\":true}";
    let active = project.path().join("active.json");
    std::fs::write(&active, initial).unwrap();
    std::fs::write(
        project.path().join("secondary.json"),
        "{\"secondary\":true}",
    )
    .unwrap();
    let bundle = config.path().join("reference.hedit-plugin");
    distribution::run_cli(&[
        "pack".into(),
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/extensions/scoped-json.json"
        )
        .into(),
        bundle.as_os_str().into(),
        "--version".into(),
        "1.0.0".into(),
    ])
    .unwrap();
    let installed = catalog::ImportPreview::open(bundle)
        .unwrap()
        .review(config.path())
        .unwrap()
        .install(config.path())
        .unwrap();
    let path = installed.view.files[0].path.clone();
    let digest = manifest::digest(std::fs::read(&path).unwrap());
    let native = std::env::current_exe()
        .unwrap()
        .ancestors()
        .map(|directory| {
            directory.join(format!("hedit-json-tools{}", std::env::consts::EXE_SUFFIX))
        })
        .find(|path| path.is_file())
        .expect("Build the workspace reference image before running installed acceptance.");
    for command in ["scoped-json.format", "scoped-json.hover"] {
        let approved =
            catalog::set_lua_approved(config.path(), &path, "scoped-json", command, &digest, true)
                .unwrap();
        let ExtensionAction::Lua(plugin) = &approved
            .contributions
            .iter()
            .find(|item| item.id == command)
            .unwrap()
            .action
        else {
            panic!()
        };
        let token = serde_json::from_value(plugin.grant.approval_id().into()).unwrap();
        let (image, _) = catalog::import_tool(
            ToolSelection {
                config: config.path(),
                path: &path,
                extension: "scoped-json",
                command,
                digest: &digest,
                token,
                profile: "json",
            },
            &native,
            &AtomicBool::new(false),
        )
        .unwrap();
        let state = BrokerState::open(config.path(), []).unwrap();
        let scope = (command == "scoped-json.hover").then(|| {
            SelectedScope::select(project.path(), vec![Right::Discover, Right::Read], &state)
                .unwrap()
        });
        catalog::approve_resources(
            config.path(),
            &path,
            "scoped-json",
            &digest,
            Approval {
                command: command.into(),
                token,
                scope,
                endpoints: vec![],
                tools: std::collections::BTreeMap::from([("json".into(), image)]),
            },
        )
        .unwrap();
    }
    let (_session, ctx, mut app) = app_with("unrelated private buffer", Some(config.path()));
    let id = app.workspace.editor.id();
    let mut document = hedit::document::Document::open(id, &active).unwrap();
    let page = document.page(0).unwrap();
    app.workspace.add(document, page).unwrap();
    // Complete the initial pane/focus reconciliation before capturing a target.
    frame(&ctx, &mut app, vec![]);
    app.settings.extensions.preview_edits = false;
    assert!(app.run_extension_by_id(&ctx, "scoped-json.format"));
    pump(&mut app, &ctx, |app| {
        (app.broker_command.is_none() && app.lua_transform.is_none()) || app.error.is_some()
    });
    assert!(app.error.is_none(), "{:?}", app.error);
    assert!(app.active().page.text.contains("\n  \""));
    assert_eq!(std::fs::read_to_string(&active).unwrap(), initial);
    for _ in 0..3 {
        assert!(app.run_extension_by_id(&ctx, "scoped-json.hover"));
        pump(&mut app, &ctx, |app| {
            app.plugin_services
                .entries
                .first()
                .and_then(|entry| entry.scoped.as_ref())
                .and_then(|scoped| scoped.ready.as_ref())
                .is_some()
        });
        let ready = app.plugin_services.entries[0]
            .scoped
            .as_ref()
            .unwrap()
            .ready
            .as_ref()
            .unwrap();
        let ResultSet::Hover(hover) = ready.result.as_ref() else {
            panic!()
        };
        assert!(
            hover.text.contains("JSON object with 2 properties"),
            "{}",
            hover.text
        );
        assert!(
            hover
                .text
                .contains("Approved project: 2 top-level JSON files"),
            "{}",
            hover.text
        );
        assert!(app.plugin_services.entries[0].error.is_none());
        app.workspace.edit(0, " ").unwrap();
        frame(&ctx, &mut app, vec![]);
    }
    let registry = app.plugin_services.entries[0]
        .scoped
        .as_ref()
        .unwrap()
        .native_services
        .clone();
    assert!(registry.current());
    catalog::revoke_resources(config.path(), "scoped-json.hover").unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while registry.current() || !registry.settled() {
        assert!(
            Instant::now() < until,
            "Revocation did not stop and reap installed native LSP without UI frames."
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    frame(&ctx, &mut app, vec![]);
    assert!(app.plugin_services.entries.iter().all(|entry| entry
        .scoped
        .as_ref()
        .is_none_or(|scoped| scoped.ready.is_none())));
}
