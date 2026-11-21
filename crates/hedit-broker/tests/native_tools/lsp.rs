//! A native protocol peer runs inside the production sandbox, never in the host.
use hedit_broker::{
    lsp::{Connection, Method},
    state::State,
    tool::{Binding, Projection, Source},
    Handle,
};
use hedit_core::language_server::wire::{self, Limits};
use serde_json::json;
use std::{
    io::{self, BufRead, Read, Write},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

pub fn peer(mode: &str) {
    let mut input = io::BufReader::new(io::stdin().lock());
    let mut output = io::stdout().lock();
    let mut requests = 0;
    loop {
        let mut header = Vec::new();
        loop {
            if input.read_until(b'\n', &mut header).unwrap() == 0 {
                return;
            }
            assert!(header.len() <= 8192);
            if header.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let length = wire::content_length(&header, Limits::SCOPED).unwrap();
        let mut body = vec![0; length];
        input.read_exact(&mut body).unwrap();
        let value = wire::parse_json(&body, Limits::SCOPED).unwrap();
        if value.get("method").is_none() {
            assert_eq!(value["error"]["code"], -32601);
            continue;
        }
        let Some(id) = value.get("id") else {
            continue;
        };
        requests += 1;
        let result = if value["method"] == "initialize" {
            json!({"capabilities":{"hoverProvider":true,"positionEncoding":"utf-16"}})
        } else {
            match mode {
                "stall" => {
                    std::thread::sleep(Duration::from_secs(30));
                    return;
                }
                "duplicate" => {
                    let body =
                        format!(r#"{{"jsonrpc":"2.0","id":{id},"result":null,"result":{{}}}}"#);
                    write!(output, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
                    output.flush().unwrap();
                    continue;
                }
                "truncated" => {
                    output.write_all(b"Content-Length: 100\r\n\r\n{}").unwrap();
                    output.flush().unwrap();
                    return;
                }
                "flood" => {
                    for _ in 0..100 {
                        output.write_all(&wire::encode(&json!({"jsonrpc":"2.0","method":"window/logMessage","params":{"message":"flood"}}), Limits::SCOPED).unwrap()).unwrap();
                    }
                }
                "server-request" => {
                    output.write_all(&wire::encode(&json!({"jsonrpc":"2.0","id":"server","method":"workspace/applyEdit","params":{"edit":{"changes":{"file:///unissued/private.txt":[]}}}}), Limits::SCOPED).unwrap()).unwrap();
                }
                _ => {}
            }
            json!({"contents":"café / 東京 / 😀", "requests":requests})
        };
        let response = json!({"jsonrpc":"2.0","id":if mode == "foreign" && requests > 1 { json!(999) } else { id.clone() },"result":result});
        let bytes = wire::encode(&response, Limits::SCOPED).unwrap();
        // Real fragmented writes exercise the OS transport, not just the codec.
        for chunk in bytes.chunks(3) {
            output.write_all(chunk).unwrap();
            output.flush().unwrap();
        }
    }
}
pub fn verify(state: &State) {
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
            Source::select(&std::env::current_exe().unwrap()).unwrap(),
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
                vec!["--lsp-peer".into(), mode.into()],
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
