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
    app.finish_opened_document(document, page, 0, None, false, None);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![]);
    app.settings.extensions.preview_edits = false;
    assert!(app.run_extension_by_id(&ctx, "scoped-json.format"));
    pump(&mut app, &ctx, |app| {
        (app.broker_command.is_none() && app.lua_transform.is_none()) || app.error.is_some()
    });
    assert!(app.error.is_none(), "{:?}", app.error);
    assert!(
        app.active().page.text.contains("\n  \""),
        "target={:?}, notice={}, activity={:?}",
        app.lua_target(),
        app.notice,
        app.extension_manager.measured_activity()
    );
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
                config,
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

struct ProfileCase {
    app: HeditApp,
    ctx: egui::Context,
    config: tempfile::TempDir,
    _project: tempfile::TempDir,
    _session: tempfile::TempDir,
}

fn profile_case(text: &str, native: bool) -> ProfileCase {
    let config = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let active = project.path().join("active.json");
    std::fs::write(&active, text).unwrap();
    std::fs::write(project.path().join("secondary.json"), "{}").unwrap();
    if native {
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
    let (session, ctx, mut app) = app_with("neutral", Some(config.path()));
    let mut document = hedit::document::Document::open(app.workspace.editor.id(), &active).unwrap();
    let page = document.page(0).unwrap();
    app.finish_opened_document(document, page, 0, None, false, None);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![]);
    if native {
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
    ProfileCase {
        app,
        ctx,
        config,
        _project: project,
        _session: session,
    }
}

#[test]
#[ignore = "controlled paired native LSP frame/retention probe; writes HEDIT_PLUGIN_UI_PROBE_OUT"]
fn profile_scoped_native_frames() {
    use crate::app::extensions::lua::tests::measured_allocations::measure;
    let mut initial_qos = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
    let mut initial_priority = 0;
    assert_eq!(
        unsafe {
            libc::pthread_get_qos_class_np(
                libc::pthread_self(),
                &mut initial_qos,
                &mut initial_priority,
            )
        },
        0
    );
    assert_eq!(
        unsafe {
            libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0)
        },
        0
    );
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
        let mut pairs = vec![
            (["disabled_idle", "native_idle"], false, [false, true]),
            (
                ["disabled_typing", "typing_with_native"],
                true,
                [false, true],
            ),
            (["control_idle_a", "control_idle_b"], false, [false, false]),
            (
                ["control_typing_a", "control_typing_b"],
                true,
                [false, false],
            ),
        ];
        if trial % 2 == 1 {
            pairs.reverse();
        }
        for (names, typing, native) in pairs {
            let mut cases = [
                profile_case(&text, native[0]),
                profile_case(&text, native[1]),
            ];
            for _ in 0..60 {
                for case in &mut cases {
                    case.app.receive(&case.ctx);
                    frame(&case.ctx, &mut case.app, vec![]);
                }
                std::thread::sleep(Duration::from_millis(16));
            }
            let sizes = cases
                .each_ref()
                .map(|case| case.app.active().page.text.len());
            let revisions = cases
                .each_ref()
                .map(|case| case.app.active().page.info.revision);
            let mut seen = cases.each_ref().map(|case| {
                case.app
                    .plugin_services
                    .entries
                    .iter()
                    .filter_map(|entry| entry.scoped.as_ref().and_then(|s| s.ready.as_ref()))
                    .map(|ready| ready.request)
                    .collect::<std::collections::BTreeSet<_>>()
            });
            let mut samples = [Vec::new(), Vec::new()];
            let mut published = [0usize; 2];
            let mut memory = Vec::new();
            let before = usage();
            let work_started = Instant::now();
            for index in 0..frames {
                let start = Instant::now();
                let order = if (index + trial) % 2 == 0 {
                    [0, 1]
                } else {
                    [1, 0]
                };
                for slot in order {
                    let ProfileCase { app, ctx, .. } = &mut cases[slot];
                    samples[slot].push(measure(|| {
                        app.receive(ctx);
                        if native[slot] && typing && index % 20 == 0 {
                            app.plugin_services.request(
                                [
                                    "scoped-json.diagnostics",
                                    "scoped-json.hover",
                                    "scoped-json.symbols",
                                ][index / 20 % 3],
                            );
                            app.plugin_services.open = false;
                        }
                        let events = if typing && index % 10 == 0 {
                            vec![egui::Event::Text(" ".into())]
                        } else {
                            vec![]
                        };
                        std::hint::black_box(frame(ctx, app, events));
                    }));
                    for entry in &app.plugin_services.entries {
                        if let Some(ready) = entry.scoped.as_ref().and_then(|s| s.ready.as_ref()) {
                            if seen[slot].insert(ready.request) {
                                published[slot] += 1;
                            }
                        }
                    }
                    if seen[slot].len() > 128 {
                        seen[slot].clear();
                        seen[slot].extend(
                            app.plugin_services
                                .entries
                                .iter()
                                .filter_map(|entry| {
                                    entry.scoped.as_ref().and_then(|s| s.ready.as_ref())
                                })
                                .map(|ready| ready.request),
                        );
                    }
                }
                if index % 60 == 0 {
                    memory.push(usage());
                }
                std::thread::sleep(Duration::from_millis(16).saturating_sub(start.elapsed()));
            }
            let elapsed_us = work_started.elapsed().as_micros();
            let after = usage();
            let phases = cases
                .each_ref()
                .map(|case| case.app.extension_manager.measured_activity());
            let registries: Vec<_> = cases
                .iter()
                .flat_map(|case| case.app.plugin_services.entries.iter())
                .filter_map(|entry| entry.scoped.as_ref())
                .map(|s| s.native_services.clone())
                .collect();
            for slot in 0..2 {
                let expected = if typing { frames.div_ceil(10) } else { 0 };
                assert_eq!(
                    cases[slot].app.active().page.text.len(),
                    sizes[slot] + expected,
                    "{}: every input must edit the real document",
                    names[slot]
                );
                assert_eq!(
                    cases[slot].app.active().page.info.revision - revisions[slot],
                    expected as u64,
                    "{}: every input must advance its revision",
                    names[slot]
                );
                assert!(
                    cases[slot]
                        .app
                        .plugin_services
                        .entries
                        .iter()
                        .all(|entry| entry.error.is_none()),
                    "{}: provider failure",
                    names[slot]
                );
                if native[slot] && typing {
                    assert!(published[slot] > 0, "No real results published");
                }
            }
            let stop_started = Instant::now();
            let stop_ui = cases
                .each_mut()
                .map(|case| measure(|| case.app.plugin_services.suspend()).0);
            let until = Instant::now() + Duration::from_secs(5);
            while registries.iter().any(|registry| !registry.settled()) {
                assert!(Instant::now() < until);
                std::thread::sleep(Duration::from_millis(5));
            }
            let stop_cleanup_us = stop_started.elapsed().as_micros();
            let stopped = usage();
            for slot in 0..2 {
                let case = &cases[slot];
                let live_runs =
                    std::fs::read_dir(case.config.path().join("extensions-broker/tool-runs"))
                        .map(|entries| {
                            entries
                                .filter_map(Result::ok)
                                .filter(|entry| entry.file_name() != "index.lock")
                                .count()
                        })
                        .unwrap_or(0);
                assert_eq!(live_runs, 0, "native run payloads retained after Stop");
                workloads.push(json!({
                    "trial":trial,"mode":names[slot],"paired_with":names[1-slot],"paired_host_usage":true,
                    "snapshot_bytes":text.len(),"frames":frames,"elapsed_us":elapsed_us,
                    "text_bytes_before":sizes[slot],"text_bytes_after":case.app.active().page.text.len(),
                    "revision_before":revisions[slot],"revision_after":case.app.active().page.info.revision,
                    "published_results":published[slot],"recent_activity":&phases[slot],
                    "samples_us_allocations_requested_bytes":&samples[slot],
                    "usage_before":&before,"usage_after":&after,"usage_after_stop":&stopped,
                    "memory_samples":&memory,"stop_ui_us":stop_ui[slot],"stop_cleanup_us":stop_cleanup_us,
                    "live_runs_after_stop":live_runs
                }));
            }
        }
    }
    std::fs::write(output,serde_json::to_vec_pretty(&json!({
        "os":std::env::consts::OS,"arch":std::env::consts::ARCH,"debug_assertions":cfg!(debug_assertions),
        "ui_thread_qos":{"initial_class":initial_qos as u32,"initial_priority":initial_priority,
            "requested":"user_interactive","relative_priority":0},
        "scope":"Foreground-QoS paired egui CPU frames/UI-thread allocations. Two independent views are interleaved within each 16-ms pair with alternating order. Disabled/native pairs share native background load to isolate incremental UI costs; separate disabled/disabled A/A pairs quantify measurement variance. Process CPU/footprint are shared per pair, exclude children, and are not per-view estimates. Unpaired runs retain whole-process comparisons. Approximately 60 KiB Unicode/CRLF JSON; every typing event is verified. No GPU/physical-input/presentation or total-process-tree-memory claim. No raw text/paths recorded.",
        "workloads":workloads})).unwrap()).unwrap();
}
