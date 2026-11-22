use super::*;
use manager::{Launch, Manager, Registration};
use serde_json::{json, Value};
use std::{
    io::Cursor,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

pub(super) fn python() -> PathBuf {
    let name = if cfg!(windows) { "python" } else { "python3" };
    let out = std::process::Command::new(name)
        .args(["-c", "import sys; print(sys.executable)"])
        .output()
        .expect("Python 3 is required by the test suite");
    assert!(out.status.success());
    PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
}
pub(super) fn launch(extra: &[&str]) -> Launch {
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/fake_language_server.py");
    Launch {
        executable: python(),
        arguments: std::iter::once(script.to_string_lossy().into_owned())
            .chain(extra.iter().map(|s| s.to_string()))
            .collect(),
    }
}
fn registration(extra: &[&str]) -> Registration {
    Registration {
        languages: vec!["rust".into()],
        external: Some(launch(extra)),
        configuration: json!({"fixture":{"enabled":true}}),
        ..Registration::default()
    }
}
fn client(directory: &std::path::Path, extra: &[&str]) -> client::Client {
    let manager = Manager::new(directory.join("servers"));
    manager.configure("fixture", registration(extra)).unwrap();
    let (launch, lease) = manager.launch("fixture").unwrap();
    client::Client::start(
        launch,
        lease,
        directory,
        json!({"fixture":{"enabled":true}}),
        &AtomicBool::new(false),
    )
    .unwrap()
}
fn wait(service: &Service, ticket: u64) -> (Result<Value, String>, Vec<Event>) {
    let start = Instant::now();
    let mut events = vec![];
    loop {
        let mut completed = None;
        for e in service.poll() {
            match &e {
                Event::Result { ticket: t, result } if *t == ticket => {
                    completed = Some(result.clone())
                }
                Event::Response { ticket: t, result } if *t == ticket => {
                    completed = Some(
                        result
                            .clone()
                            .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string())),
                    )
                }
                _ => events.push(e),
            }
        }
        if let Some(result) = completed {
            return (result, events);
        }
        assert!(
            start.elapsed() < Duration::from_secs(8),
            "worker timed out: {events:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn unicode_positions_roundtrip_for_every_scalar_and_encoding() {
    for text in ["", "abc", "a😀éאב\r\nβ\n", "عربي\n\n😀終", "\n"] {
        for encoding in [
            text::Encoding::Utf8,
            text::Encoding::Utf16,
            text::Encoding::Utf32,
        ] {
            for scalar in 0..=text.chars().count() {
                let byte = text
                    .char_indices()
                    .nth(scalar)
                    .map_or(text.len(), |(i, _)| i);
                // CRLF's interior is not a valid LSP content position.
                if text[byte..].starts_with('\n') && byte > 0 && text.as_bytes()[byte - 1] == b'\r'
                {
                    continue;
                }
                let pos = encoding.position(text, scalar).unwrap();
                assert_eq!(
                    encoding.byte(text, pos).unwrap(),
                    byte,
                    "{text:?} {encoding:?} {scalar}"
                );
            }
        }
    }
}
#[test]
fn unicode_positions_reject_surrogates_split_bytes_and_bad_lines() {
    use text::{Encoding::*, Position};
    assert!(Utf16
        .byte(
            "😀",
            Position {
                line: 0,
                character: 1
            }
        )
        .is_err());
    assert!(Utf8
        .byte(
            "é",
            Position {
                line: 0,
                character: 1
            }
        )
        .is_err());
    assert!(Utf32
        .byte(
            "a",
            Position {
                line: 1,
                character: 0
            }
        )
        .is_err());
    assert!(Utf16.position("a", 2).is_err());
    assert!(text::Encoding::parse(Some("utf-7")).is_err());
}
#[test]
fn file_uris_roundtrip_spaces_unicode_and_reject_other_schemes() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a #😀.rs");
    assert_eq!(text::path(&text::uri(&p).unwrap()).unwrap(), p);
    for value in [
        "https://example.test/file",
        "untitled:a",
        "file:///tmp/a?x=1",
        "file:///tmp/a#x",
    ] {
        assert!(text::path(value).is_err());
    }
}
#[test]
fn file_uri_paths_match_canonical_document_paths_without_reading_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let canonical = dir.path().canonicalize().unwrap().join("unsaved #😀.rs");
    let from_uri = text::path(&text::uri(&canonical).unwrap()).unwrap();
    assert!(text::same_path(&canonical, &from_uri));
    assert!(!text::same_path(
        &canonical,
        &from_uri.with_extension("cpp")
    ));
}
#[test]
fn edit_batches_validate_before_mutating_and_preserve_equal_insert_order() {
    use text::{Encoding, Position, Range, TextEdit};
    let p = |n| Position {
        line: 0,
        character: n,
    };
    let edits = vec![
        TextEdit {
            range: Range {
                start: p(1),
                end: p(3),
            },
            new_text: "X".into(),
        },
        TextEdit {
            range: Range {
                start: p(0),
                end: p(0),
            },
            new_text: "1".into(),
        },
        TextEdit {
            range: Range {
                start: p(0),
                end: p(0),
            },
            new_text: "2".into(),
        },
    ];
    assert_eq!(
        text::apply("a😀b", &edits, Encoding::Utf16).unwrap(),
        "12aXb"
    );
    let mut bad = edits.clone();
    bad.push(TextEdit {
        range: Range {
            start: p(1),
            end: p(4),
        },
        new_text: String::new(),
    });
    assert!(text::apply("a😀b", &bad, Encoding::Utf16).is_err());
}
#[test]
fn bounded_frames_accept_fragmented_utf8_and_multiple_messages() {
    let data = json!({"jsonrpc":"2.0","method":"fixture","params":{"text":"😀ع"}});
    let bytes = serde_json::to_vec(&data).unwrap();
    let wire = format!(
        "content-length: {}\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n",
        bytes.len()
    )
    .into_bytes();
    let mut all = wire.clone();
    all.extend(&bytes);
    all.extend(&wire);
    all.extend(bytes);
    let mut reader = Cursor::new(all);
    assert_eq!(
        client::read_message(&mut reader).unwrap(),
        Some(data.clone())
    );
    assert_eq!(client::read_message(&mut reader).unwrap(), Some(data));
    assert!(client::read_message(&mut reader).unwrap().is_none());
}
#[test]
fn malformed_frames_fail_closed() {
    for wire in [
        b"Content-Length: 999999999\r\n\r\n".as_slice(),
        b"Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}",
        b"Content-Length: 2\n\n{}",
        b"Content-Length: 10\r\n\r\n{}",
        b"Other: 1\r\n\r\n{}",
        b"Content-Length: 2\r\n\r\n{}",
        b"Content-Length: 0\r\n\r\n",
    ] {
        assert!(
            client::read_message(&mut Cursor::new(wire)).is_err(),
            "{wire:?}"
        );
    }
    assert!(client::read_message(&mut Cursor::new(vec![b'x'; 8193])).is_err());
}
#[test]
fn session_negotiates_all_encodings_and_handles_server_requests() {
    let dir = tempfile::tempdir().unwrap();
    for (name, encoding) in [
        ("utf-8", text::Encoding::Utf8),
        ("utf-16", text::Encoding::Utf16),
        ("utf-32", text::Encoding::Utf32),
    ] {
        let record = dir.path().join(format!("{name}.jsonl"));
        let record = record.to_str().unwrap();
        let mut client = client(
            dir.path(),
            &["--encoding", name, "--fragment", "--record", record],
        );
        assert_eq!(client.encoding, encoding);
        assert!(client.supported("completionProvider"));
        let response = client
            .request(
                "textDocument/hover",
                json!({}),
                &AtomicBool::new(false),
                Duration::from_secs(2),
            )
            .unwrap();
        assert_eq!(response["contents"]["value"], "Fixture hover information");
        client.stop();
        let records = std::fs::read_to_string(record).unwrap();
        assert!(records.contains("configuration"));
        assert!(records.contains("enabled"));
        assert!(records.contains("shutdown"));
    }
}
#[test]
fn unresponsive_requests_timeout_and_send_cancellation() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = client(dir.path(), &[]);
    let start = Instant::now();
    assert!(client
        .request(
            "test/hang",
            json!({}),
            &AtomicBool::new(false),
            Duration::from_millis(80)
        )
        .unwrap_err()
        .contains("timed out"));
    assert!(start.elapsed() < Duration::from_secs(2));
    client.stop();
}
#[test]
fn initialize_can_be_cancelled_and_releases_the_installation_lease() {
    let dir = tempfile::tempdir().unwrap();
    let manager = Manager::new(dir.path().join("servers"));
    manager
        .configure("fixture", registration(&["--hang-initialize"]))
        .unwrap();
    let (launch, lease) = manager.launch("fixture").unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let stop = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        stop.store(true, Ordering::Relaxed);
    });
    let start = Instant::now();
    assert!(client::Client::start(launch, lease, dir.path(), json!({}), &cancel).is_err());
    assert!(start.elapsed() < Duration::from_secs(2));
    manager.remove("fixture").unwrap();
}
#[test]
fn stderr_flood_is_drained_and_retention_is_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = client(dir.path(), &[]);
    assert_eq!(
        client
            .request(
                "test/log-flood",
                json!({}),
                &AtomicBool::new(false),
                Duration::from_secs(3)
            )
            .unwrap(),
        true
    );
    assert!(
        client
            .logs
            .lock()
            .unwrap()
            .iter()
            .map(String::len)
            .sum::<usize>()
            <= 32 * 2048
    );
    client.stop();
}
#[test]
fn manager_preserves_external_files_and_refuses_mutation_while_running() {
    let dir = tempfile::tempdir().unwrap();
    let manager = Manager::new(dir.path().join("servers"));
    let registration = registration(&[]);
    let executable = registration.external.as_ref().unwrap().executable.clone();
    manager.configure("fixture", registration.clone()).unwrap();
    let (_, lease) = manager.launch("fixture").unwrap();
    assert!(manager.remove("fixture").is_err());
    assert!(manager.configure("fixture", registration).is_err());
    drop(lease);
    manager.remove("fixture").unwrap();
    assert!(executable.is_file());
    assert!(manager.list().unwrap().is_empty());
}
#[test]
fn manager_rejects_invalid_ids_configurations_and_forged_receipts() {
    let dir = tempfile::tempdir().unwrap();
    let manager = Manager::new(dir.path().join("servers"));
    for id in ["../escape", "", "a/b", "a\\b", "a.b"] {
        assert!(manager.configure(id, registration(&[])).is_err());
    }
    let mut r = registration(&[]);
    r.configuration = json!([]);
    assert!(manager.configure("fixture", r).is_err());
    let mut r = registration(&[]);
    r.external.as_mut().unwrap().executable = "relative".into();
    assert!(manager.configure("fixture", r).is_err());
    let mut r = registration(&[]);
    r.managed = Some(manager::Receipt {
        version: "fake".into(),
        digest: "fake".into(),
        source: "fake".into(),
        directory: "install-fake".into(),
        executable: "server".into(),
        arguments: vec![],
    });
    manager.configure("fixture", r).unwrap();
    assert!(manager.read("fixture").unwrap().unwrap().managed.is_none());
}
#[test]
fn corrupt_registration_is_reported_and_never_overwritten_implicitly() {
    let dir = tempfile::tempdir().unwrap();
    let manager = Manager::new(dir.path().join("servers"));
    manager.configure("fixture", registration(&[])).unwrap();
    let file = manager.root().join("fixture/server.json");
    std::fs::write(&file, b"{broken").unwrap();
    assert!(manager.list().is_err());
    assert!(manager.configure("fixture", registration(&[])).is_err());
    assert_eq!(std::fs::read_to_string(file).unwrap(), "{broken");
}
#[test]
fn workspace_override_does_not_change_other_projects() {
    let mut r = registration(&[]);
    r.workspaces.insert("/project-a".into(), false);
    assert!(!r.enabled_at(std::path::Path::new("/project-a")));
    assert!(r.enabled_at(std::path::Path::new("/project-b")));
}
#[test]
fn service_drives_open_change_save_features_and_filters_old_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let record = dir.path().join("record.jsonl");
    let manager = Manager::new(dir.path().join("servers"));
    manager
        .configure(
            "fixture",
            registration(&["--record", record.to_str().unwrap()]),
        )
        .unwrap();
    std::fs::write(dir.path().join("target.rs"), "😀 target\n").unwrap();
    let service = Service::start(manager.root().to_owned(), Arc::new(|| {}));
    let mut d = Document {
        path: dir.path().join("main.rs"),
        root: dir.path().to_owned(),
        language: "rust".into(),
        revision: 1,
        text: "fo".into(),
        dirty: true,
    };
    service
        .submit(
            1,
            Operation::Sync {
                documents: vec![d.clone()],
            },
        )
        .unwrap();
    wait(&service, 1).0.unwrap();
    service
        .submit(
            2,
            Operation::Request {
                path: d.path.clone(),
                revision: 1,
                scalar: 2,
                feature: Feature::Completion,
                tab_size: 4,
                spaces: true,
            },
        )
        .unwrap();
    let (result, _) = wait(&service, 2);
    let reply: Reply = serde_json::from_value(result.unwrap()).unwrap();
    assert_eq!(reply.value["items"][0]["label"], "format_value");
    d.revision = 2;
    d.text = "foo😀\r\n".into();
    d.dirty = false;
    service
        .submit(
            3,
            Operation::Sync {
                documents: vec![d.clone()],
            },
        )
        .unwrap();
    wait(&service, 3).0.unwrap();
    service
        .submit(
            4,
            Operation::Request {
                path: d.path.clone(),
                revision: 1,
                scalar: 2,
                feature: Feature::Hover,
                tab_size: 4,
                spaces: true,
            },
        )
        .unwrap();
    assert!(wait(&service, 4).0.unwrap_err().contains("changed"));
    service
        .submit(
            5,
            Operation::Request {
                path: d.path.clone(),
                revision: 2,
                scalar: 2,
                feature: Feature::Hover,
                tab_size: 4,
                spaces: true,
            },
        )
        .unwrap();
    let (reply, events) = wait(&service, 5);
    let reply: Reply = serde_json::from_value(reply.unwrap()).unwrap();
    assert_eq!(
        reply.value["contents"]["value"],
        "Fixture hover information"
    );
    for e in events {
        if let Event::Diagnostics { items, .. } = e {
            assert!(items.iter().all(|d| d["message"] != "obsolete diagnostic"));
        }
    }
    service
        .submit(
            6,
            Operation::Stop {
                id: "fixture".into(),
            },
        )
        .unwrap();
    wait(&service, 6).0.unwrap();
    let records: Vec<Value> = std::fs::read_to_string(record)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(records
        .iter()
        .any(|v| v["method"] == "textDocument/didOpen"));
    assert!(records
        .iter()
        .any(|v| v["method"] == "textDocument/didChange"
            && v["params"]["contentChanges"][0]["range"]["end"]["character"] == 2));
    assert!(records
        .iter()
        .any(|v| v["method"] == "textDocument/didSave" && v["params"]["text"] == d.text));
}
#[test]
fn reopening_a_document_resets_its_server_lifecycle_before_reusing_versions() {
    let dir = tempfile::tempdir().unwrap();
    let record = dir.path().join("record.jsonl");
    let manager = Manager::new(dir.path().join("servers"));
    manager
        .configure(
            "fixture",
            registration(&["--record", record.to_str().unwrap()]),
        )
        .unwrap();
    let service = Service::start(manager.root().to_owned(), Arc::new(|| {}));
    for (ticket, revision, source) in [(1, 5, "old"), (2, 0, "reopened"), (3, 0, "replaced")] {
        service
            .submit(
                ticket,
                Operation::Sync {
                    documents: vec![Document {
                        path: dir.path().join("main.rs"),
                        root: dir.path().into(),
                        language: "rust".into(),
                        revision,
                        text: source.into(),
                        dirty: false,
                    }],
                },
            )
            .unwrap();
        wait(&service, ticket).0.unwrap();
    }
    service
        .submit(
            4,
            Operation::Stop {
                id: "fixture".into(),
            },
        )
        .unwrap();
    wait(&service, 4).0.unwrap();
    let records: Vec<Value> = std::fs::read_to_string(record)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .filter(|v: &Value| {
            matches!(
                v["method"].as_str(),
                Some("textDocument/didOpen" | "textDocument/didClose" | "textDocument/didChange")
            )
        })
        .collect();
    assert_eq!(
        records
            .iter()
            .map(|v| v["method"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "textDocument/didOpen",
            "textDocument/didClose",
            "textDocument/didOpen",
            "textDocument/didClose",
            "textDocument/didOpen"
        ]
    );
    for (message, source) in records
        .iter()
        .step_by(2)
        .zip(["old", "reopened", "replaced"])
    {
        assert_eq!(message["params"]["textDocument"]["text"], source);
    }
}
#[test]
fn removing_one_document_clears_its_diagnostics_without_stopping_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let manager = Manager::new(dir.path().join("servers"));
    manager.configure("fixture", registration(&[])).unwrap();
    let service = Service::start(manager.root().to_owned(), Arc::new(|| {}));
    let document = |name: &str| Document {
        path: dir.path().join(name),
        root: dir.path().to_owned(),
        language: "rust".into(),
        revision: 1,
        text: "fo".into(),
        dirty: true,
    };
    let mut first = document("first.rs");
    let second = document("second.rs");
    service
        .submit(
            1,
            Operation::Sync {
                documents: vec![first.clone(), second.clone()],
            },
        )
        .unwrap();
    wait(&service, 1).0.unwrap();
    // Round-trip a request so the server has consumed both didOpen messages.
    service
        .submit(
            2,
            Operation::Request {
                path: second.path.clone(),
                revision: 1,
                scalar: 0,
                feature: Feature::Hover,
                tab_size: 4,
                spaces: true,
            },
        )
        .unwrap();
    wait(&service, 2).0.unwrap();
    // Change the syntax without changing the text revision. The second file
    // keeps the same server/project session alive.
    first.language = "toml".into();
    service
        .submit(
            3,
            Operation::Sync {
                documents: vec![first.clone(), second.clone()],
            },
        )
        .unwrap();
    let (result, events) = wait(&service, 3);
    result.unwrap();
    assert!(
        events.iter().any(|e| matches!(e, Event::Diagnostics {
        server, path, revision: 1, items, ..
    } if server == "fixture" && *path == first.path && items.is_empty())),
        "no diagnostic invalidation: {events:?}"
    );
    assert!(!events
        .iter()
        .any(|e| matches!(e, Event::Status(s) if s.state == "Stopped")));
    service
        .submit(
            4,
            Operation::Request {
                path: second.path,
                revision: 1,
                scalar: 0,
                feature: Feature::Hover,
                tab_size: 4,
                spaces: true,
            },
        )
        .unwrap();
    wait(&service, 4).0.unwrap();
}
#[test]
fn crashes_do_not_loop_and_restart_is_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let record = dir.path().join("record.jsonl");
    let manager = Manager::new(dir.path().join("servers"));
    manager
        .configure(
            "fixture",
            registration(&["--crash-on-open", "--record", record.to_str().unwrap()]),
        )
        .unwrap();
    let service = Service::start(manager.root().to_owned(), Arc::new(|| {}));
    let d = Document {
        path: dir.path().join("main.rs"),
        root: dir.path().into(),
        language: "rust".into(),
        revision: 1,
        text: "fo".into(),
        dirty: true,
    };
    service
        .submit(
            1,
            Operation::Sync {
                documents: vec![d.clone()],
            },
        )
        .unwrap();
    wait(&service, 1).0.unwrap();
    std::thread::sleep(Duration::from_millis(100));
    for ticket in 2..5 {
        service
            .submit(
                ticket,
                Operation::Sync {
                    documents: vec![d.clone()],
                },
            )
            .unwrap();
        wait(&service, ticket).0.unwrap();
    }
    let records = std::fs::read_to_string(record).unwrap();
    assert_eq!(
        records
            .lines()
            .filter(|line| line.contains("\"method\": \"initialize\""))
            .count(),
        1
    );
}
#[test]
fn service_rejects_oversized_snapshots_and_duplicate_documents() {
    let dir = tempfile::tempdir().unwrap();
    let service = Service::start(dir.path().join("servers"), Arc::new(|| {}));
    let mut d = Document {
        path: dir.path().join("main.rs"),
        root: dir.path().into(),
        language: "rust".into(),
        revision: 1,
        text: "a".into(),
        dirty: true,
    };
    assert!(service
        .submit(
            1,
            Operation::Sync {
                documents: vec![d.clone(), d.clone()]
            }
        )
        .is_err());
    d.text = "a".repeat(MAX_DOCUMENT + 1);
    assert!(service
        .submit(2, Operation::Sync { documents: vec![d] })
        .is_err());
}

#[test]
#[ignore = "downloads verified upstream servers; requires network, curl, and a Rust/C++ toolchain"]
fn live_managed_server_install_initialize_hover_and_remove() {
    for id in ["rust-analyzer", "clangd"] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let manager = Manager::new(root.join("servers"));
        manager
            .install(id, &AtomicBool::new(false), |s| eprintln!("{id}: {s}"))
            .unwrap();
        let (launch, lease) = manager.launch(id).unwrap();
        let version = std::process::Command::new(&launch.executable)
            .arg("--version")
            .output()
            .unwrap();
        assert!(
            version.status.success(),
            "{}",
            String::from_utf8_lossy(&version.stderr)
        );
        let (file, language, source) = if id == "rust-analyzer" {
            std::fs::create_dir(root.join("src")).unwrap();
            std::fs::write(
                root.join("Cargo.toml"),
                "[package]\nname = \"lsp-smoke\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            )
            .unwrap();
            (
                root.join("src/main.rs"),
                "rust",
                "fn main() { let answer: u32 = 42; println!(\"{answer}\"); }\n",
            )
        } else {
            (
                root.join("main.cpp"),
                "cpp",
                "int main() { int answer = 42; return answer; }\n",
            )
        };
        std::fs::write(&file, source).unwrap();
        let mut client=client::Client::start(launch,lease,&root,json!({"cargo":{"buildScripts":{"enable":false}},"procMacro":{"enable":false},"checkOnSave":false}),&AtomicBool::new(false)).unwrap();
        client.notify("textDocument/didOpen",json!({"textDocument":{"uri":text::uri(&file).unwrap(),"languageId":language,"version":1,"text":source}})).unwrap();
        let scalar = source.find("answer").unwrap() + 2;
        let position = client.encoding.position(source, scalar).unwrap();
        let start = Instant::now();
        loop {
            let response = client.request(
                "textDocument/hover",
                json!({"textDocument":{"uri":text::uri(&file).unwrap()},"position":position}),
                &AtomicBool::new(false),
                Duration::from_secs(10),
            );
            if response.as_ref().is_ok_and(|v| !v.is_null()) {
                eprintln!("{id}: hover {}", response.unwrap());
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "{id}: no hover: {response:?}; {:?}",
                client.logs.lock().unwrap()
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        client.stop();
        drop(client);
        manager.remove(id).unwrap();
        assert!(manager.list().unwrap().is_empty());
        assert!(file.is_file());
    }
}

#[test]
fn dropping_a_workspace_stops_a_server_during_initialize_and_releases_its_lease() {
    let dir = tempfile::tempdir().unwrap();
    let manager = Manager::new(dir.path().join("servers"));
    let record = dir.path().join("record.jsonl");
    manager
        .configure(
            "fixture",
            registration(&["--hang-initialize", "--record", record.to_str().unwrap()]),
        )
        .unwrap();
    let service = Service::start(manager.root().to_owned(), Arc::new(|| {}));
    service
        .submit(
            1,
            Operation::Sync {
                documents: vec![Document {
                    path: dir.path().join("main.rs"),
                    root: dir.path().into(),
                    language: "rust".into(),
                    revision: 1,
                    text: "fo".into(),
                    dirty: true,
                }],
            },
        )
        .unwrap();
    let start = Instant::now();
    while !std::fs::read_to_string(&record).is_ok_and(|s| s.contains("initialize")) {
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(service);
    let start = Instant::now();
    loop {
        if manager.remove("fixture").is_ok() {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "server lease survived workspace destruction"
        );
    }
}

#[test]
fn raw_server_settings_are_returned_for_the_registered_configuration_section() {
    let dir = tempfile::tempdir().unwrap();
    let manager = Manager::new(dir.path().join("servers"));
    let record = dir.path().join("record.jsonl");
    let mut r = registration(&["--record", record.to_str().unwrap()]);
    r.configuration = json!({"enabled":true});
    manager.configure("fixture", r).unwrap();
    let service = Service::start(manager.root().to_owned(), Arc::new(|| {}));
    service
        .submit(
            1,
            Operation::Sync {
                documents: vec![Document {
                    path: dir.path().join("main.rs"),
                    root: dir.path().into(),
                    language: "rust".into(),
                    revision: 1,
                    text: "fo".into(),
                    dirty: true,
                }],
            },
        )
        .unwrap();
    wait(&service, 1).0.unwrap();
    service
        .submit(
            2,
            Operation::Stop {
                id: "fixture".into(),
            },
        )
        .unwrap();
    wait(&service, 2).0.unwrap();
    let records: Vec<Value> = std::fs::read_to_string(record)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert!(records
        .iter()
        .any(|v| v["id"] == "configuration" && v["result"][0]["enabled"] == true));
}

#[test]
fn companion_servers_route_by_feature_and_priority_with_independent_launch_settings() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let project = root.join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("project.marker"), "").unwrap();
    let manager = Manager::new(root.join("servers"));
    let mut registrations = vec![];
    for (id, priority) in [("a", 100), ("b", 0)] {
        let record = root.join(format!("{id}.jsonl"));
        let mut r = registration(&["--record", record.to_str().unwrap(), "--label", id]);
        r.priority = priority;
        r.root_markers = vec!["project.marker".into()];
        r.environment
            .insert("HEDIT_LSP_TEST_FLAG".into(), format!("env-{id}"));
        r.initialization_options = Some(json!({"initializeOnly": id}));
        r.configuration = json!({"fixture":{"configurationOnly": id}});
        if id == "a" {
            r.features.formatting = false;
            r.features.diagnostics = false;
        }
        manager.configure(id, r.clone()).unwrap();
        registrations.push(r);
    }
    let service = Service::start(manager.root().to_owned(), Arc::new(|| {}));
    let document = Document {
        path: project.join("src/main.rs"),
        root: root.clone(),
        language: "rust".into(),
        revision: 1,
        text: "fo".into(),
        dirty: true,
    };
    service
        .submit(
            1,
            Operation::Sync {
                documents: vec![document.clone()],
            },
        )
        .unwrap();
    let (_, mut events) = wait(&service, 1);
    let running: std::collections::BTreeSet<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::Status(s) if s.state == "Running" => Some(s.id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(running, ["a", "b"].into_iter().collect());
    let request = |feature| Operation::Request {
        path: document.path.clone(),
        revision: 1,
        scalar: 2,
        feature,
        tab_size: 4,
        spaces: true,
    };
    service.submit(2, request(Feature::Hover)).unwrap();
    let (reply, more) = wait(&service, 2);
    events.extend(more);
    assert_eq!(reply.unwrap()["value"]["contents"]["value"], "a");
    service.submit(3, request(Feature::Formatting)).unwrap();
    let (result, more) = wait(&service, 3);
    result.unwrap();
    events.extend(more);
    // Poll until B's diagnostics are observed; A must remain muted.
    let until = Instant::now() + Duration::from_secs(5);
    let mut sources = std::collections::BTreeSet::new();
    loop {
        for event in events.drain(..).chain(service.poll()) {
            if let Event::Diagnostics { server, items, .. } = event {
                assert!(items.iter().all(|d| d["message"] != "obsolete diagnostic"));
                sources.insert(server);
            }
        }
        if sources.contains("b") {
            break;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(sources, ["b".to_owned()].into_iter().collect());
    // Disable the higher-priority feature and fall back to the companion.
    registrations[0].features.hover = false;
    service
        .submit(
            4,
            Operation::Configure {
                id: "a".into(),
                registration: Box::new(registrations[0].clone()),
            },
        )
        .unwrap();
    wait(&service, 4).0.unwrap();
    service.submit(5, request(Feature::Hover)).unwrap();
    assert_eq!(
        wait(&service, 5).0.unwrap()["value"]["contents"]["value"],
        "b"
    );
    // Equal priority is deterministic by ID regardless of HashMap order.
    registrations[0].priority = 0;
    registrations[0].features.hover = true;
    service
        .submit(
            6,
            Operation::Configure {
                id: "a".into(),
                registration: Box::new(registrations[0].clone()),
            },
        )
        .unwrap();
    wait(&service, 6).0.unwrap();
    service.submit(7, request(Feature::Hover)).unwrap();
    assert_eq!(
        wait(&service, 7).0.unwrap()["value"]["contents"]["value"],
        "a"
    );
    for (ticket, id) in [(8, "a"), (9, "b")] {
        service
            .submit(ticket, Operation::Stop { id: id.into() })
            .unwrap();
        wait(&service, ticket).0.unwrap();
    }
    for id in ["a", "b"] {
        let records: Vec<Value> = std::fs::read_to_string(root.join(format!("{id}.jsonl")))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(records
            .iter()
            .any(|v| v["method"] == "fixture/environment"
                && v["params"]["value"] == format!("env-{id}")));
        let initialize = records
            .iter()
            .find(|v| v["method"] == "initialize")
            .unwrap();
        assert_eq!(
            initialize["params"]["rootUri"],
            text::uri(&project).unwrap()
        );
        assert_eq!(
            initialize["params"]["initializationOptions"],
            json!({"initializeOnly":id})
        );
        assert!(records
            .iter()
            .any(|v| v["method"] == "workspace/didChangeConfiguration"
                && v["params"]["settings"]["fixture"]["configurationOnly"] == id));
        assert_eq!(
            records
                .iter()
                .any(|v| v["method"] == "textDocument/formatting"),
            id == "b"
        );
    }
}
#[test]
fn registration_options_are_validated_and_legacy_initialization_remains_compatible() {
    let legacy: Registration =
        serde_json::from_value(json!({"languages":["rust"],"configuration":{"old":true}})).unwrap();
    assert!(legacy.features.completion && legacy.features.diagnostics);
    assert_eq!(legacy.priority, 0);
    assert!(legacy.initialization_options.is_none());
    for (key, value) in [
        ("environment", json!({"BAD=KEY":"x"})),
        ("environment", json!({"":"x"})),
        ("environment", json!({"KEY":"bad\u{0}value"})),
        ("arguments", json!(["bad\u{0}argument"])),
        ("root_markers", json!(["../outside"])),
        ("root_markers", json!(["/absolute"])),
        ("initialization_options", json!([])),
    ] {
        let mut value_r = serde_json::to_value(&legacy).unwrap();
        value_r[key] = value;
        let r: Registration = serde_json::from_value(value_r).unwrap();
        assert!(manager::validate(&r).is_err(), "{key}: {r:?}");
    }
    let dir = tempfile::tempdir().unwrap();
    let manager = Manager::new(dir.path().join("servers"));
    let mut r = registration(&[]);
    r.arguments = Some(launch(&["--label", "override"]).arguments);
    manager.configure("fixture", r).unwrap();
    let (launch, _) = manager.launch("fixture").unwrap();
    assert!(launch
        .arguments
        .ends_with(&["--label".into(), "override".into()]));
}

#[test]
fn session_limit_prioritizes_servers_and_reconciles_new_priorities() {
    let dir = tempfile::tempdir().unwrap();
    let manager = Manager::new(dir.path().join("servers"));
    for id in ["a", "b", "c", "d", "e", "f", "g", "h", "i"] {
        let mut r = registration(&[]);
        r.priority = if id == "a" { 0 } else { 1 };
        manager.configure(id, r).unwrap();
    }
    let service = Service::start(manager.root().to_owned(), Arc::new(|| {}));
    service
        .submit(
            1,
            Operation::Sync {
                documents: vec![Document {
                    path: dir.path().join("main.rs"),
                    root: dir.path().into(),
                    language: "rust".into(),
                    revision: 1,
                    text: "fo".into(),
                    dirty: false,
                }],
            },
        )
        .unwrap();
    let (result, events) = wait(&service, 1);
    result.unwrap();
    let running: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::Status(s) if s.state == "Running" => Some(s.id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(running.len(), 8);
    assert!(!running.contains(&"a"));
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::Status(s) if s.id == "a" && s.state == "Limited")));
    let mut a = manager.read("a").unwrap().unwrap();
    a.priority = 20;
    service
        .submit(
            2,
            Operation::Configure {
                id: "a".into(),
                registration: Box::new(a),
            },
        )
        .unwrap();
    let (result, events) = wait(&service, 2);
    result.unwrap();
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::Status(s) if s.id == "a" && s.state == "Running")));
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::Status(s) if s.id == "i" && s.state == "Stopped")));
    let mut b = manager.read("b").unwrap().unwrap();
    b.features = serde_json::from_value(json!({"completion":false,"hover":false,"definition":false,"references":false,"formatting":false,"diagnostics":false,"rename":false,"quick_fixes":false})).unwrap();
    service
        .submit(
            3,
            Operation::Configure {
                id: "b".into(),
                registration: Box::new(b),
            },
        )
        .unwrap();
    let (result, events) = wait(&service, 3);
    result.unwrap();
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::Status(s) if s.id == "b" && s.state == "Stopped")));
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::Status(s) if s.id == "i" && s.state == "Running")));
}

#[test]
fn session_cap_keeps_focused_companions_before_background_projects() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let manager = Manager::new(root.join("servers"));
    let mut primary = registration(&[]);
    primary.priority = 100;
    manager.configure("fixture", primary).unwrap();
    manager.configure("companion", registration(&[])).unwrap();
    let mut documents = vec![];
    for name in ["z-focused", "a", "b", "c", "d", "e", "f", "g", "h"] {
        let project = root.join(name);
        std::fs::create_dir(&project).unwrap();
        documents.push(Document {
            path: project.join("main.rs"),
            root: project,
            language: "rust".into(),
            revision: 1,
            text: "fo".into(),
            dirty: false,
        });
    }
    let service = Service::start(manager.root().to_owned(), Arc::new(|| {}));
    service.submit(1, Operation::Sync { documents }).unwrap();
    let (result, events) = wait(&service, 1);
    result.unwrap();
    assert!(events.iter().any(|e| matches!(e, Event::Status(s) if s.state == "Running" && s.root == root.join("z-focused"))));
    assert!(events.iter().any(|e| matches!(e, Event::Status(s) if s.id == "companion" && s.state == "Running" && s.root == root.join("z-focused"))), "active file companions must not be displaced by background servers");
    assert!(events.iter().any(
        |e| matches!(e, Event::Status(s) if s.state == "Limited" && s.root == root.join("h"))
    ));
}
#[test]
fn changing_the_session_cap_never_restarts_a_crashed_server_implicitly() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let manager = Manager::new(root.join("servers"));
    let record = root.join("crash.jsonl");
    let mut bad = registration(&["--crash-on-open", "--record", record.to_str().unwrap()]);
    bad.priority = 100;
    manager.configure("bad", bad.clone()).unwrap();
    for i in 0..8 {
        manager
            .configure(&format!("good-{i}"), registration(&[]))
            .unwrap();
    }
    let service = Service::start(manager.root().to_owned(), Arc::new(|| {}));
    let documents = vec![Document {
        path: root.join("main.rs"),
        root: root.clone(),
        language: "rust".into(),
        revision: 1,
        text: "fo".into(),
        dirty: false,
    }];
    service
        .submit(
            1,
            Operation::Sync {
                documents: documents.clone(),
            },
        )
        .unwrap();
    let (_, mut events) = wait(&service, 1);
    let until = Instant::now() + Duration::from_secs(5);
    while !events
        .iter()
        .any(|e| matches!(e, Event::Status(s) if s.id == "bad" && s.state == "Failed"))
    {
        assert!(Instant::now() < until);
        events.extend(service.poll());
        std::thread::sleep(Duration::from_millis(5));
    }
    // Another window may change priorities while the crashed server is stopped.
    for (ticket, priority) in [(2, -100), (3, 100)] {
        bad.priority = priority;
        manager.configure("bad", bad.clone()).unwrap();
        service
            .submit(
                ticket,
                Operation::Sync {
                    documents: documents.clone(),
                },
            )
            .unwrap();
        let (result, events) = wait(&service, ticket);
        result.unwrap();
        assert!(!events
            .iter()
            .any(|e| matches!(e, Event::Status(s) if s.id == "bad" && s.state == "Starting")));
    }
    let records: Vec<Value> = std::fs::read_to_string(record)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(
        records
            .iter()
            .filter(|v| v["method"] == "initialize")
            .count(),
        1
    );
}
