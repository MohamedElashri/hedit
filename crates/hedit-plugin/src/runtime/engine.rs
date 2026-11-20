//! Shared Lua 5.4 engine, bounded protocol, and invocation limits.
use super::{Input, MAX_SELECTIONS, MAX_SELECTION_BYTES, MAX_SOURCE_BYTES};
use crate::{
    options::{self as plugin_options, Value as SettingValue},
    Error,
};
use mlua::chunk::ChunkMode;
use mlua::{HookTriggers, Lua, LuaOptions, StdLib, Table, Value, VmState};
use std::cell::Cell;
use std::fs::File;
use std::io::{self, Read, Write};
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const MAX_LANGUAGE_BYTES: usize = 256;
const MAX_REPLACEMENT_BYTES: usize = 256 * 1024;
const MAX_BATCH_REQUEST_BYTES: usize = 1024 * 1024;
#[cfg(any(unix, windows))]
pub(super) const MAX_BATCH_REPLY_BYTES: usize = 2 * 1024 * 1024;
const REQUEST_MAGIC: u32 = u32::MAX - 1;
const LUA_MEMORY_BYTES: usize = 16 * 1024 * 1024;
const INSTRUCTION_INTERVAL: u32 = 1_000;
const INSTRUCTION_BUDGET: u32 = 1_000_000;
pub(super) const DEADLINE: Duration = Duration::from_secs(2);

impl Input {
    fn validate(&self) -> Result<(), Error> {
        if self.source.len() > MAX_SOURCE_BYTES
            || self.language.len() > MAX_LANGUAGE_BYTES
            || !(1..=MAX_SELECTIONS).contains(&self.selections.len())
            || self.selections.iter().map(String::len).sum::<usize>() > MAX_SELECTION_BYTES
            || self.settings.len() > plugin_options::MAX_SETTINGS
            || self
                .settings
                .iter()
                .any(|(key, value)| !plugin_options::valid_id(key) || !value.valid())
        {
            return Err(Error::Request);
        }
        Ok(())
    }
}

fn encode_request(input: &Input) -> Result<Vec<u8>, Error> {
    input.validate()?;
    let payload = serde_json::to_vec(input).map_err(|_| Error::Request)?;
    if payload.len() > MAX_BATCH_REQUEST_BYTES {
        return Err(Error::Request);
    }
    let mut bytes = Vec::with_capacity(payload.len() + 8);
    bytes.extend(REQUEST_MAGIC.to_le_bytes());
    bytes.extend((payload.len() as u32).to_le_bytes());
    bytes.extend(payload);
    Ok(bytes)
}

fn decode_request(mut input: impl Read) -> Result<Input, Error> {
    let mut header = [0; 8];
    input.read_exact(&mut header).map_err(|_| Error::Request)?;
    if u32::from_le_bytes(header[..4].try_into().unwrap()) != REQUEST_MAGIC {
        return Err(Error::Request);
    }
    let size = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
    if size > MAX_BATCH_REQUEST_BYTES {
        return Err(Error::Request);
    }
    let mut payload = vec![0; size];
    input.read_exact(&mut payload).map_err(|_| Error::Request)?;
    if input.read(&mut [0; 1]).map_err(|_| Error::Request)? != 0 {
        return Err(Error::Request);
    }
    let request: Input = serde_json::from_slice(&payload).map_err(|_| Error::Request)?;
    request.validate()?;
    Ok(request)
}

pub(super) fn run(
    input: Input,
    approval: Option<File>,
    cancelled: &AtomicBool,
    save: bool,
) -> Result<Vec<String>, Error> {
    let count = input.selections.len();
    let request = encode_request(&input)?;
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
    let edits: Vec<String> = serde_json::from_slice(&reply).map_err(|_| Error::Worker)?;
    if edits.len() != count {
        return Err(Error::Worker);
    }
    validate_edits(&edits)?;
    Ok(edits)
}

pub(super) fn run_helper(isolated: bool) -> bool {
    // No attacker-influenced request bytes or Lua code are consumed before
    // the process is isolated. Error is reported through a fixed status code.
    let result = if isolated {
        decode_request(io::stdin().lock()).and_then(|input| {
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
    let mut output = io::stdout().lock();
    output
        .write_all(&[status])
        .and_then(|_| output.write_all(body))
        .and_then(|_| output.flush())
        .is_ok()
}

#[cfg(any(unix, windows))]
pub(super) fn worker_reply(mut reply: Vec<u8>, delivered: bool) -> Result<Vec<u8>, Error> {
    if reply.len() > MAX_BATCH_REPLY_BYTES + 1 {
        return Err(Error::Worker);
    }
    let Some((&status, body)) = reply.split_first() else {
        return Err(Error::Worker);
    };
    if status != 0 {
        return Err(if body.is_empty() {
            Error::from_worker_code(status)
        } else {
            Error::Worker
        });
    }
    if !delivered {
        return Err(Error::Worker);
    }
    Ok(reply.split_off(1))
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
        validate_edits(&edits)?;
    }
    Ok(edits)
}

fn validate_edits(edits: &[String]) -> Result<(), Error> {
    if edits.iter().map(String::len).sum::<usize>() > MAX_REPLACEMENT_BYTES {
        return Err(Error::ReturnSize);
    }
    if edits.iter().any(|edit| edit.contains('\0')) {
        return Err(Error::ReturnEncoding);
    }
    Ok(())
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
    if bytes.len() > MAX_REPLACEMENT_BYTES {
        return Err(Error::ReturnSize);
    }
    if bytes.contains(&0) {
        return Err(Error::ReturnEncoding);
    }
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| Error::ReturnEncoding)
}

#[cfg(all(test, any(unix, windows)))]
mod tests {
    use super::*;

    #[test]
    fn parent_and_worker_enforce_the_same_source_byte_limit() {
        let mut source = "-- café / 東京\nreturn function(text) return text end".to_owned();
        source.push_str(&" ".repeat(MAX_SOURCE_BYTES - source.len()));
        let mut input = Input {
            source,
            selections: vec!["private text".into()],
            language: "markdown".into(),
            settings: Default::default(),
        };
        let packet = encode_request(&input).unwrap();
        assert_eq!(
            decode_request(packet.as_slice()).unwrap().source,
            input.source
        );

        input.source.push(' ');
        assert_eq!(encode_request(&input), Err(Error::Request));
        // Bypass the parent encoder to verify the worker's independent check.
        let payload = serde_json::to_vec(&input).unwrap();
        let mut packet = REQUEST_MAGIC.to_le_bytes().to_vec();
        packet.extend((payload.len() as u32).to_le_bytes());
        packet.extend(payload);
        assert!(matches!(
            decode_request(packet.as_slice()),
            Err(Error::Request)
        ));
    }

    #[test]
    fn worker_failures_keep_their_type_and_reject_payloads_or_unknown_statuses() {
        for (status, error) in [
            (1, Error::Sandbox),
            (6, Error::Execution),
            (7, Error::Instructions),
        ] {
            assert_eq!(worker_reply(vec![status], true), Err(error));
        }
        for reply in [vec![], vec![255], b"\x07private text".to_vec()] {
            assert_eq!(worker_reply(reply, true), Err(Error::Worker));
        }
        assert_eq!(worker_reply(b"\0[]".to_vec(), false), Err(Error::Worker));
    }
}
