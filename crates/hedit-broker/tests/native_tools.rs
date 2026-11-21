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
    if let Some(success) = hedit_plugin::tool::run_if_requested(&args) {
        std::process::exit(if success { 0 } else { 1 });
    }
    assert!(args.is_empty());
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
    let owner = Binding::new(
        "sample.format",
        Handle::fresh().unwrap(),
        "formatter",
        &definition,
    )
    .unwrap();
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
    images.remove(id, &owner).unwrap();
    println!("Imported native image: exact Unicode bytes, private run ownership, cancellation and cleanup passed.");
}
fn empty(path: &Path) {
    let names: Vec<_> = std::fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["index.lock"]);
}
