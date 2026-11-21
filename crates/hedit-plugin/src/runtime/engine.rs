//! Shared Lua 5.4 evaluation and invocation limits.
use super::{protocol, Input};
use crate::{options::Value as SettingValue, Error};
use mlua::chunk::ChunkMode;
use mlua::{HookTriggers, Lua, LuaOptions, StdLib, Table, Value, VmState};
use std::cell::Cell;
use std::fs::File;
use std::io::{self, Read, Write};
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const LUA_MEMORY_BYTES: usize = 16 * 1024 * 1024;
const INSTRUCTION_INTERVAL: u32 = 1_000;
const INSTRUCTION_BUDGET: u32 = 1_000_000;
pub(super) const DEADLINE: Duration = Duration::from_secs(2);

pub(super) fn run(
    input: Input,
    approval: Option<File>,
    cancelled: &AtomicBool,
    save: bool,
) -> Result<Vec<String>, Error> {
    let count = input.selections.len();
    let request = protocol::encode_request(&input)?;
    let timeout = if save {
        Duration::from_millis(400)
    } else {
        DEADLINE
    };
    let reply = super::supervisor::supervise(
        &super::supervisor::executable()?,
        request,
        cancelled,
        timeout,
        approval,
    )?;
    protocol::decode_edits(&reply, count)
}

pub(super) fn run_sdk(
    input: crate::sdk::Input,
    approval: Option<File>,
    cancelled: &AtomicBool,
    save: bool,
) -> Result<crate::sdk::Transaction, crate::sdk::Failure> {
    run_sdk_profiled(input, approval, cancelled, save, &Default::default())
}

pub(super) fn run_sdk_profiled(
    input: crate::sdk::Input,
    approval: Option<File>,
    cancelled: &AtomicBool,
    save: bool,
    trace: &crate::telemetry::Recorder,
) -> Result<crate::sdk::Transaction, crate::sdk::Failure> {
    use crate::telemetry::Phase;
    if !super::SUPPORTED {
        return Err(Error::Unavailable.into());
    }
    let request = trace.measure(Phase::Encode, || protocol::encode_sdk(&input))?;
    let reply = super::supervisor::supervise_profiled(
        &super::supervisor::executable()?,
        request,
        cancelled,
        if save {
            Duration::from_millis(400)
        } else {
            DEADLINE
        },
        approval,
        trace,
    )?;
    trace.measure(Phase::Decode, || {
        decode_sdk_reply_traced(&input, &reply, trace)
    })
}
#[cfg(any(test, feature = "fuzzing"))]
pub(super) fn decode_sdk_reply(
    input: &crate::sdk::Input,
    reply: &[u8],
) -> Result<crate::sdk::Transaction, crate::sdk::Failure> {
    decode_sdk_reply_traced(input, reply, &Default::default())
}
fn decode_sdk_reply_traced(
    input: &crate::sdk::Input,
    reply: &[u8],
    trace: &crate::telemetry::Recorder,
) -> Result<crate::sdk::Transaction, crate::sdk::Failure> {
    if reply.len() > protocol::MAX_BATCH_REPLY_BYTES {
        return Err(Error::Worker.into());
    }
    let reply: crate::sdk::Reply = serde_json::from_slice(reply).map_err(|_| Error::Worker)?;
    if reply.protocol_version != 1 || reply.worker_time_us > 2_000_000 {
        return Err(Error::Worker.into());
    }
    trace.record(
        crate::telemetry::Phase::Worker,
        std::time::Duration::from_micros(reply.worker_time_us),
    );
    match reply.outcome {
        crate::sdk::Outcome::Success(transaction) => {
            if transaction.storage.is_some() && input.storage.is_none() {
                return Err(Error::Worker.into());
            }
            transaction.prepare(&input.snapshot)?;
            Ok(transaction)
        }
        crate::sdk::Outcome::Failure(failure) => {
            failure.validate(input)?;
            Err(failure)
        }
    }
}

pub(super) fn run_service(
    input: crate::service::Input,
    approval: Option<File>,
    cancelled: &AtomicBool,
    trace: &crate::telemetry::Recorder,
) -> Result<crate::service::Output, crate::sdk::Failure> {
    if !super::SUPPORTED {
        return Err(Error::Unavailable.into());
    }
    use crate::telemetry::Phase;
    let request = trace.measure(Phase::Encode, || protocol::encode_service(&input))?;
    let reply = super::supervisor::supervise_service(
        &super::supervisor::executable()?,
        request,
        cancelled,
        Duration::from_millis(crate::service::CALLBACK_MILLIS),
        approval,
        trace,
    )?;
    trace.measure(Phase::Decode, || decode_service_reply(&input, &reply))
}
pub(super) fn decode_service_reply(
    input: &crate::service::Input,
    bytes: &[u8],
) -> Result<crate::service::Output, crate::sdk::Failure> {
    if bytes.len() > crate::service::MAX_RESULT_BYTES + 8192 {
        return Err(Error::Worker.into());
    }
    let reply: crate::service::Reply = serde_json::from_slice(bytes).map_err(|_| Error::Worker)?;
    if reply.protocol_version != crate::service::VERSION {
        return Err(Error::Worker.into());
    }
    match reply.outcome {
        crate::service::Outcome::Success(output) => {
            output.validate(input)?;
            Ok(*output)
        }
        crate::service::Outcome::Failure(failure) => {
            failure.validate(&input.package)?;
            Err(failure)
        }
    }
}

pub(super) fn run_broker(
    input: crate::broker::Input,
    approval: Option<File>,
    cancelled: &AtomicBool,
    trace: &crate::telemetry::Recorder,
) -> Result<crate::broker::Output, crate::sdk::Failure> {
    if !super::SUPPORTED {
        return Err(Error::Unavailable.into());
    }
    use crate::telemetry::Phase;
    let request = trace.measure(Phase::Encode, || protocol::encode_broker(&input))?;
    let reply = super::supervisor::supervise_service(
        &super::supervisor::executable()?,
        request,
        cancelled,
        Duration::from_millis(crate::service::CALLBACK_MILLIS),
        approval,
        trace,
    )?;
    trace.measure(Phase::Decode, || decode_broker_reply(&input, &reply))
}
pub(super) fn decode_broker_reply(
    input: &crate::broker::Input,
    bytes: &[u8],
) -> Result<crate::broker::Output, crate::sdk::Failure> {
    if bytes.len() > crate::broker::MAX_MESSAGE_BYTES {
        return Err(Error::Worker.into());
    }
    let reply: crate::broker::Reply = serde_json::from_slice(bytes).map_err(|_| Error::Worker)?;
    if reply.protocol_version != crate::broker::VERSION {
        return Err(Error::Worker.into());
    }
    match reply.outcome {
        crate::broker::Outcome::Success(output) => {
            output.validate(input)?;
            Ok(*output)
        }
        crate::broker::Outcome::Failure(failure) => {
            failure.validate(&input.package)?;
            Err(failure)
        }
    }
}

pub(super) fn run_helper(isolated: bool) -> bool {
    run_helper_io(isolated, io::stdin().lock(), io::stdout().lock())
}

pub(super) fn run_service_helper(isolated: bool) -> bool {
    let mut output = io::stdout().lock();
    if isolated
        && output
            .write_all(&[crate::service::READY])
            .and_then(|_| output.flush())
            .is_err()
    {
        return false;
    }
    run_helper_io_for(isolated, true, io::stdin().lock(), output)
}
fn run_helper_io(isolated: bool, input: impl Read, output: impl Write) -> bool {
    run_helper_io_for(isolated, false, input, output)
}
fn run_helper_io_for(
    isolated: bool,
    service: bool,
    input: impl Read,
    mut output: impl Write,
) -> bool {
    // No attacker-influenced request bytes or Lua code are consumed before
    // the process is isolated. Error is reported through a fixed status code.
    let result = if isolated {
        protocol::decode_invocation(input).and_then(|input| {
            if service
                != matches!(
                    input,
                    protocol::Invocation::Service(_) | protocol::Invocation::Broker(_)
                )
            {
                return Err(Error::Request);
            }
            match input {
                protocol::Invocation::Legacy(input) => {
                    serde_json::to_vec(&evaluate(input)?).map_err(|_| Error::Worker)
                }
                protocol::Invocation::Service(input) => {
                    let location = Rc::new(std::cell::RefCell::new(None));
                    let result = with_lua_budget(
                        crate::service::CALLBACK_INSTRUCTIONS,
                        |lua, environment| {
                            crate::service::lua::evaluate(lua, environment, input, location.clone())
                        },
                    );
                    let outcome = match result {
                        Ok(output) => crate::service::Outcome::Success(Box::new(output)),
                        Err(error) => {
                            let mut failure = crate::sdk::Failure::evaluation(
                                error,
                                location.borrow_mut().take(),
                            );
                            failure.kind = match failure.kind {
                                Error::SdkTransaction | Error::SdkType => Error::ServiceResult,
                                Error::SdkLimit => Error::ServiceLimit,
                                other => other,
                            };
                            crate::service::Outcome::Failure(failure)
                        }
                    };
                    serde_json::to_vec(&crate::service::Reply {
                        protocol_version: crate::service::VERSION,
                        outcome,
                    })
                    .map_err(|_| Error::Worker)
                }
                protocol::Invocation::Broker(input) => {
                    let location = Rc::new(std::cell::RefCell::new(None));
                    let result = with_lua_budget(
                        crate::service::CALLBACK_INSTRUCTIONS,
                        |lua, environment| {
                            crate::broker::lua::evaluate(lua, environment, input, location.clone())
                        },
                    );
                    let outcome = match result {
                        Ok(output) => crate::broker::Outcome::Success(Box::new(output)),
                        Err(error) => crate::broker::Outcome::Failure(
                            crate::sdk::Failure::evaluation(error, location.borrow_mut().take()),
                        ),
                    };
                    serde_json::to_vec(&crate::broker::Reply {
                        protocol_version: crate::broker::VERSION,
                        outcome,
                    })
                    .map_err(|_| Error::Worker)
                }
                protocol::Invocation::Sdk(input) => {
                    let worker_started = std::time::Instant::now();
                    let location = Rc::new(std::cell::RefCell::new(None));
                    let result = with_lua(|lua, environment| {
                        crate::sdk::lua::evaluate_detailed(
                            lua,
                            environment,
                            input,
                            location.clone(),
                        )
                    });
                    let outcome = match result {
                        Ok(transaction) => crate::sdk::Outcome::Success(transaction),
                        Err(error) => crate::sdk::Outcome::Failure(
                            crate::sdk::Failure::evaluation(error, location.borrow_mut().take()),
                        ),
                    };
                    serde_json::to_vec(&crate::sdk::Reply {
                        protocol_version: 1,
                        worker_time_us: worker_started.elapsed().as_micros().min(2_000_000) as u64,
                        outcome,
                    })
                    .map_err(|_| Error::Worker)
                }
            }
        })
    } else {
        Err(Error::Sandbox)
    };
    let (status, body) = match &result {
        Ok(body) => (0, body.as_slice()),
        Err(failure) => (*failure as u8, &[][..]),
    };
    output
        .write_all(&[status])
        .and_then(|_| output.write_all(body))
        .and_then(|_| output.flush())
        .is_ok()
}

fn evaluate(request: Input) -> Result<Vec<String>, Error> {
    with_lua(|lua, environment| call_transform(lua, environment, request))
}

pub(crate) fn with_lua<T>(call: impl FnOnce(&Lua, Table) -> Result<T, Error>) -> Result<T, Error> {
    with_lua_budget(INSTRUCTION_BUDGET, call)
}
pub(crate) fn with_lua_budget<T>(
    budget: u32,
    call: impl FnOnce(&Lua, Table) -> Result<T, Error>,
) -> Result<T, Error> {
    let lua = Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH,
        LuaOptions::default(),
    )
    .map_err(|_| Error::Initialization)?;
    lua.set_memory_limit(LUA_MEMORY_BYTES)
        .map_err(|_| Error::Initialization)?;
    let environment = environment(&lua).map_err(|_| Error::Initialization)?;

    let remaining = Rc::new(Cell::new(budget));
    let hook_remaining = remaining.clone();
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(INSTRUCTION_INTERVAL),
        move |_, _| {
            let left = hook_remaining.get().saturating_sub(INSTRUCTION_INTERVAL);
            hook_remaining.set(left);
            if left == 0 {
                return Err(mlua::Error::RuntimeError("instruction limit".into()));
            }
            Ok(VmState::Continue)
        },
    )
    .map_err(|_| Error::Initialization)?;

    let result = call(&lua, environment);
    if remaining.get() == 0 {
        return Err(Error::Instructions);
    }
    result
}

fn call_transform(lua: &Lua, environment: Table, request: Input) -> Result<Vec<String>, Error> {
    let entry = lua
        .load(request.source.as_bytes())
        .set_name("plugin")
        .set_mode(ChunkMode::Text)
        .set_environment(environment)
        .into_function()
        .map_err(lua_error)?
        .call::<Value>(())
        .map_err(lua_error)?;
    let Value::Function(entry) = entry else {
        return Err(Error::EntryPoint);
    };
    let mut edits = Vec::with_capacity(request.selections.len());
    for selection in &request.selections {
        let settings = lua.create_table().map_err(lua_error)?;
        for (key, value) in &request.settings {
            match value {
                SettingValue::Boolean(value) => settings.raw_set(key.as_str(), *value),
                SettingValue::Integer(value) => settings.raw_set(key.as_str(), *value),
                SettingValue::Text(value) => settings.raw_set(key.as_str(), value.as_str()),
            }
            .map_err(lua_error)?;
        }
        let result = entry
            .call::<Value>((selection.as_str(), request.language.as_str(), settings))
            .map_err(lua_error)?;
        let Value::String(replacement) = result else {
            return Err(Error::ReturnType);
        };
        edits.push(valid_replacement(&replacement.as_bytes())?);
        protocol::validate_edits(&edits)?;
    }
    Ok(edits)
}

fn environment(lua: &Lua) -> mlua::Result<Table> {
    let globals = lua.globals();
    let environment = lua.create_table()?;
    // The string metatable points to this same library table.
    globals
        .raw_get::<Table>("string")?
        .raw_set("dump", Value::Nil)?;
    for name in [
        "_VERSION", "assert", "error", "ipairs", "next", "pairs", "select", "tonumber", "tostring",
        "type", "math", "string", "table", "utf8",
    ] {
        environment.raw_set(name, globals.raw_get::<Value>(name)?)?;
    }
    Ok(environment)
}

pub(crate) fn lua_error(error: mlua::Error) -> Error {
    match error {
        mlua::Error::SyntaxError { .. } => Error::Syntax,
        mlua::Error::MemoryError(_) => Error::Memory,
        mlua::Error::CallbackError { cause, .. } => lua_error(cause.as_ref().clone()),
        _ => Error::Execution,
    }
}

fn valid_replacement(bytes: &[u8]) -> Result<String, Error> {
    if bytes.len() > protocol::MAX_REPLACEMENT_BYTES {
        return Err(Error::ReturnSize);
    }
    if bytes.contains(&0) {
        return Err(Error::ReturnEncoding);
    }
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| Error::ReturnEncoding)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Unreadable;

    impl Read for Unreadable {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            panic!("unconfined worker must not read request bytes")
        }
    }

    #[test]
    fn failed_isolation_never_reads_a_request_and_returns_only_a_fixed_status() {
        let mut output = Vec::new();
        assert!(run_helper_io(false, Unreadable, &mut output));
        assert_eq!(output, [Error::Sandbox as u8]);
    }

    #[test]
    fn invocations_do_not_share_mutated_library_tables_or_lua_state() {
        let request = |source: &str| Input {
            source: source.into(),
            selections: vec!["private fixture".into()],
            language: "markdown".into(),
            settings: Default::default(),
        };
        assert_eq!(evaluate(request("string.upper = nil; math.secret = 'private fixture'; return function(text) return text end")).unwrap(), ["private fixture"]);
        assert_eq!(
            evaluate(request(
                "assert(math.secret == nil); return function(text) return text:upper() end"
            ))
            .unwrap(),
            ["PRIVATE FIXTURE"]
        );
    }
    fn sdk_input() -> crate::sdk::Input {
        serde_json::from_value(serde_json::json!({"sdk_version":1,"source":"return function(ctx) return hedit.transaction({}) end","snapshot":{"document":"0123456789abcdef0123456789abcdef","revision":"1","language":"markdown","text":"private fixture","selections":[{"anchor":0,"head":0}],"crlf":false},"settings":{},"arguments":{}})).unwrap()
    }
    #[test]
    fn sdk_reply_binds_protocol_target_authority_and_diagnostic_locations() {
        let input = sdk_input();
        let valid = serde_json::json!({"protocol_version":1,"outcome":{"success":{"document":input.snapshot.document,"revision":"1","edits":[]}}});
        let bytes = serde_json::to_vec(&valid).unwrap();
        assert!(decode_sdk_reply(&input, &bytes).is_ok());
        for index in 0..bytes.len() {
            assert!(decode_sdk_reply(&input, &bytes[..index]).is_err());
        }
        for (path, value) in [
            ("protocol_version", serde_json::json!(2)),
            ("document", serde_json::json!("f".repeat(32))),
            ("revision", serde_json::json!("2")),
            ("storage", serde_json::json!({"private":true})),
        ] {
            let mut reply = valid.clone();
            if path == "protocol_version" {
                reply[path] = value;
            } else {
                reply["outcome"]["success"][path] = value;
            }
            assert!(decode_sdk_reply(&input, &serde_json::to_vec(&reply).unwrap()).is_err());
        }
        for (script, line, accepted) in [
            ("plugin", 1, true),
            ("private fixture", 1, false),
            ("plugin", 0, false),
            ("plugin", 2, false),
            ("../plugin", 1, false),
        ] {
            let reply = serde_json::json!({"protocol_version":1,"outcome":{"failure":{"kind":"syntax","location":{"script":script,"line":line}}}});
            let error = decode_sdk_reply(&input, &serde_json::to_vec(&reply).unwrap()).unwrap_err();
            assert_eq!(error.location.is_some(), accepted);
            assert!(!error.to_string().contains("private fixture"));
        }
        assert!(decode_sdk_reply(&input, br#"{"protocol_version":1,"protocol_version":1,"outcome":{"failure":{"kind":"syntax"}}}"#).is_err());
    }
    #[test]
    fn service_roles_framing_and_result_identity_cannot_be_confused() {
        use crate::service::{self, Kind, Trigger};
        let input = service::Input {
            protocol_version: service::VERSION,
            request: service::Request {
                id: "f".repeat(32),
                kind: Kind::Diagnostics,
                trigger: Trigger::Manual,
                position: 0,
                action: None,
                choice: None,
            },
            package: sdk_input(),
            cache: Default::default(),
            may_edit: false,
        };
        let request = protocol::encode_service(&input).unwrap();
        let mut output = Vec::new();
        assert!(run_helper_io_for(
            true,
            false,
            request.as_slice(),
            &mut output
        ));
        assert_eq!(output, [Error::Request as u8]);
        output.clear();
        assert!(run_helper_io_for(
            true,
            true,
            protocol::encode_sdk(&input.package).unwrap().as_slice(),
            &mut output
        ));
        assert_eq!(output, [Error::Request as u8]);
        let valid = serde_json::json!({"protocol_version":1,"outcome":{"success":{"request":input.request.id,"document":input.package.snapshot.document,"revision":input.package.snapshot.revision,"cache":{},"result":{"diagnostics":[]}}}});
        let bytes = serde_json::to_vec(&valid).unwrap();
        decode_service_reply(&input, &bytes).unwrap();
        for end in 0..bytes.len() {
            assert!(decode_service_reply(&input, &bytes[..end]).is_err());
        }
        for field in ["request", "document", "revision"] {
            let mut bad = valid.clone();
            bad["outcome"]["success"][field] = "foreign".into();
            assert!(decode_service_reply(&input, &serde_json::to_vec(&bad).unwrap()).is_err());
        }
        let mut trailing = bytes.clone();
        trailing.extend(b"{}");
        assert!(decode_service_reply(&input, &trailing).is_err());
        assert!(decode_service_reply(&input,br#"{"protocol_version":1,"protocol_version":1,"outcome":{"failure":{"kind":"execution"}}}"#).is_err());
    }
}

#[cfg(test)]
mod broker_tests {
    use super::*;
    use crate::broker::{self, Intent};
    #[test]
    fn broker_callbacks_rotate_and_cannot_select_the_command_role() {
        let mut input = broker::tests::input();
        input.package.source="local calls=0; return function(ctx) calls=calls+1; return {intent={discover={scope=ctx.broker.scope,query=tostring(calls)}},cache={step=ctx.broker.step}} end".into();
        for step in [0, 1] {
            input.step = step;
            let bytes = protocol::encode_broker(&input).unwrap();
            let mut reply = Vec::new();
            assert!(run_helper_io_for(true, false, bytes.as_slice(), &mut reply));
            assert_eq!(reply, [Error::Request as u8]);
            reply.clear();
            assert!(run_helper_io_for(true, true, bytes.as_slice(), &mut reply));
            assert_eq!(reply[0], 0);
            let output = decode_broker_reply(&input, &reply[1..]).unwrap();
            assert!(matches!(output.intent,Intent::Discover{query,..} if query=="1"));
            assert!(decode_sdk_reply(&input.package, &reply[1..]).is_err());
        }
    }
    #[test]
    fn broker_reply_codec_rejects_truncation_duplicates_and_foreign_requests() {
        let input = broker::tests::input();
        let output = broker::Output {
            request: input.request,
            step: 0,
            intent: Intent::Discover {
                scope: input.scope.unwrap(),
                query: String::new(),
            },
            cache: Default::default(),
        };
        let bytes = serde_json::to_vec(&broker::Reply {
            protocol_version: broker::VERSION,
            outcome: broker::Outcome::Success(Box::new(output.clone())),
        })
        .unwrap();
        decode_broker_reply(&input, &bytes).unwrap();
        for end in 0..bytes.len() {
            assert!(decode_broker_reply(&input, &bytes[..end]).is_err());
        }
        let mut output = output;
        output.request = broker::Handle::fresh().unwrap();
        let bytes = serde_json::to_vec(&broker::Reply {
            protocol_version: broker::VERSION,
            outcome: broker::Outcome::Success(Box::new(output)),
        })
        .unwrap();
        assert!(decode_broker_reply(&input, &bytes).is_err());
        assert!(decode_broker_reply(&input,br#"{"protocol_version":1,"protocol_version":1,"outcome":{"failure":{"kind":"syntax"}}}"#).is_err());
    }
    #[test]
    fn new_framing_can_hold_escaped_bounded_text_without_widening_old_protocols() {
        let mut input = broker::tests::input();
        input.package.snapshot.text = "\u{1}".repeat(64 * 1024);
        input.package.resources = (0..4)
            .map(|index| (format!("resource{index}"), "\u{1}".repeat(16_000)))
            .collect();
        input.response = Some(broker::Response::Read {
            text: "\u{1}".repeat(64 * 1024),
            next: None,
        });
        let bytes = protocol::encode_broker(&input).unwrap();
        assert!(bytes.len() > 1024 * 1024);
        assert!(matches!(
            protocol::decode_invocation(bytes.as_slice()),
            Ok(protocol::Invocation::Broker(_))
        ));
        let mut wrong = bytes.clone();
        wrong[..4].copy_from_slice(&(u32::MAX - 3).to_le_bytes());
        assert!(protocol::decode_invocation(wrong.as_slice()).is_err());
        for end in [0, 1, 7, 8, bytes.len() - 1] {
            assert!(protocol::decode_invocation(&bytes[..end]).is_err());
        }
    }
}
