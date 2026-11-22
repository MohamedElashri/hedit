use super::*;
#[test]
fn language_results_validate_against_host_snapshot_before_reaching_lua() {
    let mut request = input();
    request.response = Some(Response::LanguageServer {
        result: Box::new(service::ResultSet::Hover(service::Hover {
            range: service::Range {
                start: 0,
                finish: 5,
            },
            text: "café".into(),
        })),
    });
    request.validate().unwrap();
    if let Some(Response::LanguageServer { result }) = &mut request.response {
        let service::ResultSet::Hover(hover) = result.as_mut() else {
            panic!()
        };
        hover.range.finish = 4; // Split the UTF-8 é.
    }
    assert!(request.validate().is_err());
    request.response = Some(Response::LanguageServer {
        result: Box::new(service::ResultSet::Formatting(sdk::Transaction {
            document: request.package.snapshot.document.clone(),
            revision: "41".into(),
            edits: vec![],
            selections: None,
            storage: None,
        })),
    });
    assert!(request.validate().is_err());
    if let Some(Response::LanguageServer { result }) = &mut request.response {
        let service::ResultSet::Formatting(transaction) = result.as_mut() else {
            panic!()
        };
        transaction.revision = "42".into();
    }
    request.validate().unwrap();
    request.may_edit = false;
    assert!(request.validate().is_err());
    request.response = Some(Response::LanguageServer {
        result: Box::new(service::ResultSet::Presentation(Default::default())),
    });
    assert!(request.validate().is_err());
}
pub(crate) fn input() -> Input {
    Input{protocol_version:VERSION,request:Handle::fresh().unwrap(),step:0,package:sdk::Input{sdk_version:sdk::VERSION,source:"return function(ctx) return {intent={discover={scope=ctx.broker.scope,query=''}},cache={step=1}} end".into(),modules:Default::default(),resources:Default::default(),snapshot:sdk::Snapshot{document:"0123456789abcdef0123456789abcdef".into(),revision:"42".into(),language:"text".into(),text:"café / 東京\r\n🦀".into(),selections:vec![sdk::Selection{anchor:0,head:0}],crlf:true},settings:Default::default(),arguments:Default::default(),storage:None},scope:Some(Handle::fresh().unwrap()),may_edit:true,workspace_edit:true,may_present:true,provider:None,response:None,cache:Default::default()}
}
#[test]
fn requests_bind_scope_step_and_revision_without_accepting_owner_assertions() {
    let input = input();
    input.validate().unwrap();
    let output = Output {
        request: input.request,
        step: 0,
        intent: Intent::Discover {
            scope: input.scope.unwrap(),
            query: String::new(),
        },
        cache: Default::default(),
    };
    output.validate(&input).unwrap();
    let mut wrong = output.clone();
    wrong.request = Handle::fresh().unwrap();
    assert!(wrong.validate(&input).is_err());
    wrong = output.clone();
    wrong.step = 1;
    assert!(wrong.validate(&input).is_err());
    wrong = output.clone();
    wrong.intent = Intent::Discover {
        scope: Handle::fresh().unwrap(),
        query: String::new(),
    };
    assert!(wrong.validate(&input).is_err());
    let mut value = serde_json::to_value(output).unwrap();
    value["owner"] = "another-plugin".into();
    assert!(serde_json::from_value::<Output>(value).is_err());
    assert!(serde_json::from_str::<Intent>(r#"{"https":{"endpoint":"translation","body":"fixture","headers":{"Authorization":"secret"}}}"#).is_err());
    assert!(serde_json::from_str::<Intent>(r#"{"tool":{"profile":"formatter","input":"fixture","arguments":{},"environment":{"TOKEN":"secret"}}}"#).is_err());
}
#[test]
fn arrays_capabilities_and_aggregate_response_disclosures_are_bounded() {
    let mut input = input();
    let change = Change {
        resource: Handle::fresh().unwrap(),
        version: Handle::fresh().unwrap(),
        edits: vec![
            sdk::Edit {
                start: 0,
                finish: 0,
                text: "a".into()
            };
            65
        ],
    };
    assert!(serde_json::from_value::<Change>(serde_json::to_value(&change).unwrap()).is_err());
    let output = Output {
        request: input.request,
        step: 0,
        intent: Intent::StageEdits {
            scope: input.scope.unwrap(),
            change,
        },
        cache: Default::default(),
    };
    assert!(output.validate(&input).is_err());
    let mut output = output;
    output.intent = Intent::ReviewEdits {
        scope: input.scope.unwrap(),
        changes: None,
    };
    input.workspace_edit = false;
    assert!(output.validate(&input).is_err());
    input.step = 32;
    assert!(input.validate().is_err());
    assert!(Response::Read {
        text: "x".repeat(MAX_CHUNK_BYTES + 1),
        next: None
    }
    .validate()
    .is_err());
    assert!(Response::Discovered {
        entries: vec![
            Entry {
                relative: "x".repeat(4096),
                directory: false,
                bytes: 0
            };
            32
        ],
        truncated: false
    }
    .validate()
    .is_err());
}

#[test]
fn optional_review_changes_round_trip_without_accepting_ambiguous_null() {
    let intent = Intent::ReviewEdits {
        scope: Handle::fresh().unwrap(),
        changes: None,
    };
    let bytes = serde_json::to_vec(&intent).unwrap();
    assert!(matches!(
        serde_json::from_slice::<Intent>(&bytes).unwrap(),
        Intent::ReviewEdits { changes: None, .. }
    ));
    let mut value = serde_json::to_value(intent).unwrap();
    value["review_edits"]["changes"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<Intent>(value).is_err());
}
