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
