//! Actual imported native image through the production broker bundle lifecycle.
use hedit_broker::{
    declaration::ToolDefinition,
    state::State,
    tool::{Binding, Projection, Source},
    Handle,
};
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if let Some(success) = hedit_plugin::run_if_requested(&args) {
        std::process::exit(i32::from(!success));
    }
    if let Some(success) = hedit_plugin::tool::run_if_requested(&args) {
        std::process::exit(if success { 0 } else { 1 });
    }
    assert!(
        args.iter()
            .all(|arg| arg == "--nocapture" || arg == "--test-threads=1"),
        "unsupported test arguments"
    );
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    std::fs::create_dir(&config).unwrap();
    let state = State::open(&config, []).unwrap();
    let source = root.path().join("filter.rs");
    std::fs::write(&source, include_str!("fixtures/native_filter.rs")).unwrap();
    let image = root.path().join(if cfg!(windows) {
        "filter.exe"
    } else {
        "filter"
    });
    let mut compiler = std::process::Command::new("rustc");
    compiler.args([
        "--edition=2021",
        "--crate-name=hedit_native_filter_fixture",
        "-C",
        "opt-level=1",
        "-C",
        "debuginfo=0",
    ]);
    #[cfg(windows)]
    compiler.args(["-C", "target-feature=+crt-static"]);
    assert!(compiler
        .arg(source)
        .arg("-o")
        .arg(&image)
        .status()
        .unwrap()
        .success());
    let source = Source::select(&image).unwrap();
    let definition: ToolDefinition = serde_json::from_value(
        serde_json::json!({"protocol":"filter", "workspace":false, "arguments":[]}),
    )
    .unwrap();
    let token = Handle::fresh().unwrap();
    let owner = Binding::new("sample.format", token, "formatter", &definition).unwrap();
    let images = state.tool_store().unwrap();
    let runs = state.tool_runs().unwrap();
    let cancel = AtomicBool::new(false);
    let id = images.import(source, owner.clone(), &cancel).unwrap();
    let run = runs
        .prepare(
            images.resolve(id, &owner, &cancel).unwrap(),
            Projection::empty(),
            vec![],
            &cancel,
        )
        .unwrap();
    assert_eq!(
        run.filter(
            "document café / 東京".as_bytes(),
            &cancel,
            Duration::from_secs(10)
        )
        .unwrap()
        .stdout,
        "DOCUMENT CAFÉ / 東京".as_bytes()
    );
    empty(&config.join("extensions-broker/tool-runs"));
    let run = runs
        .prepare(
            images.resolve(id, &owner, &cancel).unwrap(),
            Projection::empty(),
            vec![],
            &cancel,
        )
        .unwrap();
    cancel.store(true, Ordering::Release);
    assert_eq!(
        run.filter(b"never send", &cancel, Duration::from_secs(10))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::Interrupted
    );
    empty(&config.join("extensions-broker/tool-runs"));
    cancel.store(false, Ordering::Release);
    duplex(&state, id, &owner);
    empty(&config.join("extensions-broker/tool-runs"));
    broker_chain(&state, id, token, definition);
    images.remove(id, &owner).unwrap();
    println!("Imported native image: exact Unicode bytes, private run ownership, cancellation and cleanup passed.");
}
fn duplex(state: &State, image: Handle, binding: &Binding) {
    use std::time::Instant;
    let cancel = AtomicBool::new(false);
    let prepare = || {
        state
            .tool_runs()
            .unwrap()
            .prepare(
                state
                    .tool_store()
                    .unwrap()
                    .resolve(image, binding, &cancel)
                    .unwrap(),
                Projection::empty(),
                vec!["--stream".into()],
                &cancel,
            )
            .unwrap()
    };
    let start = || {
        prepare()
            .service(&cancel, Instant::now() + Duration::from_secs(10))
            .unwrap()
    };
    let mut services = vec![start(), start(), start()];
    assert_eq!(
        prepare()
            .service(&cancel, Instant::now() + Duration::from_secs(10))
            .err()
            .unwrap()
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
    // Three background owners leave a fourth native slot for a foreground filter.
    let run = state
        .tool_runs()
        .unwrap()
        .prepare(
            state
                .tool_store()
                .unwrap()
                .resolve(image, binding, &cancel)
                .unwrap(),
            Projection::empty(),
            vec![],
            &cancel,
        )
        .unwrap();
    assert_eq!(
        run.filter(b"foreground", &cancel, Duration::from_secs(10))
            .unwrap()
            .stdout,
        b"FOREGROUND"
    );
    for round in 0..8 {
        for service in &mut services {
            let text = format!("round {round}: café / 東京 / 😀");
            let mut request = (text.len() as u32).to_le_bytes().to_vec();
            request.extend(text.as_bytes());
            let mut sent = 0;
            let mut response = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(10);
            let expected = text.to_uppercase();
            while response.len() < 4 + expected.len() {
                let mut buffer = [0; 4096];
                let progress = service
                    .poll(&request[sent..], &mut buffer, &cancel, deadline)
                    .unwrap();
                sent += progress.written;
                response.extend_from_slice(&buffer[..progress.read]);
                if progress.idle() {
                    service.wait(sent < request.len()).unwrap();
                }
            }
            assert_eq!(sent, request.len());
            assert_eq!(
                u32::from_le_bytes(response[..4].try_into().unwrap()) as usize,
                expected.len()
            );
            assert_eq!(&response[4..], expected.as_bytes());
        }
    }
    cancel.store(true, Ordering::Release);
    for service in &mut services {
        let mut output = [0; 4096];
        assert_eq!(
            service
                .poll(
                    b"never deliver",
                    &mut output,
                    &cancel,
                    Instant::now() + Duration::from_secs(10)
                )
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::Interrupted
        );
        cancel.store(false, Ordering::Release);
        assert!(service
            .poll(
                b"cannot reuse",
                &mut output,
                &cancel,
                Instant::now() + Duration::from_secs(10)
            )
            .is_err());
        cancel.store(true, Ordering::Release);
    }
    drop(services);
    cancel.store(false, Ordering::Release);
    drop(start());
    let mut expired = start();
    assert_eq!(
        expired
            .poll(b"never deliver", &mut [0; 4096], &cancel, Instant::now())
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::TimedOut
    );
    assert!(expired
        .poll(
            &[],
            &mut [0; 4096],
            &cancel,
            Instant::now() + Duration::from_secs(10)
        )
        .is_err());
    drop(expired);
    let run = state
        .tool_runs()
        .unwrap()
        .prepare(
            state
                .tool_store()
                .unwrap()
                .resolve(image, binding, &cancel)
                .unwrap(),
            Projection::empty(),
            vec!["--stderr-flood".into()],
            &cancel,
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut noisy = run.service(&cancel, deadline).unwrap();
    loop {
        match noisy.poll(&[], &mut [0; 4096], &cancel, deadline) {
            Ok(_) => noisy.wait(false).unwrap(),
            Err(error) => {
                assert_eq!(error.to_string(), "Tool output limit exceeded.");
                break;
            }
        }
    }
    drop(noisy);
    println!("Duplex native transport: repeated Unicode exchanges, foreground reservation, cancellation, poison and lease cleanup passed.");
}
fn empty(path: &Path) {
    let names: Vec<_> = std::fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["index.lock"]);
}

fn broker_chain(state: &State, image: Handle, token: Handle, recipe: ToolDefinition) {
    use hedit_broker::{
        cancel::Cancel,
        declaration::Definition,
        journal::Owner,
        network::Network,
        session::{Config, Effect, Session},
        state::Approval,
    };
    use std::{collections::BTreeMap, sync::Arc};
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
    std::fs::copy(
        &executable,
        executable.parent().unwrap().join("hedit-plugin-worker.exe"),
    )
    .unwrap();
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = executable;
    let definition: Definition = serde_json::from_value(
        serde_json::json!({"capabilities":["tools.run"], "tools":{"formatter":recipe}}),
    )
    .unwrap();
    state
        .approve(
            Approval {
                command: "sample.format".into(),
                token,
                scope: None,
                endpoints: vec![],
                tools: BTreeMap::from([("formatter".into(), image)]),
            },
            &definition,
        )
        .unwrap();
    let resolved = state.resolve("sample.format", token, &definition).unwrap();
    let package: hedit_plugin::sdk::Input = serde_json::from_value(serde_json::json!({
        "sdk_version":1,
        "source":r#"assert(io == nil and os == nil and package == nil)
local calls = 0
return function(ctx)
    calls = calls + 1
    assert(calls == 1)
    if ctx.broker.response == nil then
        return {intent={tool={profile="formatter",input="document café / 東京",arguments={}}}}
    end
    assert(ctx.broker.response.tool.status == 0)
    assert(ctx.broker.response.tool.output == "DOCUMENT CAFÉ / 東京")
    return {intent={finish={}}}
end"#,
        "snapshot":{"document":"0123456789abcdef0123456789abcdef","revision":"0","language":"plain_text","text":"","selections":[{"anchor":0,"head":0}],"crlf":false},
        "settings":{},"arguments":{}
    })).unwrap();
    let configuration = || Config {
        source_token: Some(token),
        tools: resolved.tools.clone(),
        owner: Some(Owner {
            command: "sample.format".into(),
            digest: "a".repeat(64),
        }),
        package: package.clone(),
        provider: None,
        cache: Default::default(),
        definition: Arc::new(definition.clone()),
        resources: None,
        endpoints: Default::default(),
        network: Arc::new(Network::default()),
        recovery: state.recovery(),
        cancel: Arc::new(Cancel::default()),
        wake: Arc::new(|| {}),
    };
    for variant in 0..5 {
        let mut foreign = configuration();
        match variant {
            0 => foreign.source_token = Some(Handle::fresh().unwrap()),
            1 => foreign.owner.as_mut().unwrap().command = "foreign.command".into(),
            2 => foreign.source_token = None,
            3 => foreign.owner = None,
            _ => {
                let mut changed = (*foreign.definition).clone();
                changed.tools.get_mut("formatter").unwrap().arguments.push(
                    hedit_broker::declaration::Argument::Literal("--different-recipe".into()),
                );
                foreign.definition = Arc::new(changed);
            }
        }
        assert_eq!(
            Session::new(foreign).err().unwrap().kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }
    let mut session = Session::new(configuration()).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let input = session.begin_callback().unwrap();
    let reply = hedit_plugin::run_broker_fixture(input).unwrap();
    assert!(matches!(
        runtime.block_on(session.accept(reply, || Ok(()))).unwrap(),
        Effect::Continue
    ));
    let input = session.begin_callback().unwrap();
    let reply = hedit_plugin::run_broker_fixture(input).unwrap();
    assert!(matches!(
        runtime.block_on(session.accept(reply, || Ok(()))).unwrap(),
        Effect::Finished { .. }
    ));
    let mut revoked = Session::new(configuration()).unwrap();
    let input = revoked.begin_callback().unwrap();
    let reply = hedit_plugin::run_broker_fixture(input).unwrap();
    assert_eq!(
        runtime
            .block_on(revoked.accept(reply, || Err::<(), _>(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "revoked"
            ))))
            .err()
            .unwrap()
            .kind(),
        std::io::ErrorKind::PermissionDenied
    );
    println!("Real rotated Lua callbacks invoked the approved native filter through the broker.");
}
