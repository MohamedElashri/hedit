//! Standalone test peer: no GUI, broker, network or package dependencies.
//! The test host emits known compact JSON requests; this is not an LSP SDK.
use std::io::{self, BufRead, Read, Write};
fn emit(output: &mut impl Write, body: &str) {
    let bytes = format!("Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes();
    for chunk in bytes.chunks(3) { output.write_all(chunk).unwrap(); output.flush().unwrap(); }
}
fn main() {
    let mode = std::env::args().nth(1).unwrap();
    let mut input = io::BufReader::new(io::stdin().lock());
    let mut output = io::stdout().lock();
    let mut requests = 0;
    loop {
        let mut header = String::new();
        loop {
            if input.read_line(&mut header).unwrap() == 0 { return; }
            assert!(header.len() <= 8192);
            if header.ends_with("\r\n\r\n") { break; }
        }
        let length: usize = header.strip_prefix("Content-Length: ").unwrap().split_once("\r\n").unwrap().0.parse().unwrap();
        assert!(length <= 256 * 1024);
        let mut body = vec![0; length];
        input.read_exact(&mut body).unwrap();
        let body = String::from_utf8(body).unwrap();
        if body.contains("\"error\":") { assert!(body.contains("-32601")); continue; }
        // All host requests in this fixture use numeric IDs, and params are
        // empty. Notifications contain no ID. Never use this parser in a tool.
        let Some((_, rest)) = body.split_once("\"id\":") else { continue; };
        let id: String = rest.trim_start().chars().take_while(char::is_ascii_digit).collect();
        assert!(!id.is_empty());
        requests += 1;
        if body.contains("\"method\":\"initialize\"") {
            emit(&mut output, &format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{{"capabilities":{{"hoverProvider":true,"positionEncoding":"utf-16"}}}}}}"#));
            continue;
        }
        match mode.as_str() {
            "stall" => { std::thread::sleep(std::time::Duration::from_secs(30)); return; }
            "duplicate" => { emit(&mut output, &format!(r#"{{"jsonrpc":"2.0","id":{id},"result":null,"result":{{}}}}"#)); continue; }
            "truncated" => { output.write_all(b"Content-Length: 100\r\n\r\n{}").unwrap(); output.flush().unwrap(); return; }
            "flood" => { for _ in 0..100 { emit(&mut output, r#"{"jsonrpc":"2.0","method":"window/logMessage","params":{"message":"flood"}}"#); } }
            "server-request" => { emit(&mut output, r#"{"jsonrpc":"2.0","id":"server","method":"workspace/applyEdit","params":{"edit":{"changes":{"file:///unissued/private.txt":[]}}}}"#); }
            _ => {}
        }
        let id = if mode == "foreign" { "999" } else { &id };
        emit(&mut output, &format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{{"contents":"café / 東京 / 😀","requests":{requests}}}}}"#));
    }
}
