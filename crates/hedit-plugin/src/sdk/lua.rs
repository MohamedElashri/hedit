//! Lua bindings have no editor references; every result is validated by the host.
use super::{Edit, Input, Selection, Transaction, MAX_EDITS};
use crate::{options, Error, MAX_SELECTIONS};
use mlua::chunk::ChunkMode;
use mlua::{Lua, Table, Value};

fn failure(_: mlua::Error) -> Error {
    Error::Execution
}
fn fields(table: &Table, allowed: &[&str]) -> Result<(), Error> {
    for pair in table.clone().pairs::<Value, Value>() {
        let (key, _) = pair.map_err(failure)?;
        let Value::String(key) = key else {
            return Err(Error::ReturnType);
        };
        let key = key.to_str().map_err(|_| Error::ReturnEncoding)?;
        if !allowed.contains(&key.as_ref()) {
            return Err(Error::ReturnType);
        }
    }
    Ok(())
}
fn text(table: &Table, key: &str, max: usize) -> Result<String, Error> {
    let Value::String(value) = table.raw_get(key).map_err(failure)? else {
        return Err(Error::ReturnType);
    };
    if value.as_bytes().len() > max {
        return Err(Error::ReturnSize);
    }
    value
        .to_str()
        .map(|s| s.to_owned())
        .map_err(|_| Error::ReturnEncoding)
}
fn index(table: &Table, key: &str) -> Result<usize, Error> {
    let Value::Integer(value) = table.raw_get(key).map_err(failure)? else {
        return Err(Error::ReturnType);
    };
    usize::try_from(value).map_err(|_| Error::ReturnType)
}
fn array(table: &Table, max: usize) -> Result<Vec<Table>, Error> {
    let len = table.raw_len();
    if len > max {
        return Err(Error::ReturnSize);
    }
    let mut count = 0;
    for pair in table.clone().pairs::<Value, Value>() {
        let (key, _) = pair.map_err(failure)?;
        let Value::Integer(key) = key else {
            return Err(Error::ReturnType);
        };
        if key < 1 || key as usize > len {
            return Err(Error::ReturnType);
        }
        count += 1;
        if count > max {
            return Err(Error::ReturnSize);
        }
    }
    if count != len {
        return Err(Error::ReturnType);
    }
    (1..=len)
        .map(|at| table.raw_get::<Table>(at).map_err(|_| Error::ReturnType))
        .collect()
}
fn options(
    lua: &Lua,
    values: &std::collections::BTreeMap<String, options::Value>,
) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    for (key, value) in values {
        match value {
            options::Value::Boolean(value) => table.raw_set(key.as_str(), *value),
            options::Value::Integer(value) => table.raw_set(key.as_str(), *value),
            options::Value::Text(value) => table.raw_set(key.as_str(), value.as_str()),
        }?;
    }
    Ok(table)
}

#[cfg(test)]
pub(crate) fn evaluate(lua: &Lua, environment: Table, input: Input) -> Result<Transaction, Error> {
    evaluate_detailed(
        lua,
        environment,
        input,
        std::rc::Rc::new(std::cell::RefCell::new(None)),
    )
}
fn diagnostic(
    error: &mlua::Error,
    script: &str,
    source: &str,
    location: &std::cell::RefCell<Option<super::Location>>,
) {
    let mlua::Error::SyntaxError { message, .. } = error else {
        return;
    };
    let prefix = format!("[string \"{script}\"]:");
    let short = format!("{script}:");
    let Some(rest) = message
        .strip_prefix(&prefix)
        .or_else(|| message.strip_prefix(&short))
    else {
        return;
    };
    let Some((line, _)) = rest.split_once(':') else {
        return;
    };
    let Ok(line) = line.parse::<u32>() else {
        return;
    };
    if line > 0
        && line as usize <= source.bytes().filter(|b| *b == b'\n').count() + 1
        && location.borrow().is_none()
    {
        *location.borrow_mut() = Some(super::Location {
            script: script.into(),
            line,
        });
    }
}
pub(crate) fn evaluate_detailed(
    lua: &Lua,
    environment: Table,
    input: Input,
    location: std::rc::Rc<std::cell::RefCell<Option<super::Location>>>,
) -> Result<Transaction, Error> {
    input.validate()?;
    let document = lua.create_table().map_err(failure)?;
    document
        .raw_set("id", input.snapshot.document.as_str())
        .map_err(failure)?;
    document
        .raw_set("revision", input.snapshot.revision.as_str())
        .map_err(failure)?;
    document
        .raw_set("text", input.snapshot.text.as_str())
        .map_err(failure)?;
    document
        .raw_set("language", input.snapshot.language.as_str())
        .map_err(failure)?;
    document
        .raw_set("crlf", input.snapshot.crlf)
        .map_err(failure)?;
    let selections = lua.create_table().map_err(failure)?;
    for (at, selection) in input.snapshot.selections.iter().enumerate() {
        let range = lua.create_table().map_err(failure)?;
        range.raw_set("anchor", selection.anchor).map_err(failure)?;
        range.raw_set("head", selection.head).map_err(failure)?;
        selections.raw_set(at + 1, range).map_err(failure)?;
    }
    document
        .raw_set("selections", selections)
        .map_err(failure)?;
    let context = lua.create_table().map_err(failure)?;
    context.raw_set("document", document).map_err(failure)?;
    context
        .raw_set("settings", options(lua, &input.settings).map_err(failure)?)
        .map_err(failure)?;
    context
        .raw_set(
            "arguments",
            options(lua, &input.arguments).map_err(failure)?,
        )
        .map_err(failure)?;
    let storage_allowed = input.storage.is_some();
    if let Some(storage) = &input.storage {
        context
            .raw_set("storage", options(lua, &storage.0).map_err(failure)?)
            .map_err(failure)?;
    }
    let api = lua.create_table().map_err(failure)?;
    let identity = (
        input.snapshot.document.clone(),
        input.snapshot.revision.clone(),
    );
    api.raw_set(
        "transaction",
        lua.create_function(move |lua, (edits, selections): (Table, Option<Table>)| {
            let transaction = lua.create_table()?;
            transaction.raw_set("document", identity.0.as_str())?;
            transaction.raw_set("revision", identity.1.as_str())?;
            transaction.raw_set("edits", edits)?;
            if let Some(selections) = selections {
                transaction.raw_set("selections", selections)?;
            }
            Ok(transaction)
        })
        .map_err(failure)?,
    )
    .map_err(failure)?;
    api.raw_set(
        "edit",
        lua.create_function(
            |lua, (start, finish, text): (usize, usize, mlua::LuaString)| {
                let edit = lua.create_table()?;
                edit.raw_set("start", start)?;
                edit.raw_set("finish", finish)?;
                edit.raw_set("text", text)?;
                Ok(edit)
            },
        )
        .map_err(failure)?,
    )
    .map_err(failure)?;
    let identity = (
        input.snapshot.document.clone(),
        input.snapshot.revision.clone(),
        input.snapshot.text.len(),
    );
    api.raw_set(
        "replace",
        lua.create_function(move |lua, text: mlua::LuaString| {
            let transaction = lua.create_table()?;
            transaction.raw_set("document", identity.0.as_str())?;
            transaction.raw_set("revision", identity.1.as_str())?;
            let edits = lua.create_table()?;
            let edit = lua.create_table()?;
            edit.raw_set("start", 0)?;
            edit.raw_set("finish", identity.2)?;
            edit.raw_set("text", text)?;
            edits.raw_set(1, edit)?;
            transaction.raw_set("edits", edits)?;
            Ok(transaction)
        })
        .map_err(failure)?,
    )
    .map_err(failure)?;
    let coordinates = std::rc::Rc::new(super::Coordinates::new(&input.snapshot.text));
    let snapshot = coordinates.clone();
    api.raw_set(
        "line_column",
        lua.create_function(move |_, byte: usize| {
            snapshot
                .line_column(byte)
                .map_err(|_| mlua::Error::RuntimeError("invalid coordinate".into()))
        })
        .map_err(failure)?,
    )
    .map_err(failure)?;
    let snapshot = coordinates;
    api.raw_set(
        "offset",
        lua.create_function(move |_, (line, column): (usize, usize)| {
            snapshot
                .offset(line, column)
                .map_err(|_| mlua::Error::RuntimeError("invalid coordinate".into()))
        })
        .map_err(failure)?,
    )
    .map_err(failure)?;
    let modules = input.modules;
    let cache = lua.create_table().map_err(failure)?;
    let stack = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let module_environment = environment.clone();
    let module_location = location.clone();
    api.raw_set(
        "import",
        lua.create_function(move |lua, name: String| {
            let cached: Value = cache.raw_get(name.as_str())?;
            if !matches!(cached, Value::Nil) {
                return Ok(cached);
            }
            let source = modules
                .get(&name)
                .ok_or_else(|| mlua::Error::RuntimeError("unknown module".into()))?;
            {
                let mut stack = stack.borrow_mut();
                if stack.len() >= 8 || stack.contains(&name) {
                    return Err(mlua::Error::RuntimeError(
                        "module import cycle or depth limit".into(),
                    ));
                }
                stack.push(name.clone());
            }
            let result = lua
                .load(source.as_bytes())
                .set_name(name.as_str())
                .set_mode(ChunkMode::Text)
                .set_environment(module_environment.clone())
                .eval::<Value>();
            stack.borrow_mut().pop();
            if let Err(error) = &result {
                diagnostic(error, &name, source, &module_location);
            }
            let value = result?;
            if matches!(value, Value::Nil) {
                return Err(mlua::Error::RuntimeError(
                    "module must return a value".into(),
                ));
            }
            cache.raw_set(name, value.clone())?;
            Ok(value)
        })
        .map_err(failure)?,
    )
    .map_err(failure)?;
    let resources = input.resources;
    api.raw_set(
        "resource",
        lua.create_function(move |lua, name: String| {
            let resource = resources
                .get(&name)
                .ok_or_else(|| mlua::Error::RuntimeError("unknown resource".into()))?;
            lua.create_string(resource.as_bytes())
        })
        .map_err(failure)?,
    )
    .map_err(failure)?;
    environment.raw_set("hedit", api).map_err(failure)?;
    let entry = lua
        .load(input.source.as_bytes())
        .set_name("plugin")
        .set_mode(ChunkMode::Text)
        .set_environment(environment)
        .into_function()
        .map_err(|error| {
            diagnostic(&error, "plugin", &input.source, &location);
            super::super::runtime::engine::lua_error(error)
        })?
        .call::<Value>(())
        .map_err(super::super::runtime::engine::lua_error)?;
    let Value::Function(entry) = entry else {
        return Err(Error::EntryPoint);
    };
    let result = entry
        .call::<Value>(context)
        .map_err(super::super::runtime::engine::lua_error)?;
    let Value::Table(table) = result else {
        return Err(Error::ReturnType);
    };
    fields(
        &table,
        &["document", "revision", "edits", "selections", "storage"],
    )?;
    let edits = array(
        &table
            .raw_get::<Table>("edits")
            .map_err(|_| Error::ReturnType)?,
        MAX_EDITS,
    )?;
    let mut output = Vec::with_capacity(edits.len());
    let mut bytes = 0usize;
    for edit in edits {
        fields(&edit, &["start", "finish", "text"])?;
        let replacement = text(&edit, "text", super::MAX_OUTPUT_BYTES)?;
        bytes = bytes
            .checked_add(replacement.len())
            .ok_or(Error::ReturnSize)?;
        if bytes > super::MAX_OUTPUT_BYTES {
            return Err(Error::ReturnSize);
        }
        output.push(Edit {
            start: index(&edit, "start")?,
            finish: index(&edit, "finish")?,
            text: replacement,
        });
    }
    let selections = match table.raw_get::<Value>("selections").map_err(failure)? {
        Value::Nil => None,
        Value::Table(selections) => Some(
            array(&selections, MAX_SELECTIONS)?
                .into_iter()
                .map(|selection| {
                    fields(&selection, &["anchor", "head"])?;
                    Ok(Selection {
                        anchor: index(&selection, "anchor")?,
                        head: index(&selection, "head")?,
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?,
        ),
        _ => return Err(Error::ReturnType),
    };
    let storage = match table.raw_get::<Value>("storage").map_err(failure)? {
        Value::Nil => None,
        Value::Table(values) if storage_allowed => {
            let mut output = std::collections::BTreeMap::new();
            for pair in values.pairs::<String, Value>() {
                let (key, value) = pair.map_err(failure)?;
                if output.len() == crate::options::MAX_SETTINGS || !crate::options::valid_id(&key) {
                    return Err(Error::ReturnSize);
                }
                let value = match value {
                    Value::Boolean(value) => crate::options::Value::Boolean(value),
                    Value::Integer(value) => crate::options::Value::Integer(
                        i32::try_from(value).map_err(|_| Error::ReturnType)?,
                    ),
                    Value::String(value)
                        if value.as_bytes().len() <= crate::options::MAX_VALUE_BYTES =>
                    {
                        crate::options::Value::Text(
                            value
                                .to_str()
                                .map_err(|_| Error::ReturnEncoding)?
                                .to_owned(),
                        )
                    }
                    _ => return Err(Error::ReturnType),
                };
                if !value.valid() {
                    return Err(Error::ReturnEncoding);
                }
                output.insert(key, value);
            }
            Some(super::Storage(output))
        }
        _ => return Err(Error::Worker),
    };
    let transaction = Transaction {
        document: text(&table, "document", 32)?,
        revision: text(&table, "revision", 20)?,
        edits: output,
        selections,
        storage,
    };
    transaction.prepare(&input.snapshot)?;
    Ok(transaction)
}
