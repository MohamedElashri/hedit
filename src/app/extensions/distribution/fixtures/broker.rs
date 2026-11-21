//! Offline transcript runner. Responses are fixture data; no host effects occur.
use super::*;
use crate::plugin_runtime::{broker, sdk, service, telemetry};
use hedit_broker::declaration::{Capability as C, Definition, ToolProtocol};
use std::time::{Duration, Instant};

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Transcript {
    #[serde(default)]
    scope: bool,
    #[serde(default)]
    exchanges: Vec<Exchange>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Exchange {
    intent: broker::Intent,
    response: broker::Response,
}

pub(super) fn run(
    manifest: &Manifest,
    definition: &Definition,
    source: &str,
    case: &Fixture,
    settings: BTreeMap<String, hedit::plugin_options::Value>,
    trace: &telemetry::Recorder,
) -> Result<String, String> {
    if case.storage.is_some() || case.expected_storage.is_some() {
        return Err("SDK 3 uses explicit cache, without legacy private storage.".into());
    }
    let empty = Transcript::default();
    let transcript = case.broker.as_ref().unwrap_or(&empty);
    if transcript.exchanges.len() >= broker::MAX_CALLBACKS {
        return Err("Supply at most 31 exchanges before the final callback.".into());
    }
    if transcript.scope
        && !definition
            .capabilities
            .iter()
            .any(|cap| cap.workspace().is_some())
    {
        return Err("A synthetic scope requires a declared workspace capability.".into());
    }
    let readable = definition.has(C::DocumentRead);
    let mut snapshot = crate::app::extensions::options::sdk_snapshot(
        &case.language,
        if readable {
            case.input.clone()
        } else {
            String::new()
        },
        if readable {
            case.selections.clone().unwrap_or_else(|| {
                vec![sdk::Selection {
                    anchor: 0,
                    head: case.input.len(),
                }]
            })
        } else {
            vec![sdk::Selection { anchor: 0, head: 0 }]
        },
        readable && case.input.contains("\r\n"),
    )
    .map_err(|error| error.to_string())?;
    snapshot.document = "a".repeat(32);
    snapshot.revision = "0".into();
    let request: broker::Handle = serde_json::from_value("f".repeat(32).into()).unwrap();
    let provider = manifest
        .provider(&case.command)
        .map(|provider| -> Result<service::Request, String> {
            if !case.arguments.is_empty() {
                return Err("Providers do not accept command arguments.".into());
            }
            let trigger = case.trigger.unwrap_or_else(|| {
                if provider.events.contains(&service::Trigger::Manual) {
                    service::Trigger::Manual
                } else {
                    provider.events[0]
                }
            });
            if trigger != service::Trigger::Interaction && !provider.events.contains(&trigger) {
                return Err("Fixture trigger is not declared by the provider.".into());
            }
            Ok(service::Request {
                id: request.to_string(),
                kind: provider.kind,
                trigger,
                position: snapshot.selections[0].head,
                action: case.action.clone(),
                choice: case.choice.clone(),
            })
        })
        .transpose()?;
    if provider.is_none()
        && (case.trigger.is_some() || case.action.is_some() || case.choice.is_some())
    {
        return Err("Trigger/action/choice fixture fields require a provider.".into());
    }
    let mut input = broker::Input {
        protocol_version: broker::VERSION,
        request,
        step: 0,
        package: sdk::Input {
            sdk_version: sdk::VERSION,
            source: source.into(),
            modules: manifest
                .modules
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect(),
            resources: manifest.resources.as_ref().clone(),
            snapshot: snapshot.clone(),
            settings,
            arguments: manifest.fixture_arguments(&case.command, &case.arguments)?,
            storage: None,
        },
        scope: transcript
            .scope
            .then(|| serde_json::from_value("c".repeat(32).into()).unwrap()),
        may_edit: definition.has(C::DocumentEdit),
        workspace_edit: definition.has(C::Edit) && transcript.scope,
        may_present: definition.has(C::Present),
        provider,
        response: None,
        cache: case.cache.clone(),
    };
    if input
        .provider
        .as_ref()
        .is_some_and(|request| request.trigger == service::Trigger::Close)
    {
        input.package.snapshot.text.clear();
        input.package.snapshot.selections = vec![sdk::Selection { anchor: 0, head: 0 }];
        input.cache = Default::default();
        if let Some(provider) = &mut input.provider {
            provider.position = 0;
        }
        input.may_edit = false;
        input.workspace_edit = false;
        input.scope = None;
    }
    let started = Instant::now();
    for index in 0..broker::MAX_CALLBACKS {
        if started.elapsed() >= Duration::from_secs(10) {
            return Err("Broker fixture exceeded ten seconds of active work.".into());
        }
        let output = crate::plugin_runtime::run_broker_fixture_traced(input.clone(), trace)
            .map_err(|error| error.to_string())?;
        if let broker::Intent::Finish {
            transaction,
            result,
        } = output.intent
        {
            if index != transcript.exchanges.len() {
                return Err("Callback finished before all transcript exchanges.".into());
            }
            if output.cache
                != case
                    .expected_cache
                    .clone()
                    .unwrap_or_else(|| case.cache.clone())
            {
                return Err("Broker cache differs from expected_cache.".into());
            }
            match (&case.expected_result, &result) {
                (Some(expected), Some(result))
                    if serde_json::to_value(result).map_err(|e| e.to_string())? == *expected => {}
                (None, None) => {}
                _ => return Err("Broker result differs from expected_result.".into()),
            }
            return match (transaction.as_ref(), result.as_deref()) {
                (Some(transaction), _) | (_, Some(service::ResultSet::Formatting(transaction))) => {
                    transaction
                        .prepare(&input.package.snapshot)
                        .map(|prepared| prepared.text)
                        .map_err(|error| error.to_string())
                }
                _ => Ok(case.input.clone()),
            };
        }
        let exchange = transcript
            .exchanges
            .get(index)
            .ok_or("Callback requested an operation missing from the transcript.")?;
        authorize(definition, &output.intent)?;
        if serde_json::to_value(&output.intent).map_err(|e| e.to_string())?
            != serde_json::to_value(&exchange.intent).map_err(|e| e.to_string())?
        {
            return Err("Broker intent differs from the transcript.".into());
        }
        response_kind(&output.intent, &exchange.response)?;
        input.step += 1;
        input.response = Some(exchange.response.clone());
        input.cache = output.cache;
        input.validate().map_err(|error| error.to_string())?;
    }
    Err("Broker fixture did not finish within 32 callbacks.".into())
}
fn authorize(definition: &Definition, intent: &broker::Intent) -> Result<(), String> {
    use broker::Intent as I;
    let capability = match intent {
        I::Discover { .. } => C::Discover,
        I::Search { .. } | I::Open { .. } | I::OpenMany { .. } | I::Read { .. } => C::Read,
        I::Release { .. } => return Ok(()),
        I::StageEdits { .. } | I::ReviewEdits { .. } => C::Edit,
        I::Watch { .. } | I::Unwatch { .. } | I::WaitChanges { .. } => C::Watch,
        I::Tool {
            profile,
            input,
            arguments,
        } => {
            let tool = definition
                .tools
                .get(profile)
                .ok_or("Undeclared tool profile.")?;
            if tool.protocol != ToolProtocol::Filter || input.len() > broker::MAX_CHUNK_BYTES {
                return Err("Tool request does not match its filter profile.".into());
            }
            tool.argv(arguments)?;
            C::Tool
        }
        I::LanguageServer { profile, .. } => {
            if !definition
                .tools
                .get(profile)
                .is_some_and(|tool| tool.protocol == ToolProtocol::LanguageServer)
            {
                return Err("Undeclared language-server profile.".into());
            }
            C::Tool
        }
        I::Https { endpoint, body } => {
            let endpoint = definition
                .endpoints
                .get(endpoint)
                .ok_or("Undeclared HTTPS endpoint.")?;
            if body.len() > endpoint.upload
                || (matches!(endpoint.method, hedit_broker::declaration::Method::GET)
                    && !body.is_empty())
            {
                return Err("HTTPS body exceeds the declared upload policy.".into());
            }
            C::Https
        }
        I::Finish { .. } => return Ok(()),
    };
    if definition.has(capability) {
        Ok(())
    } else {
        Err(format!("Broker intent requires {}.", capability.name()))
    }
}
fn response_kind(intent: &broker::Intent, response: &broker::Response) -> Result<(), String> {
    use broker::{Intent as I, Response as R};
    let matches = matches!(
        (intent, response),
        (_, R::Failed { .. })
            | (I::Discover { .. }, R::Discovered { .. })
            | (I::Search { .. }, R::Searched { .. })
            | (I::Open { .. }, R::Opened(_))
            | (I::OpenMany { .. }, R::OpenedMany { .. })
            | (I::Read { .. }, R::Read { .. })
            | (I::Release { .. }, R::Released { .. })
            | (I::StageEdits { .. }, R::Staged { .. })
            | (I::ReviewEdits { .. }, R::Edited { .. })
            | (I::Watch { .. }, R::Watching { .. })
            | (I::Unwatch { .. }, R::Unwatched { .. })
            | (I::WaitChanges { .. }, R::Changed { .. })
            | (I::Tool { .. }, R::Tool { .. })
            | (I::LanguageServer { .. }, R::LanguageServer { .. })
            | (I::Https { .. }, R::Https { .. })
    );
    if matches {
        response.validate().map_err(|error| error.to_string())
    } else {
        Err("Transcript response does not match the broker operation.".into())
    }
}
