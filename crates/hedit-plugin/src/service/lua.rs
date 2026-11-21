//! The same restricted Lua engine, with typed provider context and results.
use super::*;
use mlua::{Lua, Table, Value};
use std::{cell::RefCell, rc::Rc};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Proposal {
    result: ResultSet,
    #[serde(default)]
    cache: Option<sdk::Storage>,
}
fn failure(_: mlua::Error) -> Error {
    Error::Execution
}
fn to_lua(lua: &Lua, value: &serde_json::Value) -> mlua::Result<Value> {
    Ok(match value {
        serde_json::Value::Null => Value::Nil,
        serde_json::Value::Bool(v) => Value::Boolean(*v),
        serde_json::Value::Number(v) => Value::Integer(
            v.as_i64()
                .ok_or_else(|| mlua::Error::RuntimeError("invalid integer".into()))?,
        ),
        serde_json::Value::String(v) => Value::String(lua.create_string(v)?),
        serde_json::Value::Array(values) => {
            let table = lua.create_table()?;
            for (at, value) in values.iter().enumerate() {
                table.raw_set(at + 1, to_lua(lua, value)?)?;
            }
            Value::Table(table)
        }
        serde_json::Value::Object(values) => {
            let table = lua.create_table()?;
            for (key, value) in values {
                table.raw_set(key.as_str(), to_lua(lua, value)?)?;
            }
            Value::Table(table)
        }
    })
}
struct Budget {
    bytes: usize,
    nodes: usize,
}
impl Budget {
    fn take(&mut self, bytes: usize) -> Result<(), Error> {
        self.nodes += 1;
        self.bytes = self.bytes.checked_add(bytes).ok_or(Error::ReturnSize)?;
        if self.nodes > 4096 || self.bytes > MAX_RESULT_BYTES {
            return Err(Error::ReturnSize);
        }
        Ok(())
    }
}
fn from_lua(
    value: Value,
    key: &str,
    depth: usize,
    budget: &mut Budget,
) -> Result<serde_json::Value, Error> {
    if depth > 16 {
        return Err(Error::ReturnSize);
    }
    budget.take(0)?;
    Ok(match value {
        Value::Boolean(value) => value.into(),
        Value::Integer(value) => value.into(),
        Value::String(value) => {
            budget.take(value.as_bytes().len())?;
            serde_json::Value::String(
                value
                    .to_str()
                    .map_err(|_| Error::ReturnEncoding)?
                    .to_owned(),
            )
        }
        Value::Table(table) => {
            let array = matches!(
                key,
                "diagnostics"
                    | "completions"
                    | "symbols"
                    | "code_actions"
                    | "actions"
                    | "annotations"
                    | "items"
                    | "choices"
                    | "edits"
                    | "selections"
            );
            if array {
                let len = table.raw_len();
                if len > MAX_ITEMS {
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
                    if count > MAX_ITEMS {
                        return Err(Error::ReturnSize);
                    }
                }
                if count != len {
                    return Err(Error::ReturnType);
                }
                let mut values = Vec::with_capacity(len);
                for at in 1..=len {
                    values.push(from_lua(
                        table.raw_get(at).map_err(failure)?,
                        "",
                        depth + 1,
                        budget,
                    )?);
                }
                serde_json::Value::Array(values)
            } else {
                let mut values = serde_json::Map::new();
                for pair in table.pairs::<Value, Value>() {
                    if values.len() >= MAX_ITEMS {
                        return Err(Error::ReturnSize);
                    }
                    let (key, value) = pair.map_err(failure)?;
                    let Value::String(key) = key else {
                        return Err(Error::ReturnType);
                    };
                    if key.as_bytes().len() > 128 {
                        return Err(Error::ReturnSize);
                    }
                    let key = key.to_str().map_err(|_| Error::ReturnEncoding)?;
                    budget.take(key.len())?;
                    values.insert(key.to_owned(), from_lua(value, &key, depth + 1, budget)?);
                }
                serde_json::Value::Object(values)
            }
        }
        _ => return Err(Error::ReturnType),
    })
}
pub(crate) fn evaluate(
    lua: &Lua,
    environment: Table,
    input: Input,
    location: Rc<RefCell<Option<sdk::Location>>>,
) -> Result<Output, Error> {
    input.validate()?;
    let request = serde_json::to_value(&input.request).map_err(|_| Error::Request)?;
    let cache = serde_json::to_value(&input.cache).map_err(|_| Error::Request)?;
    let coordinates = std::sync::Arc::new(super::coordinates::Positions::new(
        &input.package.snapshot.text,
    ));
    let (value, snapshot, _) = sdk::lua::invoke(
        lua,
        environment,
        input.package.clone(),
        location,
        |lua, context, api| {
            context
                .raw_set("request", to_lua(lua, &request).map_err(failure)?)
                .map_err(failure)?;
            context
                .raw_set("cache", to_lua(lua, &cache).map_err(failure)?)
                .map_err(failure)?;
            let to_coordinates = coordinates.clone();
            api.raw_set(
                "to_utf16",
                lua.create_function(move |_, byte: usize| {
                    to_coordinates
                        .position(byte)
                        .map_err(|_| mlua::Error::RuntimeError("invalid coordinate".into()))
                })
                .map_err(failure)?,
            )
            .map_err(failure)?;
            api.raw_set(
                "from_utf16",
                lua.create_function(move |_, (line, character): (usize, usize)| {
                    coordinates
                        .offset(line, character)
                        .map_err(|_| mlua::Error::RuntimeError("invalid coordinate".into()))
                })
                .map_err(failure)?,
            )
            .map_err(failure)?;
            Ok(())
        },
    )?;
    let value = from_lua(value, "", 0, &mut Budget { bytes: 0, nodes: 0 })?;
    let proposal: Proposal = serde_json::from_value(value).map_err(|_| Error::ReturnType)?;
    let output = Output {
        request: input.request.id.clone(),
        document: snapshot.document,
        revision: snapshot.revision,
        cache: proposal.cache.unwrap_or_else(|| input.cache.clone()),
        result: proposal.result,
    };
    output.validate(&input)?;
    Ok(output)
}
