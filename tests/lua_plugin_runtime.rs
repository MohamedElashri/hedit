#[cfg(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "x86_64", target_arch = "aarch64"),
    target_endian = "little",
    target_pointer_width = "64"
))]
mod supported {
    use std::io::Write;
    use std::process::{Command, Stdio};

    const WORKER: &str = "--hedit-plugin-worker";

    fn worker() -> std::path::PathBuf {
        let binary = std::path::PathBuf::from(env!("CARGO_BIN_EXE_hedit"));
        #[cfg(target_os = "macos")]
        {
            static READY: std::sync::OnceLock<()> = std::sync::OnceLock::new();
            READY.get_or_init(|| {
                let prepared = Command::new("python3")
                    .arg(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/scripts/macos_plugin_helper.py"
                    ))
                    .arg("--editor")
                    .arg(&binary)
                    .output()
                    .unwrap();
                assert!(
                    prepared.status.success(),
                    "{}",
                    String::from_utf8_lossy(&prepared.stderr)
                );
            });
            binary
                .parent()
                .unwrap()
                .join("hedit-plugin.app/Contents/MacOS/hedit")
        }
        #[cfg(not(target_os = "macos"))]
        binary
    }

    fn request(source: &str, selection: &str, language: &str) -> Vec<u8> {
        let payload = serde_json::to_vec(&serde_json::json!({
            "source": source, "selections": [selection], "language": language, "settings": {}
        }))
        .unwrap();
        let mut packet = (u32::MAX - 1).to_le_bytes().to_vec();
        packet.extend((payload.len() as u32).to_le_bytes());
        packet.extend(payload);
        packet
    }

    fn exchange(payload: Vec<u8>) -> Vec<u8> {
        let mut worker = Command::new(worker())
            .arg(WORKER)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = worker.stdin.take().unwrap();
        let writer = std::thread::spawn(move || input.write_all(&payload));
        let reply = worker.wait_with_output().unwrap();
        let write = writer.join().unwrap();
        if let Err(error) = write {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
        }
        assert!(reply.status.success(), "worker status: {:?}", reply.status);
        assert!(reply.stderr.is_empty());
        reply.stdout
    }

    fn call(source: &str, selection: &str) -> Vec<u8> {
        let reply = exchange(request(source, selection, "markdown"));
        if reply.first() != Some(&0) {
            return reply;
        }
        let edits: Vec<String> = serde_json::from_slice(&reply[1..]).unwrap();
        assert_eq!(edits.len(), 1);
        let mut result = vec![0];
        result.extend_from_slice(edits[0].as_bytes());
        result
    }

    #[test]
    fn transforms_unicode_across_multiple_pipe_reads_and_writes() {
        let selection = "hé✨".repeat(10_000);
        let mut expected = vec![0];
        expected.extend_from_slice(selection.repeat(3).as_bytes());
        expected.extend_from_slice(b":markdown");
        assert_eq!(
            call(
                "return function(selection, language)
                    return selection:rep(3) .. ':' .. language
                end",
                &selection,
            ),
            expected,
        );
    }

    #[test]
    fn source_has_only_text_processing_capabilities() {
        let source = "
            assert(io == nil and os == nil and package == nil and debug == nil)
            assert(load == nil and loadfile == nil and dofile == nil and require == nil)
            assert(print == nil and coroutine == nil and collectgarbage == nil)
            assert(getmetatable == nil and setmetatable == nil and string.dump == nil)
            return function(selection) return selection:lower() end
        ";
        assert_eq!(call(source, "HELLO"), b"\0hello");
        assert_eq!(call("\u{1b}Lua", "secret"), [4]);
        assert_eq!(
            call("return function(selection) error(selection) end", "secret"),
            [6]
        );
    }

    #[test]
    fn validates_requests_and_strict_string_results() {
        assert_eq!(exchange(u32::MAX.to_le_bytes().to_vec()), [2]);
        assert_eq!(exchange(vec![0; 13]), [2]);
        assert_eq!(call("return true", ""), [5]);
        assert_eq!(call("return function() return 12 end", ""), [9]);
        assert_eq!(call("return function() return '\\xff' end", ""), [10]);
        assert_eq!(call("return function() return '\\0' end", ""), [10]);
        assert_eq!(
            call("return function() return ('x'):rep(262145) end", ""),
            [11]
        );
        assert_eq!(call("return function() return '' end", ""), [0]);
    }

    #[test]
    fn enforces_instruction_and_memory_limits() {
        assert_eq!(call("while true do end", ""), [7]);
        assert_eq!(call("return function() while true do end end", ""), [7]);
        assert_eq!(
            call("return function() return ('x'):rep(33554432) end", ""),
            [8]
        );
    }

    #[test]
    fn fails_closed_when_standard_input_is_not_a_pipe() {
        let mut input = tempfile::tempfile().unwrap();
        input
            .write_all(&request("return function() return 'ran' end", "", ""))
            .unwrap();
        let reply = Command::new(worker())
            .arg(WORKER)
            .stdin(input)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .unwrap();
        assert!(reply.status.success());
        assert_eq!(reply.stdout, [1]);
    }

    #[test]
    fn runs_the_documented_markdown_quote_plugin() {
        let example: serde_json::Value =
            serde_json::from_str(include_str!("../examples/extensions/markdown-quote.json"))
                .unwrap();
        let script = example["scripts"]["quote.lua"].as_str().unwrap();
        let result = call(script, "café\r\n\r\n東京\r\n");
        assert_eq!(result, "\0> café\n> \n> 東京\n".as_bytes());
    }
    #[test]
    fn typed_batch_has_isolated_settings_and_returns_every_edit() {
        let payload = serde_json::to_vec(&serde_json::json!({
        "source": "return function(text, language, settings) local p = settings.prefix; settings.prefix = 'changed'; return p .. text .. ':' .. language end",
        "selections": ["λ", "東京"],
        "language": "markdown",
        "settings": {"prefix": "→ "}
    })).unwrap();
        let mut packet = (u32::MAX - 1).to_le_bytes().to_vec();
        packet.extend((payload.len() as u32).to_le_bytes());
        packet.extend(payload);
        let reply = exchange(packet);
        assert_eq!(reply[0], 0);
        assert_eq!(
            serde_json::from_slice::<Vec<String>>(&reply[1..]).unwrap(),
            ["→ λ:markdown", "→ 東京:markdown"]
        );
    }
}

#[cfg(not(all(
    any(target_os = "linux", target_os = "macos", windows),
    any(target_arch = "x86_64", target_arch = "aarch64"),
    target_endian = "little",
    target_pointer_width = "64"
)))]
#[test]
fn worker_exits_without_output_when_isolation_is_unavailable() {
    use std::process::{Command, Stdio};

    let output = Command::new(env!("CARGO_BIN_EXE_hedit"))
        .arg("--hedit-plugin-worker")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[cfg(any(target_os = "macos", windows))]
#[test]
fn main_executable_cannot_execute_lua_without_the_confined_helper() {
    use std::process::{Command, Stdio};
    let output = Command::new(env!("CARGO_BIN_EXE_hedit"))
        .arg("--hedit-plugin-worker")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, [1]);
    assert!(output.stderr.is_empty());
}

#[cfg(windows)]
#[test]
fn windows_cli_uses_the_shared_engine_and_enforces_limits() {
    use std::{fs, process::Command};
    let root = tempfile::tempdir().unwrap();
    let manifest = root.path().join("fixture.json");
    let fixture = root.path().join("cases.json");
    for (source, text, expected, error) in [
        ("assert(io == nil and os == nil and package == nil and debug == nil and require == nil and load == nil and string.dump == nil); return function(text) return text end", "café\r\n東京".to_string(), "café\r\n東京".to_string(), None),
        ("return function(text) return text end", "x".repeat(65536), "x".repeat(65536), None),
        ("while true do end", "".into(), "".into(), Some("instruction limit")),
        ("return function() return ('x'):rep(33554432) end", "".into(), "".into(), Some("memory limit")),
        ("return function() return 12 end", "".into(), "".into(), Some("must return a string")),
        ("return function() return '\\0' end", "".into(), "".into(), Some("without NUL")),
    ] {
        fs::write(&manifest, serde_json::to_vec(&serde_json::json!({"api_version":2,"id":"fixture","permissions":["selection.read","selection.replace"],"scripts":{"run.lua":source},"commands":[{"id":"run","title":"Fixture","action":{"kind":"lua","source":"run.lua"}}]})).unwrap()).unwrap();
        fs::write(&fixture, serde_json::to_vec(&serde_json::json!([{"command":"fixture.run","input":text,"expected":expected}])).unwrap()).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_hedit")).arg("plugin").arg("test").arg(&manifest).arg(&fixture).output().unwrap();
        let diagnostics = String::from_utf8_lossy(&output.stderr);
        if let Some(error) = error { assert!(!output.status.success()); assert!(diagnostics.contains(error), "{diagnostics}"); }
        else { assert!(output.status.success(), "{diagnostics}"); }
    }
}
