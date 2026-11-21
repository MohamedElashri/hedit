use std::{fs, process::Command};

#[test]
fn developer_cli_packs_checks_and_tests_an_extension() {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("python3")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/scripts/macos_plugin_helper.py"
            ))
            .arg("--editor")
            .arg(env!("CARGO_BIN_EXE_hedit"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("author.json");
    let fixture = directory.path().join("fixtures.json");
    let key = directory.path().join("publisher.key");
    let bundle = directory.path().join("quote.hedit-plugin");
    fs::write(&source, r#"{
        "api_version":2,
        "id":"quote",
        "permissions":["selection.read", "selection.replace"],
        "script_files":["quote.lua"],
        "settings":{"prefix":{"label":"Prefix","default":"> "}},
        "commands":[{"id":"transform", "title":"Quote", "action":{"kind":"lua", "source":"quote.lua"}}]
    }"#).unwrap();
    fs::write(
        directory.path().join("quote.lua"),
        "return function(text, language, settings) return settings.prefix .. text end",
    )
    .unwrap();
    fs::write(
        &fixture,
        r#"[
          {"command":"quote.transform", "input":"λ", "expected":"> λ"},
          {"command":"quote.transform", "input":"λ", "settings":{"prefix":"- "}, "expected":"- λ"}
        ]"#,
    )
    .unwrap();

    let cli = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_hedit"));
        command.arg("plugin");
        command
    };
    let run = |command: &mut Command| {
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    let generated = run(cli().arg("keygen").arg(&key));
    let fingerprint = generated
        .trim()
        .strip_prefix("Publisher key fingerprint ")
        .unwrap();
    assert_eq!(fingerprint.len(), 64);
    run(cli()
        .arg("pack")
        .arg(&source)
        .arg(&bundle)
        .args([
            "--version",
            "1.0.0",
            "--publisher",
            "Fixture author",
            "--sign-key",
        ])
        .arg(&key));
    let checked = run(cli().arg("check").arg(&bundle));
    assert!(checked.contains("Version 1.0.0"));
    assert!(checked.contains(&format!("Signed by Fixture author ({fingerprint})")));

    if cfg!(all(
        any(target_os = "linux", target_os = "macos", windows),
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )) {
        let tested = run(cli().arg("test").arg(&bundle).arg(&fixture));
        assert!(tested.contains("PASS 1 quote.transform"));
        assert!(tested.contains("PASS 2 quote.transform"));
        fs::write(
            &fixture,
            r#"[{"command":"quote.transform", "input":"λ", "expected":"distinct"}]"#,
        )
        .unwrap();
        let rejected = cli()
            .arg("test")
            .arg(&bundle)
            .arg(&fixture)
            .output()
            .unwrap();
        assert!(!rejected.status.success());
        assert!(
            String::from_utf8_lossy(&rejected.stderr).contains("Output differs from expected text")
        );
    }
}

#[test]
fn scoped_cli_rotates_callbacks_checks_transcripts_and_preserves_legacy_routes() {
    use serde_json::json;
    #[cfg(target_os = "macos")]
    assert!(Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/macos_plugin_helper.py"
        ))
        .arg("--editor")
        .arg(env!("CARGO_BIN_EXE_hedit"))
        .status()
        .unwrap()
        .success());
    let directory = tempfile::tempdir().unwrap();
    let manifest = directory.path().join("scoped.json");
    let fixtures = directory.path().join("cases.json");
    let format = r#"local calls=0; return function(ctx)
        calls=calls+1; assert(calls==1)
        local cache={calls=(ctx.cache.calls or 0)+1}
        if ctx.broker.step==0 then
            return {intent={tool={profile='format',input=ctx.document.text,arguments={}}},cache=cache}
        end
        assert(ctx.broker.response.tool.status==0)
        return {intent={finish={transaction=hedit.replace(ctx.broker.response.tool.output)}},cache=cache}
    end"#;
    let source = json!({"api_version":5,"sdk_version":3,"id":"scoped-fixture",
    "scripts":{"format.lua":format,"hover.lua":"return function(ctx) if ctx.request.trigger=='close' then return {intent={finish={result='closed'}}} end if ctx.broker.step==0 then return {intent={language_server={profile='server',method='hover',position=ctx.request.position}}} end return {intent={finish={result=ctx.broker.response.language_server.result}}} end",
    "neutral.lua":"return function(ctx) assert(ctx.document.text==''); return {intent={finish={result={presentation={status='Ready'}}}}} end"},
    "commands":[
      {"id":"format","title":"Format","broker":{"capabilities":["document.read","document.edit","tools.run"],"tools":{"format":{"protocol":"filter","workspace":false,"arguments":[]}}},"action":{"kind":"lua","source":"format.lua"}},
      {"id":"hover","title":"Hover","provider":{"kind":"hover","events":["manual","close"]},"broker":{"capabilities":["document.read","events.observe","ui.present","tools.run"],"tools":{"server":{"protocol":"language_server","workspace":false,"arguments":[]}}},"action":{"kind":"lua","source":"hover.lua"}},
      {"id":"neutral","title":"Neutral","broker":{"capabilities":["ui.present"]},"action":{"kind":"lua","source":"neutral.lua"}}
    ]});
    fs::write(&manifest, source.to_string()).unwrap();
    let hover = json!({"hover":{"range":{"start":0,"finish":5},"text":"Unicode word"}});
    let cases = json!([
      {"command":"scoped-fixture.format","input":"café / 東京","expected":"CAFÉ / 東京","expected_cache":{"calls":2},
       "broker":{"exchanges":[{"intent":{"tool":{"profile":"format","input":"café / 東京","arguments":{}}},"response":{"tool":{"status":0,"output":"CAFÉ / 東京"}}}]}},
      {"command":"scoped-fixture.hover","input":"café / 東京","expected":"café / 東京","selections":[{"anchor":0,"head":0}],
       "expected_result":hover,"broker":{"exchanges":[{"intent":{"language_server":{"profile":"server","method":"hover","position":0}},"response":{"language_server":{"result":hover}}}]}},
      {"command":"scoped-fixture.hover","input":"private original","expected":"private original","trigger":"close","cache":{"old":true},"expected_cache":{},"expected_result":"closed"},
      {"command":"scoped-fixture.neutral","input":"private original","expected":"private original","expected_result":{"presentation":{"status":"Ready","output":null,"annotations":[],"quick_list":null,"input":null}}}
    ]);
    fs::write(&fixtures, cases.to_string()).unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_hedit"))
            .args(["plugin", "test"])
            .arg(&manifest)
            .arg(&fixtures)
            .arg("--timings")
            .output()
            .unwrap()
    };
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.matches("PASS ").count(), 4);
    assert_eq!(stdout.matches("TIMING ").count(), 4);
    for mutation in ["missing", "intent", "response", "cache", "scope"] {
        let mut cases = cases.clone();
        let case = &mut cases[0];
        match mutation {
            "missing" => case["broker"]["exchanges"] = json!([]),
            "intent" => {
                case["broker"]["exchanges"][0]["intent"]["tool"]["input"] = json!("different")
            }
            "response" => {
                case["broker"]["exchanges"][0]["response"] =
                    json!({"read":{"text":"different","next":null}})
            }
            "cache" => case["expected_cache"] = json!({"calls":1}),
            "scope" => case["broker"]["scope"] = json!(true),
            _ => unreachable!(),
        }
        fs::write(&fixtures, cases.to_string()).unwrap();
        assert!(
            !run().status.success(),
            "accepted malformed {mutation} fixture"
        );
    }
}
