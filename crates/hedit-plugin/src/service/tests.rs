use super::*;
fn input(source: &str, kind: Kind) -> Input {
    Input {
        protocol_version: VERSION,
        may_edit: false,
        cache: Default::default(),
        request: Request {
            id: "f".repeat(32),
            kind,
            trigger: Trigger::Change,
            position: 0,
            action: None,
            choice: None,
        },
        package: sdk::Input {
            sdk_version: 1,
            source: source.into(),
            modules: Default::default(),
            resources: Default::default(),
            snapshot: sdk::Snapshot {
                document: "a".repeat(32),
                revision: "18446744073709551615".into(),
                language: "markdown".into(),
                text: "λ\r\n東京🦀".into(),
                selections: vec![sdk::Selection { anchor: 0, head: 0 }],
                crlf: true,
            },
            arguments: Default::default(),
            settings: Default::default(),
            storage: None,
        },
    }
}
fn evaluate(input: Input) -> Result<Output, Error> {
    crate::runtime::engine::with_lua_budget(CALLBACK_INSTRUCTIONS, |lua, env| {
        super::lua::evaluate(
            lua,
            env,
            input,
            std::rc::Rc::new(std::cell::RefCell::new(None)),
        )
    })
}
#[test]
fn callbacks_rotate_lua_globals_and_keep_only_explicit_bounded_cache() {
    let source="counter=(counter or 0)+1;assert(counter==1 and io==nil and os==nil and require==nil);return function(ctx) ctx.cache.count=(ctx.cache.count or 0)+1;assert(ctx.document.revision=='18446744073709551615');return {cache=ctx.cache,result={diagnostics={}}} end";
    let mut cache = sdk::Storage::default();
    for count in 1..20 {
        let mut request = input(source, Kind::Diagnostics);
        request.cache = cache;
        let output = evaluate(request).unwrap();
        assert_eq!(output.cache.0["count"], options::Value::Integer(count));
        cache = output.cache;
    }
}
#[test]
fn results_cannot_change_kind_identity_authority_or_escape_ui_quotas() {
    let request = input(
        "return function(ctx) return {result={diagnostics={}}} end",
        Kind::Diagnostics,
    );
    let valid = evaluate(input(&request.package.source, Kind::Diagnostics)).unwrap();
    valid.validate(&request).unwrap();
    for n in 0..5 {
        let mut forged = valid.clone();
        match n {
            0 => forged.request = "0".repeat(32),
            1 => forged.document = "0".repeat(32),
            2 => forged.revision = "0".into(),
            3 => {
                forged.result = ResultSet::Hover(Hover {
                    range: Range {
                        start: 0,
                        finish: 2,
                    },
                    text: "private fixture".into(),
                })
            }
            _ => {
                forged.result = ResultSet::Diagnostics(vec![Diagnostic {
                    range: Range {
                        start: 1,
                        finish: 2,
                    },
                    severity: Severity::Error,
                    message: "bad split".into(),
                    actions: vec![],
                }])
            }
        };
        assert!(forged.validate(&request).is_err());
    }
    let mut readonly = input(
        "return function(ctx) return {result={formatting=hedit.replace('changed')}} end",
        Kind::Formatting,
    );
    assert!(evaluate(input(&readonly.package.source, Kind::Formatting)).is_err());
    readonly.may_edit = true;
    assert!(evaluate(readonly).is_ok());
    for source in [
        "return function(ctx) return {result={presentation={output=string.rep('x',16385)}}} end",
        "return function(ctx) return {result={presentation={fetch='https://example.com'}}} end",
        "return function(ctx) local a={};a.result=a;return a end",
        "return function(ctx) return {result={presentation={}},other=ctx.document.text} end",
    ] {
        assert!(evaluate(input(source, Kind::Presentation)).is_err());
    }
}
#[test]
fn callback_limits_and_surrogate_helpers_are_shared_and_bounded() {
    let output=evaluate(input("return function(ctx) local line,col=hedit.to_utf16(14);assert(line==1 and col==4 and hedit.from_utf16(1,4)==14);return {result={hover={range={start=4,finish=14},text='ok'}}} end",Kind::Hover)).unwrap();
    assert!(matches!(output.result, ResultSet::Hover(_)));
    assert!(matches!(
        evaluate(input(
            "return function(ctx) while true do end end",
            Kind::Diagnostics
        )),
        Err(Error::Instructions)
    ));
    let count =
        vec![r#"{"range":{"start":0,"finish":0},"severity":"hint","message":"x"}"#; MAX_ITEMS + 1]
            .join(",");
    assert!(serde_json::from_str::<ResultSet>(&format!(r#"{{"diagnostics":[{count}]}}"#)).is_err());
}
#[test]
fn close_callbacks_cannot_receive_or_retain_document_data() {
    let mut request=input("return function(ctx) assert(ctx.document.text=='' and next(ctx.cache)==nil);return {result='closed'} end",Kind::Diagnostics);
    request.request.trigger = Trigger::Close;
    assert!(request.validate().is_err());
    request.package.snapshot.text.clear();
    assert!(evaluate(request).is_ok());
}
