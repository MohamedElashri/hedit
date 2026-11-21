//! A native protocol peer runs inside the production sandbox, never in the host.
use hedit_broker::{
    lsp::{Connection, Method},
    state::State,
    tool::{Binding, Projection, Source},
    Handle,
};
use serde_json::json;
use std::{
    io,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

pub fn verify(state: &State, executable: &std::path::Path) {
    let cancel = AtomicBool::new(false);
    let definition = serde_json::from_value(
        json!({"protocol":"language_server","workspace":false,"arguments":[]}),
    )
    .unwrap();
    let binding = Binding::new(
        "sample.lsp",
        Handle::fresh().unwrap(),
        "server",
        &definition,
    )
    .unwrap();
    let images = state.tool_store().unwrap();
    let runs = state.tool_runs().unwrap();
    let image = images
        .import(
            Source::select(executable).unwrap(),
            binding.clone(),
            &cancel,
        )
        .unwrap();
    for mode in [
        "normal",
        "server-request",
        "foreign",
        "duplicate",
        "truncated",
        "flood",
        "stall",
    ] {
        let deadline = Instant::now() + Duration::from_secs(10);
        let run = runs
            .prepare(
                images.resolve(image, &binding, &cancel).unwrap(),
                Projection::empty(),
                vec![mode.into()],
                &cancel,
            )
            .unwrap();
        let mut connection = Connection::new(run.service(&cancel, deadline).unwrap());
        connection
            .initialize(json!({"capabilities":{}}), &cancel, deadline)
            .unwrap();
        if matches!(mode, "normal" | "server-request") {
            for count in 2..10 {
                let value = connection
                    .request(
                        Method::Hover,
                        json!({}),
                        &cancel,
                        Instant::now() + Duration::from_secs(10),
                    )
                    .unwrap();
                assert_eq!(value["requests"], count);
                assert_eq!(value["contents"], "café / 東京 / 😀");
            }
        } else {
            let error = connection
                .request(
                    Method::Hover,
                    json!({}),
                    &cancel,
                    Instant::now()
                        + Duration::from_millis(if mode == "stall" { 100 } else { 5000 }),
                )
                .unwrap_err();
            if mode == "stall" {
                assert_eq!(error.kind(), io::ErrorKind::TimedOut);
            }
            assert!(connection
                .request(
                    Method::Hover,
                    json!({}),
                    &cancel,
                    Instant::now() + Duration::from_secs(1)
                )
                .is_err());
        }
        drop(connection);
    }
    owner(state, image, &binding);
    broker_chain(state, executable);
    images.remove(image, &binding).unwrap();
    println!("Confined LSP peer: persistent fragmented Unicode requests, server-request denial, foreign/duplicate/truncated/flooded responses, timeout and cleanup passed.");
}

fn owner(state: &State, image: Handle, binding: &Binding) {
    use hedit_broker::{cancel::Cancel, lsp::Owner};
    use hedit_plugin::{sdk, service::ResultSet};
    use std::sync::Arc;
    let cancel = AtomicBool::new(false);
    let snapshot = sdk::Snapshot {
        document: "a".repeat(32),
        revision: "42".into(),
        language: "json".into(),
        text: "café / 東京 / 😀".into(),
        selections: vec![sdk::Selection { anchor: 0, head: 0 }],
        crlf: false,
    };
    let scope = Arc::new(Cancel::default());
    let make = |mode: &str| {
        let mut projection = Projection::empty();
        let relative = projection.document(&snapshot, None, None).unwrap();
        let run = state
            .tool_runs()
            .unwrap()
            .prepare(
                state
                    .tool_store()
                    .unwrap()
                    .resolve(image, binding, &cancel)
                    .unwrap(),
                projection,
                vec![mode.into()],
                &cancel,
            )
            .unwrap();
        Owner::start(
            run,
            relative,
            snapshot.clone(),
            false,
            scope.clone(),
            Instant::now() + Duration::from_secs(10),
            None,
        )
        .unwrap()
    };
    let owner = make("normal");
    for revision in [42, 43, 44] {
        let mut current = snapshot.clone();
        current.revision = revision.to_string();
        current.text = format!("version {revision}: café / 東京 / 😀");
        if revision == 42 {
            current.text = snapshot.text.clone();
        }
        let ResultSet::Hover(hover) = owner
            .request(
                &current,
                Method::Hover,
                0,
                false,
                Arc::new(Cancel::default()),
                Instant::now() + Duration::from_secs(10),
            )
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(hover.text, current.text);
        assert!(owner.running());
    }
    let mut foreign = snapshot.clone();
    foreign.document = "b".repeat(32);
    assert!(owner
        .request(
            &foreign,
            Method::Hover,
            0,
            false,
            Arc::new(Cancel::default()),
            Instant::now() + Duration::from_secs(10)
        )
        .is_err());
    assert_eq!(
        owner
            .request(
                &snapshot,
                Method::Symbols,
                0,
                false,
                Arc::new(Cancel::default()),
                Instant::now() + Duration::from_secs(10)
            )
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    ); // Earlier revision cannot restore document state.
    drop(owner);
    let owner = make("normal");
    assert_eq!(
        owner
            .request(
                &snapshot,
                Method::Symbols,
                0,
                false,
                Arc::new(Cancel::default()),
                Instant::now() + Duration::from_secs(10)
            )
            .unwrap_err()
            .kind(),
        io::ErrorKind::Unsupported
    );
    assert!(owner.running());
    drop(owner);
    let owner = make("stall");
    let request_cancel = Arc::new(Cancel::default());
    let trigger = request_cancel.clone();
    let thread = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        trigger.cancel();
    });
    assert_eq!(
        owner
            .request(
                &snapshot,
                Method::Hover,
                0,
                false,
                request_cancel,
                Instant::now() + Duration::from_secs(10)
            )
            .unwrap_err()
            .kind(),
        io::ErrorKind::Interrupted
    );
    thread.join().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !owner.settled() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(owner.settled());
    drop(owner);
    let owner = make("normal");
    // The owner observes scope revocation while parked, without UI polling.
    scope.cancel();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !owner.settled() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!owner.running());
    assert!(owner.settled());
    drop(owner);
    println!("Persistent LSP owner: revision updates, foreign/old snapshots, unsupported methods and parked scope cancellation passed.");
}

fn broker_chain(state: &State, executable: &std::path::Path) {
    use hedit_broker::{
        cancel::Cancel,
        declaration::Definition,
        journal::Owner,
        network::Network,
        session::{Config, Effect, Session},
        state::Approval,
    };
    use hedit_plugin::{options::Value, sdk};
    use std::{collections::BTreeMap, sync::Arc};
    let definition: Definition = serde_json::from_value(json!({"capabilities":["document.read","tools.run"],"tools":{"server":{"protocol":"language_server","workspace":false,"arguments":[{"literal":"counted"}]}}})).unwrap();
    let token = Handle::fresh().unwrap();
    let binding = Binding::new(
        "sample.language",
        token,
        "server",
        &definition.tools["server"],
    )
    .unwrap();
    let images = state.tool_store().unwrap();
    let image = images
        .import(
            Source::select(executable).unwrap(),
            binding.clone(),
            &AtomicBool::new(false),
        )
        .unwrap();
    let approval = |image| Approval {
        command: "sample.language".into(),
        token,
        scope: None,
        endpoints: vec![],
        tools: BTreeMap::from([("server".into(), image)]),
    };
    state.approve(approval(image), &definition).unwrap();
    let resolved = state
        .resolve("sample.language", token, &definition)
        .unwrap();
    let registry = Arc::new(hedit_broker::lsp::Registry::default());
    let lifetime = hedit_core::lifetime::Lifetime::default();
    registry.bind_lifetime(&lifetime.lease()).unwrap();
    let package: sdk::Input = serde_json::from_value(json!({
        "sdk_version":1,
        "source":r#"local calls=0
return function(ctx)
  calls=calls+1
  assert(calls==1 and io==nil and os==nil)
  if ctx.broker.response==nil then
    return {intent={language_server={profile="server",method="hover",position=0}}}
  end
  assert(ctx.broker.response.language_server.result.hover.text == ctx.document.text .. " #" .. ctx.arguments.expected)
  return {intent={finish={}}}
end"#,
        "snapshot":{"document":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","revision":"42","language":"json","text":"café / 東京 / 😀","selections":[{"anchor":0,"head":0}],"crlf":false},
        "settings":{},"arguments":{"expected":2}
    })).unwrap();
    let configuration = || Config {
        services: registry.clone(),
        scope_epoch: Some(vec![1; 32]),
        document_relative: None,
        source_token: Some(token),
        tools: resolved.tools.clone(),
        owner: Some(Owner {
            command: "sample.language".into(),
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
    let mut no_epoch = configuration();
    no_epoch.scope_epoch = None;
    assert_eq!(
        Session::new(no_epoch).err().unwrap().kind(),
        io::ErrorKind::PermissionDenied
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    // Two different broker sessions/fresh Lua callbacks share one native peer.
    for expected in [2, 3] {
        let mut config = configuration();
        config
            .package
            .arguments
            .insert("expected".into(), Value::Integer(expected));
        let mut session = Session::new(config).unwrap();
        let reply = hedit_plugin::run_broker_fixture(session.begin_callback().unwrap()).unwrap();
        assert!(matches!(
            runtime.block_on(session.accept(reply, || Ok(()))).unwrap(),
            Effect::Continue
        ));
        let reply = hedit_plugin::run_broker_fixture(session.begin_callback().unwrap()).unwrap();
        assert!(matches!(
            runtime.block_on(session.accept(reply, || Ok(()))).unwrap(),
            Effect::Finished { .. }
        ));
    }
    // Substituting another import with the same recipe/token cannot reuse a
    // cached owner, even if a caller incorrectly supplies the old epoch.
    let replacement = images
        .import(
            Source::select(executable).unwrap(),
            binding.clone(),
            &AtomicBool::new(false),
        )
        .unwrap();
    state.approve(approval(replacement), &definition).unwrap();
    let mut changed = configuration();
    changed.tools = state
        .resolve("sample.language", token, &definition)
        .unwrap()
        .tools;
    let mut session = Session::new(changed).unwrap();
    let reply = hedit_plugin::run_broker_fixture(session.begin_callback().unwrap()).unwrap();
    assert_eq!(
        runtime
            .block_on(session.accept(reply, || Ok(())))
            .err()
            .unwrap()
            .kind(),
        io::ErrorKind::PermissionDenied
    );
    assert!(registry.cancellation().is_cancelled());
    lifetime.close();
    drop(session);
    let fresh = Arc::new(hedit_broker::lsp::Registry::default());
    let document = hedit_core::lifetime::Lifetime::default();
    fresh.bind_lifetime(&document.lease()).unwrap();
    let mut config = configuration();
    config.services = fresh.clone();
    config.tools = state
        .resolve("sample.language", token, &definition)
        .unwrap()
        .tools;
    let mut session = Session::new(config).unwrap();
    let reply = hedit_plugin::run_broker_fixture(session.begin_callback().unwrap()).unwrap();
    assert!(matches!(
        runtime.block_on(session.accept(reply, || Ok(()))).unwrap(),
        Effect::Continue
    ));
    document.close();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !fresh.settled() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(fresh.settled());
    assert!(session.begin_callback().is_err());
    drop(session);
    images.remove(image, &binding).unwrap();
    images.remove(replacement, &binding).unwrap();
    println!("Source-bound native LSP: fresh Lua callbacks, cross-session reuse, epoch requirement and cached image substitution denial passed.");
}
