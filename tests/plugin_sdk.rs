//! Exercise the public SDK through the real editor CLI and native helper.
use std::{fs, process::Command};
fn prepare() -> &'static str {
    let binary = env!("CARGO_BIN_EXE_hedit");
    #[cfg(target_os = "macos")]
    {
        static READY: std::sync::OnceLock<()> = std::sync::OnceLock::new();
        READY.get_or_init(|| {
            let result = Command::new("python3")
                .arg(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/scripts/macos_plugin_helper.py"
                ))
                .arg("--editor")
                .arg(binary)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        });
    }
    binary
}
#[test]
fn sdk_fixtures_run_confined_with_arguments_unicode_and_crlf() {
    let result = Command::new(prepare())
        .args([
            "plugin",
            "test",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/examples/extensions/sdk-markdown.json"
            ),
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/examples/extensions/sdk-markdown.fixtures.json"
            ),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&result.stdout)
            .matches("PASS")
            .count(),
        2
    );
}
#[test]
fn sdk_rejects_forged_targets_invalid_edits_and_extra_authority() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("plugin.json");
    let fixture = directory.path().join("fixtures.json");
    fs::write(&fixture,r#"[{"command":"sdk-markdown.prefix","input":"private fixture","expected":"private fixture"}]"#).unwrap();
    for source in [
        "return function(ctx) local t=hedit.replace(ctx.document.text); t.document=string.rep('f',32); return t end",
        "return function(ctx) local t=hedit.replace(ctx.document.text); t.revision='1'; return t end",
        "return function(ctx) return hedit.transaction({hedit.edit(0,999999,'')}) end",
        "assert(io ~= nil or os ~= nil or require ~= nil); return function(ctx) return hedit.replace(ctx.document.text) end",
        "return function(ctx) local t=hedit.replace(ctx.document.text); t.extra=true; return t end",
    ] {
        let mut manifest:serde_json::Value=serde_json::from_str(include_str!("../examples/extensions/sdk-markdown.json")).unwrap();
        manifest["scripts"]["prefix.lua"]=source.into();fs::write(&path,manifest.to_string()).unwrap();
        let result=Command::new(prepare()).arg("plugin").arg("test").arg(&path).arg(&fixture).output().unwrap();
        assert!(!result.status.success());
        assert!(!String::from_utf8_lossy(&result.stderr).contains("private fixture"));
    }
}

#[test]
fn sdk_pack_embeds_sibling_modules_and_resources_and_rejects_traversal() {
    let directory = tempfile::tempdir().unwrap();
    let manifest = directory.path().join("author.json");
    let bundle = directory.path().join("author.hedit-plugin");
    let fixture = directory.path().join("fixtures.json");
    fs::write(directory.path().join("prefix.lua"),"return function(ctx) return hedit.replace(hedit.import('helper.lua') .. hedit.resource('suffix')) end").unwrap();
    fs::write(
        directory.path().join("helper.lua"),
        "assert(io==nil and require==nil); return '→ '",
    )
    .unwrap();
    fs::write(directory.path().join("suffix.txt"), "東京").unwrap();
    let mut source = serde_json::json!({"api_version":3,"sdk_version":1,"id":"pack-test","permissions":["document.read","document.edit"],"commands":[{"id":"run","title":"Run","action":{"kind":"lua","source":"prefix.lua"}}],"script_files":["prefix.lua"],"module_files":["helper.lua"],"resource_files":{"suffix":"suffix.txt"}});
    fs::write(&manifest, source.to_string()).unwrap();
    fs::write(
        &fixture,
        r#"[{"command":"pack-test.run","input":"unused","expected":"→ 東京"}]"#,
    )
    .unwrap();
    let result = Command::new(prepare())
        .arg("plugin")
        .arg("pack")
        .arg(&manifest)
        .arg(&bundle)
        .args(["--version", "1.0.0"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    fs::remove_file(directory.path().join("helper.lua")).unwrap();
    fs::remove_file(directory.path().join("suffix.txt")).unwrap();
    let result = Command::new(prepare())
        .arg("plugin")
        .arg("test")
        .arg(&bundle)
        .arg(&fixture)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    source["module_files"] = serde_json::json!(["../outside.lua"]);
    fs::write(&manifest, source.to_string()).unwrap();
    assert!(!Command::new(prepare())
        .arg("plugin")
        .arg("check")
        .arg(&manifest)
        .status()
        .unwrap()
        .success());
}

#[test]
fn private_storage_fixtures_are_explicit_and_do_not_write_configuration() {
    let directory = tempfile::tempdir().unwrap();
    let manifest = directory.path().join("state.json");
    let fixture = directory.path().join("fixtures.json");
    let mut source = serde_json::json!({"api_version":3,"sdk_version":1,"id":"state-test","permissions":["document.read","document.edit","storage.private"],"scripts":{"run.lua":"return function(ctx) local t=hedit.transaction({});t.storage={count=ctx.storage.count+1};return t end"},"commands":[{"id":"run","title":"Counter","private_storage":true,"action":{"kind":"lua","source":"run.lua"}}]});
    fs::write(&manifest, source.to_string()).unwrap();
    fs::write(&fixture,r#"[{"command":"state-test.run","input":"private fixture","expected":"private fixture","storage":{"count":3},"expected_storage":{"count":4}}]"#).unwrap();
    let result = Command::new(prepare())
        .arg("plugin")
        .arg("test")
        .arg(&manifest)
        .arg(&fixture)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
    source["permissions"] = serde_json::json!(["document.read", "document.edit"]);
    fs::write(&manifest, source.to_string()).unwrap();
    let result = Command::new(prepare())
        .arg("plugin")
        .arg("test")
        .arg(&manifest)
        .arg(&fixture)
        .output()
        .unwrap();
    assert!(!result.status.success());
}

#[test]
fn sdk_reference_converter_and_save_formatter_pass_their_executable_fixtures() {
    for name in ["sdk-table", "sdk-trim"] {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/extensions");
        let result = Command::new(prepare())
            .args(["plugin", "test"])
            .arg(root.join(format!("{name}.json")))
            .arg(root.join(format!("{name}.fixtures.json")))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

#[test]
fn sdk_reports_verified_syntax_locations_without_exposing_runtime_text() {
    let directory = tempfile::tempdir().unwrap();
    let manifest = directory.path().join("diagnostic.json");
    let fixture = directory.path().join("fixtures.json");
    fs::write(
        &fixture,
        r#"[{"command":"diagnostic.run","input":"private fixture","expected":"private fixture"}]"#,
    )
    .unwrap();
    for (source, module, expected) in [
        ("-- first\nlocal x =\n@", "return true", "plugin:3"),
        (
            "return function(ctx) return hedit.import('helper.lua') end",
            "-- first\nlocal x =\n@",
            "helper.lua:3",
        ),
        (
            "return function(ctx) error(ctx.document.text) end",
            "return true",
            "The Lua plugin raised an error.",
        ),
    ] {
        let package = serde_json::json!({"api_version":3,"sdk_version":1,"id":"diagnostic","permissions":["document.read","document.edit"],"scripts":{"run.lua":source},"modules":{"helper.lua":module},"commands":[{"id":"run","title":"Run","action":{"kind":"lua","source":"run.lua"}}]});
        fs::write(&manifest, package.to_string()).unwrap();
        let result = Command::new(prepare())
            .args(["plugin", "test"])
            .arg(&manifest)
            .arg(&fixture)
            .output()
            .unwrap();
        assert!(!result.status.success());
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("private fixture"));
        assert!(!error.contains('@'));
    }
}

#[test]
fn opt_in_phase_samples_have_only_fixed_names_and_numeric_counters() {
    let result = Command::new(prepare())
        .args([
            "plugin",
            "test",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/examples/extensions/sdk-markdown.json"
            ),
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/examples/extensions/sdk-markdown.fixtures.json"
            ),
            "--timings",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8(result.stdout).unwrap();
    let mut batches = 0;
    for line in stdout
        .lines()
        .filter_map(|line| line.strip_prefix("TIMING "))
    {
        let samples: Vec<serde_json::Value> = serde_json::from_str(line).unwrap();
        let phases: std::collections::BTreeSet<_> = samples
            .iter()
            .map(|sample| sample["phase"].as_str().unwrap())
            .collect();
        for phase in [
            "startup",
            "delivery",
            "transport",
            "worker",
            "decode",
            "encode",
        ] {
            assert!(phases.contains(phase));
        }
        for sample in samples {
            assert_eq!(sample.as_object().unwrap().len(), 3);
            assert!(sample["total_us"].is_u64());
            assert_eq!(sample["count"], 1);
        }
        batches += 1;
    }
    assert_eq!(batches, 2);
    assert!(!stdout.contains("café") && !stdout.contains("東京"));
}

#[test]
fn provider_reference_fixtures_run_with_the_distinct_native_role() {
    let result = Command::new(prepare())
        .args([
            "plugin",
            "test",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/examples/extensions/document-assist.json"
            ),
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/examples/extensions/document-assist.fixtures.json"
            ),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&result.stdout)
            .matches("PASS")
            .count(),
        4
    );
}
