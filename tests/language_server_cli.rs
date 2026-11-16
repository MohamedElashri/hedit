//! Exercise the real headless entry point: these commands must never launch a GUI.
use hedit::language_server::manager::{Launch, Manager, Registration};
use serde_json::Value;
use std::{
    path::Path,
    process::{Command, Output},
};

fn run(store: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hedit"))
        .args(["--lsp-dir"])
        .arg(store)
        .arg("--lsp")
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn language_server_cli_lists_catalog_without_creating_a_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("servers");
    let output = run(&store, &["list"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["installed"], serde_json::json!([]));
    assert!(value["catalog"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["id"] == "rust-analyzer"));
    assert!(!store.exists());
}
#[test]
fn language_server_cli_removes_registration_and_preserves_external_executable() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("servers");
    let executable = dir.path().canonicalize().unwrap().join("external-server");
    std::fs::write(&executable, "user-owned executable").unwrap();
    Manager::new(store.clone())
        .configure(
            "external",
            Registration {
                languages: vec!["rust".into()],
                external: Some(Launch {
                    executable: executable.clone(),
                    arguments: vec![],
                }),
                ..Registration::default()
            },
        )
        .unwrap();
    let output = run(&store, &["list"]);
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["installed"][0]["id"], "external");
    let output = run(&store, &["remove", "external"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(Manager::new(store).list().unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(executable).unwrap(),
        "user-owned executable"
    );
}
#[test]
fn language_server_cli_rejects_unsupported_installers_and_ambiguous_editor_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("servers");
    for id in ["unknown-server", "gopls"] {
        let output = run(&store, &["install", id]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("no managed installer"));
        assert!(!store.exists());
    }
    let output = run(&store, &["list", "file.rs"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be combined"));
}
