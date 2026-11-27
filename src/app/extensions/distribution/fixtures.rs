use super::*;
mod broker;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    command: String,
    #[serde(default)]
    broker: Option<broker::Transcript>,
    #[serde(default = "plain_text")]
    language: String,
    input: String,
    expected: String,
    #[serde(default)]
    settings: BTreeMap<String, hedit::plugin_options::Value>,
    #[serde(default)]
    arguments: BTreeMap<String, hedit::plugin_options::Value>,
    #[serde(default)]
    selections: Option<Vec<crate::plugin_runtime::sdk::Selection>>,
    #[serde(default)]
    storage: Option<crate::plugin_runtime::sdk::Storage>,
    #[serde(default)]
    expected_storage: Option<crate::plugin_runtime::sdk::Storage>,
    #[serde(default)]
    expected_result: Option<serde_json::Value>,
    #[serde(default)]
    cache: crate::plugin_runtime::sdk::Storage,
    #[serde(default)]
    expected_cache: Option<crate::plugin_runtime::sdk::Storage>,
    #[serde(default)]
    trigger: Option<crate::plugin_runtime::service::Trigger>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    choice: Option<hedit::plugin_options::Value>,
}

fn plain_text() -> String {
    hedit::syntax::Language::PlainText.id().into()
}

pub(super) fn test(path: &Path, fixtures: &Path) -> Result<(), String> {
    test_traced(path, fixtures, false)
}
pub(super) fn test_traced(path: &Path, fixtures: &Path, timings: bool) -> Result<(), String> {
    let (manifest, _) = load_cli_manifest(path)?;
    let bytes = read_cli_file(fixtures, MAX_FIXTURE_BYTES)?;
    let fixtures: Vec<Fixture> =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if !(1..=MAX_FIXTURES).contains(&fixtures.len()) {
        return Err(format!("Supply between 1 and {MAX_FIXTURES} fixtures."));
    }
    let mut failures = 0;
    for (index, case) in fixtures.into_iter().enumerate() {
        let command = case.command.clone();
        let trace = crate::plugin_runtime::telemetry::Recorder::default();
        let result: Result<(), String> = (|| {
            let source = manifest
                .lua_source(&case.command)
                .ok_or("Unknown Lua command.")?;
            if hedit::syntax::Language::from_id(&case.language).is_none()
                && !hedit::plugin_options::valid_command_id(&case.language)
            {
                return Err("Unknown fixture language.".into());
            }
            let settings = manifest.fixture_settings(&case.settings)?;
            if manifest.broker(&case.command).is_none() && case.broker.is_some() {
                return Err("Broker transcripts require manifest API 5 / SDK 3.".into());
            }
            let result = if let Some(declaration) = manifest.broker(&case.command) {
                broker::run(&manifest, declaration, source, &case, settings, &trace)
            } else if let Some(definition) = manifest.provider(&case.command) {
                use crate::plugin_runtime::{sdk, service};
                if !case.arguments.is_empty()
                    || case.storage.is_some()
                    || case.expected_storage.is_some()
                {
                    return Err("Providers use explicit cache, without command arguments or private storage.".into());
                }
                let original = case.input;
                let mut snapshot = crate::app::extensions::options::sdk_snapshot(
                    &case.language,
                    original.clone(),
                    case.selections
                        .unwrap_or_else(|| vec![sdk::Selection { anchor: 0, head: 0 }]),
                    original.contains("\r\n"),
                )
                .map_err(|error| error.to_string())?;
                snapshot.document = "a".repeat(32);
                snapshot.revision = "0".into();
                let trigger = case.trigger.unwrap_or_else(|| {
                    if definition.events.contains(&service::Trigger::Manual) {
                        service::Trigger::Manual
                    } else {
                        definition.events[0]
                    }
                });
                if trigger != service::Trigger::Interaction && !definition.events.contains(&trigger)
                {
                    return Err("Fixture trigger is not declared by the provider.".into());
                }
                let expected_cache = case.expected_cache.unwrap_or_else(|| case.cache.clone());
                let input = service::Input {
                    protocol_version: service::VERSION,
                    request: service::Request {
                        id: "f".repeat(32),
                        kind: definition.kind,
                        trigger,
                        position: snapshot.selections[0].head,
                        action: case.action,
                        choice: case.choice,
                    },
                    package: sdk::Input {
                        sdk_version: sdk::VERSION,
                        source: source.to_owned(),
                        modules: manifest
                            .modules
                            .iter()
                            .map(|(k, v)| (k.clone(), v.to_string()))
                            .collect(),
                        resources: manifest.resources.as_ref().clone(),
                        snapshot: snapshot.clone(),
                        settings,
                        arguments: Default::default(),
                        storage: None,
                    },
                    cache: case.cache,
                    may_edit: definition.may_edit,
                };
                crate::plugin_runtime::run_service_fixture(input)
                    .map_err(|error| error.to_string())
                    .and_then(|output| {
                        if output.cache != expected_cache {
                            return Err("Provider cache differs from expected_cache.".into());
                        }
                        if let Some(expected) = case.expected_result {
                            if serde_json::to_value(&output.result).map_err(|e| e.to_string())?
                                != expected
                            {
                                return Err("Provider result differs from expected_result.".into());
                            }
                        } else if !matches!(output.result, service::ResultSet::Formatting(_)) {
                            return Err("Supply expected_result for a provider fixture.".into());
                        }
                        match output.result {
                            service::ResultSet::Formatting(transaction) => transaction
                                .prepare(&snapshot)
                                .map(|prepared| prepared.text)
                                .map_err(|error| error.to_string()),
                            _ => Ok(original),
                        }
                    })
            } else if manifest.sdk {
                let len = case.input.len();
                let crlf = case.input.contains("\r\n");
                let snapshot = crate::app::extensions::options::sdk_snapshot(
                    &case.language,
                    case.input,
                    case.selections.unwrap_or_else(|| {
                        vec![crate::plugin_runtime::sdk::Selection {
                            anchor: 0,
                            head: len,
                        }]
                    }),
                    crlf,
                )
                .map_err(|e| e.to_string())?;
                let arguments = manifest.fixture_arguments(&case.command, &case.arguments)?;
                let storage_allowed = manifest.uses_storage(&case.command);
                if !storage_allowed && (case.storage.is_some() || case.expected_storage.is_some()) {
                    return Err("This command has no private storage permission.".into());
                }
                let initial_storage = storage_allowed.then(|| case.storage.unwrap_or_default());
                crate::plugin_runtime::run_sdk_fixture_traced(
                    crate::plugin_runtime::sdk::Input {
                        sdk_version: crate::plugin_runtime::sdk::VERSION,
                        modules: manifest
                            .modules
                            .iter()
                            .map(|(k, v)| (k.clone(), v.to_string()))
                            .collect(),
                        resources: manifest.resources.as_ref().clone(),
                        source: source.to_owned(),
                        snapshot,
                        settings,
                        arguments,
                        storage: initial_storage,
                    },
                    &trace,
                )
                .and_then(|prepared| {
                    if prepared.storage != case.expected_storage {
                        return Err(crate::plugin_runtime::Error::Worker.into());
                    }
                    Ok(prepared.text)
                })
                .map_err(|error| error.to_string())
            } else {
                if !case.arguments.is_empty()
                    || case.selections.is_some()
                    || case.storage.is_some()
                    || case.expected_storage.is_some()
                {
                    return Err("Structured fixture fields require API version 3.".into());
                }
                crate::plugin_runtime::run_fixture(crate::plugin_runtime::Input {
                    source: source.to_owned(),
                    selections: vec![case.input],
                    language: case.language,
                    settings,
                })
                .map_err(|error| error.to_string())
            }?;
            if result != case.expected {
                return Err("Output differs from expected text.".into());
            }
            Ok(())
        })();
        if timings {
            println!(
                "TIMING {}",
                serde_json::to_string(&trace.snapshot())
                    .map_err(|_| "Cannot encode numeric timings.")?
            );
        }
        match result {
            Ok(()) => println!("PASS {} {}", index + 1, command),
            Err(error) => {
                failures += 1;
                eprintln!("FAIL {} {:?} ({error})", index + 1, command);
            }
        }
    }
    if failures > 0 {
        return Err(format!("{failures} fixture(s) failed."));
    }
    Ok(())
}
