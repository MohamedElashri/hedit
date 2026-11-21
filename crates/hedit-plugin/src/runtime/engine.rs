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
    let request = protocol::encode_sdk(&input)?;
    let reply = super::supervisor::supervise(
        &super::supervisor::executable()?,
        request,
        cancelled,
        if save {
            Duration::from_millis(400)
        } else {
            DEADLINE
        },
        approval,
    )?;
    decode_sdk_reply(&input, &reply)
}
pub(super) fn decode_sdk_reply(
    input: &crate::sdk::Input,
    reply: &[u8],
) -> Result<crate::sdk::Transaction, crate::sdk::Failure> {
    if reply.len() > protocol::MAX_BATCH_REPLY_BYTES {
        return Err(Error::Worker.into());
    }
    let reply: crate::sdk::Reply = serde_json::from_slice(reply).map_err(|_| Error::Worker)?;
    if reply.protocol_version != 1 {
        return Err(Error::Worker.into());
    }
    match reply.outcome {
        crate::sdk::Outcome::Success { transaction } => {
            if transaction.storage.is_some() && input.storage.is_none() {
                return Err(Error::Worker.into());
            }
            transaction.prepare(&input.snapshot)?;
            Ok(transaction)
        }
        crate::sdk::Outcome::Failure { failure } => {
            failure.validate(input)?;
            Err(failure)
        }
    }
}

pub(super) fn run_helper(isolated: bool) -> bool {
    run_helper_io(isolated, io::stdin().lock(), io::stdout().lock())
}

fn run_helper_io(isolated: bool, input: impl Read, mut output: impl Write) -> bool {
    // No attacker-influenced request bytes or Lua code are consumed before
    // the process is isolated. Error is reported through a fixed status code.
    let result = if isolated {
        protocol::decode_invocation(input).and_then(|input| match input {
            protocol::Invocation::Legacy(input) => {
                serde_json::to_vec(&evaluate(input)?).map_err(|_| Error::Worker)
            }
            protocol::Invocation::Sdk(input) => {
                let location = Rc::new(std::cell::RefCell::new(None));
                let result = with_lua(|lua, environment| {
                    crate::sdk::lua::evaluate_detailed(lua, environment, input, location.clone())
                });
                let outcome = match result {
                    Ok(transaction) => crate::sdk::Outcome::Success { transaction },
                    Err(error) => crate::sdk::Outcome::Failure {
                        failure: crate::sdk::Failure::evaluation(
                            error,
                            location.borrow_mut().take(),
                        ),
                    },
                };
                serde_json::to_vec(&crate::sdk::Reply {
                    protocol_version: 1,
                    outcome,
                })
                .map_err(|_| Error::Worker)
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
    let lua = Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH,
        LuaOptions::default(),
    )
    .map_err(|_| Error::Initialization)?;
    lua.set_memory_limit(LUA_MEMORY_BYTES)
        .map_err(|_| Error::Initialization)?;
    let environment = environment(&lua).map_err(|_| Error::Initialization)?;

    let remaining = Rc::new(Cell::new(INSTRUCTION_BUDGET));
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
        let valid = serde_json::json!({"protocol_version":1,"outcome":{"status":"success","transaction":{"document":input.snapshot.document,"revision":"1","edits":[]}}});
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
                reply["outcome"]["transaction"][path] = value;
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
            let reply = serde_json::json!({"protocol_version":1,"outcome":{"status":"failure","failure":{"kind":"syntax","location":{"script":script,"line":line}}}});
            let error = decode_sdk_reply(&input, &serde_json::to_vec(&reply).unwrap()).unwrap_err();
            assert_eq!(error.location.is_some(), accepted);
            assert!(!error.to_string().contains("private fixture"));
        }
        assert!(decode_sdk_reply(&input, br#"{"protocol_version":1,"protocol_version":1,"outcome":{"status":"failure","failure":{"kind":"syntax"}}}"#).is_err());
    }
}
