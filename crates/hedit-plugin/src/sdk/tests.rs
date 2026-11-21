use super::*;
fn snapshot(text: &str) -> Snapshot {
    Snapshot {
        document: "0123456789abcdef0123456789abcdef".into(),
        revision: "42".into(),
        language: "markdown".into(),
        text: text.into(),
        selections: vec![Selection { anchor: 0, head: 0 }],
        crlf: text.contains("\r\n"),
    }
}
#[test]
fn transactions_reject_foreign_stale_overlapping_and_invalid_coordinates() {
    let snapshot = snapshot("λ\r\n東京");
    let valid = Transaction {
        storage: None,
        document: snapshot.document.clone(),
        revision: snapshot.revision.clone(),
        edits: vec![
            Edit {
                start: 0,
                finish: 2,
                text: "α".into(),
            },
            Edit {
                start: 4,
                finish: 10,
                text: "β".into(),
            },
        ],
        selections: None,
    };
    assert_eq!(valid.prepare(&snapshot).unwrap().text, "α\r\nβ");
    for invalid in 0..6 {
        let mut transaction = valid.clone();
        match invalid {
            0 => transaction.document = "f".repeat(32),
            1 => transaction.revision = "41".into(),
            2 => transaction.edits[0].finish = 1,
            3 => transaction.edits[0].finish = 3,
            4 => transaction.edits[1].start = 0,
            _ => transaction.edits[1].finish = usize::MAX,
        }
        assert!(transaction.prepare(&snapshot).is_err());
        assert_eq!(snapshot.text, "λ\r\n東京");
    }
}
#[test]
fn normalized_limits_and_result_selections_are_validated_before_application() {
    let snapshot = snapshot("a\r\nb");
    let mut transaction = Transaction {
        storage: None,
        document: snapshot.document.clone(),
        revision: snapshot.revision.clone(),
        edits: vec![Edit {
            start: 0,
            finish: 4,
            text: "λ\n東京".into(),
        }],
        selections: Some(vec![Selection {
            anchor: 4,
            head: 10,
        }]),
    };
    assert_eq!(transaction.prepare(&snapshot).unwrap().text, "λ\r\n東京");
    transaction.selections = Some(vec![Selection { anchor: 3, head: 3 }]);
    assert!(transaction.prepare(&snapshot).is_err());
    transaction.selections = None;
    transaction.edits[0].text = "\n".repeat(MAX_OUTPUT_BYTES / 2 + 1);
    assert!(matches!(
        transaction.prepare(&snapshot),
        Err(Error::ReturnSize)
    ));
}
#[test]
fn coordinate_helpers_match_independent_line_slices() {
    let snapshot = snapshot("λ\r\n東京\n\n🦀");
    for (line, expected) in [(0, "λ"), (1, "東京"), (2, ""), (3, "🦀")] {
        for col in expected
            .char_indices()
            .map(|(n, _)| n)
            .chain(std::iter::once(expected.len()))
        {
            let byte = snapshot.offset(line, col).unwrap();
            assert_eq!(snapshot.line_column(byte).unwrap(), (line, col));
            assert_eq!(
                &snapshot.text[byte - col..byte - col + expected.len()],
                expected
            );
        }
    }
    assert!(snapshot.offset(1, 1).is_err());
    assert!(snapshot.offset(9, 0).is_err());
    assert!(snapshot.line_column(3).is_err());
}
#[test]
fn shared_lua_engine_runs_structured_edits_with_no_extra_authority() {
    let snapshot = snapshot("café / 東京");
    let input=Input {storage:None,sdk_version:VERSION,modules:Default::default(),resources:Default::default(),source:"assert(io == nil and os == nil and require == nil); return function(ctx) assert(ctx.document.revision == '42'); return hedit.replace(ctx.arguments.prefix .. ctx.document.text) end".into(),snapshot:snapshot.clone(),settings:Default::default(),arguments:[("prefix".into(),options::Value::Text("→ ".into()))].into()};
    input.validate().unwrap();
    let transaction =
        crate::runtime::engine::with_lua(|lua, env| lua::evaluate(lua, env, input)).unwrap();
    assert_eq!(
        transaction.prepare(&snapshot).unwrap().text,
        "→ café / 東京"
    );
}
#[test]
fn serialized_transactions_bound_arrays_and_refuse_unknown_fields() {
    let prefix = format!(
        "{{\"document\":\"{}\",\"revision\":\"42\",\"edits\":[",
        snapshot("").document
    );
    let edits = vec!["{\"start\":0,\"finish\":0,\"text\":\"\"}"; MAX_EDITS + 1].join(",");
    assert!(serde_json::from_str::<Transaction>(&format!("{prefix}{edits}]}}")).is_err());
    let payload = format!("{prefix}] ,\"extra\":true}}");
    assert!(serde_json::from_str::<Transaction>(&payload).is_err());
}

fn invocation(source: &str) -> Input {
    Input {
        storage: None,
        sdk_version: VERSION,
        source: source.into(),
        modules: Default::default(),
        resources: Default::default(),
        snapshot: snapshot("café / 東京"),
        arguments: Default::default(),
        settings: Default::default(),
    }
}
#[test]
fn modules_are_cached_only_within_invocation_and_resources_are_read_only_data() {
    let mut input=invocation("return function(ctx) local a=hedit.import('prefix.lua'); assert(a==hedit.import('prefix.lua')); return hedit.replace(a.prefix .. hedit.resource('suffix')) end");
    input.modules.insert(
        "prefix.lua".into(),
        "assert(io==nil and require==nil); return {prefix='→ '}".into(),
    );
    input.resources.insert("suffix".into(), "東京".into());
    let original = input.snapshot.clone();
    let transaction =
        crate::runtime::engine::with_lua(|lua, env| lua::evaluate(lua, env, input)).unwrap();
    assert_eq!(transaction.prepare(&original).unwrap().text, "→ 東京");
    let input =
        invocation("return function(ctx) return hedit.replace(hedit.resource('suffix')) end");
    assert!(crate::runtime::engine::with_lua(|lua, env| lua::evaluate(lua, env, input)).is_err());
}
#[test]
fn module_cycles_paths_and_package_limits_fail_closed() {
    let mut input =
        invocation("return function(ctx) return hedit.replace(hedit.import('loop.lua')) end");
    input
        .modules
        .insert("loop.lua".into(), "return hedit.import('loop.lua')".into());
    assert!(crate::runtime::engine::with_lua(|lua, env| lua::evaluate(lua, env, input)).is_err());
    for name in [
        "../outside.lua",
        "/tmp/secret.lua",
        "native.so",
        "UPPER.lua",
    ] {
        let mut input = invocation("return function(ctx) return hedit.replace('') end");
        input.modules.insert(name.into(), "return 1".into());
        assert!(input.validate().is_err());
    }
    let mut input = invocation("return function(ctx) return hedit.replace('') end");
    input
        .resources
        .insert("large".into(), "x".repeat(MAX_SOURCE_BYTES + 1));
    assert!(input.validate().is_err());
}
#[test]
fn coordinates_preserve_a_lone_trailing_carriage_return() {
    let snapshot = snapshot("x\r");
    assert_eq!(snapshot.offset(0, 2).unwrap(), 2);
    assert_eq!(snapshot.line_column(2).unwrap(), (0, 2));
}

#[test]
fn private_storage_requires_authority_and_cannot_mix_with_document_effects() {
    let mut input = invocation(
        "return function(ctx) local t=hedit.transaction({}); t.storage={count=1}; return t end",
    );
    assert!(crate::runtime::engine::with_lua(|lua, env| lua::evaluate(lua, env, input)).is_err());
    input=invocation("return function(ctx) local t=hedit.transaction({}); t.storage={count=ctx.storage.count+1}; return t end");
    input.storage = Some(Storage(
        [("count".into(), options::Value::Integer(3))].into(),
    ));
    let snapshot = input.snapshot.clone();
    let mut transaction =
        crate::runtime::engine::with_lua(|lua, env| lua::evaluate(lua, env, input)).unwrap();
    assert_eq!(
        transaction.storage.as_ref().unwrap().0["count"],
        options::Value::Integer(4)
    );
    transaction.edits.push(Edit {
        start: 0,
        finish: 0,
        text: "bad".into(),
    });
    assert!(transaction.prepare(&snapshot).is_err());
    transaction.edits.clear();
    transaction.selections = Some(vec![Selection { anchor: 0, head: 0 }]);
    assert!(transaction.prepare(&snapshot).is_err());
}

#[test]
fn no_op_edits_preserve_selections_and_new_crlf_pairs_have_valid_default_carets() {
    let snapshot = snapshot("a\rX\nb");
    let mut transaction = Transaction {
        document: snapshot.document.clone(),
        revision: snapshot.revision.clone(),
        edits: vec![Edit {
            start: 0,
            finish: snapshot.text.len(),
            text: snapshot.text.clone(),
        }],
        selections: None,
        storage: None,
    };
    assert_eq!(
        transaction.prepare(&snapshot).unwrap().selections,
        snapshot.selections
    );
    transaction.edits = vec![Edit {
        start: 2,
        finish: 3,
        text: String::new(),
    }];
    let prepared = transaction.prepare(&snapshot).unwrap();
    assert_eq!(prepared.text, "a\r\nb");
    assert!(boundary(&prepared.text, prepared.selections[0].head));
}

#[test]
fn combined_raw_replacements_cannot_bypass_limits_through_normalization() {
    let snapshot = snapshot("ab");
    let transaction = Transaction {
        document: snapshot.document.clone(),
        revision: snapshot.revision.clone(),
        selections: None,
        storage: None,
        edits: vec![
            Edit {
                start: 0,
                finish: 1,
                text: "\r\n".repeat(MAX_OUTPUT_BYTES / 4 + 1),
            },
            Edit {
                start: 1,
                finish: 2,
                text: "\r\n".repeat(MAX_OUTPUT_BYTES / 4 + 1),
            },
        ],
    };
    assert!(matches!(
        transaction.prepare(&snapshot),
        Err(Error::ReturnSize)
    ));
}

#[test]
fn single_edit_results_match_an_independent_character_splice_oracle() {
    for original in ["", "a", "e\u{301}🦀東京", "λ\r\n東京\n", "a\rb"] {
        let snapshot = snapshot(original);
        let points: Vec<_> = original
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(original.len()))
            .collect();
        for &start in &points {
            for &finish in &points {
                if finish < start {
                    continue;
                }
                let transaction = Transaction {
                    document: snapshot.document.clone(),
                    revision: snapshot.revision.clone(),
                    selections: None,
                    storage: None,
                    edits: vec![Edit {
                        start,
                        finish,
                        text: "✨".into(),
                    }],
                };
                let prepared = transaction.prepare(&snapshot);
                let splits_crlf = |offset: usize| {
                    offset > 0
                        && original.as_bytes().get(offset - 1) == Some(&b'\r')
                        && original.as_bytes().get(offset) == Some(&b'\n')
                };
                if splits_crlf(start) || splits_crlf(finish) {
                    assert!(prepared.is_err());
                    continue;
                }
                let left = original[..start].chars().count();
                let right = original[..finish].chars().count();
                let expected: String = original
                    .chars()
                    .take(left)
                    .chain("✨".chars())
                    .chain(original.chars().skip(right))
                    .collect();
                assert_eq!(prepared.unwrap().text, expected);
            }
        }
    }
}

#[test]
fn reply_decoder_rejects_excess_edits_before_buffering_or_parsing_their_payload() {
    let input = invocation("return function(ctx) return hedit.transaction({}) end");
    let edits = vec![r#"{"start":0,"finish":0,"text":""}"#; MAX_EDITS].join(",");
    // The 65th value is deliberately truncated. A streaming bounded decoder
    // rejects the count before looking at this value; buffering would hit EOF.
    let reply = format!(
        r#"{{"protocol_version":1,"outcome":{{"success":{{"document":"{}","revision":"42","edits":[{},{{"#,
        input.snapshot.document, edits
    );
    let error = serde_json::from_str::<Reply>(&reply).err().unwrap();
    assert!(error.to_string().contains("too many entries"), "{error}");
}
