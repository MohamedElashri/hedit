//! Shared Lua 5.4 engine, bounded protocol, and invocation limits.
use super::{Input, MAX_SELECTIONS, MAX_SELECTION_BYTES};
use crate::options::{self as plugin_options, Value as SettingValue};
use mlua::chunk::ChunkMode;
use mlua::{HookTriggers, Lua, LuaOptions, StdLib, Table, Value, VmState};
use std::cell::Cell;
use std::fs::File;
use std::io::{self, Read, Write};
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const MAX_SOURCE_BYTES: usize = 64 * 1024;
const MAX_LANGUAGE_BYTES: usize = 256;
const MAX_REPLACEMENT_BYTES: usize = 256 * 1024;
const MAX_BATCH_REQUEST_BYTES: usize = 1024 * 1024;
#[cfg(any(unix, windows))]
pub(super) const MAX_BATCH_REPLY_BYTES: usize = 2 * 1024 * 1024;
const BATCH_REQUEST: u32 = u32::MAX - 1;
const LUA_MEMORY_BYTES: usize = 16 * 1024 * 1024;
const INSTRUCTION_INTERVAL: u32 = 1_000;
const INSTRUCTION_BUDGET: u32 = 1_000_000;
pub(super) const DEADLINE: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub(super) enum Failure {
    Sandbox = 1,
    Request,
    Initialization,
    Syntax,
    EntryPoint,
    Execution,
    Instructions,
    Memory,
    ReturnType,
    ReturnEncoding,
    ReturnSize,
    Worker,
    #[cfg(any(unix, windows))]
    Deadline,
    #[cfg(any(unix, windows))]
    Cancelled,
}

impl Failure {
    fn message(self) -> &'static str {
        match self {
            Self::Sandbox => "The OS could not isolate the Lua worker.",
            Self::Request => "The Lua plugin input is invalid or exceeds its limits.",
            Self::Initialization => "The Lua runtime could not start.",
            Self::Syntax => "The plugin has a Lua syntax error.",
            Self::EntryPoint => "The Lua script must return a function.",
            Self::Execution => "The Lua plugin raised an error.",
            Self::Instructions => "The Lua plugin reached its instruction limit.",
            Self::Memory => "The Lua plugin reached its memory limit.",
            Self::ReturnType => "The Lua plugin must return a string.",
            Self::ReturnEncoding => "The plugin replacement must be UTF-8 text without NUL bytes.",
            Self::ReturnSize => "The Lua plugin replacement exceeds 256 KiB.",
            Self::Worker => "The Lua worker stopped or returned an invalid response.",
            #[cfg(any(unix, windows))]
            Self::Deadline => "The Lua plugin exceeded its time limit.",
            #[cfg(any(unix, windows))]
            Self::Cancelled => "The Lua plugin was cancelled.",
        }
    }

    #[cfg(any(unix, windows))]
    pub(super) fn from_worker_code(code: u8) -> Self {
        match code {
            1 => Self::Sandbox,
            2 => Self::Request,
            3 => Self::Initialization,
            4 => Self::Syntax,
            5 => Self::EntryPoint,
            6 => Self::Execution,
            7 => Self::Instructions,
            8 => Self::Memory,
            9 => Self::ReturnType,
            10 => Self::ReturnEncoding,
            11 => Self::ReturnSize,
            _ => Self::Worker,
        }
    }
}

impl Input {
    fn validate(&self) -> Result<(), Failure> {
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
            return Err(Failure::Request);
        }
        Ok(())
    }
}

fn encode_request(input: &Input) -> Result<Vec<u8>, Failure> {
    input.validate()?;
    let payload = serde_json::to_vec(input).map_err(|_| Failure::Request)?;
    if payload.len() > MAX_BATCH_REQUEST_BYTES {
        return Err(Failure::Request);
    }
    let mut bytes = Vec::with_capacity(payload.len() + 8);
    bytes.extend(BATCH_REQUEST.to_le_bytes());
    bytes.extend((payload.len() as u32).to_le_bytes());
    bytes.extend(payload);
    Ok(bytes)
}

fn decode_request(mut input: impl Read) -> Result<(Input, bool), Failure> {
    fn length(input: &mut impl Read) -> Result<u32, Failure> {
        let mut size = [0; 4];
        input.read_exact(&mut size).map_err(|_| Failure::Request)?;
        Ok(u32::from_le_bytes(size))
    }

    fn bytes(input: &mut impl Read, size: u32, maximum: usize) -> Result<Vec<u8>, Failure> {
        if size as usize > maximum {
            return Err(Failure::Request);
        }
        let mut bytes = vec![0; size as usize];
        input.read_exact(&mut bytes).map_err(|_| Failure::Request)?;
        Ok(bytes)
    }

    fn text(input: &mut impl Read, size: u32, maximum: usize) -> Result<String, Failure> {
        String::from_utf8(bytes(input, size, maximum)?).map_err(|_| Failure::Request)
    }

    let first = length(&mut input)?;
    let batch = first == BATCH_REQUEST;
    let request = if batch {
        let size = length(&mut input)?;
        let payload = bytes(&mut input, size, MAX_BATCH_REQUEST_BYTES)?;
        serde_json::from_slice(&payload).map_err(|_| Failure::Request)?
    } else {
        let source = text(&mut input, first, MAX_SOURCE_BYTES)?;
        let size = length(&mut input)?;
        let selection = text(&mut input, size, MAX_SELECTION_BYTES)?;
        let size = length(&mut input)?;
        let language = text(&mut input, size, MAX_LANGUAGE_BYTES)?;
        Input {
            source,
            selections: vec![selection],
            language,
            settings: Default::default(),
        }
    };
    if input.read(&mut [0; 1]).map_err(|_| Failure::Request)? != 0 {
        return Err(Failure::Request);
    }
    request.validate()?;
    Ok((request, batch))
}

pub(super) fn run(
    input: Input,
    approval: Option<File>,
    cancelled: &AtomicBool,
    save: bool,
) -> Result<Vec<String>, String> {
    let count = input.selections.len();
    let response = (|| {
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
        let edits: Vec<String> = serde_json::from_slice(&reply).map_err(|_| Failure::Worker)?;
        if edits.len() != count {
            return Err(Failure::Worker);
        }
        validate_edits(&edits)?;
        Ok(edits)
    })();
    response.map_err(|failure| failure.message().to_owned())
}

pub(super) fn run_helper(isolated: bool) -> bool {
    // No attacker-influenced request bytes or Lua code are consumed before
    // the process is isolated. Failure is reported through a fixed status code.
    let result = if isolated {
        decode_request(io::stdin().lock()).and_then(|(input, batch)| {
            let edits = evaluate(input, batch)?;
            if batch {
                serde_json::to_vec(&edits).map_err(|_| Failure::Worker)
            } else {
                Ok(edits
                    .into_iter()
                    .next()
                    .ok_or(Failure::Worker)?
                    .into_bytes())
            }
        })
    } else {
        Err(Failure::Sandbox)
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
pub(super) fn worker_reply(mut reply: Vec<u8>, delivered: bool) -> Result<Vec<u8>, Failure> {
    if reply.len() > MAX_BATCH_REPLY_BYTES + 1 {
        return Err(Failure::Worker);
    }
    let Some((&status, body)) = reply.split_first() else {
        return Err(Failure::Worker);
    };
    if status != 0 {
        return Err(if body.is_empty() {
            Failure::from_worker_code(status)
        } else {
            Failure::Worker
        });
    }
    if !delivered {
        return Err(Failure::Worker);
    }
    Ok(reply.split_off(1))
}

fn evaluate(request: Input, batch: bool) -> Result<Vec<String>, Failure> {
    let lua = Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH,
        LuaOptions::default(),
    )
    .map_err(|_| Failure::Initialization)?;
    lua.set_memory_limit(LUA_MEMORY_BYTES)
        .map_err(|_| Failure::Initialization)?;
    let environment = environment(&lua).map_err(|_| Failure::Initialization)?;

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
    .map_err(|_| Failure::Initialization)?;

    let result = call_transform(&lua, environment, request, batch);
    if remaining.get() == 0 {
        return Err(Failure::Instructions);
    }
    result
}

fn call_transform(
    lua: &Lua,
    environment: Table,
    request: Input,
    batch: bool,
) -> Result<Vec<String>, Failure> {
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
        return Err(Failure::EntryPoint);
    };
    let mut edits = Vec::with_capacity(request.selections.len());
    for selection in &request.selections {
        let result = if batch {
            let settings = lua.create_table().map_err(lua_error)?;
            for (key, value) in &request.settings {
                match value {
                    SettingValue::Boolean(value) => settings.raw_set(key.as_str(), *value),
                    SettingValue::Integer(value) => settings.raw_set(key.as_str(), *value),
                    SettingValue::Text(value) => settings.raw_set(key.as_str(), value.as_str()),
                }
                .map_err(lua_error)?;
            }
            entry.call::<Value>((selection.as_str(), request.language.as_str(), settings))
        } else {
            entry.call::<Value>((selection.as_str(), request.language.as_str()))
        }
        .map_err(lua_error)?;
        let Value::String(replacement) = result else {
            return Err(Failure::ReturnType);
        };
        edits.push(valid_replacement(&replacement.as_bytes())?);
        validate_edits(&edits)?;
    }
    Ok(edits)
}

fn validate_edits(edits: &[String]) -> Result<(), Failure> {
    if edits.iter().map(String::len).sum::<usize>() > MAX_REPLACEMENT_BYTES {
        return Err(Failure::ReturnSize);
    }
    if edits.iter().any(|edit| edit.contains('\0')) {
        return Err(Failure::ReturnEncoding);
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

fn lua_error(error: mlua::Error) -> Failure {
    match error {
        mlua::Error::SyntaxError { .. } => Failure::Syntax,
        mlua::Error::MemoryError(_) => Failure::Memory,
        _ => Failure::Execution,
    }
}

fn valid_replacement(bytes: &[u8]) -> Result<String, Failure> {
    if bytes.len() > MAX_REPLACEMENT_BYTES {
        return Err(Failure::ReturnSize);
    }
    if bytes.contains(&0) {
        return Err(Failure::ReturnEncoding);
    }
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| Failure::ReturnEncoding)
}
