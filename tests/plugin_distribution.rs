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
