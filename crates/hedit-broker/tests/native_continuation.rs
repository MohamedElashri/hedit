//! Real rotated workers and host-owned synthetic resources. The test executable
//! also handles the trusted worker entry point on Linux; no sandbox override.
use cap_std::fs::Dir;
use hedit_broker::{
    cancel::Cancel,
    declaration::Definition,
    network::Network,
    recovery_store::RecoveryStore,
    resources::Resources,
    session::{Config, Effect, Session},
    workspace::{Policy, Protected, Right, Root},
};
use hedit_plugin::{
    broker::{self, Handle, Intent, Response},
    options::Value,
    sdk,
};
use std::{collections::BTreeMap, path::Path, sync::Arc, time::Instant};

fn private(parent: &Path, name: &str) -> Dir {
    let path = parent.join(name);
    let builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
    builder.create(&path).unwrap();
    #[cfg(unix)]
    let file = std::fs::File::open(&path).unwrap();
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::*;
        std::fs::OpenOptions::new()
            .access_mode(FILE_GENERIC_READ | WRITE_DAC)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&path)
            .unwrap()
    };
    hedit_core::private_io::protect_directory(&file).unwrap();
    Dir::open_ambient_dir(path, cap_std::ambient_authority()).unwrap()
}
fn package(source: &str, text: &str) -> sdk::Input {
    sdk::Input {
        sdk_version: sdk::VERSION,
        source: source.into(),
        modules: Default::default(),
        resources: Default::default(),
        snapshot: sdk::Snapshot {
            document: "a".repeat(32),
            revision: "0".into(),
            language: "plain_text".into(),
            text: text.into(),
            selections: vec![sdk::Selection {
                anchor: 0,
                head: text.len(),
            }],
            crlf: text.contains("\r\n"),
        },
        settings: Default::default(),
        arguments: Default::default(),
        storage: None,
    }
}
fn callback(input: broker::Input) -> broker::Output {
    let start = Instant::now();
    let step = input.step;
    let output = hedit_plugin::run_broker_fixture(input).unwrap();
    println!(
        "{{\"probe\":\"native_broker_callback\",\"step\":{step},\"elapsed_us\":{}}}",
        start.elapsed().as_micros()
    );
    output
}
fn measure(probe: &str, start: Instant) {
    println!(
        "{{\"probe\":\"{probe}\",\"elapsed_us\":{}}}",
        start.elapsed().as_micros()
    );
}
fn workspace_config(outer: &Path, cancel: Arc<Cancel>) -> Config {
    let config = private(outer, "config");
    let protected = Protected::persistent(&config, []).unwrap();
    let store = Arc::new(
        RecoveryStore::open(private(&outer.join("config"), "recovery"), &protected).unwrap(),
    );
    let project = outer.join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("first.txt"), "old café / 東京\r\n").unwrap();
    std::fs::write(project.join("second.txt"), "old 🦀 / old\n").unwrap();
    let root = Root::open_selected(
        &project,
        Policy::new([Right::Discover, Right::Read, Right::Edit]).unwrap(),
        protected,
    )
    .unwrap();
    let mut input = package(include_str!("fixtures/rename.lua"), "");
    input.arguments = BTreeMap::from([
        ("before".into(), Value::Text("old".into())),
        ("after".into(), Value::Text("new".into())),
    ]);
    let definition: Definition = serde_json::from_str(
        r#"{"capabilities":["workspace.discover","workspace.read","workspace.edit"]}"#,
    )
    .unwrap();
    Config {
        source_token: None,
        tools: Default::default(),
        owner: None,
        package: input,
        provider: None,
        cache: Default::default(),
        definition: Arc::new(definition),
        resources: Some(Resources::new(root).unwrap()),
        endpoints: Default::default(),
        network: Arc::new(Network::default()),
        recovery: store,
        cancel,
        wake: Arc::new(|| {}),
    }
}
fn workspace_chain() {
    let outer = tempfile::tempdir().unwrap();
    let project = outer.path().join("project");
    let mut session =
        Session::new(workspace_config(outer.path(), Arc::new(Cancel::default()))).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for step in 0..4 {
        let started = Instant::now();
        let input = session.begin_callback().unwrap();
        measure("broker_input_copy", started);
        assert_eq!(input.step, step);
        let validation = input.clone();
        let output = callback(input);
        let started = Instant::now();
        output.validate(&validation).unwrap();
        measure("broker_reply_validation", started);
        let started = Instant::now();
        let effect = runtime.block_on(session.accept(output, || Ok(()))).unwrap();
        measure("broker_io", started);
        match effect {
            Effect::Continue => assert!(step < 2),
            Effect::Review(plan) => {
                assert_eq!(step, 2);
                assert_eq!(plan.files().len(), 2);
                assert_eq!(
                    std::fs::read_to_string(project.join("first.txt")).unwrap(),
                    "old café / 東京\r\n"
                );
                // No editor buffers exist in this isolated fixture. A real host
                // must verify/freeze affected clean buffers before this call.
                let started = Instant::now();
                let journal = session.prepare_review(&plan, || Ok(())).unwrap();
                measure("journal_prepare", started);
                let started = Instant::now();
                session.apply_review(&plan, &journal, || Ok(())).unwrap();
                measure("journal_apply", started);
            }
            Effect::Finished {
                transaction: None,
                result: None,
                ..
            } => assert_eq!(step, 3),
            _ => panic!("Unexpected fixture effect"),
        }
    }
    assert_eq!(
        std::fs::read_to_string(project.join("first.txt")).unwrap(),
        "new café / 東京\r\n"
    );
    assert_eq!(
        std::fs::read_to_string(project.join("second.txt")).unwrap(),
        "new 🦀 / new\n"
    );
    assert!(session.begin_callback().is_err());
}
fn synthetic_translation() {
    let original = "café / 東京 / 🦀\r\n";
    let mut input = broker::Input {
        protocol_version: broker::VERSION,
        request: Handle::fresh().unwrap(),
        step: 0,
        package: package(include_str!("fixtures/translation.lua"), original),
        scope: None,
        may_edit: true,
        workspace_edit: false,
        may_present: false,
        provider: None,
        response: None,
        cache: Default::default(),
    };
    let output = callback(input.clone());
    let Intent::Https { endpoint, body } = output.intent else {
        panic!("Missing HTTPS intent")
    };
    assert_eq!(endpoint, "translation");
    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body, serde_json::json!({"text":original,"target":"fr"}));
    // Synthetic transport: this fixture never contacts an Internet service.
    input.response = Some(Response::Https {
        status: 200,
        body: r#"{"text":"bonjour / 東京 / 🦀\r\n"}"#.into(),
    });
    input.step = 1;
    let output = callback(input.clone());
    let Intent::Finish {
        transaction: Some(transaction),
        result: None,
    } = output.intent
    else {
        panic!("Missing edit proposal")
    };
    assert_eq!(
        transaction.prepare(&input.package.snapshot).unwrap().text,
        "bonjour / 東京 / 🦀\r\n"
    );
}
fn asynchronous_host() {
    use hedit_broker::host::{Authority, Decision, Event, Host};
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        time::Duration,
    };
    for scenario in ["apply", "cancel", "stale"] {
        let outer = tempfile::tempdir().unwrap();
        let path = outer.path().to_path_buf();
        let project = path.join("project");
        let policy = path.join("fixture-policy.lock");
        let caller = std::thread::current().id();
        let calls = Arc::new(AtomicUsize::new(0));
        let checked = calls.clone();
        let authority: Arc<Authority> = Arc::new(move |cancel| {
            if cancel.load(Ordering::Acquire) {
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            checked.fetch_add(1, Ordering::AcqRel);
            // Synthetic host approval only: production must resolve the exact
            // source, selected resources and current grant generation here.
            let file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(&policy)?;
            file.lock_shared()?;
            Ok(file)
        });
        let (sender, receiver) = mpsc::sync_channel(1);
        let host = Host::start(
            move |cancel| {
                assert_ne!(std::thread::current().id(), caller);
                Ok(workspace_config(&path, cancel))
            },
            authority,
            Arc::new(move || {
                let _ = sender.try_send(());
            }),
        )
        .unwrap();
        receiver.recv_timeout(Duration::from_secs(15)).unwrap();
        let Event::Review(plan) = host.try_event().unwrap() else {
            panic!("Missing host preview")
        };
        assert!(calls.load(Ordering::Acquire) >= 6);
        assert_eq!(
            std::fs::read_to_string(project.join("first.txt")).unwrap(),
            "old café / 東京\r\n"
        );
        match scenario {
            "apply" => host
                .decide(Decision {
                    plan: plan.id(),
                    accept: true,
                })
                .unwrap(),
            "cancel" => host.stop(),
            "stale" => host
                .decide(Decision {
                    plan: Handle::fresh().unwrap(),
                    accept: true,
                })
                .unwrap(),
            _ => unreachable!(),
        }
        receiver.recv_timeout(Duration::from_secs(15)).unwrap();
        let event = host.try_event().unwrap();
        match scenario {
            "apply" => {
                assert!(matches!(
                    event,
                    Event::Finished {
                        transaction: None,
                        result: None,
                        ..
                    }
                ));
                assert_eq!(
                    std::fs::read_to_string(project.join("first.txt")).unwrap(),
                    "new café / 東京\r\n"
                );
            }
            "cancel" => assert!(matches!(
                event,
                Event::Failed(broker::FailureCode::Cancelled)
            )),
            "stale" => assert!(matches!(event, Event::Failed(broker::FailureCode::Failed))),
            _ => unreachable!(),
        }
        if scenario != "apply" {
            assert_eq!(
                std::fs::read_to_string(project.join("first.txt")).unwrap(),
                "old café / 東京\r\n"
            );
        }
        assert!(host.try_event().is_none());
    }
}
fn foreground_callback_survives_full_background_admission() {
    let background: Vec<_> = (0..3)
        .map(|_| hedit_plugin::service_admission().unwrap())
        .collect();
    assert!(!hedit_plugin::service_available());
    let input = broker::Input {
        protocol_version: broker::VERSION,
        request: Handle::fresh().unwrap(),
        step: 0,
        package: package("return function(ctx) return {intent={finish={}}} end", ""),
        scope: None,
        may_edit: false,
        workspace_edit: false,
        may_present: false,
        provider: None,
        response: None,
        cache: Default::default(),
    };
    let result = hedit_plugin::run_broker_fixture(input).unwrap();
    assert!(matches!(
        result.intent,
        Intent::Finish {
            transaction: None,
            result: None
        }
    ));
    assert!(!hedit_plugin::service_available());
    drop(background);
    assert!(hedit_plugin::service_available());
}
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if let Some(completed) = hedit_plugin::run_if_requested(&args) {
        std::process::exit(i32::from(!completed));
    }
    let executable = std::env::current_exe().unwrap();
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("python3")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../scripts/macos_plugin_helper.py"
            ))
            .arg("--editor")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[cfg(windows)]
    {
        std::fs::copy(
            &executable,
            executable.parent().unwrap().join("hedit-plugin-worker.exe"),
        )
        .unwrap();
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = executable;
    if args.first().is_some_and(|argument| argument == "--measure") {
        assert_eq!(args.len(), 2, "Supply --measure with 1–100 trials.");
        let trials: usize = args[1].to_str().unwrap().parse().unwrap();
        assert!((1..=100).contains(&trials));
        for _ in 0..trials {
            workspace_chain();
        }
        return;
    }
    workspace_chain();
    synthetic_translation();
    asynchronous_host();
    foreground_callback_survives_full_background_admission();
    println!("PASS: confined workspace, synthetic HTTPS and foreground admission fixtures");
}
