use super::*;
#[test]
fn formatter_is_unicode_safe_duplicate_strict_and_bounded() {
    assert_eq!(
        format("{\"東京\":\"café😀\"}".as_bytes()).unwrap(),
        "{\n  \"東京\": \"café😀\"\n}\n"
    );
    for bytes in [
        br#"{"same":1,"same":2}"#.as_slice(),
        b"[1,]",
        &[255],
        &vec![b' '; 65537],
    ] {
        assert!(format(bytes).is_err());
    }
    let nested = format!("{}0{}", "[".repeat(65), "]".repeat(65));
    assert!(format(nested.as_bytes()).is_err());
    let expansion = format!("[{}]", "[0],".repeat(8190).trim_end_matches(','));
    assert!(format(expansion.as_bytes()).is_err());
}
fn message(
    server: &mut Server,
    method: &str,
    id: Option<i32>,
    params: Value,
) -> io::Result<Option<Value>> {
    let mut value = json!({"jsonrpc":"2.0","method":method,"params":params});
    if let Some(id) = id {
        value["id"] = id.into();
    }
    server.message(value).map(|(response, _)| response)
}
fn open(text: &str) -> Server {
    let mut server = Server::default();
    message(&mut server, "initialize", Some(1), json!({})).unwrap();
    message(&mut server, "initialized", None, json!({})).unwrap();
    message(
        &mut server,
        "textDocument/didOpen",
        None,
        json!({"textDocument":{"uri":"file:///approved.json","version":1,"text":text}}),
    )
    .unwrap();
    server
}
#[test]
fn symbols_index_actual_escaped_unicode_key_spans_and_skip_nested_values() {
    let text = "{\r\n\"東京😀\": {\"nested\":\"colon: here\"}, \"a\\\"b\":2, \"\\u0063\":3}";
    let document = Document::new("file:///approved.json", 1, text).unwrap();
    assert_eq!(
        document
            .keys
            .iter()
            .map(|key| key.0.as_str())
            .collect::<Vec<_>>(),
        ["東京😀", "a\"b", "c"]
    );
    for (name, start, end) in &document.keys {
        assert_eq!(
            serde_json::from_str::<String>(&text[*start..*end]).unwrap(),
            *name
        );
    }
    assert_eq!(
        document.result("textDocument/documentSymbol").unwrap()[0]["range"]["start"],
        json!({"line":1,"character":0})
    );
}
#[test]
fn diagnostics_survive_incomplete_edits_and_document_identity_is_exact() {
    let mut server = open("{\"a\":1}");
    let query = json!({"textDocument":{"uri":"file:///approved.json"}});
    assert_eq!(
        message(
            &mut server,
            "textDocument/diagnostic",
            Some(2),
            query.clone()
        )
        .unwrap()
        .unwrap()["result"]["items"],
        json!([])
    );
    message(&mut server,"textDocument/didChange",None,json!({"textDocument":{"uri":"file:///approved.json","version":2},"contentChanges":[{"text":"{\"a\":"}]})).unwrap();
    assert_eq!(
        message(
            &mut server,
            "textDocument/diagnostic",
            Some(3),
            query.clone()
        )
        .unwrap()
        .unwrap()["result"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        message(
            &mut server,
            "textDocument/formatting",
            Some(4),
            query.clone()
        )
        .unwrap()
        .unwrap()["result"],
        json!([])
    );
    assert!(message(&mut server,"textDocument/didChange",None,json!({"textDocument":{"uri":"file:///approved.json","version":2},"contentChanges":[{"text":"{}"}]})).is_err());
    assert!(message(
        &mut server,
        "textDocument/hover",
        Some(5),
        json!({"textDocument":{"uri":"file:///foreign.json"}})
    )
    .is_err());
    assert!(message(&mut Server::default(), "textDocument/hover", Some(1), query).is_err());
}
#[test]
fn protocol_round_trip_correlates_ids_and_rejects_truncated_frames() {
    let messages = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///approved.json","version":1,"text":"{\"a\":1}"}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{"textDocument":{"uri":"file:///approved.json"}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"shutdown","params":{}}),
        json!({"jsonrpc":"2.0","method":"exit","params":{}}),
    ];
    let bytes: Vec<_> = messages
        .iter()
        .flat_map(|value| lsp::encode(value, Limits::SCOPED).unwrap())
        .collect();
    struct Fragment<'a>(&'a [u8]);
    impl Read for Fragment<'_> {
        fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
            let count = self.0.len().min(into.len()).min(1);
            into[..count].copy_from_slice(&self.0[..count]);
            self.0 = &self.0[count..];
            Ok(count)
        }
    }
    let mut output = Vec::new();
    serve(Fragment(&bytes), &mut output).unwrap();
    let mut decoder = Decoder::new(Limits::SCOPED);
    let mut ids = Vec::new();
    for chunk in output.chunks(READ_BYTES) {
        decoder.push(chunk).unwrap();
        while let Some(value) = decoder.next_message().unwrap() {
            ids.push(value["id"].clone());
        }
    }
    assert_eq!(ids, json!([1, 2, 3]).as_array().unwrap().clone());
    let frame = lsp::encode(&messages[0], Limits::SCOPED).unwrap();
    assert!(serve(&frame[..frame.len() - 1], Vec::new()).is_err());
}
