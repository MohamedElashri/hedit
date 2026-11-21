//! Bounded, non-executable conversion used by rotated callback protocols.
use crate::{
    service::{MAX_ITEMS, MAX_RESULT_BYTES},
    Error,
};
use mlua::{Lua, Value};
fn failure(_: mlua::Error) -> Error {
    Error::Execution
}
pub(crate) fn to_lua(lua: &Lua, value: &serde_json::Value) -> mlua::Result<Value> {
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
pub(crate) struct Budget {
    pub(crate) bytes: usize,
    pub(crate) nodes: usize,
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
pub(crate) fn from_lua(
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
                    | "entries"
                    | "matches"
                    | "paths"
                    | "changes"
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
