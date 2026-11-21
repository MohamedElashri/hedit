//! Opt-in synthetic measurements; never a timing assertion in the test suite.
use hedit_plugin::sdk::{Edit, Selection, Snapshot, Transaction};
use std::{fs, hint::black_box, process::Command, time::Instant};

fn percentiles(samples: &[u128]) -> serde_json::Value {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let at = |percent: usize| sorted[(sorted.len() * percent).div_ceil(100).saturating_sub(1)];
    serde_json::json!({"samples_us":samples,"p50_us":at(50),"p95_us":at(95),"p99_us":at(99)})
}

#[test]
#[ignore = "manual synthetic latency probe; writes HEDIT_SDK_PROBE_OUT"]
fn sdk_latency_probe() {
    let out = std::env::var_os("HEDIT_SDK_PROBE_OUT")
        .expect("Set HEDIT_SDK_PROBE_OUT to a JSON output path");
    let binary = env!("CARGO_BIN_EXE_hedit");
    #[cfg(target_os = "macos")]
    assert!(Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/macos_plugin_helper.py"
        ))
        .args(["--editor", binary])
        .status()
        .unwrap()
        .success());
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("probe.json");
    let fixtures = directory.path().join("fixtures.json");
    let source = "return function(ctx) local edits={} for _,r in ipairs(ctx.document.selections) do edits[#edits+1]=hedit.edit(r.head,r.head,'→ ') end return hedit.transaction(edits) end";
    fs::write(&package, serde_json::json!({"api_version":3,"sdk_version":1,"id":"probe","permissions":["document.read","document.edit"],"scripts":{"run.lua":source},"commands":[{"id":"run","title":"Probe","action":{"kind":"lua","source":"run.lua"}}]}).to_string()).unwrap();
    let mut workloads = Vec::new();
    for (name, text, count) in [
        ("small-unicode", "café / 東京\r\n".repeat(10), 1),
        ("near-limit-unicode", "東京\r\n".repeat(8192), 1),
        ("sixteen-ranges-near-end", "東京\r\n".repeat(8192), 16),
    ] {
        let selections: Vec<_> = (0..count)
            .map(|i| Selection {
                anchor: text.len() - (count - i) * 8,
                head: text.len() - (count - i) * 8,
            })
            .collect();
        let snapshot = Snapshot {
            document: "0123456789abcdef0123456789abcdef".into(),
            revision: "42".into(),
            language: "markdown".into(),
            crlf: true,
            text: text.clone(),
            selections: selections.clone(),
        };
        let transaction = Transaction {
            document: snapshot.document.clone(),
            revision: snapshot.revision.clone(),
            edits: selections
                .iter()
                .map(|s| Edit {
                    start: s.head,
                    finish: s.head,
                    text: "→ ".into(),
                })
                .collect(),
            selections: None,
            storage: None,
        };
        let expected = transaction.prepare(&snapshot).unwrap().text;
        fs::write(&fixtures, serde_json::json!([{"command":"probe.run","input":text,"expected":expected,"selections":selections}]).to_string()).unwrap();
        let mut prepare = Vec::new();
        for _ in 0..500 {
            let start = Instant::now();
            black_box(transaction.prepare(black_box(&snapshot)).unwrap());
            prepare.push(start.elapsed().as_micros());
        }
        let mut invocations = Vec::new();
        for _ in 0..41 {
            let start = Instant::now();
            let result = Command::new(binary)
                .args(["plugin", "test"])
                .arg(&package)
                .arg(&fixtures)
                .output()
                .unwrap();
            let micros = start.elapsed().as_micros();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            invocations.push(micros);
        }
        workloads.push(serde_json::json!({"name":name,"snapshot_bytes":text.len(),"edits":count,"host_validation_and_preparation":percentiles(&prepare),"first_cli_invocation_us":invocations[0],"repeated_cli_invocations":percentiles(&invocations[1..])}));
    }
    let rustc = Command::new("rustc").arg("--version").output().unwrap();
    let revision = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let clean = Command::new("git")
        .args(["diff", "--quiet", "HEAD"])
        .status()
        .unwrap()
        .success();
    fs::write(out,serde_json::to_vec_pretty(&serde_json::json!({"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"debug_assertions":cfg!(debug_assertions),"revision":String::from_utf8_lossy(&revision.stdout).trim(),"working_tree_dirty":!clean,"rustc":String::from_utf8_lossy(&rustc.stdout).trim(),"scope":"Synthetic single-command measurements. CLI includes editor process startup, fixture parsing, confinement, IPC, Lua, validation, and process exit. Preparation excludes authorization, GUI, Undo, and commit. First invocation is not a guaranteed filesystem cold start. No UI responsiveness or cross-platform claim.","workloads":workloads})).unwrap()).unwrap();
}
