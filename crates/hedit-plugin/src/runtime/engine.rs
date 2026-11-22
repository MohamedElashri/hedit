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

pub(super) fn run_helper(isolated: bool) -> bool {
    run_helper_io(isolated, io::stdin().lock(), io::stdout().lock())
}

fn run_helper_io(isolated: bool, input: impl Read, mut output: impl Write) -> bool {
    // No attacker-influenced request bytes or Lua code are consumed before
    // the process is isolated. Error is reported through a fixed status code.
    let result = if isolated {
        protocol::decode_request(input).and_then(|input| {
            let edits = evaluate(input)?;
            serde_json::to_vec(&edits).map_err(|_| Error::Worker)
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

    let result = call_transform(&lua, environment, request);
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

fn lua_error(error: mlua::Error) -> Error {
    match error {
        mlua::Error::SyntaxError { .. } => Error::Syntax,
        mlua::Error::MemoryError(_) => Error::Memory,
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
}
