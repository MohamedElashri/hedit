#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64"),
    target_endian = "little",
    target_pointer_width = "64"
))]
mod linux {
    use std::io::Write;
    use std::process::{Command, Stdio};

    const WORKER: &str = "--hedit-plugin-worker";

    fn request(source: &str, selection: &str, language: &str) -> Vec<u8> {
        let mut payload = Vec::new();
        for field in [source, selection, language] {
            payload.extend_from_slice(&(field.len() as u32).to_le_bytes());
            payload.extend_from_slice(field.as_bytes());
        }
        payload
    }

    fn exchange(payload: Vec<u8>) -> Vec<u8> {
        let mut worker = Command::new(env!("CARGO_BIN_EXE_hedit"))
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
        exchange(request(source, selection, "markdown"))
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
        let reply = Command::new(env!("CARGO_BIN_EXE_hedit"))
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
    target_os = "linux",
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
