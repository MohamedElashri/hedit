use super::*;
use serde_json::json;
fn document(encoding: Encoding) -> Document {
    Document::new(
        sdk::Snapshot {
            document: "a".repeat(32),
            revision: "42".into(),
            language: "rust".into(),
            text: "a😀é\r\n東京 foo".into(),
            selections: vec![sdk::Selection {
                anchor: 19,
                head: 19,
            }],
            crlf: true,
        },
        if cfg!(windows) {
            "file:///C:/issued/current.rs"
        } else {
            "file:///issued/current.rs"
        }
        .into(),
        7,
        encoding,
    )
    .unwrap()
}
fn range(line: u32, start: u32, end: u32) -> Value {
    json!({"start":{"line":line,"character":start},"end":{"line":line,"character":end}})
}
#[test]
fn all_encodings_map_exact_unicode_and_reject_crlf_interiors() {
    for (encoding, column) in [
        (Encoding::Utf8, 5),
        (Encoding::Utf16, 3),
        (Encoding::Utf32, 2),
    ] {
        let doc = document(encoding);
        assert_eq!(
            doc.position(5).unwrap(),
            text::Position {
                line: 0,
                character: column
            }
        );
        assert_eq!(
            doc.position(9).unwrap(),
            text::Position {
                line: 1,
                character: 0
            }
        );
        assert!(doc.position(2).is_err());
        assert!(doc.position(8).is_err());
        assert!(doc.position(20).is_err());
        let value = json!({"contents":"hover", "range":range(0, 1, column)});
        let ResultSet::Hover(hover) = doc.translate(Method::Hover, &value, 5, false).unwrap()
        else {
            panic!()
        };
        assert_eq!((hover.range.start, hover.range.finish), (1, 5));
    }
    let doc = document(Encoding::Utf16);
    assert!(doc
        .translate(
            Method::Hover,
            &json!({"contents":"bad","range":range(0, 1, 2)}),
            5,
            false
        )
        .is_err());
    assert!(doc
        .translate(
            Method::Hover,
            &json!({"contents":"bad","range":range(0, 0, 5)}),
            5,
            false
        )
        .is_err());
}

#[test]
fn indexed_coordinates_match_independent_prefixes_at_every_boundary() {
    for encoding in [Encoding::Utf8, Encoding::Utf16, Encoding::Utf32] {
        for text in ["", "a\r\n\n", "λ東京😀e\u{301}\r", "\r\n😀\r\n"] {
            let mut snapshot = document(encoding).snapshot().clone();
            snapshot.text = text.into();
            snapshot.selections = vec![sdk::Selection { anchor: 0, head: 0 }];
            let doc = Document::new(snapshot, document(encoding).uri, 1, encoding).unwrap();
            for byte in 0..=text.len() {
                if !sdk::boundary(text, byte) {
                    assert!(doc.position(byte).is_err());
                    continue;
                }
                let before = &text[..byte];
                let line = before.bytes().filter(|byte| *byte == b'\n').count() as u32;
                let column_text = &before[before.rfind('\n').map_or(0, |at| at + 1)..];
                let character = match encoding {
                    Encoding::Utf8 => column_text.len(),
                    Encoding::Utf16 => column_text.encode_utf16().count(),
                    Encoding::Utf32 => column_text.chars().count(),
                } as u32;
                let expected = text::Position { line, character };
                assert_eq!(doc.position(byte).unwrap(), expected);
                assert_eq!(doc.byte(expected).unwrap(), byte);
            }
        }
    }
}
#[test]
fn published_diagnostics_require_issued_uri_and_exact_version() {
    let doc = document(Encoding::Utf16);
    let value = json!({"uri":doc.uri(),"version":7,"diagnostics":[{"range":range(0,1,3),"severity":2,"message":"café 東京"}]});
    let ResultSet::Diagnostics(items) = doc.published_diagnostics(&value).unwrap() else {
        panic!()
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].range.finish, 5);
    for version in [Value::Null, json!(6), json!(8), json!("7")] {
        let mut stale = value.clone();
        stale["version"] = version;
        assert!(doc.published_diagnostics(&stale).is_err());
    }
    let mut foreign = value;
    foreign["uri"] = json!("file:///private/secret");
    assert!(doc.published_diagnostics(&foreign).is_err());
    assert!(doc
        .translate(
            Method::Diagnostics,
            &json!({"kind":"full","items":[],"relatedDocuments":{"file:///secret":{"items":[]}}}),
            0,
            false
        )
        .is_err());
}
#[test]
fn simple_completions_replace_prefix_and_never_execute_commands_or_snippets() {
    let doc = document(Encoding::Utf16);
    let ResultSet::Completions(items) = doc
        .translate(
            Method::Completions,
            &json!([{"label":"foobar","detail":"東京"}]),
            19,
            true,
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!((items[0].range.start, items[0].range.finish), (16, 19));
    assert_eq!(items[0].insert, "foobar");
    for unsafe_item in [
        json!({"label":"x","command":{"command":"shell"}}),
        json!({"label":"x","insertTextFormat":2,"insertText":"${1:unsafe}"}),
        json!({"label":"x","additionalTextEdits":[{"range":range(0,0,1),"newText":"x"}]}),
        json!({"label":"x","textEdit":{"range":range(0,1,2),"newText":"x"}}),
    ] {
        assert!(doc
            .translate(Method::Completions, &json!([unsafe_item]), 19, true)
            .is_err());
    }
    assert!(doc
        .translate(Method::Completions, &json!([]), 19, false)
        .is_err());
    assert!(doc
        .translate(
            Method::Completions,
            &json!({"items":[],"itemDefaults":{"insertTextFormat":2}}),
            19,
            true
        )
        .is_err());
}
#[test]
fn formatting_uses_revision_bound_sdk_transactions_and_shared_edit_rules() {
    let doc = document(Encoding::Utf16);
    let edits = json!([{"range":range(1,3,6),"newText":"bar\nbaz"}]);
    let ResultSet::Formatting(transaction) =
        doc.translate(Method::Formatting, &edits, 0, true).unwrap()
    else {
        panic!()
    };
    assert_eq!(transaction.document, "a".repeat(32));
    assert_eq!(transaction.revision, "42");
    assert_eq!(
        transaction.prepare(doc.snapshot()).unwrap().text,
        "a😀é\r\n東京 bar\r\nbaz"
    );
    assert!(doc.translate(Method::Formatting, &edits, 0, false).is_err());
    let overlap =
        json!([{"range":range(0,0,3),"newText":"x"},{"range":range(0,1,4),"newText":"y"}]);
    assert!(doc
        .translate(Method::Formatting, &overlap, 0, true)
        .is_err());
    let mut stale = doc.snapshot().clone();
    stale.revision = "43".into();
    assert!(transaction.prepare(&stale).is_err());
    assert!(doc
        .translate(
            Method::Formatting,
            &json!([{"range":range(0,0,0),"newText":"x".repeat(256*1024+1)}]),
            0,
            true
        )
        .is_err());
}
#[test]
fn code_actions_cannot_cross_uris_create_files_or_invoke_commands() {
    let doc = document(Encoding::Utf16);
    let edits = json!([{"range":range(1,3,6),"newText":"bar"}]);
    let valid = json!([{"title":"Fix","edit":{"changes":{doc.uri():edits}}}]);
    assert!(matches!(
        doc.translate(Method::CodeActions, &valid, 0, true).unwrap(),
        ResultSet::CodeActions(_)
    ));
    for edit in [
        json!({"changes":{"file:///secret":edits}}),
        json!({"documentChanges":[{"kind":"create","uri":doc.uri()}]}),
        json!({"documentChanges":[{"textDocument":{"uri":doc.uri(),"version":6},"edits":edits}]}),
        json!({"changes":{doc.uri():edits},"documentChanges":[]}),
    ] {
        assert!(doc
            .translate(
                Method::CodeActions,
                &json!([{"title":"Unsafe","edit":edit}]),
                0,
                true
            )
            .is_err());
    }
    assert!(doc
        .translate(
            Method::CodeActions,
            &json!([{"title":"Run","command":"shell","arguments":[]}]),
            0,
            true
        )
        .is_err());
    assert!(doc.translate(Method::CodeActions, &json!([{"title":"Run","command":{"command":"shell"},"edit":{"changes":{doc.uri():edits}}}]), 0, true).is_err());
}
#[test]
fn symbols_and_hover_are_bounded_plain_data() {
    let doc = document(Encoding::Utf16);
    let child = json!({"name":"子","range":range(1,0,2),"selectionRange":range(1,0,1)});
    let value = json!([{"name":"Parent","range":{"start":{"line":0,"character":0},"end":{"line":1,"character":6}},"selectionRange":range(0,0,1),"children":[child]}]);
    let ResultSet::Symbols(symbols) = doc.translate(Method::Symbols, &value, 0, false).unwrap()
    else {
        panic!()
    };
    assert_eq!(symbols.len(), 2);
    assert_eq!(symbols[1].range.start, 9);
    assert!(doc
        .translate(
            Method::Symbols,
            &json!([{"name":"Foreign","location":{"uri":"file:///secret","range":range(0,0,1)}}]),
            0,
            false
        )
        .is_err());
    assert!(doc
        .translate(
            Method::Symbols,
            &json!(vec![value[0].clone(); 129]),
            0,
            false
        )
        .is_err());
    let ResultSet::Hover(hover) = doc.translate(Method::Hover, &json!({"contents":{"kind":"markdown","value":"[link](command:run) <script>data</script>"}}), 0, false).unwrap() else { panic!() };
    assert!(hover.text.contains("command:run")); // Passed to native text rendering, never an action.
    assert!(doc
        .translate(
            Method::Hover,
            &json!({"contents":"x".repeat(16385)}),
            0,
            false
        )
        .is_err());
}
