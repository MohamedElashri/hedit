//! The shipped image runs inside production confinement, never a pipe-only mock.
use hedit_broker::{
    cancel::Cancel,
    declaration::ToolDefinition,
    lsp::{Method, Owner},
    state::State,
    tool::{Binding, Projection, Source},
    Handle,
};
use hedit_plugin::{sdk, service::ResultSet};
use serde_json::json;
use std::{
    path::Path,
    sync::{atomic::AtomicBool, Arc},
    time::{Duration, Instant},
};
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(10)
}
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if let Some(ok) = hedit_plugin::tool::run_if_requested(&args) {
        std::process::exit(i32::from(!ok));
    }
    assert!(args
        .iter()
        .all(|arg| arg == "--nocapture" || arg == "--test-threads=1"));
    let directory = tempfile::tempdir().unwrap();
    let state = State::open(directory.path(), []).unwrap();
    let images = state.tool_store().unwrap();
    let runs = state.tool_runs().unwrap();
    let binary = Path::new(env!("CARGO_BIN_EXE_hedit-json-tools"));
    let cancel = AtomicBool::new(false);
    let binding = |protocol: &str, profile: &str, argument: &str| {
        let definition: ToolDefinition = serde_json::from_value(
            json!({"protocol":protocol,"workspace":false,"arguments":[{"literal":argument}]}),
        )
        .unwrap();
        Binding::new(
            "reference.run",
            Handle::fresh().unwrap(),
            profile,
            &definition,
        )
        .unwrap()
    };
    let filter = binding("filter", "format", "--format");
    let image = images
        .import(Source::select(binary).unwrap(), filter.clone(), &cancel)
        .unwrap();
    let text = "{\"東京\":\"café😀\",\"active\":true}";
    let run = runs
        .prepare(
            images.resolve(image, &filter, &cancel).unwrap(),
            Projection::empty(),
            vec!["--format".into()],
            &cancel,
        )
        .unwrap();
    assert_eq!(
        run.filter(text.as_bytes(), &cancel, Duration::from_secs(10))
            .unwrap()
            .stdout,
        hedit_json_tools::format(text.as_bytes())
            .unwrap()
            .as_bytes()
    );
    let run = runs
        .prepare(
            images.resolve(image, &filter, &cancel).unwrap(),
            Projection::empty(),
            vec!["--format".into()],
            &cancel,
        )
        .unwrap();
    assert!(run
        .filter(br#"{"a":1,"a":2}"#, &cancel, Duration::from_secs(10))
        .is_err());
    images.remove(image, &filter).unwrap();
    let server = binding("language_server", "server", "--project-lsp");
    let image = images
        .import(Source::select(binary).unwrap(), server.clone(), &cancel)
        .unwrap();
    let mut snapshot = sdk::Snapshot {
        document: "a".repeat(32),
        revision: "1".into(),
        language: "json".into(),
        text: text.into(),
        selections: vec![sdk::Selection { anchor: 0, head: 0 }],
        crlf: false,
    };
    let mut projection = Projection::empty();
    let relative = projection.document(&snapshot, None, None).unwrap();
    let run = runs
        .prepare(
            images.resolve(image, &server, &cancel).unwrap(),
            projection,
            vec!["--project-lsp".into()],
            &cancel,
        )
        .unwrap();
    let scope = Arc::new(Cancel::default());
    let owner = Owner::start(
        run,
        relative,
        snapshot.clone(),
        false,
        scope.clone(),
        deadline(),
        None,
    )
    .unwrap();
    let request = |snapshot: &sdk::Snapshot, method, edit| {
        owner
            .request(
                snapshot,
                method,
                0,
                edit,
                Arc::new(Cancel::default()),
                deadline(),
            )
            .unwrap()
    };
    for revision in 1..=64 {
        if revision != 1 {
            snapshot.text = format!("{{\"東京\":\"café😀\",\"revision\":{revision}}}");
        }
        snapshot.revision = revision.to_string();
        let ResultSet::Hover(hover) = request(&snapshot, Method::Hover, false) else {
            panic!()
        };
        assert_eq!(
            hover.text,
            "JSON object with 2 properties\nApproved project: 1 top-level JSON files (1 valid)"
        );
        let ResultSet::Diagnostics(diagnostics) = request(&snapshot, Method::Diagnostics, false)
        else {
            panic!()
        };
        assert!(diagnostics.is_empty());
    }
    let ResultSet::Symbols(symbols) = request(&snapshot, Method::Symbols, false) else {
        panic!()
    };
    assert_eq!(symbols.len(), 2);
    assert_eq!(symbols[0].name, "東京");
    let ResultSet::Formatting(transaction) = request(&snapshot, Method::Formatting, true) else {
        panic!()
    };
    assert_eq!(
        transaction.prepare(&snapshot).unwrap().text,
        hedit_json_tools::format(snapshot.text.as_bytes()).unwrap()
    );
    snapshot.revision = "65".into();
    snapshot.text = "{\"incomplete\":".into();
    let ResultSet::Diagnostics(diagnostics) = request(&snapshot, Method::Diagnostics, false) else {
        panic!()
    };
    assert_eq!(diagnostics.len(), 1);
    let ResultSet::Formatting(transaction) = request(&snapshot, Method::Formatting, true) else {
        panic!()
    };
    assert_eq!(transaction.prepare(&snapshot).unwrap().text, snapshot.text);
    snapshot.revision = "66".into();
    snapshot.text = "[]".into();
    let ResultSet::Hover(hover) = request(&snapshot, Method::Hover, false) else {
        panic!()
    };
    assert_eq!(
        hover.text,
        "JSON array with 0 elements\nApproved project: 1 top-level JSON files (1 valid)"
    );
    assert!(owner.running());
    scope.cancel();
    let until = Instant::now() + Duration::from_secs(3);
    while !owner.settled() {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(owner);
    images.remove(image, &server).unwrap();
    let names: Vec<_> = std::fs::read_dir(directory.path().join("extensions-broker/tool-runs"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["index.lock"]);
    println!("Native JSON reference: confined formatting, strict duplicates, 64 live revisions, typed diagnostics/hover/symbols/formatting, invalid-edit recovery and cleanup passed.");
}
