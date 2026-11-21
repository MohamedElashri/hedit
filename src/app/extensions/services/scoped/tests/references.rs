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
    install_json_reference(
        config.path(),
        project.path(),
        &["scoped-json.format", "scoped-json.hover"],
    );
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

fn install_json_reference(config: &Path, project: &Path, commands: &[&str]) {
    let bundle = config.join("reference.hedit-plugin");
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
        .review(config)
        .unwrap()
        .install(config)
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
    for &command in commands {
        let approved =
            catalog::set_lua_approved(config, &path, "scoped-json", command, &digest, true)
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
                config: config,
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
        let state = BrokerState::open(config, []).unwrap();
        let scope = (command != "scoped-json.format").then(|| {
            SelectedScope::select(project, vec![Right::Discover, Right::Read], &state).unwrap()
        });
        catalog::approve_resources(
            config,
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
}

#[derive(serde::Serialize)]
struct Usage {
    cpu_us: u64,
    footprint_bytes: u64,
    peak_rss_bytes: u64,
    voluntary_switches: u64,
    involuntary_switches: u64,
}
fn usage() -> Usage {
    let mut value = std::mem::MaybeUninit::<libc::rusage>::uninit();
    assert_eq!(
        unsafe { libc::getrusage(libc::RUSAGE_SELF, value.as_mut_ptr()) },
        0
    );
    let value = unsafe { value.assume_init() };
    let micros = |time: libc::timeval| time.tv_sec as u64 * 1_000_000 + time.tv_usec as u64;
    let mut physical = std::mem::MaybeUninit::<libc::rusage_info_v0>::uninit();
    assert_eq!(
        unsafe {
            libc::proc_pid_rusage(std::process::id() as i32, 0, physical.as_mut_ptr().cast())
        },
        0
    );
    Usage {
        cpu_us: micros(value.ru_utime) + micros(value.ru_stime),
        footprint_bytes: unsafe { physical.assume_init() }.ri_phys_footprint,
        peak_rss_bytes: value.ru_maxrss as u64,
        voluntary_switches: value.ru_nvcsw as u64,
        involuntary_switches: value.ru_nivcsw as u64,
    }
}

#[test]
#[ignore = "controlled scoped/native LSP frame and retention probe; writes HEDIT_PLUGIN_UI_PROBE_OUT"]
fn profile_scoped_native_frames() {
    use crate::app::extensions::lua::tests::measured_allocations::measure;
    let output =
        std::env::var_os("HEDIT_PLUGIN_UI_PROBE_OUT").expect("Set HEDIT_PLUGIN_UI_PROBE_OUT");
    let frames = std::env::var("HEDIT_PLUGIN_UI_PROBE_FRAMES")
        .ok()
        .map(|n| n.parse::<usize>().unwrap())
        .unwrap_or(1200);
    assert!((20..=3600).contains(&frames));
    prepare_native();
    let text = serde_json::to_string_pretty(&json!({"items":vec!["café / 東京 / 😀";2000]}))
        .unwrap()
        .replace("\n", "\r\n")
        + "\r\n";
    assert!(text.len() < 64 * 1024 - 1000);
    let mut workloads = Vec::new();
    for trial in 0..3 {
        let mut modes = vec![
            "disabled_idle",
            "native_idle",
            "disabled_typing",
            "typing_with_native",
        ];
        if trial % 2 == 1 {
            modes.reverse();
        }
        for mode in modes {
            let config = tempfile::tempdir().unwrap();
            let project = tempfile::tempdir().unwrap();
            let active = project.path().join("active.json");
            std::fs::write(&active, &text).unwrap();
            std::fs::write(project.path().join("secondary.json"), "{}").unwrap();
            if !mode.starts_with("disabled") {
                install_json_reference(
                    config.path(),
                    project.path(),
                    &[
                        "scoped-json.diagnostics",
                        "scoped-json.hover",
                        "scoped-json.symbols",
                    ],
                );
            }
            let (_session, ctx, mut app) = app_with("neutral", Some(config.path()));
            let id = app.workspace.editor.id();
            let mut document = hedit::document::Document::open(id, &active).unwrap();
            let page = document.page(0).unwrap();
            app.workspace.add(document, page).unwrap();
            frame(&ctx, &mut app, vec![]);
            if !mode.starts_with("disabled") {
                for command in [
                    "scoped-json.diagnostics",
                    "scoped-json.hover",
                    "scoped-json.symbols",
                ] {
                    app.plugin_services.request(command);
                    app.plugin_services.open = false;
                    pump(&mut app, &ctx, |app| {
                        app.plugin_services
                            .entries
                            .iter()
                            .find(|entry| entry.contribution.id == command)
                            .and_then(|entry| entry.scoped.as_ref())
                            .and_then(|scoped| scoped.ready.as_ref())
                            .is_some()
                    });
                }
                assert_eq!(app.plugin_services.entries.len(), 3);
            }
            for _ in 0..60 {
                frame(&ctx, &mut app, vec![]);
                std::thread::sleep(Duration::from_millis(16));
            }
            let before = usage();
            let work_started = Instant::now();
            let mut published = 0usize;
            let mut seen = std::collections::BTreeSet::new();
            for entry in &app.plugin_services.entries {
                if let Some(ready) = entry
                    .scoped
                    .as_ref()
                    .and_then(|scoped| scoped.ready.as_ref())
                {
                    seen.insert(ready.request);
                }
            }
            let mut samples = Vec::new();
            let mut memory = Vec::new();
            for index in 0..frames {
                let start = Instant::now();
                let sample = measure(|| {
                    app.receive(&ctx);
                    if mode == "typing_with_native" && index % 20 == 0 {
                        app.plugin_services.request(
                            [
                                "scoped-json.diagnostics",
                                "scoped-json.hover",
                                "scoped-json.symbols",
                            ][index / 20 % 3],
                        );
                        app.plugin_services.open = false;
                    }
                    let events = if matches!(mode, "disabled_typing" | "typing_with_native")
                        && index % 10 == 0
                    {
                        vec![egui::Event::Text(" ".into())]
                    } else {
                        vec![]
                    };
                    std::hint::black_box(frame(&ctx, &mut app, events));
                });
                samples.push(sample);
                for entry in &app.plugin_services.entries {
                    if let Some(ready) = entry
                        .scoped
                        .as_ref()
                        .and_then(|scoped| scoped.ready.as_ref())
                    {
                        if seen.insert(ready.request) {
                            published += 1;
                        }
                    }
                }
                if seen.len() > 128 {
                    seen.clear();
                    for entry in &app.plugin_services.entries {
                        if let Some(ready) = entry
                            .scoped
                            .as_ref()
                            .and_then(|scoped| scoped.ready.as_ref())
                        {
                            seen.insert(ready.request);
                        }
                    }
                }
                if index % 60 == 0 {
                    memory.push(usage());
                }
                std::thread::sleep(Duration::from_millis(16).saturating_sub(start.elapsed()));
            }
            let elapsed_us = work_started.elapsed().as_micros();
            let after = usage();
            if mode == "typing_with_native" {
                assert!(published > 0, "No real results published during typing");
            }
            let phases = app.extension_manager.measured_activity();
            let errors: Vec<_> = app
                .plugin_services
                .entries
                .iter()
                .filter_map(|entry| entry.error.as_ref().map(|_| "provider_failure"))
                .collect();
            assert!(
                errors.is_empty(),
                "{mode}: provider failed during sustained work"
            );
            let registries: Vec<_> = app
                .plugin_services
                .entries
                .iter()
                .filter_map(|entry| entry.scoped.as_ref())
                .map(|scoped| scoped.native_services.clone())
                .collect();
            let before_stop = Instant::now();
            let stop_ui = measure(|| app.plugin_services.suspend());
            let until = Instant::now() + Duration::from_secs(5);
            while registries.iter().any(|registry| !registry.settled()) {
                assert!(Instant::now() < until);
                std::thread::sleep(Duration::from_millis(5));
            }
            let stopped = usage();
            let live_runs = std::fs::read_dir(config.path().join("extensions-broker/tool-runs"))
                .map(|entries| {
                    entries
                        .filter_map(Result::ok)
                        .filter(|entry| entry.file_name() != "index.lock")
                        .count()
                })
                .unwrap_or(0);
            assert_eq!(live_runs, 0, "native payloads retained after Stop");
            workloads.push(
                json!({"trial":trial,"mode":mode,"snapshot_bytes":text.len(),
                "frames":frames,"elapsed_us":elapsed_us,"published_results":published,"recent_activity":phases,"samples_us_allocations_requested_bytes":samples,
                "usage_before":before,"usage_after":after,"usage_after_stop":stopped,
                "memory_samples":memory,"stop_cleanup_us":before_stop.elapsed().as_micros(),"stop_ui_us":stop_ui.0,
                "live_runs_after_stop":live_runs}),
            );
        }
    }
    std::fs::write(output,serde_json::to_vec_pretty(&json!({
        "os":std::env::consts::OS,"arch":std::env::consts::ARCH,"debug_assertions":cfg!(debug_assertions),
        "scope":"Same-build instrumented egui CPU frames and UI-thread allocation traffic. Three source-approved native LSP profiles use real rotated Lua callbacks and private project snapshots. Approximately 60 KiB Unicode JSON; typing at one event per ten 16-ms frames. Physical host footprint, self CPU and context switches exclude child usage. No GPU/OS input/presentation latency or total-process-tree memory claim. Raw text/paths are not recorded.",
        "workloads":workloads})).unwrap()).unwrap();
}
