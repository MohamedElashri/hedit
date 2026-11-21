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
    images.remove(image, &binding).unwrap();
    println!("Confined LSP peer: persistent fragmented Unicode requests, server-request denial, foreign/duplicate/truncated/flooded responses, timeout and cleanup passed.");
}
